use anyhow::{Context, Result};
use colored::Colorize;
use log::{debug, info, warn};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::ContainerConfig;
use crate::models::problem::SWEBenchProblem;

/// Container run result with exit code and success status
#[derive(Debug, Clone)]
pub struct ContainerResult {
    pub name: String,
    pub exit_code: i32,
    pub success: bool,
    pub logs: Vec<String>,
}

/// Run a Docker container that executes the lint script
pub async fn run_lint_container(
    problem: &SWEBenchProblem,
    tag: &str,
    config: &ContainerConfig,
) -> Result<ContainerResult> {
    info!("Running lint container");

    let container_name = format!("lint-{}", problem.id);

    // Run container with lint script
    let result = run_container(
        &container_name,
        tag,
        "lint-script.sh",
        config,
        "[LINT]".bright_blue().to_string(),
    )
    .await?;

    info!("Lint container exited with code {}", result.exit_code);

    Ok(result)
}

/// Run a Docker container that executes the test script
pub async fn run_test_container(
    problem: &SWEBenchProblem,
    tag: &str,
    config: &ContainerConfig,
) -> Result<ContainerResult> {
    info!("Running test container");

    if config.retry_tests {
        // Use the retry-enabled version which can regenerate scripts/dockerfiles
        check_and_regenerate_on_test_failure(problem, tag, config).await
    } else {
        // Run the test once without retries
        let container_name = format!("test-{}", problem.id);
        let result = run_container(
            &container_name,
            tag,
            "test-script.sh",
            config,
            "[TEST]".bright_green().to_string(),
        )
        .await?;

        info!("Test container exited with code {}", result.exit_code);
        Ok(result)
    }
}

