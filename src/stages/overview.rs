use anyhow::{Context, Result};
use log::info;
use regex::Regex;

use crate::config::Config;
use crate::models::overview::OverviewData;
use crate::models::problem::SWEBenchProblem;
use crate::utils::trajectory_store::TrajectoryStore;

/// Generate an overview document that summarizes the reasoning across all stages
pub async fn generate_overview(config: &Config, problem: &SWEBenchProblem) -> Result<()> {
    info!("Starting overview generation for problem: {}", problem.id);

    // Get the trajectory directory for this problem
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, problem).context(format!(
        "Failed to create trajectory store for problem: {}",
        problem.id
    ))?;

    // Initialize overview data
    let mut overview = OverviewData::new(&problem.id, &problem.problem_statement);

    // Get all reasoning files in the reasoning directory
    let reasoning_files = trajectory_store.list_reasoning_files().context(format!(
        "Failed to list reasoning files for problem: {}",
        problem.id
    ))?;

    info!("Found {} reasoning files", reasoning_files.len());

    // Process each reasoning file and add to overview
    let file_selection_re = Regex::new(r"file_selection_.*\.json$").unwrap();
    let relevance_re = Regex::new(r"relevance_.*_(.+)\.json$").unwrap();
    let ranking_re = Regex::new(r"ranking_.*\.json$").unwrap();
    let setup_script_re = Regex::new(r"setup_script_.*\.json$").unwrap();
    let lint_script_re = Regex::new(r"lint_script_.*\.json$").unwrap();
    let test_script_re = Regex::new(r"test_script_.*\.json$").unwrap();
    let single_test_script_re = Regex::new(r"single_test_script_.*\.json$").unwrap();
    let dockerfile_re = Regex::new(r"dockerfile_.*\.json$").unwrap();
    let dockerfile_error_re = Regex::new(r"dockerfile_error_(\d+)\.json$").unwrap();
    let test_script_error_re = Regex::new(r"test_script_error_(\d+)\.json$").unwrap();

    for file_path in reasoning_files {
        if let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) {
            // Process file according to its pattern
            if file_selection_re.is_match(file_name) {
                if let Ok((reasoning, _)) =
                    trajectory_store.load_stage_reasoning("file_selection", "")
                {
                    overview.file_selection_reasoning = Some(reasoning);
                }
            } else if let Some(captures) = relevance_re.captures(file_name) {
                if let Some(file_path_match) = captures.get(1) {
                    let file_path_str = file_path_match.as_str();
                    if let Ok((reasoning, _)) = trajectory_store
                        .load_stage_reasoning("relevance", &format!("_{}", file_path_str))
                    {
                        overview
                            .relevance_reasoning
                            .insert(file_path_str.to_string(), reasoning);
                    }
                }
            } else if ranking_re.is_match(file_name) {
                if let Ok((reasoning, _)) = trajectory_store.load_stage_reasoning("ranking", "") {
                    overview.ranking_reasoning = Some(reasoning);
                }
            } else if setup_script_re.is_match(file_name) {
                if let Ok((reasoning, _)) =
                    trajectory_store.load_stage_reasoning("setup_script", "")
                {
                    overview.setup_script_reasoning = Some(reasoning);
                }
            } else if lint_script_re.is_match(file_name) {
                if let Ok((reasoning, _)) = trajectory_store.load_stage_reasoning("lint_script", "")
                {
                    overview.lint_script_reasoning = Some(reasoning);
                }
            } else if test_script_re.is_match(file_name)
                && !file_name.contains("single_test_script")
                && !file_name.contains("test_script_error")
            {
                if let Ok((reasoning, _)) = trajectory_store.load_stage_reasoning("test_script", "")
                {
                    overview.test_script_reasoning = Some(reasoning);
                }
            } else if single_test_script_re.is_match(file_name) {
                if let Ok((reasoning, _)) =
                    trajectory_store.load_stage_reasoning("single_test_script", "")
                {
                    overview.single_test_script_reasoning = Some(reasoning);
                }
            } else if dockerfile_re.is_match(file_name) && !file_name.contains("dockerfile_error") {
                if let Ok((reasoning, _)) = trajectory_store.load_stage_reasoning("dockerfile", "")
                {
                    overview.dockerfile_reasoning = Some(reasoning);
                }
            } else if let Some(captures) = dockerfile_error_re.captures(file_name) {
                if let Some(attempt_match) = captures.get(1) {
                    let attempt = attempt_match.as_str();
                    if let Ok((reasoning, _)) = trajectory_store
                        .load_stage_reasoning("dockerfile_error", &format!("_{}", attempt))
                    {
                        overview
                            .dockerfile_error_reasoning
                            .insert(attempt.to_string(), reasoning);
                    }
                }
            } else if let Some(captures) = test_script_error_re.captures(file_name) {
                if let Some(attempt_match) = captures.get(1) {
                    let attempt = attempt_match.as_str();
                    if let Ok((reasoning, _)) = trajectory_store
                        .load_stage_reasoning("test_script_error", &format!("_{}", attempt))
                    {
                        overview
                            .test_script_error_reasoning
                            .insert(attempt.to_string(), reasoning);
                    }
                }
            }
        }
    }

    // Save the detailed overview data
    trajectory_store
        .save_overview_data(&overview)
        .context(format!(
            "Failed to save overview data for problem: {}",
            problem.id
        ))?;

    // Generate and save the summarized version
    info!("Generating summarized overview...");
    match overview.to_summarized_markdown(config).await {
        Ok(summarized_content) => {
            // Save the summarized markdown
            let summarized_path = trajectory_store.problem_dir().join("overview_summary.md");
            std::fs::write(&summarized_path, &summarized_content).context(format!(
                "Failed to write summarized overview to {:?}",
                summarized_path
            ))?;
            info!("Summarized overview saved to {:?}", summarized_path);
        }
        Err(e) => {
            info!("Failed to generate summarized overview: {}", e);
            info!("Only the detailed overview is available");
        }
    }

    info!("Overview generation completed");
    info!(
        "Detailed overview saved to {:?}",
        trajectory_store.overview_md_path()
    );

    Ok(())
}

/// Function to add reasoning from an LLM response to the trajectory store
pub fn save_reasoning(
    config: &Config,
    problem: &SWEBenchProblem,
    stage: &str,
    suffix: &str,
    reasoning: &str,
    metadata: Option<serde_json::Value>,
) -> Result<()> {
    // Get the trajectory directory for this problem
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, problem).context(format!(
        "Failed to create trajectory store for problem: {}",
        problem.id
    ))?;

    // Save the reasoning
    trajectory_store
        .save_stage_reasoning(stage, suffix, reasoning, metadata)
        .context(format!(
            "Failed to save reasoning for stage {} of problem: {}",
            stage, problem.id
        ))?;

    Ok(())
}
