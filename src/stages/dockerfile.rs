use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{Config, DockerfileConfig};
use crate::llm::client::create_client;
use crate::llm::prompts::{
    get_dockerfile_error_user_prompt, get_test_dockerfile_user_prompt,
};
use crate::models::problem::SWEBenchProblem;
use crate::models::relevance::RelevanceStatus;
use crate::utils::trajectory_store::TrajectoryStore;

/// Generate a test-focused Dockerfile based on ranked files
pub async fn generate_dockerfile(
    config: &Config,
    mut problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting test-focused Dockerfile generation");

    // Get the trajectory directory for this problem
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let trajectory_store =
        TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
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
    let ranking_context = trajectory_store
        .load_ranking()
        .context(format!("Failed to load ranking for problem: {}", problem.id))?;

    // Extract ranked files
    let ranked_files = ranking_context.ranked_files;

    if ranked_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No ranked files found for problem: {}",
            problem.id
        ));
    }

    info!("Found {} ranked files", ranked_files.len());

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

    // Generate the prompt for the LLM
    let prompt = get_test_dockerfile_user_prompt(&problem.problem_statement, &ranked_files, &file_contents);

    // Send the request to the LLM
    let llm_response = client
        .completion_with_tracing(
            &prompt,
            config.dockerfile.max_tokens,
            config.dockerfile.temperature,
            None,
            Some(&format!("dockerfile_{}", problem.id)),
            None,
        )
        .await
        .context("Failed to get Dockerfile generation from LLM")?;

    // Extract the Dockerfile content
    let dockerfile_content = llm_response.content.clone();

    // Try to extract the Dockerfile content from markdown code blocks
    let dockerfile_content = match extract_dockerfile_from_response(&dockerfile_content) {
        Some(content) => content,
        None => {
            warn!("Could not extract Dockerfile from LLM response, using raw response");
            dockerfile_content
        }
    };

    info!("Generated Dockerfile content");

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
    let trajectory_store =
        TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Load all relevance decisions
    let all_decisions = trajectory_store
        .load_all_relevance_decisions()
        .context(format!(
            "Failed to load relevance decisions for problem: {}",
            problem.id
        ))?;

    // Get relevant files
    let relevant_files = all_decisions
        .into_iter()
        .filter(|(_, decision)| decision.status == RelevanceStatus::Relevant)
        .map(|(path, decision)| (path, decision.summary.unwrap_or_default()))
        .collect::<Vec<_>>();

    if relevant_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No relevant files found for problem: {}",
            problem.id
        ));
    }

    info!("Found {} relevant files", relevant_files.len());

    // Limit to top N files to avoid context overflow
    let max_files = 10;
    let relevant_files = relevant_files
        .into_iter()
        .take(max_files)
        .collect::<Vec<_>>();

    // Load file contents
    let mut file_contents = Vec::new();

    for (path, _) in &relevant_files {
        // Clone the problem first to allow for mutable borrowing in get_file
        let mut problem_clone = problem.clone();
        match problem_clone.get_file(path) {
            Ok(file_data) => {
                file_contents.push((path.clone(), file_data.content.clone()));
            }
            Err(e) => {
                warn!("Failed to read file {}: {}", path, e);
            }
        }
    }

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
        info!("Copied Dockerfile from {:?} to Docker context: {:?}", source_path, dest_path);

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
        
        let updated_dockerfile =
            update_dockerfile_from_error(&dockerfile_config, problem, &dockerfile_path, &error_output)
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
) -> Result<String> {
    // Read the current Dockerfile
    let dockerfile_content = fs::read_to_string(dockerfile_path).context(format!(
        "Failed to read Dockerfile at {:?}",
        dockerfile_path
    ))?;

    // Get the parent config to access the API key
    let parent_config = std::env::var("ENGINE_BUILDER_CONFIG")
        .map(|path| crate::config::Config::from_file(Some(&path)))
        .unwrap_or_else(|_| crate::config::Config::from_file(None));
    
    // Get API key, first from environment then from config
    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        parent_config
            .ok()
            .map(|c| c.anthropic_api_key)
            .unwrap_or_default()
    });

    // Create LLM config with the API key
    let llm_config = crate::config::LLMConfig {
        model_type: "anthropic".to_string(),
        model: config.model.clone().unwrap_or_else(|| "claude-3-opus-20240229".to_string()),
        api_key: api_key,
        base_url: None,
        timeout: 60,
        max_retries: 3,
    };

    // Create LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    // Generate the prompt for the LLM
    let prompt = get_dockerfile_error_user_prompt(
        &problem.problem_statement,
        &dockerfile_content,
        error_output,
    );

    // Send the request to the LLM
    let llm_response = client
        .completion_with_tracing(
            &prompt,
            config.max_tokens,
            config.temperature,
            None,
            Some(&format!("dockerfile_error_{}", problem.id)),
            None,
        )
        .await
        .context("Failed to get Dockerfile fix from LLM")?;

    // Extract the updated Dockerfile content
    let updated_dockerfile = llm_response.content.clone();

    // Try to extract the Dockerfile content from markdown code blocks
    match extract_dockerfile_from_response(&updated_dockerfile) {
        Some(content) => Ok(content),
        None => {
            // If we can't extract a code block, try to look for lines that might be Dockerfile instructions
            // This is a fallback in case the LLM responds in a different format
            let lines = updated_dockerfile.lines();
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