/// Run test with retry mechanism that can regenerate test scripts or dockerfiles on failure
pub async fn check_and_regenerate_on_test_failure(
    problem: &SWEBenchProblem,
    tag: &str,
    config: &ContainerConfig,
) -> Result<ContainerResult> {
    let mut retry_count = 0;
    let max_retries = config.max_retries;

    let container_name = format!("test-{}", problem.id);
    let mut last_result: Option<ContainerResult> = None;

    while retry_count <= max_retries {
        info!(
            "Running test container (attempt {}/{})",
            retry_count + 1,
            max_retries + 1
        );

        // Run the test
        let result = run_container(
            &container_name,
            tag,
            "test-script.sh",
            config,
            "[TEST]".bright_green().to_string(),
        )
        .await?;

        // Keep track of the last result
        last_result = Some(result.clone());

        info!("Test container exited with code {}", result.exit_code);

        // If test succeeded, return the result
        if result.success {
            println!("\nTest completed successfully!");
            info!("Test completed successfully");
            return Ok(result);
        }

        println!("\nTest failed!");
        info!("Test failed with error");

        // Check if we've reached the maximum number of retries
        if retry_count >= max_retries {
            println!(
                "Maximum retry attempts ({}) reached. Giving up.",
                max_retries
            );
            info!("Maximum retry attempts reached. Giving up.");
            break;
        }

        // Analyze the test failure
        println!("\nAnalyzing test failure...");
        info!("Analyzing test failure to determine fix approach");

        // Get the full config from main
        // Instead of reloading the config, use the one that was passed to RunTest command in main.rs
        let full_config = match std::env::var("ENGINE_BUILDER_CONFIG") {
            Ok(config_path) => {
                info!("Loading config from ENGINE_BUILDER_CONFIG environment variable: {}", config_path);
                crate::config::Config::from_file(Some(&config_path))
                    .context("Failed to load configuration for test failure analysis")?
            }
            Err(_) => {
                // Fallback if environment variable is not set
                warn!("ENGINE_BUILDER_CONFIG environment variable not set, using default configuration values");
                crate::config::Config::default()
            }
        };

        // Use LLM to analyze the failure
        let (fix_dockerfile, fix_test_script) = match analyze_test_failure_with_llm(
            &full_config,
            problem,
            &result.logs,
        )
        .await {
            Ok(decisions) => decisions,
            Err(e) => {
                // If LLM analysis fails, fall back to heuristic-based analysis
                warn!("LLM analysis failed: {}, falling back to heuristic analysis", e);
                analyze_test_failure_fallback(&result.logs)
            }
        };

        if fix_dockerfile {
            // Get the Dockerfile path
            println!("\nAttempting to fix Dockerfile...");
            info!("Attempting to fix Dockerfile based on test failure");

            // Get the Dockerfile path - first check .engines folder, then fall back to codebase path
            let codebase_path = problem.get_codebase_path()
                .map_or_else(|| PathBuf::from("."), |p| p.clone());
            let engines_dockerfile = PathBuf::from(".engines").join("Dockerfile");
            let dockerfile_path = if engines_dockerfile.exists() {
                engines_dockerfile
            } else {
                codebase_path.join("Dockerfile")
            };
            let error_output = result.logs.join("\n");

            // Update the Dockerfile using the full config
            let updated_dockerfile = crate::stages::dockerfile::update_dockerfile_from_error(
                &full_config,
                problem,
                &dockerfile_path,
                &error_output,
                retry_count,
            )
            .await?;
            
            // Create a backup of the original Dockerfile
            let backup_path = dockerfile_path.with_extension(format!("backup.{}", retry_count));
            fs::copy(&dockerfile_path, &backup_path).context(format!(
                "Failed to create backup of Dockerfile at {:?}",
                backup_path
            ))?;
            println!("Created backup of original Dockerfile at {:?}", backup_path);
            info!("Created backup of original Dockerfile at {:?}", backup_path);
            
            // Write the updated Dockerfile to disk
            fs::write(&dockerfile_path, &updated_dockerfile).context(format!(
                "Failed to write updated Dockerfile to {:?}",
                dockerfile_path
            ))?;
            println!("Updated Dockerfile written to {:?}", dockerfile_path);
            info!("Updated Dockerfile written to {:?}", dockerfile_path);

            // Rebuild the Docker image with the updated Dockerfile
            println!("\nRebuilding Docker image with updated Dockerfile...");
            info!("Rebuilding Docker image with updated Dockerfile");

            // Rebuild Docker image with the updated Dockerfile
            crate::stages::dockerfile::build_docker_image(&full_config, problem, tag).await?;
        }

        if fix_test_script {
            // Get the test script path
            println!("\nAttempting to fix test script...");
            info!("Attempting to fix test script based on test failure");

            // First check .engines folder, then fall back to codebase path
            let engines_script = PathBuf::from(".engines").join("test-script.sh");
            let test_script_path = if engines_script.exists() {
                engines_script
            } else {
                // Create scripts directory path in codebase
                let codebase_path = problem.get_codebase_path()
                    .map_or_else(|| PathBuf::from("."), |p| p.clone());
                let scripts_dir = codebase_path.join("scripts");
                scripts_dir.join("test-script.sh")
            };

            // Update the test script using the full config
            let updated_test_script = crate::stages::scripts::update_test_script_from_error(
                &full_config,
                problem,
                &test_script_path,
                &result.logs,
                retry_count,
            )
            .await?;

            // Save the updated test script
            let backup_path = test_script_path.with_extension(format!("backup.{}", retry_count));
            fs::copy(&test_script_path, &backup_path).context(format!(
                "Failed to create backup of test script at {:?}",
                backup_path
            ))?;
            println!("Created backup of original test script at {:?}", backup_path);
            info!("Created backup of original test script at {:?}", backup_path);

            fs::write(&test_script_path, &updated_test_script).context(format!(
                "Failed to write updated test script to {:?}",
                test_script_path
            ))?;
            println!("Updated test script written to {:?}", test_script_path);
            info!("Updated test script written to {:?}", test_script_path);

            // Make the script executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&test_script_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&test_script_path, perms)?;
            }

            // If we also updated the Dockerfile, we need to rebuild the image with the updated test script
            if fix_dockerfile {
                // The Dockerfile was already rebuilt above
                println!("\nDockerfile and test script both updated, image is already rebuilt");
                info!("Dockerfile and test script both updated, image is already rebuilt");
            } else {
                // Rebuild the image to include the updated test script
                println!("\nRebuilding Docker image with updated test script...");
                info!("Rebuilding Docker image with updated test script");

                // Rebuild Docker image with the updated test script
                crate::stages::dockerfile::build_docker_image(&full_config, problem, tag).await?;
            }
        }

        retry_count += 1;
    }

    // Return the last result if we have one, otherwise error
    match last_result {
        Some(result) => Ok(result),
        None => Err(anyhow::Error::msg("Failed to run test container")),
    }
}

/// System prompt for failure analysis
const FAILURE_ANALYSIS_SYSTEM_PROMPT: &str = r#"You are an expert diagnostician analyzing test failure logs from a Docker container. Your task is to determine what needs to be updated to fix the failure:

1. The Dockerfile that builds the container image
2. The test script that runs inside the container
3. Both the Dockerfile and test script

When analyzing test failures, consider:
- Missing dependencies in the Dockerfile (package not found, command not found)
- Permission issues (access denied, permission denied)
- Resource constraints (out of memory, killed)
- Script syntax errors (unexpected token, syntax error)
- Configuration issues (invalid option)
- Test framework issues (test not found, framework errors)

