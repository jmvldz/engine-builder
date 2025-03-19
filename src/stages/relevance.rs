use anyhow::{Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info, warn};
use regex::Regex;
use std::fs;

use crate::config::{CodebaseConfig, Config, RelevanceConfig};
use crate::llm::client::{create_client, LLMClient};
use crate::llm::prompts::get_relevance_user_prompt;
use crate::models::exclusion::ExclusionConfig;
use crate::models::file::FilePatternSelection;
use crate::models::problem::SWEBenchProblem;
use crate::models::relevance::{RelevanceDecision, RelevanceStatus};
use crate::utils::token_counter::count_tokens;
use crate::utils::trajectory_store::TrajectoryStore;

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
        r"Final decision on the relevance(?:\s+of the file)?(?:\s+to the issue)?:\s*Not\s+Relevant",
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
    if response_lower.contains("not relevant")
        || response_lower.contains("file is not relevant")
        || response_lower.contains("is not relevant to the issue")
        || (response_lower.contains("relevance") && response_lower.contains("not relevant"))
    {
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
        r"File relevance:\s*(?:Yes|Relevant|Include)",
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
    if response_lower.contains("yes") && response_lower.contains("relevance")
        || response_lower.contains("is relevant")
        || response_lower.contains("highly relevant")
        || response_lower.contains("partially relevant")
        || (response_lower.contains("relevant") && !response_lower.contains("not relevant"))
    {
        // Try to find a summary paragraph
        let mut summary = "Summary not explicitly provided, but file was marked as relevant";

        // Look for common summary indicators
        let summary_indicators = [
            r"Summary:\s*(.*?)(?:\n\n|\n[A-Z]|$)",
            r"Summary of the File:\s*(.*?)(?:\n\n|\n[A-Z]|$)",
            r"Important Functions.*?(?:\n\n|\n[A-Z]|$)(.*?)(?:\n\n|\n[A-Z]|$)",
            r"Functions in the File.*?(?:\n\n|\n[A-Z]|$)(.*?)(?:\n\n|\n[A-Z]|$)",
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

        if output_part.contains("Not Relevant")
            || output_part.to_lowercase().contains("not relevant")
        {
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
fn should_process_file(file_path: &str, file_patterns: &FilePatternSelection) -> bool {
    file_patterns.matches(file_path)
}

/// Assess the relevance of a file to a problem
async fn assess_file_relevance(
    problem: &SWEBenchProblem,
    file_path: &str,
    file_content: &str,
    client: &dyn LLMClient,
    config: &RelevanceConfig,
    trajectory_store: &TrajectoryStore,
    trace_id: Option<&str>,
) -> Result<crate::llm::client::TokenUsage> {
    // Check if we already have a relevance decision for this file
    if trajectory_store.relevance_decision_exists(file_path) {
        debug!("Skipping already assessed file: {}", file_path);
        return Ok(crate::llm::client::TokenUsage::default());
    }

    // Check if the file is too large
    let token_count = count_tokens(file_content);
    if token_count > config.max_file_tokens {
        warn!("File too large ({}): {}", token_count, file_path);
        return Ok(crate::llm::client::TokenUsage::default());
    }

    // Generate the prompt
    let prompt = get_relevance_user_prompt(problem, file_path, file_content);

    // Add tracing metadata
    let metadata = serde_json::json!({
        "problem_id": problem.id,
        "file_path": file_path,
        "stage": "relevance",
        "token_count": token_count,
    });

    // Send the request to the LLM with tracing
    let llm_response = client
        .completion_with_tracing(
            &prompt, 
            config.max_tokens, 
            0.0,
            trace_id,
            Some(&format!("relevance_{}", file_path.replace("/", "_"))),
            Some(metadata),
        )
        .await
        .context(format!("Failed to get completion for file: {}", file_path))?;

    // Parse the response
    let relevance_decision = parse_response(&llm_response.content);

    // Save the decision
    trajectory_store
        .save_per_file_relevance_decision(file_path, relevance_decision)
        .context(format!(
            "Failed to save relevance decision for file: {}",
            file_path
        ))?;

    Ok(llm_response.usage)
}

use crate::stages::file_selection::{parse_file_patterns, save_file_patterns};
use std::path::Path;

/// Process the codebase to assess file relevance
pub async fn process_codebase(
    config: RelevanceConfig,
    codebase_config: &CodebaseConfig,
    problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting relevance assessment");
    
    // Create LLM config for Anthropic
    // Get the config with the API key
    let config_ref = std::env::var("CONFIG").unwrap_or_default();
    let global_config = Config::from_file(Some(&config_ref)).unwrap_or_else(|_| Config::default());
    
    let llm_config = crate::config::LLMConfig {
        model_type: "anthropic".to_string(),
        model: config.model.model.clone(),
        api_key: global_config.anthropic_api_key.clone(),
        base_url: None,
        timeout: config.model.timeout,
        max_retries: config.model.max_retries,
    };
    
    // Set up Langfuse trace for the entire relevance stage
    let trace_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "relevance",
        "max_workers": config.max_workers,
        "max_tokens": config.max_tokens,
        "model_type": "anthropic",
        "model": config.model.model,
    });
    
    // Create a new trace
    let trace_id = match crate::llm::langfuse::get_tracer() {
        Ok(tracer) => {
            match tracer.create_trace(&format!("relevance_{}", problem.id), Some(trace_metadata)).await {
                Ok(id) => {
                    debug!("Created Langfuse trace for relevance: {}", id);
                    Some(id)
                },
                Err(e) => {
                    warn!("Failed to create Langfuse trace: {}", e);
                    None
                },
            }
        },
        Err(_) => None,
    };

    // Create the LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    info!("Processing problem: {}", problem.id);

    // Load exclusion config from file
    info!(
        "Loading exclusion config from: {}",
        codebase_config.exclusions_path
    );
    let exclusion_config = match ExclusionConfig::from_file(&codebase_config.exclusions_path) {
        Ok(config) => {
            info!("Successfully loaded exclusion config with {} extensions, {} files, and {} directories to skip",
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

    // Setup the problem with codebase configuration
    let mut configured_problem = problem
        .with_codebase_path(&codebase_config.path)
        .with_exclusion_config(exclusion_config);

    // Initialize the problem to scan the codebase
    configured_problem
        .initialize()
        .context("Failed to initialize problem")?;

    // Create a trajectory store for this problem
    let config_ref = std::env::var("CONFIG").unwrap_or_default();
    let global_config = Config::from_file(Some(&config_ref)).unwrap_or_else(|_| Config::default());
    let trajectory_dir = global_config.get_trajectory_dir(&configured_problem.id);
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, &configured_problem)
        .context(format!(
            "Failed to create trajectory store for problem: {}",
            configured_problem.id
        ))?;

    // Load file patterns from previously generated response file
    let response_path = Path::new(&trajectory_dir)
        .join("codebase_tree_response.txt");

    // Read existing file selection response
    let response_content = fs::read_to_string(&response_path)
        .context(format!("Failed to read codebase tree response from {:?}. Run file_selection first.", response_path))?;

    // Parse file patterns from the existing response
    let file_patterns = parse_file_patterns(&response_content)
        .context("Failed to parse file patterns from response file")?;

    // Track total token usage across all LLM calls
    let mut total_usage = crate::llm::client::TokenUsage::default();

    // Save the file patterns for future reference
    save_file_patterns(
        &trajectory_dir,
        &configured_problem,
        &file_patterns,
    )?;

    // Get all file paths for this problem
    let all_files = configured_problem.all_file_paths();

    // Filter files based on the LLM's selection
    let relevant_files: Vec<_> = all_files
        .into_iter()
        .filter(|path| should_process_file(path, &file_patterns))
        .collect();

    info!(
        "Found {} matching files for problem: {}",
        relevant_files.len(),
        configured_problem.id
    );

    // Set up progress bar
    let progress_bar = ProgressBar::new(relevant_files.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap(),
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
    // Clone trace_id for use in async blocks
    let trace_id_for_async = trace_id.clone();
    
    let futures =
        futures::stream::iter(file_contents.into_iter().map(|(file_path, file_content)| {
            let file_path_clone = file_path.clone();
            let client_ref = &*client;
            let config_ref = &config;
            let trajectory_store_ref = &trajectory_store;
            let problem_ref = &configured_problem;
            let progress_bar_ref = &progress_bar;
            let trace_id_local = trace_id_for_async.clone();

            async move {
                if file_content.is_empty() {
                    progress_bar_ref.inc(1);
                    progress_bar_ref.set_message(format!("Skipped (empty): {}", file_path_clone));
                    return Ok(crate::llm::client::TokenUsage::default());
                }

                let result = assess_file_relevance(
                    problem_ref,
                    &file_path_clone,
                    &file_content,
                    client_ref,
                    config_ref,
                    trajectory_store_ref,
                    trace_id_local.as_deref(),
                )
                .await;

                if let Err(e) = &result {
                    warn!("Error assessing file {}: {}", file_path_clone, e);
                }

                progress_bar_ref.inc(1);
                progress_bar_ref.set_message(format!("Processed: {}", file_path_clone));

                result
            }
        }))
        .buffer_unordered(config.max_workers);

    // Collect all the futures results
    let usage_results = futures.collect::<Vec<_>>().await;

    progress_bar.finish_with_message(format!("Completed problem: {}", configured_problem.id));

    // Aggregate token usage across all relevance assessments
    for result in usage_results {
        if let Ok(usage) = result {
            total_usage.prompt_tokens += usage.prompt_tokens;
            total_usage.completion_tokens += usage.completion_tokens;
            total_usage.total_tokens += usage.total_tokens;
        }
    }

    // Calculate and display cost
    let cost = client.calculate_cost(&total_usage);
    info!("Relevance assessment LLM usage: {}", total_usage);
    info!("Relevance assessment LLM cost: {}", cost);

    info!("Relevance assessment completed");
    Ok(())
}
