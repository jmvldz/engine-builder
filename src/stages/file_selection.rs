use anyhow::{Result, Context};
use log::{info, debug};
use regex::Regex;
use serde_json;
use std::fs;
use std::path::Path;

use crate::config::{RelevanceConfig, CodebaseConfig};
use crate::models::problem::SWEBenchProblem;
use crate::models::file::FilePatternSelection;
use crate::llm::client::create_client;
use crate::llm::prompts::{get_codebase_tree_user_prompt};
use crate::utils::trajectory_store::TrajectoryStore;

/// Parse the LLM response to extract the file patterns
pub fn parse_file_patterns(response: &str) -> Result<FilePatternSelection> {
    // Try to extract a JSON array from the response
    let json_pattern = Regex::new(r"```(?:json)?\s*(\[[\s\S]*?\])```").unwrap();
    
    if let Some(captures) = json_pattern.captures(response) {
        if let Some(json_str) = captures.get(1) {
            let patterns: Vec<String> = serde_json::from_str(json_str.as_str())
                .context("Failed to parse file patterns JSON")?;
            
            return Ok(FilePatternSelection::new(patterns));
        }
    }
    
    // If regex didn't match, try to find any list-like structure
    let fallback_pattern = Regex::new(r"\[([\s\S]*?)\]").unwrap();
    if let Some(captures) = fallback_pattern.captures(response) {
        if let Some(list_str) = captures.get(1) {
            // Try to split by commas and clean up each entry
            let patterns: Vec<String> = list_str.as_str()
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                // Remove quotes if present
                .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
                .collect();
            
            if !patterns.is_empty() {
                return Ok(FilePatternSelection::new(patterns));
            }
        }
    }
    
    // If all else fails, return an error
    Err(anyhow::anyhow!("Failed to parse file patterns from LLM response"))
}

/// Run the file selection process
pub async fn run_file_selection(config: &RelevanceConfig, codebase_config: &CodebaseConfig, problem: &SWEBenchProblem) -> Result<(FilePatternSelection, crate::llm::client::TokenUsage)> {
    info!("Starting file selection process");
    
    // Create the LLM client
    let client = create_client(&config.llm).await
        .context("Failed to create LLM client")?;
    
    // Initialize the problem to scan the codebase
    let mut configured_problem = problem.clone()
        .with_codebase_path(&codebase_config.path)
        .with_exclude_dirs(codebase_config.exclude_dirs.clone());
    
    configured_problem.initialize()
        .context("Failed to initialize problem")?;
    
    // Get all file paths for this problem
    let all_files = configured_problem.all_file_paths();
    info!("Found {} files in codebase", all_files.len());
    
    // Generate a tree representation of the codebase
    info!("Generating codebase tree structure");
    let tree_output = configured_problem.generate_tree();
    
    // Ask the LLM which files to process based on the tree
    info!("Asking LLM to select files for processing");
    let tree_prompt = get_codebase_tree_user_prompt(&configured_problem, &tree_output);
    
    let llm_response = client.completion(&tree_prompt, config.max_tokens, 0.0).await
        .context("Failed to get file selection from LLM")?;
    
    let file_patterns = parse_file_patterns(&llm_response.content)
        .context("Failed to parse file patterns from LLM response")?;
    
    info!("LLM selected {} file patterns for processing", file_patterns.patterns.len());
    for pattern in &file_patterns.patterns {
        debug!("Selected pattern: {}", pattern);
    }
    
    Ok((file_patterns, llm_response.usage))
}

/// Save file patterns to the trajectory store
pub fn save_file_patterns(trajectory_store_dir: &str, problem: &SWEBenchProblem, file_patterns: &FilePatternSelection) -> Result<()> {
    // Create the trajectory store dir if it doesn't exist
    let problem_dir = Path::new(trajectory_store_dir).join(&problem.id);
    fs::create_dir_all(&problem_dir)
        .context(format!("Failed to create trajectory store directory: {:?}", problem_dir))?;
    
    // Save file patterns
    let file_patterns_path = problem_dir.join("file_patterns.json");
    let file_patterns_json = serde_json::to_string_pretty(file_patterns)
        .context("Failed to serialize file patterns")?;
    
    fs::write(&file_patterns_path, file_patterns_json)
        .context(format!("Failed to write file patterns to: {:?}", file_patterns_path))?;
    
    info!("Saved file patterns to: {:?}", file_patterns_path);
    
    Ok(())
}

/// Process the codebase to select relevant files
pub async fn process_file_selection(config: RelevanceConfig, codebase_config: &CodebaseConfig, problem: SWEBenchProblem) -> Result<()> {
    info!("Starting file selection process");
    
    // Create a trajectory store for this problem (for future use)
    let _trajectory_store = TrajectoryStore::new(&config.trajectory_store_dir, &problem)
        .context(format!("Failed to create trajectory store for problem: {}", problem.id))?;
    
    // Run file selection and get token usage
    let (file_patterns, token_usage) = run_file_selection(&config, codebase_config, &problem).await?;
    
    // Create the LLM client to access pricing information
    let client = create_client(&config.llm).await
        .context("Failed to create LLM client")?;
    let cost = client.calculate_cost(&token_usage);
    
    // Output cost information
    info!("File selection LLM usage: {}", token_usage);
    info!("File selection LLM cost: {}", cost);
    
    // Save the results
    save_file_patterns(&config.trajectory_store_dir, &problem, &file_patterns)?;
    
    info!("File selection process completed");
    Ok(())
}