Your output must follow this exact format:

```json
{
  "fix_dockerfile": true|false,
  "fix_test_script": true|false,
  "reasoning": "A detailed explanation of your analysis and recommendations"
}
```

Be thorough in your reasoning but make a definitive recommendation on what should be fixed.
"#;

/// Analyze test failure logs using LLM to determine what to fix
pub async fn analyze_test_failure_with_llm(
    config: &crate::config::Config,
    problem: &SWEBenchProblem,
    logs: &[String],
) -> Result<(bool, bool)> {
    info!("Analyzing test failure logs with LLM");
    
    // Convert logs to a single string
    let logs_str = logs.join("\n");
    
    // Create the user prompt
    let user_prompt = format!(
        r#"Please analyze the following test failure logs from a Docker container and determine what needs to be fixed:

Test Failure Logs:
```
{}
```

Based on these logs, decide whether the Dockerfile, the test script, or both need to be fixed.
Your analysis should be thorough and consider clues like missing dependencies, permission issues, syntax errors, etc.
Respond in the exact JSON format specified in the system instructions.
"#,
        logs_str
    );
    
    // Create LLM config
    let llm_config = config.to_llm_config(&None);
    
    // Create LLM client
    let client = crate::llm::client::create_client(&llm_config)
        .await
        .context("Failed to create LLM client for test failure analysis")?;
        
    // Combine system and user prompts
    let combined_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        FAILURE_ANALYSIS_SYSTEM_PROMPT, user_prompt
    );
    
    // Send the request to the LLM
    let llm_response = client
        .completion_with_tracing(
            &combined_prompt,
            2000, // Reasonable token limit for this task
            0.2,  // Low temperature for more deterministic results
            None,
            Some(&format!("test_failure_analysis_{}", problem.id)),
            None,
        )
        .await
        .context("Failed to get test failure analysis from LLM")?;
        
    // Extract the JSON response
    let response_content = llm_response.content.clone();
    
    // Save reasoning for reference
    let metadata = serde_json::json!({
        "model": llm_config.model,
        "tokens": llm_response.usage.total_tokens
    });
    
    // Try to extract the reasoning for display
    let reasoning = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_content) {
        json["reasoning"].as_str().unwrap_or("").to_string()
    } else {
        // Try regex to extract reasoning if JSON parsing fails
        let re = regex::Regex::new(r#""reasoning":\s*"([^"]*)"#).unwrap();
        re.captures(&response_content)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "No reasoning available".to_string())
    };
    
    // Display reasoning to the user
    if !reasoning.is_empty() {
        println!("\nLLM Analysis: {}", reasoning);
    }
    
    crate::stages::overview::save_reasoning(
        config,
        problem,
        "test_failure_analysis",
        "",
        &response_content,
        Some(metadata),
    )
    .context("Failed to save test failure analysis to structured storage")?;
    
    // Extract the JSON portion using regex
    let re = regex::Regex::new(r#"\{[\s\S]*"fix_dockerfile"[\s\S]*"fix_test_script"[\s\S]*\}"#).unwrap();
    let json_str = match re.find(&response_content) {
        Some(mat) => mat.as_str(),
        None => {
            // Fallback to using the analyze_test_failure_fallback function
            warn!("Failed to extract JSON from LLM response, using fallback heuristic analysis");
            return Ok(analyze_test_failure_fallback(logs));
        }
    };
    
    // Parse the JSON response
    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(json) => {
            let fix_dockerfile = json["fix_dockerfile"].as_bool().unwrap_or(true);
            let fix_test_script = json["fix_test_script"].as_bool().unwrap_or(true);
            
            // Log the decision to console
            let decision_str = match (fix_dockerfile, fix_test_script) {
                (true, true) => "Will update both Dockerfile and test script",
                (true, false) => "Will update Dockerfile only",
                (false, true) => "Will update test script only",
                (false, false) => "No updates needed (unusual state, will still proceed)"
            };
            
            println!("\nLLM Decision: {}", decision_str);
            
            info!(
                "LLM analysis result: fix_dockerfile={}, fix_test_script={}",
                fix_dockerfile, fix_test_script
            );
            
            Ok((fix_dockerfile, fix_test_script))
        },
        Err(e) => {
            // Fallback to the regex-based analysis on parsing error
            warn!("Failed to parse LLM response as JSON: {}, using fallback heuristic analysis", e);
            Ok(analyze_test_failure_fallback(logs))
        }
    }
}

