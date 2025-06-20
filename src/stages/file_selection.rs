use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, warn};
use regex::Regex;
use serde_json;
use std::fs;
use std::path::Path;

use crate::config::{CodebaseConfig, Config, RelevanceConfig};
use crate::llm::client::create_client;
use crate::llm::prompts::get_codebase_tree_user_prompt;
use crate::models::exclusion::ExclusionConfig;
use crate::models::file::FilePatternSelection;
use crate::models::problem::SWEBenchProblem;
use crate::utils::trajectory_store::TrajectoryStore;

/// Parse the LLM response to extract the file patterns
pub fn parse_file_patterns(response: &str) -> Result<FilePatternSelection> {
    debug!("Parsing file patterns from LLM response");

    // Try to extract a JSON array from the response
    let json_pattern = Regex::new(r"```(?:json)?\s*(\[[\s\S]*?\])```").unwrap();

    if let Some(captures) = json_pattern.captures(response) {
        if let Some(json_str) = captures.get(1) {
            debug!("Found JSON pattern in response");
            let json_content = json_str.as_str();
            debug!("Extracted JSON content: {}", json_content);

            match serde_json::from_str::<Vec<String>>(json_content) {
                Ok(patterns) => {
                    debug!(
                        "Successfully parsed {} file patterns from JSON",
                        patterns.len()
                    );
                    return Ok(FilePatternSelection::new(patterns));
                }
                Err(e) => {
                    warn!("Failed to parse JSON content: {}", e);
                    // Continue to fallback pattern
                }
            }
        }
    } else {
        debug!("No JSON pattern found in response, trying fallback pattern");
    }

    // If regex didn't match, try to find any list-like structure
    let fallback_pattern = Regex::new(r"\[([\s\S]*?)\]").unwrap();
    if let Some(captures) = fallback_pattern.captures(response) {
        if let Some(list_str) = captures.get(1) {
            debug!("Found array-like pattern in response");
            // Try to split by commas and clean up each entry
            let patterns: Vec<String> = list_str
                .as_str()
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                // Remove quotes if present
                .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
                .collect();

            if !patterns.is_empty() {
                debug!(
                    "Successfully parsed {} file patterns using fallback method",
                    patterns.len()
                );
                return Ok(FilePatternSelection::new(patterns));
            } else {
                warn!("Found array-like pattern but no valid patterns after cleaning");
            }
        }
    } else {
        warn!("No array-like pattern found in response");
    }

    // For debugging purposes, log a portion of the response
    if response.len() > 500 {
        warn!("Response excerpt (first 500 chars): {}", &response[..500]);
    } else {
        warn!("Full response: {}", response);
    }

    // If all else fails, return an error
    Err(anyhow::anyhow!(
        "Failed to parse file patterns from LLM response"
    ))
}

