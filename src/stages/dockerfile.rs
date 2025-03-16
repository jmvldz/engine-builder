use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{RankingConfig, RelevanceConfig};
use crate::llm::client::create_client;
use crate::llm::prompts::{
    get_dockerfile_error_user_prompt, get_test_dockerfile_user_prompt,
    DOCKERFILE_ERROR_SYSTEM_PROMPT, TEST_DOCKERFILE_SYSTEM_PROMPT,
};
use crate::models::problem::SWEBenchProblem;
use crate::models::relevance::RelevanceStatus;
use crate::utils::trajectory_store::TrajectoryStore;

/// Generate a test-focused Dockerfile based on ranked files
pub async fn generate_dockerfile(
    config: RankingConfig,
    mut problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting test-focused Dockerfile generation");

    // Create a trajectory store for this problem
    let trajectory_store =
        TrajectoryStore::new(&config.trajectory_store_dir, &problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Check if ranking exists
    if !trajectory_store.ranking_exists() {
        return Err(anyhow::anyhow!(
            "No ranking found for problem: {}. Run the ranking step first.",
            problem.id
        ));
    }

    // Load ranking
    let ranking = trajectory_store.load_ranking().context(format!(
        "Failed to load ranking for problem: {}",
        problem.id
    ))?;

    // Get ranked files (limit to top N files to avoid context overflow)
    let max_files = 10; // Limiting to top 5 files
    let ranked_files = ranking
        .ranked_files
        .iter()
        .take(max_files)
        .cloned()
        .collect::<Vec<_>>();

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

    // Create LLM client
    let client = create_client(&config.llm)
        .await
        .context("Failed to create LLM client")?;

    // Generate test-focused prompt
    let prompt =
        get_test_dockerfile_user_prompt(&problem.problem_statement, &ranked_files, &file_contents);

    // Generate test-focused Dockerfile
    info!("Generating test-focused Dockerfile...");
    // Create a combined prompt with system and user instructions
    let combined_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_DOCKERFILE_SYSTEM_PROMPT, prompt
    );

    // Save the prompt to a file in the trajectory store
    let prompt_path = trajectory_store.problem_dir().join("dockerfile-prompt.txt");
    fs::write(&prompt_path, &format!("{}\n\n{}", TEST_DOCKERFILE_SYSTEM_PROMPT, prompt))
        .context(format!("Failed to write Dockerfile prompt to {:?}", prompt_path))?;
    info!("Dockerfile prompt saved to {:?}", prompt_path);

    let response = client
        .completion(&combined_prompt, config.max_tokens, config.temperature)
        .await
        .context("Failed to generate test-focused Dockerfile")?;

    // Track usage
    let usage = response.usage;
    let cost = client.calculate_cost(&usage);
    info!("Test Dockerfile generation LLM usage: {}", usage);
    info!("Test Dockerfile generation LLM cost: {}", cost);

    // Extract Dockerfile content
    let dockerfile_content = extract_dockerfile(&response.content)
        .context("Failed to extract Dockerfile content from LLM response")?;

    // Check if scripts exist and append commands to copy them into the Docker image
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
        script_commands.push_str("\n# Make scripts executable\nRUN chmod +x ");

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

    // Save to the trajectory store directory
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    fs::write(&dockerfile_path, &final_dockerfile_content).context(format!(
        "Failed to write test-focused Dockerfile to {:?}",
        dockerfile_path
    ))?;

    info!("Test-focused Dockerfile saved to {:?}", dockerfile_path);

    Ok(())
}