/// Fallback function that uses heuristics to analyze test failures
/// This is used when the LLM analysis fails
pub fn analyze_test_failure_fallback(logs: &[String]) -> (bool, bool) {
    // Convert logs to a single string for easier searching
    let logs_str = logs.join("\n");
    let logs_lower = logs_str.to_lowercase();

    // Indicators for Dockerfile issues
    let dockerfile_issues = [
        "command not found",
        "no such file or directory",
        "missing dependency",
        "not installed",
        "cannot find",
        "permission denied",
        "access denied",
        "executable file not found",
        "no such program",
        "segmentation fault",
        "killed",
        "out of memory",
        "resource temporarily unavailable",
    ];

    // Indicators for test script issues
    let test_script_issues = [
        "syntax error",
        "unexpected end of file",
        "unexpected token",
        "unbound variable",
        "undefined reference",
        "unrecognized option",
        "invalid option",
        "too few arguments",
        "too many arguments",
        "unknown command",
        "invalid syntax",
        "incorrect usage",
        "cannot execute",
    ];

    // Count the number of indicators for each category
    let dockerfile_issues_found: Vec<&str> = dockerfile_issues
        .iter()
        .filter(|issue| logs_lower.contains(&issue.to_lowercase()))
        .map(|s| *s)
        .collect();
    let dockerfile_count = dockerfile_issues_found.len();

    let test_script_issues_found: Vec<&str> = test_script_issues
        .iter()
        .filter(|issue| logs_lower.contains(&issue.to_lowercase()))
        .map(|s| *s)
        .collect();
    let test_script_count = test_script_issues_found.len();
    
    // Debug output
    #[cfg(test)]
    {
        println!("Logs: {:?}", logs);
        println!("Dockerfile issues found: {:?}", dockerfile_issues_found);
        println!("Test script issues found: {:?}", test_script_issues_found);
        println!("Dockerfile count: {}, Test script count: {}", dockerfile_count, test_script_count);
    }

    // Make the decision based on the number of indicators
    match (dockerfile_count, test_script_count) {
        (0, 0) => {
            // No clear indicators, try fixing both
            info!("No clear indicators in error logs, will try to fix both Dockerfile and test script");
            println!("\nFallback Analysis: No clear indicators in error logs, will try to fix both Dockerfile and test script");
            (true, true)
        }
        (d, t) if d > t => {
            // More Dockerfile issues, focus on that
            info!("Detected primarily Dockerfile issues ({} indicators vs {} for test script)", d, t);
            println!("\nFallback Analysis: Detected primarily Dockerfile issues ({} indicators vs {} for test script)", d, t);
            (true, false)
        }
        (d, t) if t > d => {
            // More test script issues, focus on that
            info!("Detected primarily test script issues ({} indicators vs {} for Dockerfile)", t, d);
            println!("\nFallback Analysis: Detected primarily test script issues ({} indicators vs {} for Dockerfile)", t, d);
            (false, true)
        }
        (d, _) => {
            // Equal number of issues, prioritize test script as it's easier to fix
            info!("Equal indicators for Dockerfile and test script issues ({} each), prioritizing test script", d);
            println!("\nFallback Analysis: Equal indicators for Dockerfile and test script issues ({} each), prioritizing test script", d);
            (false, true)
        }
    }
}