/// Run the file selection process
pub async fn run_file_selection(
    config: &Config,
    relevance_config: &RelevanceConfig,
    codebase_config: &CodebaseConfig,
    problem: &SWEBenchProblem,
    trajectory_dir: &str,
) -> Result<(FilePatternSelection, crate::llm::client::TokenUsage)> {
    debug!("Starting file selection process");

    // Get the LLM config which uses the top-level model as fallback
    let llm_config = config.to_llm_config(&relevance_config.model);

    // Create the LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    // Load exclusion config from file
    debug!(
        "Loading exclusion config from: {}",
        codebase_config.exclusions_path
    );
    let exclusion_config = match ExclusionConfig::from_file(&codebase_config.exclusions_path) {
        Ok(config) => {
            debug!("Successfully loaded exclusion config with {} extensions, {} files, and {} directories to skip",
                  config.extensions_to_skip.len(),
                  config.files_to_skip.len(),
                  config.directories_to_skip.len());
            config
        }
        Err(e) => {
            warn!("Failed to load exclusion config: {}, using default", e);
            ExclusionConfig::default()
        }
    };

    // Initialize the problem to scan the codebase
    let mut configured_problem = problem
        .clone()
        .with_codebase_path(&codebase_config.path)
        .with_exclusion_config(exclusion_config);

    configured_problem
        .initialize()
        .context("Failed to initialize problem")?;

    // Get all file paths for this problem
    let all_files = configured_problem.all_file_paths();
    debug!("Found {} files in codebase", all_files.len());

    // Generate a tree representation of the codebase
    debug!("Generating codebase tree structure");
    let tree_output = configured_problem.generate_tree();

    // Use the trajectory dir provided by the caller (via config from main)

    // Save the tree output to a file
    let tree_path = Path::new(&trajectory_dir).join("codebase_tree.txt");

    // Create the directory if it doesn't exist
    if let Some(parent) = tree_path.parent() {
        fs::create_dir_all(parent).context(format!(
            "Failed to create directory for tree output: {:?}",
            parent
        ))?;
    }

    // Write the tree output to a file
    fs::write(&tree_path, &tree_output)
        .context(format!("Failed to write tree output to: {:?}", tree_path))?;

    debug!("Saved codebase tree to: {:?}", tree_path);

    // Ask the LLM which files to process based on the tree
    debug!("Asking LLM to select files for processing");
    let tree_prompt = get_codebase_tree_user_prompt(&configured_problem, &tree_output);

    // Save the prompt to a file
    let prompt_path = Path::new(&trajectory_dir).join("codebase_tree_prompt.txt");

    // Write the prompt to a file
    fs::write(&prompt_path, &tree_prompt)
        .context(format!("Failed to write prompt to: {:?}", prompt_path))?;

    debug!("Saved prompt to: {:?}", prompt_path);

    // Add tracing metadata
    let metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "file_selection",
        "temperature": 0.0,
        "files_count": all_files.len(),
    });

    let llm_response = client
        .completion_with_tracing(
            &tree_prompt,
            relevance_config.max_tokens,
            0.0,  // Fixed temperature
            None, // Auto-generate trace ID
            Some(&format!("file_selection_{}", problem.id)),
            Some(metadata),
        )
        .await
        .context("Failed to get file selection from LLM")?;

    // Save the LLM response to a file
    let response_path = Path::new(trajectory_dir).join("codebase_tree_response.txt");

    // Write the LLM response to a file
    fs::write(&response_path, &llm_response.content).context(format!(
        "Failed to write LLM response to: {:?}",
        response_path
    ))?;

    debug!("Saved LLM response to: {:?}", response_path);

    let file_patterns = parse_file_patterns(&llm_response.content)
        .context("Failed to parse file patterns from LLM response")?;

    debug!(
        "LLM selected {} file patterns for processing",
        file_patterns.patterns.len()
    );
    for pattern in &file_patterns.patterns {
        debug!("Selected pattern: {}", pattern);
    }

    Ok((file_patterns, llm_response.usage))
}

/// Save file patterns to the trajectory store
pub fn save_file_patterns(
    trajectory_dir: &str,
    _problem: &SWEBenchProblem,
    file_patterns: &FilePatternSelection,
) -> Result<()> {
    // Create the trajectory store dir if it doesn't exist
    let problem_dir = Path::new(trajectory_dir);
    fs::create_dir_all(&problem_dir).context(format!(
        "Failed to create trajectory store directory: {:?}",
        problem_dir
    ))?;

    // Save file patterns
    let file_patterns_path = problem_dir.join("file_patterns.json");
    let file_patterns_json =
        serde_json::to_string_pretty(file_patterns).context("Failed to serialize file patterns")?;

    fs::write(&file_patterns_path, file_patterns_json).context(format!(
        "Failed to write file patterns to: {:?}",
        file_patterns_path
    ))?;

    debug!("Saved file patterns to: {:?}", file_patterns_path);

    Ok(())
}

/// Process the codebase to select relevant files
pub async fn process_file_selection(
    config: &Config,
    codebase_config: &CodebaseConfig,
    problem: SWEBenchProblem,
    trajectory_dir: &str,
) -> Result<()> {
    // Set up progress bar for file selection process
    let progress_bar = ProgressBar::new(4); // 4 steps: init, LLM call, save patterns, finalize
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap(),
    );
    progress_bar.set_message("Initializing file selection");

    // Create a trajectory store for this problem (for future use)
    let _trajectory_store = TrajectoryStore::new(trajectory_dir, &problem).context(format!(
        "Failed to create trajectory store for problem: {}",
        problem.id
    ))?;

    progress_bar.inc(1);
    progress_bar.set_message("Requesting file pattern selection from LLM");

    // Run file selection and get token usage
    let (file_patterns, token_usage) = run_file_selection(
        config,
        &config.relevance,
        codebase_config,
        &problem,
        trajectory_dir,
    )
    .await?;

    progress_bar.inc(1);
    progress_bar.set_message("Saving file patterns");

    // Create the LLM client to access pricing information
    let client = create_client(&config.to_llm_config(&config.relevance.model))
        .await
        .context("Failed to create LLM client")?;

    let cost = client.calculate_cost(&token_usage);

    // Output cost information through logs (not stdout)
    debug!("File selection LLM usage: {}", token_usage);
    debug!("File selection LLM cost: {}", cost);

    // Save the results
    save_file_patterns(trajectory_dir, &problem, &file_patterns)?;

    progress_bar.inc(1);
    progress_bar.finish_with_message(format!("File selection completed for problem: {}", problem.id));
    
    debug!("File selection process completed");
    Ok(())
}
