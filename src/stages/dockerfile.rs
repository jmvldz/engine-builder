use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{Config, DockerfileConfig};
use crate::llm::client::create_client;
use crate::llm::prompts::{
    get_dockerfile_error_user_prompt, get_test_dockerfile_user_prompt,
    DOCKERFILE_ERROR_SYSTEM_PROMPT, TEST_DOCKERFILE_SYSTEM_PROMPT,
};
use crate::models::problem::SWEBenchProblem;
use crate::utils::trajectory_store::TrajectoryStore;

/// Generate a test-focused Dockerfile based on ranked files
pub async fn generate_dockerfile(config: &Config, mut problem: SWEBenchProblem) -> Result<()> {
    info!("Starting test-focused Dockerfile generation");

    // Get the trajectory directory for this problem
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
        "Failed to create trajectory store for problem: {}",
        problem.id
    ))?;

    // Check if ranking exists
    if !trajectory_store.ranking_exists() {
        return Err(anyhow::anyhow!(
            "Ranking not found for problem: {}. Run ranking step first.",
            problem.id
        ));
    }

    // Load the ranking
    let ranking_context = trajectory_store.load_ranking().context(format!(
        "Failed to load ranking for problem: {}",
        problem.id
    ))?;

    // Extract ranked files
    let ranked_files = ranking_context.ranked_files;

    if ranked_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No ranked files found for problem: {}",
            problem.id
        ));
    }

    // Limit to top 5 files to reduce context size
    let max_files = 5;
    let ranked_files = ranked_files.into_iter().take(max_files).collect::<Vec<_>>();

    info!(
        "Found {} ranked files, using top {} for Dockerfile generation",
        ranked_files.len(),
        max_files
    );

    // Load file contents
    let mut file_contents = Vec::new();

    for file in &ranked_files {
        match problem.get_file(&file.path) {
            Ok(file_data) => {
                file_contents.push((file.path.clone(), file_data.content.clone()));
            }
            Err(e) => {
                warn!("Failed to read file {}: {}", file.path, e);
            }
        }
    }

    // Create LLM config using the config's to_llm_config method
    let llm_config = config.to_llm_config(&config.dockerfile.model);

    // Create LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    info!("Generating Dockerfile from ranked files");

    // Generate the user prompt for the LLM
    let user_prompt =
        get_test_dockerfile_user_prompt(&problem.problem_statement, &ranked_files, &file_contents);

    // Combine with system prompt
    let combined_dockerfile_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_DOCKERFILE_SYSTEM_PROMPT, user_prompt
    );

    // Send the request to the LLM
    let llm_response = client
        .completion_with_tracing(
            &combined_dockerfile_prompt,
            config.dockerfile.max_tokens,
            config.dockerfile.temperature,
            None,
            Some(&format!("dockerfile_{}", problem.id)),
            None,
        )
        .await
        .context("Failed to get Dockerfile generation from LLM")?;

    // Save full LLM response which contains reasoning
    let full_llm_response = llm_response.content.clone();

    // Try to extract the Dockerfile content from markdown code blocks
    let dockerfile_content = match extract_dockerfile_from_response(&full_llm_response) {
        Some(content) => content,
        None => {
            warn!("Could not extract Dockerfile from LLM response, using raw response");
            full_llm_response.clone()
        }
    };

    // Save the full response with reasoning to the reasoning directory
    let reasoning_path = Path::new(&config.get_dockerfile_path(&problem.id))
        .with_file_name(format!("dockerfile_reasoning_{}.md", problem.id));

    fs::create_dir_all(reasoning_path.parent().unwrap()).context(format!(
        "Failed to create directory for Dockerfile reasoning at {:?}",
        reasoning_path.parent().unwrap()
    ))?;

    fs::write(&reasoning_path, &full_llm_response).context(format!(
        "Failed to write Dockerfile reasoning to {:?}",
        reasoning_path
    ))?;

    // Also save to the structured reasoning storage
    let metadata = serde_json::json!({
        "model": config.dockerfile.model,
        "tokens": llm_response.usage.total_tokens,
        "temperature": config.dockerfile.temperature
    });

    crate::stages::overview::save_reasoning(
        config,
        &problem,
        "dockerfile",
        "",
        &full_llm_response,
        Some(metadata),
    )
    .context("Failed to save Dockerfile reasoning to structured storage")?;

    info!("Generated Dockerfile content");
    info!("Saved Dockerfile reasoning to {:?}", reasoning_path);

    // Check if there are any setup/lint/test scripts from the scripts stage
    let setup_script_path = trajectory_store.problem_dir().join("setup-script.sh");
    let lint_script_path = trajectory_store.problem_dir().join("lint-script.sh");
    let test_script_path = trajectory_store.problem_dir().join("test-script.sh");
    let single_test_script_path = trajectory_store.problem_dir().join("single-test-script.sh");

    let mut final_dockerfile_content = dockerfile_content.clone();

    // Initialize a string to hold the script commands
    let mut script_commands = String::new();

    // Start building the script commands
    script_commands.push_str("\n# Copy scripts\n");

    // Add each script that exists
    if setup_script_path.exists() {
        script_commands.push_str("COPY setup-script.sh /usr/local/bin/setup-script.sh\n");
    }

    if lint_script_path.exists() {
        script_commands.push_str("COPY lint-script.sh /usr/local/bin/lint-script.sh\n");
    }

    if test_script_path.exists() {
        script_commands.push_str("COPY test-script.sh /usr/local/bin/test-script.sh\n");
    }

    if single_test_script_path.exists() {
        script_commands
            .push_str("COPY single-test-script.sh /usr/local/bin/single-test-script.sh\n");
    }

    // Add the RUN chmod command if any scripts exist
    if setup_script_path.exists()
        || lint_script_path.exists()
        || test_script_path.exists()
        || single_test_script_path.exists()
    {
        script_commands.push_str("RUN chmod +x ");

        let mut executables = Vec::new();
        if setup_script_path.exists() {
            executables.push("/usr/local/bin/setup-script.sh");
        }

        if lint_script_path.exists() {
            executables.push("/usr/local/bin/lint-script.sh");
        }

        if test_script_path.exists() {
            executables.push("/usr/local/bin/test-script.sh");
        }

        if single_test_script_path.exists() {
            executables.push("/usr/local/bin/single-test-script.sh");
        }

        script_commands.push_str(&executables.join(" "));
        script_commands.push_str("\n");

        info!("Found scripts, adding them to the Dockerfile");

        final_dockerfile_content.push_str(&script_commands);
    }

    // Save to the output directory under dockerfiles/{problem_id}/
    let dockerfile_path_str = config.get_dockerfile_path(&problem.id);
    let dockerfile_dir = Path::new(&dockerfile_path_str).parent().unwrap();
    fs::create_dir_all(dockerfile_dir).context(format!(
        "Failed to create Dockerfile directory at {:?}",
        dockerfile_dir
    ))?;

    let dockerfile_path = Path::new(&config.get_dockerfile_path(&problem.id)).to_path_buf();
    fs::write(&dockerfile_path, &final_dockerfile_content).context(format!(
        "Failed to write test-focused Dockerfile to {:?}",
        dockerfile_path
    ))?;

    info!("Test-focused Dockerfile saved to {:?}", dockerfile_path);

    Ok(())
}

