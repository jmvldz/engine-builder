use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;
use std::process::Command;

use crate::config::RankingConfig;
use crate::llm::client::create_client;
use crate::llm::prompts::{get_test_dockerfile_user_prompt, TEST_DOCKERFILE_SYSTEM_PROMPT};
use crate::models::problem::SWEBenchProblem;
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
    let max_files = 10;
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

    // Save to the trajectory store directory
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    fs::write(&dockerfile_path, &dockerfile_content).context(format!(
        "Failed to write test-focused Dockerfile to {:?}",
        dockerfile_path
    ))?;

    info!("Test-focused Dockerfile saved to {:?}", dockerfile_path);

    Ok(())
}

/// Extract Dockerfile content from LLM response
fn extract_dockerfile(response: &str) -> Result<String> {
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

/// Build a Docker image from the generated Dockerfile
pub fn build_docker_image(config: &RankingConfig, problem: &SWEBenchProblem, tag: &str) -> Result<()> {
    info!("Building Docker image with tag: {}", tag);

    // Create a trajectory store for this problem
    let trajectory_store = TrajectoryStore::new(&config.trajectory_store_dir, &problem).context(format!(
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
    let docker_context_dir = problem.get_codebase_path()
        .ok_or_else(|| anyhow!("Codebase path not set for problem"))?;
    info!("Using repository as Docker context: {:?}", docker_context_dir);

    // Run docker build command
    info!("Running docker build...");
    let output = Command::new("docker")
        .arg("build")
        .arg("-t")
        .arg(tag)
        .arg("-f")
        .arg(&dockerfile_path)
        .arg(docker_context_dir)
        .output()
        .context("Failed to execute docker build command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Docker build failed: {}", stderr));
    }

    let _stdout = String::from_utf8_lossy(&output.stdout);
    info!("Docker build completed successfully");
    info!("Image built with tag: {}", tag);

    Ok(())
}
