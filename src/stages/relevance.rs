use std::path::Path;
use anyhow::{Result, Context};
use log::{info, debug, warn};
use regex::Regex;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};

use crate::config::{RelevanceConfig, CodebaseConfig};
use crate::models::problem::SWEBenchProblem;
use crate::models::relevance::{RelevanceDecision, RelevanceStatus};
use crate::utils::token_counter::count_tokens;
use crate::utils::trajectory_store::TrajectoryStore;
use crate::llm::client::{LLMClient, create_client};
use crate::llm::prompts::{RELEVANCE_SYSTEM_PROMPT, get_relevance_user_prompt};

/// Parse the LLM response to extract the relevance decision
fn parse_response(response: &str) -> RelevanceDecision {
    // More flexible patterns that match what the model is actually outputting
    
    // Check for "Not Relevant" in various formats
    let not_relevant_patterns = [
        r"RELEVANCE:\s*Not\s+Relevant", 
        r"Relevance\s*(?:of\s*(?:File|the\s*file))?\s*(?:to\s*(?:Issue|the\s*issue))?:\s*Not\s+Relevant",
        r"Not\s+[Rr]elevant",
        r"(?:file|relevance)(?:\s+is|\s*:)\s*Not\s+Relevant",
        r"(?:^|[:\n])\s*The file is not relevant",
        r"Relevance(?:\s+Decision)?:\s*Not Relevant",
        r"File relevance:\s*Not relevant",
        r"Final decision on the relevance(?:\s+of the file)?(?:\s+to the issue)?:\s*Not\s+Relevant"
    ];
    
    // Check response line by line for patterns
    let response_lower = response.to_lowercase();
    
    // First check for not relevant
    for pattern in not_relevant_patterns {
        if Regex::new(pattern).unwrap().is_match(response) {
            return RelevanceDecision {
                message: response.to_string(),
                status: RelevanceStatus::NotRelevant,
                summary: None,
            };
        }
    }
    
    // If response contains "not relevant" in various common forms
    if response_lower.contains("not relevant") || 
       response_lower.contains("file is not relevant") ||
       response_lower.contains("is not relevant to the issue") ||
       (response_lower.contains("relevance") && response_lower.contains("not relevant")) {
        return RelevanceDecision {
            message: response.to_string(),
            status: RelevanceStatus::NotRelevant,
            summary: None,
        };
    }
    
    // Check for "Relevant" with a summary in various formats
    let relevant_patterns = [
        r"RELEVANCE:\s*(?:Yes|Relevant)\s*\nSUMMARY:(.*?)(?:\n|$)",
        r"Relevance\s*(?:of\s*(?:File|the\s*file))?\s*(?:to\s*(?:Issue|the\s*issue))?:\s*(?:Yes|Relevant)",
        r"(?:file|relevance)(?:\s+is|\s*:)\s*(?:Yes|Relevant)",
        r"Relevance (?:Decision|Reasoning):\s*(?:Yes|Relevant|Include)",
        r"Final Decision on Relevance:\s*(?:Yes|Relevant|Include)",
        r"File relevance:\s*(?:Yes|Relevant|Include)"
    ];
    
    // First check strict patterns with summary capture
    for pattern in relevant_patterns {
        if let Some(captures) = Regex::new(pattern).unwrap().captures(response) {
            if let Some(summary) = captures.get(1) {
                return RelevanceDecision {
                    message: response.to_string(),
                    status: RelevanceStatus::Relevant,
                    summary: Some(summary.as_str().trim().to_string()),
                };
            }
        }
    }
    
    // Check if file is marked as relevant
    if response_lower.contains("yes") && response_lower.contains("relevance") ||
       response_lower.contains("is relevant") ||
       response_lower.contains("highly relevant") ||
       response_lower.contains("partially relevant") ||
       (response_lower.contains("relevant") && !response_lower.contains("not relevant")) {
        
        // Try to find a summary paragraph
        let mut summary = "Summary not explicitly provided, but file was marked as relevant";
        
        // Look for common summary indicators
        let summary_indicators = [
            r"Summary:\s*(.*?)(?:\n\n|\n[A-Z]|$)",
            r"Summary of the File:\s*(.*?)(?:\n\n|\n[A-Z]|$)",
            r"Important Functions.*?(?:\n\n|\n[A-Z]|$)(.*?)(?:\n\n|\n[A-Z]|$)",
            r"Functions in the File.*?(?:\n\n|\n[A-Z]|$)(.*?)(?:\n\n|\n[A-Z]|$)"
        ];
        
        for pattern in summary_indicators {
            if let Some(captures) = Regex::new(pattern).unwrap().captures(response) {
                if let Some(matched_summary) = captures.get(1) {
                    summary = matched_summary.as_str().trim();
                    break;
                }
            }
        }
        
        return RelevanceDecision {
            message: response.to_string(),
            status: RelevanceStatus::Relevant,
            summary: Some(summary.to_string()),
        };
    }
    
    // Additional check for "Output:" header followed by a definitive relevance
    if let Some(output_start) = response.find("Output:") {
        let output_part = &response[output_start..];
        
        if output_part.contains("Not Relevant") || 
           output_part.to_lowercase().contains("not relevant") {
            return RelevanceDecision {
                message: response.to_string(),
                status: RelevanceStatus::NotRelevant,
                summary: None,
            };
        } else if output_part.contains("Relevant") && !output_part.contains("Not Relevant") {
            return RelevanceDecision {
                message: response.to_string(),
                status: RelevanceStatus::Relevant,
                summary: Some("Summary extracted from Output section".to_string()),
            };
        }
    }
    
    // If we couldn't parse properly, return a parse error
    RelevanceDecision {
        message: response.to_string(),
        status: RelevanceStatus::ParseError,
        summary: None,
    }
}

