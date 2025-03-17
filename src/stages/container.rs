use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use anyhow::{Context, Result};
use log::{info, warn, debug};
use colored::Colorize;

use crate::models::problem::SWEBenchProblem;
use crate::config::ContainerConfig;

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
    ).await?;
    
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
    
    let container_name = format!("test-{}", problem.id);
    
    // Run container with test script
    let result = run_container(
        &container_name,
        tag,
        "test-script.sh",
        config,
        "[TEST]".bright_green().to_string(),
    ).await?;
    
    info!("Test container exited with code {}", result.exit_code);
    
    Ok(result)
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
    let stdout = child.stdout.take()
        .expect("Failed to capture stdout");
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
    let stderr = child.stderr.take()
        .expect("Failed to capture stderr");
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
                    debug!("Container {} completed before timeout, cancelling timeout thread", container_name);
                    // Container completed normally, no need to kill it
                }
                Err(_) => {
                    // Timeout reached or channel disconnected
                    warn!("Container timeout reached for {}, stopping container", container_name);
                    
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
    let status = child.wait()
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