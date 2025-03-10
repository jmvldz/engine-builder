use std::collections::{HashMap, HashSet};
use anyhow::{Result, Context};
use log::{info, warn};
use indicatif::{ProgressBar, ProgressStyle};
use futures::StreamExt;

use crate::config::RankingConfig;
use crate::models::problem::SWEBenchProblem;
use crate::models::ranking::{FileRanking, ProblemContext, RankedCodebaseFile, RelevantFileDataForPrompt};
use crate::models::relevance::RelevanceStatus;
use crate::utils::token_counter::count_tokens;
use crate::utils::trajectory_store::TrajectoryStore;
use crate::utils::json_utils::extract_last_json;
use crate::llm::client::{LLMClient, create_client};
use crate::llm::prompts::get_ranking_user_prompt;

/// Merge multiple rankings into a single ranking
fn merge_rankings(rankings: &[Vec<String>]) -> Vec<String> {
    // Get all unique files across rankings
    let all_files: HashSet<String> = rankings.iter()
        .flat_map(|ranking| ranking.iter().cloned())
        .collect();
    
    // Create a map from file to relative ranks
    let mut file_to_relative_ranks: HashMap<String, Vec<f64>> = HashMap::new();
    
    for file in all_files.iter() {
        file_to_relative_ranks.insert(file.clone(), Vec::new());
    }
    
    // Calculate relative ranks for each file
    for ranking in rankings {
        let total = ranking.len() as f64;
        
        // For files in this ranking, add their relative position
        for (i, file) in ranking.iter().enumerate() {
            file_to_relative_ranks.get_mut(file)
                .unwrap()
                .push(i as f64 / total);
        }
        
        // For files not in this ranking, pretend they were ranked at the end
        for file in all_files.iter() {
            if !ranking.contains(file) {
                file_to_relative_ranks.get_mut(file)
                    .unwrap()
                    .push(1.0);
            }
        }
    }
    
    // Calculate average relative rank for each file
    let mut files_with_rank: Vec<(String, f64)> = file_to_relative_ranks.into_iter()
        .map(|(file, ranks)| {
            let avg = ranks.iter().sum::<f64>() / ranks.len() as f64;
            (file, avg)
        })
        .collect();
    
    // Sort by rank (lowest average rank first)
    files_with_rank.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    
    // Return the sorted files
    files_with_rank.into_iter()
        .map(|(file, _)| file)
        .collect()
}

/// Get relevant files for a problem
fn get_relevant_files(
    trajectory_store: &TrajectoryStore,
    problem: &mut SWEBenchProblem,
) -> Result<Vec<RelevantFileDataForPrompt>> {
    let decisions = trajectory_store.load_relevance_decisions()?;
    
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
            "This file was marked as relevant to the issue: Determine how to run the server.".to_string()
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
            },
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
) -> Result<crate::llm::client::TokenUsage> {
    info!("Ranking files for problem: {}", problem.id);
    
    // Create a trajectory store for this problem
    let trajectory_store = TrajectoryStore::new(&config.trajectory_store_dir, problem)
        .context(format!("Failed to create trajectory store for problem: {}", problem.id))?;
    
    // Check if ranking already exists
    if trajectory_store.ranking_exists() {
        info!("Ranking already exists for problem: {}", problem.id);
        return Ok(crate::llm::client::TokenUsage::default());
    }
    
    // Get relevant files
    let relevant_files = get_relevant_files(&trajectory_store, problem)
        .context(format!("Failed to get relevant files for problem: {}", problem.id))?;
    
    if relevant_files.is_empty() {
        info!("No relevant files found for problem: {}", problem.id);
        return Ok(crate::llm::client::TokenUsage::default());
    }
    
    info!("Found {} relevant files for problem: {}", relevant_files.len(), problem.id);
    
    // Generate prompt
    let prompt = get_ranking_user_prompt(
        &problem.problem_statement,
        &relevant_files,
        120_000, // max_tokens
        60_000,  // target_tokens
    );
    
    // Set up progress bar for rankings
    let progress_bar = ProgressBar::new(config.num_rankings as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap()
    );
    
    // Get multiple rankings
    let mut rankings = Vec::new();
    let mut prompt_caching_usages = Vec::new();
    
    // Track total token usage
    let mut total_usage = crate::llm::client::TokenUsage::default();
    
    // Run multiple ranking requests in parallel
    let mut futures = futures::stream::iter(
        (0..config.num_rankings).map(|i| {
            let client_ref = &*client;
            let prompt_ref = &prompt;
            let progress_bar_ref = &progress_bar;
            
            async move {
                progress_bar_ref.set_message(format!("Running ranking {}", i + 1));
                
                let result = client_ref.completion(prompt_ref, config.max_tokens, config.temperature).await
                    .context(format!("Failed to get completion for ranking {}", i + 1));
                
                progress_bar_ref.inc(1);
                result
            }
        })
    ).buffer_unordered(config.max_workers);
    
    while let Some(result) = futures.next().await {
        match result {
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
                    },
                    Err(e) => {
                        warn!("Failed to extract ranking: {}", e);
                        
                        // Try a more direct approach - just look for file paths
                        let path_re = regex::Regex::new(r#"["']([^"']+\.[^"']+)["']"#).unwrap();
                        let matches: Vec<String> = path_re.captures_iter(&llm_response.content)
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
                                .filter(|line| line.contains("/") && !line.starts_with("```") && !line.starts_with("- "))
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
            },
            Err(e) => {
                warn!("Failed to get ranking: {}", e);
            }
        }
    }
    
    progress_bar.finish_with_message("Rankings completed");
    
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
    
    // Merge the rankings
    let final_ranking = merge_rankings(&rankings.iter().map(|r| r.ranking.clone()).collect::<Vec<_>>());
    
    // Convert to RankedCodebaseFile objects
    let path_to_token_count: HashMap<String, usize> = relevant_files.iter()
        .map(|file| (file.path.clone(), file.token_count))
        .collect();
    
    let ranked_files: Vec<RankedCodebaseFile> = final_ranking.into_iter()
        .filter_map(|path| {
            path_to_token_count.get(&path).map(|&tokens| {
                RankedCodebaseFile {
                    tokens,
                    path,
                }
            })
        })
        .collect();
    
    // Save the ranking
    let context = ProblemContext {
        model_rankings: rankings,
        ranked_files,
        prompt_caching_usages: prompt_caching_usages.into_iter()
            .map(|map| map)
            .collect(),
    };
    
    trajectory_store.save_ranking(context)
        .context(format!("Failed to save ranking for problem: {}", problem.id))?;
    
    info!("Ranking completed for problem: {}", problem.id);
    Ok(total_usage)
}

/// Process rankings for all problems
pub async fn process_rankings(config: RankingConfig, mut problem: SWEBenchProblem) -> Result<()> {
    info!("Starting file ranking");
    
    // Create the LLM client
    let client = create_client(&config.llm)
        .context("Failed to create LLM client")?;
    
    info!("Processing problem: {}", problem.id);
    
    match rank_problem_files(&mut problem, &config, &*client).await {
        Ok(token_usage) => {
            // Calculate and display cost
            let cost = client.calculate_cost(&token_usage);
            info!("Ranking LLM usage: {}", token_usage);
            info!("Ranking LLM cost: {}", cost);
            
            info!("File ranking completed");
            Ok(())
        },
        Err(e) => {
            warn!("Error ranking files for problem {}: {}", problem.id, e);
            Err(e)
        }
    }
}