/// Extract Dockerfile content from LLM response
pub fn extract_dockerfile(response: &str) -> Result<String> {
    // Try to extract content between ```dockerfile and ``` tags
    let re = Regex::new(r"```dockerfile\s*([\s\S]*?)\s*```").unwrap();
    if let Some(captures) = re.captures(response) {
        if let Some(content) = captures.get(1) {
            return Ok(content.as_str().to_string());
        }
    }

    // If that fails, try to extract content between ``` and ``` tags
    let re = Regex::new(r"```\s*([\s\S]*?)\s*```").unwrap();
    if let Some(captures) = re.captures(response) {
        if let Some(content) = captures.get(1) {
            return Ok(content.as_str().to_string());
        }
    }

    // If all else fails, just return the entire response
    warn!("Failed to extract Dockerfile content from response, returning entire response");
    Ok(response.to_string())
}

/// Update a Dockerfile based on build errors using LLM suggestions
pub async fn update_dockerfile_from_error(
    config: &RankingConfig,
    problem: &SWEBenchProblem,
    dockerfile_path: &Path,
    error_message: &str,
) -> Result<String> {
    info!("Updating Dockerfile based on build error");

    // Read the current Dockerfile content
    let dockerfile_content = fs::read_to_string(dockerfile_path).context(format!(
        "Failed to read Dockerfile at {:?}",
        dockerfile_path
    ))?;

    // Create LLM client
    let client = create_client(&config.llm)
        .await
        .context("Failed to create LLM client")?;

    // Generate prompt for error analysis
    let prompt = get_dockerfile_error_user_prompt(
        &problem.problem_statement,
        &dockerfile_content,
        error_message,
    );

    // Create a combined prompt with system and user instructions
    let combined_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        DOCKERFILE_ERROR_SYSTEM_PROMPT, prompt
    );

    // Save the prompt to a file in the trajectory store
    let prompt_dir = Path::new(dockerfile_path).parent().unwrap_or(Path::new("."));
    let prompt_path = prompt_dir.join("dockerfile-error-prompt.txt");
    fs::write(&prompt_path, &format!("{}\n\n{}", DOCKERFILE_ERROR_SYSTEM_PROMPT, prompt))
        .context(format!("Failed to write Dockerfile error prompt to {:?}", prompt_path))?;
    info!("Dockerfile error prompt saved to {:?}", prompt_path);

    info!("Asking LLM for Dockerfile fixes...");
    let response = client
        .completion(&combined_prompt, config.max_tokens, config.temperature)
        .await
        .context("Failed to get Dockerfile fix suggestions")?;

    // Track usage
    let usage = response.usage;
    let cost = client.calculate_cost(&usage);
    info!("Dockerfile error analysis LLM usage: {}", usage);
    info!("Dockerfile error analysis LLM cost: {}", cost);

    // Extract updated Dockerfile content
    let updated_dockerfile = extract_dockerfile(&response.content)
        .context("Failed to extract updated Dockerfile from LLM response")?;

    Ok(updated_dockerfile)
}