/// Check if a file should be included in the relevance assessment
fn should_include_file(file_path: &str, problem: &SWEBenchProblem) -> bool {
    // Check if the file extension is in the configured include_extensions
    if !problem.include_extensions.is_empty() {
        if let Some(extension) = Path::new(file_path).extension() {
            if let Some(ext_str) = extension.to_str() {
                if !problem.include_extensions.contains(&ext_str.to_string()) {
                    return false;
                }
            } else {
                return false; // Can't parse extension
            }
        } else {
            return false; // No extension
        }
    }
    
    // Exclude test directories as a heuristic
    let path = Path::new(file_path);
    if let Some(first_dir) = path.iter().next() {
        let dir_name = first_dir.to_string_lossy();
        if dir_name == "test" || dir_name == "tests" {
            return false;
        }
    }
    
    true
}

/// Assess the relevance of a file to a problem
async fn assess_file_relevance(
    problem: &SWEBenchProblem,
    file_path: &str,
    file_content: &str,
    client: &dyn LLMClient,
    config: &RelevanceConfig,
    trajectory_store: &TrajectoryStore,
) -> Result<()> {
    // Check if we already have a relevance decision for this file
    if trajectory_store.relevance_decision_exists(file_path) {
        debug!("Skipping already assessed file: {}", file_path);
        return Ok(());
    }
    
    // Check if the file is too large
    let token_count = count_tokens(file_content);
    if token_count > config.max_file_tokens {
        warn!("File too large ({}): {}", token_count, file_path);
        return Ok(());
    }
    
    // Generate the prompt
    let prompt = get_relevance_user_prompt(problem, file_path, file_content);
    
    // Send the request to the LLM
    let _messages = vec![
        ("system", RELEVANCE_SYSTEM_PROMPT),
        ("user", &prompt),
    ];
    
    let response = client.completion(&prompt, config.max_tokens, 0.0).await
        .context(format!("Failed to get completion for file: {}", file_path))?;
    
    // Parse the response
    let relevance_decision = parse_response(&response);
    
    // Save the decision
    trajectory_store.save_per_file_relevance_decision(file_path, relevance_decision)
        .context(format!("Failed to save relevance decision for file: {}", file_path))?;
    
    Ok(())
}

/// Process the codebase to assess file relevance
pub async fn process_codebase(config: RelevanceConfig, codebase_config: &CodebaseConfig, problem: SWEBenchProblem) -> Result<()> {
    info!("Starting relevance assessment");
    
    // Create the LLM client
    let client = create_client(&config.llm)
        .context("Failed to create LLM client")?;
    
    info!("Processing problem: {}", problem.id);
    
    // Setup the problem with codebase configuration
    let mut configured_problem = problem
        .with_codebase_path(&codebase_config.path)
        .with_extensions(codebase_config.include_extensions.clone())
        .with_exclude_dirs(codebase_config.exclude_dirs.clone());
    
    // Initialize the problem to scan the codebase
    configured_problem.initialize()
        .context("Failed to initialize problem")?;
    
    // Create a trajectory store for this problem
    let trajectory_store = TrajectoryStore::new(&config.trajectory_store_dir, &configured_problem)
        .context(format!("Failed to create trajectory store for problem: {}", configured_problem.id))?;
    
    // Get all file paths for this problem
    let file_paths = configured_problem.all_file_paths();
    info!("Found {} files in codebase", file_paths.len());
    
    let relevant_files: Vec<_> = file_paths.into_iter()
        .filter(|path| should_include_file(path, &configured_problem))
        .collect();
    
    info!("Found {} relevant files after filtering: {}", relevant_files.len(), configured_problem.id);
    
    // Set up progress bar
    let progress_bar = ProgressBar::new(relevant_files.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap()
    );
    
    // Prepare file contents before creating futures
    let mut file_contents = Vec::new();
    for file_path in relevant_files {
        let file_content = match configured_problem.get_file(&file_path) {
            Ok(file) => file.content.clone(),
            Err(e) => {
                warn!("Error reading file {}: {}", file_path, e);
                String::new()
            }
        };
        file_contents.push((file_path, file_content));
    }
    
    // Create a fixed-size buffer of futures to limit concurrency
    let futures = futures::stream::iter(
        file_contents.into_iter().map(|(file_path, file_content)| {
            let file_path_clone = file_path.clone();
            let client_ref = &*client;
            let config_ref = &config;
            let trajectory_store_ref = &trajectory_store;
            let problem_ref = &configured_problem;
            let progress_bar_ref = &progress_bar;
            
            async move {
                if file_content.is_empty() {
                    progress_bar_ref.inc(1);
                    progress_bar_ref.set_message(format!("Skipped (empty): {}", file_path_clone));
                    return Ok(());
                }
                
                let result = assess_file_relevance(
                    problem_ref,
                    &file_path_clone,
                    &file_content,
                    client_ref,
                    config_ref,
                    trajectory_store_ref,
                ).await;
                
                if let Err(e) = &result {
                    warn!("Error assessing file {}: {}", file_path_clone, e);
                }
                
                progress_bar_ref.inc(1);
                progress_bar_ref.set_message(format!("Processed: {}", file_path_clone));
                
                result
            }
        })
    ).buffer_unordered(config.max_workers);
    
    // Process all futures
    futures.for_each(|_| async {}).await;
    
    progress_bar.finish_with_message(format!("Completed problem: {}", configured_problem.id));
    
    info!("Relevance assessment completed");
    Ok(())
}