/// Helper function to clean up copied files after Docker build
fn cleanup_copied_files(docker_context_dir: &Path) -> Result<()> {
    info!("Cleaning up files copied to Docker context");

    // List of files to clean up
    let files_to_clean = vec![
        "Dockerfile",
        "setup-script.sh",
        "lint-script.sh",
        "test-script.sh",
        "single-test-script.sh",
    ];

    for file in files_to_clean {
        let file_path = docker_context_dir.join(file);
        if file_path.exists() {
            fs::remove_file(&file_path)
                .context(format!("Failed to remove copied file: {:?}", file_path))?;
            info!("Removed copied file: {:?}", file_path);
        }
    }

    Ok(())
}

/// Build a Docker image using the generated Dockerfile
pub async fn build_docker_image(
    config: &Config,
    problem: &SWEBenchProblem,
    tag: &str,
) -> Result<()> {
    // Get the max retries from config
    let max_retries = config.dockerfile.max_retries;

    // Get trajectory directory for this problem
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
        "Failed to create trajectory store for problem: {}",
        problem.id
    ))?;

    let mut retry_count = 0;
    while retry_count <= max_retries {
        let dockerfile_path = Path::new(&config.get_dockerfile_path(&problem.id)).to_path_buf();

        if retry_count > 0 {
            info!("Retry {} of {}", retry_count, max_retries);
            println!("\nRetry {} of {}", retry_count, max_retries);
        }

        // Use the repository directory as the Docker context
        // This makes files from the repository available during the build
        let docker_context_dir = problem
            .get_codebase_path()
            .ok_or_else(|| anyhow!("Codebase path not set for problem"))?;
        info!(
            "Using repository as Docker context: {:?}",
            docker_context_dir
        );

        // Copy scripts to the Docker context if they exist
        let setup_script_path = trajectory_store.problem_dir().join("setup-script.sh");
        let lint_script_path = trajectory_store.problem_dir().join("lint-script.sh");
        let test_script_path = trajectory_store.problem_dir().join("test-script.sh");
        let single_test_script_path = trajectory_store.problem_dir().join("single-test-script.sh");

        if setup_script_path.exists() {
            let dest_path = docker_context_dir.join("setup-script.sh");
            fs::copy(&setup_script_path, &dest_path).context(format!(
                "Failed to copy setup script to Docker context: {:?}",
                dest_path
            ))?;
            info!("Copied setup script to Docker context: {:?}", dest_path);
        }

        if lint_script_path.exists() {
            let dest_path = docker_context_dir.join("lint-script.sh");
            fs::copy(&lint_script_path, &dest_path).context(format!(
                "Failed to copy lint script to Docker context: {:?}",
                dest_path
            ))?;
            info!("Copied lint script to Docker context: {:?}", dest_path);
        }

        if test_script_path.exists() {
            let dest_path = docker_context_dir.join("test-script.sh");
            fs::copy(&test_script_path, &dest_path).context(format!(
                "Failed to copy test script to Docker context: {:?}",
                dest_path
            ))?;
            info!("Copied test script to Docker context: {:?}", dest_path);
        }

        if single_test_script_path.exists() {
            let dest_path = docker_context_dir.join("single-test-script.sh");
            fs::copy(&single_test_script_path, &dest_path).context(format!(
                "Failed to copy single test script to Docker context: {:?}",
                dest_path
            ))?;
            info!(
                "Copied single test script to Docker context: {:?}",
                dest_path
            );
        }

        // Look for the Dockerfile in the .engines folder
        let engines_dockerfile = Path::new(".engines").join("Dockerfile");
        let source_path = if engines_dockerfile.exists() {
            engines_dockerfile
        } else {
            dockerfile_path.clone()
        };

        // Copy the Dockerfile to the Docker context
        let dest_path = docker_context_dir.join("Dockerfile");
        fs::copy(&source_path, &dest_path).context(format!(
            "Failed to copy Dockerfile to Docker context: {:?}",
            dest_path
        ))?;
        info!(
            "Copied Dockerfile from {:?} to Docker context: {:?}",
            source_path, dest_path
        );

        // Build the Docker image
        info!("Building Docker image with tag: {}", tag);
        println!("\nBuilding Docker image with tag: {}", tag);

        let mut docker_build_command = Command::new("docker");
        docker_build_command.arg("build");
        docker_build_command.arg("-t");
        docker_build_command.arg(tag);
        docker_build_command.arg(".");
        docker_build_command.current_dir(&docker_context_dir);

        // For capturing stderr
        docker_build_command.stderr(Stdio::piped());

        info!("Running docker build command: {:?}", docker_build_command);
        println!("\nRunning docker build...");

        let build_process = docker_build_command
            .spawn()
            .context("Failed to spawn docker build process")?;

        let build_output = build_process
            .wait_with_output()
            .context("Failed to wait for docker build process")?;

        // Log stderr for debugging
        let error_output = String::from_utf8_lossy(&build_output.stderr).into_owned();
        if !error_output.is_empty() {
            warn!("Docker build stderr: {}", error_output);
        }

        // Clean up copied files from Docker context
        if let Err(e) = cleanup_copied_files(&docker_context_dir) {
            warn!("Failed to clean up copied files: {}", e);
        }

        // Check if the build was successful
        if build_output.status.success() {
            println!("\nDocker build completed successfully!");
            info!("Docker build completed successfully");
            info!("Image built with tag: {}", tag);
            return Ok(());
        }

        println!("\nDocker build failed!");
        info!("Docker build failed with error");

        // Check if we've reached the maximum number of retries
        if retry_count >= max_retries {
            println!(
                "Maximum retry attempts ({}) reached. Giving up.",
                max_retries
            );
            info!("Maximum retry attempts reached. Giving up.");
            return Err(anyhow!(
                "Docker build failed after {} attempts",
                max_retries + 1
            ));
        }

        // Update the Dockerfile using LLM suggestions
        println!("\nAnalyzing build error and updating Dockerfile...");
        info!("Attempting to fix Dockerfile using LLM...");

        // Create a config for the update_dockerfile_from_error function
        let dockerfile_config = DockerfileConfig {
            model: Some("claude-3-opus-20240229".to_string()),
            max_tokens: 4096,
            temperature: 0.0,
            max_retries: 3,
        };

        let updated_dockerfile = update_dockerfile_from_error(
            &dockerfile_config,
            problem,
            &dockerfile_path,
            &error_output,
            retry_count,
        )
        .await?;

        // Save the updated Dockerfile
        let backup_path = dockerfile_path.with_extension(format!("backup.{}", retry_count));
        fs::copy(&dockerfile_path, &backup_path).context(format!(
            "Failed to create backup of Dockerfile at {:?}",
            backup_path
        ))?;
        println!("Created backup of original Dockerfile at {:?}", backup_path);
        info!("Created backup of original Dockerfile at {:?}", backup_path);

        fs::write(&dockerfile_path, &updated_dockerfile).context(format!(
            "Failed to write updated Dockerfile to {:?}",
            dockerfile_path
        ))?;
        println!("Updated Dockerfile written to {:?}", dockerfile_path);
        info!("Updated Dockerfile written to {:?}", dockerfile_path);

        retry_count += 1;
    }

    Err(anyhow!(
        "Docker build failed after {} attempts",
        max_retries + 1
    ))
}