/// Build a Docker image from the generated Dockerfile
/// Generate a test-focused Dockerfile based on relevance data
pub async fn generate_dockerfile_from_relevance(
    config: RelevanceConfig,
    mut problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting test-focused Dockerfile generation from relevance data");

    // Create a trajectory store for this problem
    let trajectory_store =
        TrajectoryStore::new(&config.trajectory_store_dir, &problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Check if relevance decisions exist in the consolidated file
    let relevance_decisions_path = trajectory_store.relevance_decisions_path();
    if !relevance_decisions_path.exists() {
        return Err(anyhow::anyhow!(
            "No relevance decisions found for problem: {}. Run the relevance step first.",
            problem.id
        ));
    }

    // Load all relevance decisions and find relevant files
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

    for (file_path, _) in &relevant_files {
        match problem.get_file(file_path) {
            Ok(file_data) => {
                file_contents.push((file_path.clone(), file_data.content.clone()));
            }
            Err(e) => {
                warn!("Failed to read file {}: {}", file_path, e);
            }
        }
    }

    // Create LLM client
    let client = create_client(&config.llm)
        .await
        .context("Failed to create LLM client")?;

    // Generate test-focused prompt
    // Convert relevant_files to a format similar to ranked_files
    let formatted_files = relevant_files
        .iter()
        .map(|(path, _summary)| {
            crate::models::ranking::RankedCodebaseFile {
                path: path.clone(),
                tokens: 0, // Using a placeholder value since we don't need token counts here
            }
        })
        .collect::<Vec<_>>();

    let prompt = get_test_dockerfile_user_prompt(
        &problem.problem_statement,
        &formatted_files,
        &file_contents,
    );

    // Generate test-focused Dockerfile
    info!("Generating test-focused Dockerfile...");
    // Create a combined prompt with system and user instructions
    let combined_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_DOCKERFILE_SYSTEM_PROMPT, prompt
    );

    // Save the prompt to a file in the trajectory store
    let prompt_path = trajectory_store.problem_dir().join("dockerfile-prompt.txt");
    fs::write(&prompt_path, &format!("{}\n\n{}", TEST_DOCKERFILE_SYSTEM_PROMPT, prompt))
        .context(format!("Failed to write Dockerfile prompt to {:?}", prompt_path))?;
    info!("Dockerfile prompt saved to {:?}", prompt_path);

    let response = client
        .completion(&combined_prompt, config.max_tokens, 0.0)
        .await
        .context("Failed to generate test-focused Dockerfile")?;

    // Track usage
    let usage = response.usage;
    let cost = client.calculate_cost(&usage);
    info!("Test Dockerfile generation LLM usage: {}", usage);
    info!("Test Dockerfile generation LLM cost: {}", cost);

    // Extract Dockerfile content
    let dockerfile_content = extract_dockerfile(&response.content)
        .context("Failed to extract Dockerfile content from LLM response")?;

    // Check if scripts exist and append commands to copy them into the Docker image
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
        script_commands.push_str("\n# Make scripts executable\nRUN chmod +x ");

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

    // Save to the trajectory store directory
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    fs::write(&dockerfile_path, &final_dockerfile_content).context(format!(
        "Failed to write test-focused Dockerfile to {:?}",
        dockerfile_path
    ))?;

    info!("Test-focused Dockerfile saved to {:?}", dockerfile_path);

    Ok(())
}

