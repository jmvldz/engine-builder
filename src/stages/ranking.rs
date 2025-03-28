use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use log::{info, warn};
use std::collections::HashMap;

use crate::config::{Config, RankingConfig};
use crate::llm::client::{create_client, LLMClient};
use crate::llm::prompts::get_ranking_user_prompt;
use crate::models::problem::SWEBenchProblem;
use crate::models::ranking::{
    FileRanking, ProblemContext, RankedCodebaseFile, RelevantFileDataForPrompt,
};
use crate::models::relevance::RelevanceStatus;
use crate::utils::json_utils::extract_last_json;
use crate::utils::token_counter::count_tokens;
use crate::utils::trajectory_store::TrajectoryStore;


/// Get relevant files for a problem
fn get_relevant_files(
    trajectory_store: &TrajectoryStore,
    problem: &mut SWEBenchProblem,
) -> Result<Vec<RelevantFileDataForPrompt>> {
    // Check for existence of relevance decisions file
    let relevance_path = trajectory_store.relevance_decisions_path();
    if !relevance_path.exists() {
        return Err(anyhow::anyhow!(
            "Relevance decisions file not found at: {:?}. Run the relevance step first with 'cargo run --release -- relevance'",
            relevance_path
        ));
    }

    // Check for existence of file patterns (to ensure file_selection was run)
    let file_patterns_path = trajectory_store.problem_dir().join("file_patterns.json");
    if !file_patterns_path.exists() {
        return Err(anyhow::anyhow!(
            "File patterns not found at: {:?}. Run the file_selection step first with 'cargo run --release -- file_selection'",
            file_patterns_path
        ));
    }

    let decisions = trajectory_store.load_relevance_decisions()?;
    if decisions.is_empty() {
        return Err(anyhow::anyhow!(
            "No relevance decisions found in {:?}. Run the relevance step first with 'cargo run --release -- relevance'",
            relevance_path
        ));
    }

    let mut relevant_files = Vec::new();

    for (path, decision) in decisions {
        // Skip non-relevant files
        if decision.status != RelevanceStatus::Relevant {
            continue;
        }

        // Get the summary (should be Some since the file is relevant)
        // If no summary is provided, create one from the message
        let summary = decision.summary.unwrap_or_else(|| {
            // Use the message as a fallback summary if no summary is provided
            "This file was marked as relevant to the issue."
                .to_string()
        });

        // Get the file content to count tokens, skip if file doesn't exist
        match problem.get_file(&path) {
            Ok(file) => {
                let token_count = count_tokens(&file.content);

                relevant_files.push(RelevantFileDataForPrompt {
                    path,
                    summary,
                    token_count,
                });
            }
            Err(e) => {
                warn!("Skipping missing or unreadable file {}: {}", path, e);
                // Continue with other files instead of failing
            }
        }
    }

    info!("Found {} relevant files", relevant_files.len());
    for file in &relevant_files {
        info!("Relevant file: {}", file.path);
    }

    Ok(relevant_files)
}