/// Extract Dockerfile content from LLM response, looking for a markdown code block
pub fn extract_dockerfile_from_response(response: &str) -> Option<String> {
    // Try to match ```dockerfile ... ``` blocks (case insensitive)
    let dockerfile_re = Regex::new(r"(?i)```\s*dockerfile\s*\n([\s\S]*?)\n\s*```").unwrap();
    if let Some(captures) = dockerfile_re.captures(response) {
        return captures.get(1).map(|m| m.as_str().to_string());
    }

    // Try to match plain ``` ... ``` blocks that might contain a Dockerfile
    let plain_code_re = Regex::new(r"```\s*\n([\s\S]*?)\n\s*```").unwrap();
    if let Some(captures) = plain_code_re.captures(response) {
        let content = captures.get(1).map(|m| m.as_str().to_string());
        if let Some(content) = content {
            // Check if it looks like a Dockerfile (has FROM instruction)
            if content.contains("FROM ") {
                return Some(content);
            }
        }
    }

    None
}

/// Update a Dockerfile based on error output from a failed build
async fn update_dockerfile_from_error(
    config: &DockerfileConfig,
    problem: &SWEBenchProblem,
    dockerfile_path: &Path,
    error_output: &str,
    attempt: usize,
) -> Result<String> {
    // Read the current Dockerfile
    let dockerfile_content = fs::read_to_string(dockerfile_path).context(format!(
        "Failed to read Dockerfile at {:?}",
        dockerfile_path
    ))?;

    // Get API key from environment
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

    // Get the parent config to access additional settings
    let parent_config = std::env::var("ENGINE_BUILDER_CONFIG")
        .map(|path| crate::config::Config::from_file(Some(&path)))
        .unwrap_or_else(|_| crate::config::Config::from_file(None));

    // Create LLM config with the API key
    let llm_config = crate::config::LLMConfig {
        model_type: "anthropic".to_string(),
        model: config
            .model
            .clone()
            .unwrap_or_else(|| "claude-3-opus-20240229".to_string()),
        api_key,
        base_url: None,
        timeout: 60,
        max_retries: 3,
    };

    // Create LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    // Generate the user prompt for the LLM
    let user_prompt = get_dockerfile_error_user_prompt(
        &problem.problem_statement,
        &dockerfile_content,
        error_output,
    );

    // Combine with system prompt
    let combined_error_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        DOCKERFILE_ERROR_SYSTEM_PROMPT, user_prompt
    );

    // Send the request to the LLM
    let llm_response = client
        .completion_with_tracing(
            &combined_error_prompt,
            config.max_tokens,
            config.temperature,
            None,
            Some(&format!("dockerfile_error_{}", problem.id)),
            None,
        )
        .await
        .context("Failed to get Dockerfile fix from LLM")?;

    // Extract the updated Dockerfile content
    let full_llm_response = llm_response.content.clone();

    // Save the reasoning to a file
    let reasoning_path =
        dockerfile_path.with_file_name(format!("dockerfile_error_reasoning_{}.md", problem.id));

    fs::write(&reasoning_path, &full_llm_response).context(format!(
        "Failed to write Dockerfile error reasoning to {:?}",
        reasoning_path
    ))?;

    // Use parent_config if available, or create a minimal one
    let parent_config_val = match parent_config {
        Ok(ref conf) => conf.clone(),
        Err(_) => {
            // Just use environment variables for API keys and minimal default settings
            let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

            // Load the existing CodebaseConfig since it doesn't have a Default impl
            let codebase = crate::config::CodebaseConfig {
                path: if let Some(path) = problem.get_codebase_path() {
                    path.clone()
                } else {
                    PathBuf::from(".")
                },
                problem_id: problem.id.clone(),
                problem_statement: problem.problem_statement.clone(),
                exclusions_path: "exclusions.json".to_string(),
            };

            crate::config::Config {
                anthropic_api_key: api_key,
                model: "claude-3-opus-20240229".to_string(),
                relevance: crate::config::RelevanceConfig::default(),
                ranking: crate::config::RankingConfig::default(),
                codebase,
                dockerfile: crate::config::DockerfileConfig::default(),
                scripts: crate::config::ScriptConfig::default(),
                chat: crate::config::ChatConfig::default(),
                container: crate::config::ContainerConfig::default(),
                observability: crate::config::ObservabilityConfig::default(),
                output_path: None,
            }
        }
    };

    // Add attempt number as identifier
    let metadata = serde_json::json!({
        "model": config.model,
        "tokens": llm_response.usage.total_tokens,
        "temperature": config.temperature,
        "attempt": attempt
    });

    crate::stages::overview::save_reasoning(
        &parent_config_val,
        problem,
        "dockerfile_error",
        &format!("_{}", attempt),
        &full_llm_response,
        Some(metadata),
    )
    .context("Failed to save Dockerfile error reasoning to structured storage")?;

    info!("Saved Dockerfile error reasoning to {:?}", reasoning_path);

    // Try to extract the Dockerfile content from markdown code blocks
    match extract_dockerfile_from_response(&full_llm_response) {
        Some(content) => Ok(content),
        None => {
            // If we can't extract a code block, try to look for lines that might be Dockerfile instructions
            // This is a fallback in case the LLM responds in a different format
            let lines = full_llm_response.lines();
            let dockerfile_lines: Vec<_> = lines
                .filter(|line| {
                    line.contains("FROM ")
                        || line.contains("RUN ")
                        || line.contains("COPY ")
                        || line.contains("WORKDIR ")
                        || line.contains("ENV ")
                        || line.contains("EXPOSE ")
                        || line.contains("CMD ")
                        || line.contains("ENTRYPOINT ")
                })
                .collect();

            if !dockerfile_lines.is_empty() {
                Ok(dockerfile_lines.join("\n"))
            } else {
                warn!("Could not extract updated Dockerfile from LLM response, using original");
                Ok(dockerfile_content)
            }
        }
    }
}