/// Run a Docker container with a specific command
async fn run_container(
    container_name: &str,
    image_tag: &str,
    script: &str,
    config: &ContainerConfig,
    output_prefix: String,
) -> Result<ContainerResult> {
    // Check if container already exists and remove it if necessary
    let check_output = Command::new("docker")
        .args(["ps", "-a", "-q", "-f", &format!("name={}", container_name)])
        .output()
        .context("Failed to check if container exists")?;

    if !check_output.stdout.is_empty() {
        info!("Container {} already exists, removing it", container_name);
        Command::new("docker")
            .args(["rm", "-f", container_name])
            .output()
            .context("Failed to remove existing container")?;
    }

    // Prepare docker run command
    let mut docker_cmd = Command::new("docker");
    docker_cmd
        .arg("run")
        .arg("--rm")
        .arg("--name")
        .arg(container_name)
        .arg("-i")  // Interactive mode to allow output streaming
        .arg(image_tag)
        .arg("bash")
        .arg("-c")
        .arg(format!("if [ -f /usr/local/bin/setup-script.sh ]; then /usr/local/bin/setup-script.sh; fi && /usr/local/bin/{}", script));

    info!("Starting container: {}", container_name);

    // Start container
    let mut child = docker_cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn docker container")?;

    // Collect logs
    let logs = Arc::new(Mutex::new(Vec::new()));
    let logs_clone = Arc::clone(&logs);

    // Stream stdout
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stdout_reader = BufReader::new(stdout);
    let stdout_prefix = output_prefix.clone();
    let stdout_logs = Arc::clone(&logs);

    let stdout_handle = thread::spawn(move || {
        for line in stdout_reader.lines() {
            if let Ok(line) = line {
                println!("{} {}", stdout_prefix, line);

                // Store log
                let mut logs = stdout_logs.lock().unwrap();
                logs.push(line);
            }
        }
    });

    // Stream stderr
    let stderr = child.stderr.take().expect("Failed to capture stderr");
    let stderr_reader = BufReader::new(stderr);
    let stderr_prefix = output_prefix.clone();
    let stderr_logs = Arc::clone(&logs);

    let stderr_handle = thread::spawn(move || {
        for line in stderr_reader.lines() {
            if let Ok(line) = line {
                println!("{} {}", stderr_prefix, line);

                // Store log
                let mut logs = stderr_logs.lock().unwrap();
                logs.push(line);
            }
        }
    });

    // Set up timeout cancellation channel
    let (timeout_tx, timeout_rx) = mpsc::channel();

    // Set timeout if configured
    let timeout = Duration::from_secs(config.timeout);
    let timeout_handle = if config.timeout > 0 {
        let container_name = container_name.to_string();
        let handle = thread::spawn(move || {
            debug!("Timeout thread started for container {}", container_name);

            // Wait for either timeout or cancellation signal
            match timeout_rx.recv_timeout(timeout) {
                Ok(_) => {
                    debug!(
                        "Container {} completed before timeout, cancelling timeout thread",
                        container_name
                    );
                    // Container completed normally, no need to kill it
                }
                Err(_) => {
                    // Timeout reached or channel disconnected
                    warn!(
                        "Container timeout reached for {}, stopping container",
                        container_name
                    );

                    // Kill container if it's still running
                    let _ = Command::new("docker")
                        .args(["stop", &container_name])
                        .output();
                }
            }

            debug!("Timeout thread for container {} exiting", container_name);
        });
        Some((handle, timeout_tx))
    } else {
        None
    };

    // Wait for container to complete
    let status = child
        .wait()
        .context("Failed to wait for docker container")?;

    // Wait for output threads to complete
    stdout_handle.join().expect("Failed to join stdout thread");
    stderr_handle.join().expect("Failed to join stderr thread");

    // Cancel timeout if it's still waiting by sending a message
    if let Some((handle, tx)) = timeout_handle {
        debug!("Container completed, signaling timeout thread to terminate");
        // Send cancellation signal - ignore errors if receiver is already dropped
        let _ = tx.send(());
        // Join the timeout thread
        handle.join().expect("Failed to join timeout thread");
    }

    // Clean up container if needed
    if config.remove {
        let _ = Command::new("docker")
            .args(["rm", "-f", container_name])
            .output();
    }

    // Get exit code
    let exit_code = status.code().unwrap_or(-1);
    let success = status.success();

    // Get collected logs
    let logs = logs_clone.lock().unwrap().clone();

    Ok(ContainerResult {
        name: container_name.to_string(),
        exit_code,
        success,
        logs,
    })
}

/// Run both lint and test containers, optionally in parallel
pub async fn run_containers(
    problem: &SWEBenchProblem,
    tag: &str,
    config: &ContainerConfig,
) -> Result<(ContainerResult, ContainerResult)> {
    info!("Running lint and test containers");

    if config.parallel {
        // Run both containers in parallel
        info!("Running containers in parallel mode");

        // Clone all data needed for the second task
        let problem_clone = problem.clone();
        let tag_clone = tag.to_string();
        let config_clone = config.clone();

        // Create separate clones for the lint task
        let lint_problem = problem.clone();
        let lint_tag = tag.to_string();
        let lint_config = config.clone();

        let lint_handle = tokio::spawn(async move {
            run_lint_container(&lint_problem, &lint_tag, &lint_config).await
        });

        let test_handle = tokio::spawn(async move {
            run_test_container(&problem_clone, &tag_clone, &config_clone).await
        });

        // Wait for both containers to complete
        let (lint_result, test_result) = tokio::try_join!(lint_handle, test_handle)
            .context("Failed to run containers in parallel")?;

        Ok((lint_result?, test_result?))
    } else {
        // Run containers sequentially
        info!("Running containers in sequential mode");

        let lint_result = run_lint_container(problem, tag, config).await?;
        let test_result = run_test_container(problem, tag, config).await?;

        Ok((lint_result, test_result))
    }
}