/// Rank files for a problem
async fn rank_problem_files(
    problem: &mut SWEBenchProblem,
    config: &RankingConfig,
    client: &dyn LLMClient,
    output_dir: &str,
) -> Result<crate::llm::client::TokenUsage> {
    info!("Ranking files for problem: {}", problem.id);

    // Create a trajectory store for this problem
    let trajectory_store =
        TrajectoryStore::new(output_dir, problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Check if ranking already exists
    if trajectory_store.ranking_exists() {
        info!("Ranking already exists for problem: {}", problem.id);
        return Ok(crate::llm::client::TokenUsage::default());
    }

    // Get relevant files
    let relevant_files = get_relevant_files(&trajectory_store, problem).context(format!(
        "Failed to get relevant files for problem: {}",
        problem.id
    ))?;

    if relevant_files.is_empty() {
        info!("No relevant files found for problem: {}", problem.id);
        return Ok(crate::llm::client::TokenUsage::default());
    }

    info!(
        "Found {} relevant files for problem: {}",
        relevant_files.len(),
        problem.id
    );

    // Generate prompt
    let prompt = get_ranking_user_prompt(
        &problem.problem_statement,
        &relevant_files,
        120_000, // max_tokens
        60_000,  // target_tokens
    );

    // Set up progress bar for the ranking
    let progress_bar = ProgressBar::new(1);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap(),
    );

    // Prepare for single ranking
    let mut rankings = Vec::new();
    let mut prompt_caching_usages = Vec::new();

    // Track total token usage
    let mut total_usage = crate::llm::client::TokenUsage::default();

    // Clone problem_id for use in async blocks
    let problem_id = problem.id.clone();
    
    progress_bar.set_message("Running ranking");

    // Add tracing metadata
    let metadata = serde_json::json!({
        "problem_id": problem_id,
        "stage": "ranking",
        "temperature": config.temperature,
    });

    // Execute a single ranking request

    let llm_result = client
        .completion_with_tracing(
            &prompt, 
            config.max_tokens, 
            config.temperature,
            None, // Use auto-generated trace ID
            Some(&format!("ranking_{}", problem_id)),
            Some(metadata),
        )
        .await
        .context("Failed to get ranking completion");
        
    progress_bar.inc(1);
    
    match llm_result {
        Ok(llm_response) => {
            // Add to the total token usage
            total_usage.prompt_tokens += llm_response.usage.prompt_tokens;
            total_usage.completion_tokens += llm_response.usage.completion_tokens;
            total_usage.total_tokens += llm_response.usage.total_tokens;

            // Extract the ranking
            warn!("Got response: {}", llm_response.content);
            match extract_last_json(&llm_response.content) {
                Ok(ranking) => {
                    info!("Successfully extracted ranking: {:?}", ranking);
                    rankings.push(FileRanking {
                        message: llm_response.content.clone(),
                        ranking,
                    });
                    // Add the usage for prompt caching
                    let usage_map = HashMap::new();
                    prompt_caching_usages.push(usage_map);
                }
                Err(e) => {
                    warn!("Failed to extract ranking: {}", e);

                    // Try a more direct approach - just look for file paths
                    let path_re = regex::Regex::new(r#"["']([^"']+\.[^"']+)["']"#).unwrap();
                    let matches: Vec<String> = path_re
                        .captures_iter(&llm_response.content)
                        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
                        .collect();

                    if !matches.is_empty() {
                        info!("Found file paths using regex: {:?}", matches);
                        rankings.push(FileRanking {
                            message: llm_response.content.clone(),
                            ranking: matches,
                        });
                        // Add the usage for prompt caching
                        let usage_map = HashMap::new();
                        prompt_caching_usages.push(usage_map);
                    } else {
                        // Still not working, try another approach - look for lines that start with file paths
                        let lines = llm_response.content.lines();
                        let file_paths: Vec<String> = lines
                            .filter(|line| {
                                line.contains("/")
                                    && !line.starts_with("```")
                                    && !line.starts_with("- ")
                            })
                            .map(|line| line.trim().to_string())
                            .collect();

                        if !file_paths.is_empty() {
                            info!("Found file paths by line parsing: {:?}", file_paths);
                            rankings.push(FileRanking {
                                message: llm_response.content.clone(),
                                ranking: file_paths,
                            });
                            // Add the usage for prompt caching
                            let usage_map = HashMap::new();
                            prompt_caching_usages.push(usage_map);
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to get ranking: {}", e);
        }
    }

    progress_bar.finish_with_message("Ranking completed");

    if rankings.is_empty() {
        info!("No valid rankings were obtained from the LLM. Falling back to using all relevant files in order of their path names...");
        // Just use all relevant files in alphabetical order as a fallback
        let all_files: Vec<String> = relevant_files.iter().map(|f| f.path.clone()).collect();
        rankings.push(FileRanking {
            message: "Fallback ranking - all files in alphabetical order".to_string(),
            ranking: all_files,
        });
        prompt_caching_usages.push(HashMap::new());
    }

    // Get the single ranking result
    let final_ranking = if !rankings.is_empty() {
        rankings[0].ranking.clone()
    } else {
        Vec::new()
    };

    // Convert to RankedCodebaseFile objects
    let path_to_token_count: HashMap<String, usize> = relevant_files
        .iter()
        .map(|file| (file.path.clone(), file.token_count))
        .collect();

    let ranked_files: Vec<RankedCodebaseFile> = final_ranking
        .into_iter()
        .filter_map(|path| {
            path_to_token_count
                .get(&path)
                .map(|&tokens| RankedCodebaseFile { tokens, path })
        })
        .collect();

    // Save the ranking
    let context = ProblemContext {
        model_rankings: rankings,
        ranked_files,
        prompt_caching_usages: prompt_caching_usages.into_iter().collect(),
    };

    trajectory_store.save_ranking(context).context(format!(
        "Failed to save ranking for problem: {}",
        problem_id
    ))?;

    info!("Ranking completed for problem: {}", problem_id);
    Ok(total_usage)
}

/// Process rankings for all problems
pub async fn process_rankings(config: &Config, mut problem: SWEBenchProblem) -> Result<()> {
    info!("Starting file ranking");

    // Create a trajectory store for this problem to check if previous steps were run
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, &problem)
        .context(format!("Failed to create trajectory store for problem: {}", problem.id))?;

    // Check if file selection step was run
    let file_patterns_path = trajectory_store.problem_dir().join("file_patterns.json");
    if !file_patterns_path.exists() {
        warn!("File patterns file not found at: {:?}", file_patterns_path);
        warn!("Make sure you have run the file_selection step first with: cargo run --release -- file_selection");
        return Err(anyhow::anyhow!("File selection step not run. Run 'cargo run --release -- file_selection' first."));
    }

    // Check if relevance step was run
    let relevance_path = trajectory_store.relevance_decisions_path();
    if !relevance_path.exists() {
        warn!("Relevance decisions file not found at: {:?}", relevance_path);
        warn!("Make sure you have run the relevance step first with: cargo run --release -- relevance");
        return Err(anyhow::anyhow!("Relevance step not run. Run 'cargo run --release -- relevance' first."));
    }

    // Create LLM config using the config's to_llm_config method
    let llm_config = config.to_llm_config(&config.ranking.model);

    // Create the LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    info!("Processing problem: {}", problem.id);

    let output_dir = config.get_trajectory_dir(&problem.id);
    match rank_problem_files(&mut problem, &config.ranking, &*client, &output_dir).await {
        Ok(token_usage) => {
            // Calculate and display cost
            let cost = client.calculate_cost(&token_usage);
            info!("Ranking LLM usage: {}", token_usage);
            info!("Ranking LLM cost: {}", cost);

            info!("File ranking completed");
            Ok(())
        }
        Err(e) => {
            warn!("Error ranking files for problem {}: {}", problem.id, e);
            Err(e)
        }
    }
}