pub async fn build_docker_image_from_relevance(
    config: &RelevanceConfig,
    problem: &SWEBenchProblem,
    tag: &str,
    max_retries: usize,
) -> Result<()> {
    info!("Building Docker image with tag: {}", tag);

    // Create a trajectory store for this problem
    let trajectory_store =
        TrajectoryStore::new(&config.trajectory_store_dir, problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Check if Dockerfile exists
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    if !dockerfile_path.exists() {
        return Err(anyhow!(
            "Dockerfile not found at {:?}. Generate it first with the 'dockerfile' command.",
            dockerfile_path
        ));
    }

    info!("Using Dockerfile at {:?}", dockerfile_path);

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

    // Try building the Docker image, with retries if it fails
    let mut retry_count = 0;

    loop {
        // Run docker build command with streaming output
        println!(
            "\n=== Docker Build (Attempt {}/{}) ===",
            retry_count + 1,
            max_retries + 1
        );
        info!(
            "Running docker build (attempt {}/{})...",
            retry_count + 1,
            max_retries + 1
        );

        let mut child = Command::new("docker")
            .arg("build")
            .arg("-t")
            .arg(tag)
            .arg("-f")
            .arg(&dockerfile_path)
            .arg(docker_context_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute docker build command")?;

        // Stream stdout in real-time
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stdout_reader = BufReader::new(stdout);
        let stderr = child.stderr.take().expect("Failed to capture stderr");
        let stderr_reader = BufReader::new(stderr);

        // Collect stderr for potential error analysis
        let mut error_output = String::new();

        // Create a thread to read and display stdout
        let stdout_handle = std::thread::spawn(move || {
            for line in stdout_reader.lines() {
                if let Ok(line) = line {
                    println!("[stdout] {}", line);
                }
            }
        });

        // Read and display stderr, also collecting it for error analysis if needed
        for line in stderr_reader.lines() {
            if let Ok(line) = line {
                println!("[stderr] {}", line);
                error_output.push_str(&line);
                error_output.push('\n');
            }
        }

        // Wait for stdout thread to complete
        stdout_handle.join().expect("Failed to join stdout thread");

        // Wait for the command to complete and get the exit status
        let status = child
            .wait()
            .context("Failed to wait for docker build command")?;

        if status.success() {
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

        // Create a ranking config from the relevance config for using with update_dockerfile_from_error
        let ranking_config = RankingConfig {
            llm: config.llm.clone(),
            num_rankings: 3,
            max_workers: config.max_workers,
            max_tokens: config.max_tokens,
            temperature: 0.0,
            trajectory_store_dir: config.trajectory_store_dir.clone(),
        };

        // Update the Dockerfile using LLM suggestions
        println!("\nAnalyzing build error and updating Dockerfile...");
        info!("Attempting to fix Dockerfile using LLM...");
        let updated_dockerfile =
            update_dockerfile_from_error(&ranking_config, problem, &dockerfile_path, &error_output)
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
        println!("Updated Dockerfile with LLM suggestions");
        info!("Updated Dockerfile with LLM suggestions");

        // Increment retry counter
        retry_count += 1;
    }
}

pub async fn build_docker_image(
    config: &RankingConfig,
    problem: &SWEBenchProblem,
    tag: &str,
    max_retries: usize,
) -> Result<()> {
    info!("Building Docker image with tag: {}", tag);

    // Create a trajectory store for this problem
    let trajectory_store =
        TrajectoryStore::new(&config.trajectory_store_dir, problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Check if Dockerfile exists
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    if !dockerfile_path.exists() {
        return Err(anyhow!(
            "Dockerfile not found at {:?}. Generate it first with the 'dockerfile' command.",
            dockerfile_path
        ));
    }

    info!("Using Dockerfile at {:?}", dockerfile_path);

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

    // Try building the Docker image, with retries if it fails
    let mut retry_count = 0;

    loop {
        // Run docker build command with streaming output
        println!(
            "\n=== Docker Build (Attempt {}/{}) ===",
            retry_count + 1,
            max_retries + 1
        );
        info!(
            "Running docker build (attempt {}/{})...",
            retry_count + 1,
            max_retries + 1
        );

        let mut child = Command::new("docker")
            .arg("build")
            .arg("-t")
            .arg(tag)
            .arg("-f")
            .arg(&dockerfile_path)
            .arg(docker_context_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to execute docker build command")?;

        // Stream stdout in real-time
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stdout_reader = BufReader::new(stdout);
        let stderr = child.stderr.take().expect("Failed to capture stderr");
        let stderr_reader = BufReader::new(stderr);

        // Collect stderr for potential error analysis
        let mut error_output = String::new();

        // Create a thread to read and display stdout
        let stdout_handle = std::thread::spawn(move || {
            for line in stdout_reader.lines() {
                if let Ok(line) = line {
                    println!("[stdout] {}", line);
                }
            }
        });

        // Read and display stderr, also collecting it for error analysis if needed
        for line in stderr_reader.lines() {
            if let Ok(line) = line {
                println!("[stderr] {}", line);
                error_output.push_str(&line);
                error_output.push('\n');
            }
        }

        // Wait for stdout thread to complete
        stdout_handle.join().expect("Failed to join stdout thread");

        // Wait for the command to complete and get the exit status
        let status = child
            .wait()
            .context("Failed to wait for docker build command")?;

        if status.success() {
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
        let updated_dockerfile =
            update_dockerfile_from_error(config, problem, &dockerfile_path, &error_output).await?;

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
        println!("Updated Dockerfile with LLM suggestions");
        info!("Updated Dockerfile with LLM suggestions");

        // Increment retry counter
        retry_count += 1;
    }
}
