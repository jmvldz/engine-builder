use std::fs;
use std::path::Path;
use anyhow::{Result, Context};
use log::{info, warn};
use regex::Regex;

use crate::config::RankingConfig;
use crate::models::problem::SWEBenchProblem;
use crate::utils::trajectory_store::TrajectoryStore;
use crate::llm::client::create_client;
use crate::llm::prompts::{DOCKERFILE_SYSTEM_PROMPT, get_dockerfile_user_prompt};

/// Generate a Dockerfile based on ranked files
pub async fn generate_dockerfile(config: RankingConfig, mut problem: SWEBenchProblem) -> Result<()> {
    info!("Starting Dockerfile generation");
    
    // Create a trajectory store for this problem
    let trajectory_store = TrajectoryStore::new(&config.trajectory_store_dir, &problem)
        .context(format!("Failed to create trajectory store for problem: {}", problem.id))?;
    
    // Check if ranking exists
    if !trajectory_store.ranking_exists() {
        return Err(anyhow::anyhow!("No ranking found for problem: {}. Run the ranking step first.", problem.id));
    }
    
    // Load ranking
    let ranking = trajectory_store.load_ranking()
        .context(format!("Failed to load ranking for problem: {}", problem.id))?;
    
    // Get ranked files (limit to top N files to avoid context overflow)
    let max_files = 10;
    let ranked_files = ranking.ranked_files.iter()
        .take(max_files)
        .cloned()
        .collect::<Vec<_>>();
    
    if ranked_files.is_empty() {
        return Err(anyhow::anyhow!("No ranked files found for problem: {}", problem.id));
    }
    
    info!("Found {} ranked files", ranked_files.len());
    
    // Load file contents
    let mut file_contents = Vec::new();
    
    for file in &ranked_files {
        match problem.get_file(&file.path) {
            Ok(file_data) => {
                file_contents.push((file.path.clone(), file_data.content.clone()));
            },
            Err(e) => {
                warn!("Failed to read file {}: {}", file.path, e);
            }
        }
    }
    
    // Create LLM client
    let client = create_client(&config.llm).await
        .context("Failed to create LLM client")?;
    
    // Generate prompt
    let prompt = get_dockerfile_user_prompt(
        &problem.problem_statement,
        &ranked_files,
        &file_contents,
    );
    
    // Generate Dockerfile
    info!("Generating Dockerfile...");
    // Create a combined prompt with system and user instructions
    let combined_prompt = format!("System instructions:\n{}\n\nUser request:\n{}", 
                                 DOCKERFILE_SYSTEM_PROMPT, 
                                 prompt);
    
    let response = client.completion(
        &combined_prompt,
        config.max_tokens,
        config.temperature,
    ).await.context("Failed to generate Dockerfile")?;
    
    // Track usage
    let usage = response.usage;
    let cost = client.calculate_cost(&usage);
    info!("Dockerfile generation LLM usage: {}", usage);
    info!("Dockerfile generation LLM cost: {}", cost);
    
    // Extract Dockerfile content
    let dockerfile_content = extract_dockerfile(&response.content)
        .context("Failed to extract Dockerfile content from LLM response")?;
    
    // Save Dockerfile
    let config_dir = Path::new("data").join("dockerfiles");
    fs::create_dir_all(&config_dir)
        .context(format!("Failed to create directory: {:?}", config_dir))?;
    
    let dockerfile_path = config_dir.join(format!("{}_Dockerfile", problem.id));
    fs::write(&dockerfile_path, &dockerfile_content)
        .context(format!("Failed to write Dockerfile to {:?}", dockerfile_path))?;
    
    info!("Dockerfile generated and saved to {:?}", dockerfile_path);
    
    // Also create the actual Dockerfile in the current directory
    let output_path = Path::new("Dockerfile");
    fs::write(output_path, &dockerfile_content)
        .context(format!("Failed to write Dockerfile to {:?}", output_path))?;
    
    info!("Dockerfile also saved to {:?}", output_path);
    
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