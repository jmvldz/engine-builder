use anyhow::{Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;

use crate::config::{RelevanceConfig, ScriptConfig};
use crate::llm::client::create_client;
use crate::llm::prompts::{get_lint_script_user_prompt, get_test_script_user_prompt, 
                         LINT_SCRIPT_SYSTEM_PROMPT, TEST_SCRIPT_SYSTEM_PROMPT};
use crate::models::problem::SWEBenchProblem;
use crate::models::relevance::RelevanceStatus;
use crate::utils::trajectory_store::TrajectoryStore;

/// Extract shell script content from LLM response
pub fn extract_script(response: &str) -> Result<String> {
    // Try to extract content between ```sh and ``` tags
    let re = Regex::new(r"```sh\s*([\s\S]*?)\s*```").unwrap();
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
    warn!("Failed to extract script content from response, returning entire response");
    Ok(response.to_string())
}

/// Generate lint and test scripts based on relevance data
pub async fn generate_scripts(
    config: RelevanceConfig,
    script_config: ScriptConfig,
    mut problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting script generation from relevance data");

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
    let all_decisions = trajectory_store.load_all_relevance_decisions().context(format!(
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
    let client = create_client(&script_config.llm)
        .await
        .context("Failed to create LLM client")?;

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

    // Generate lint script
    info!("Generating lint script...");
    let lint_prompt = get_lint_script_user_prompt(&problem.problem_statement, &formatted_files, &file_contents);
    
    // Create a combined prompt with system and user instructions
    let combined_lint_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        LINT_SCRIPT_SYSTEM_PROMPT, lint_prompt
    );

    let lint_response = client
        .completion(&combined_lint_prompt, script_config.max_tokens, script_config.temperature)
        .await
        .context("Failed to generate lint script")?;

    // Track usage
    let lint_usage = lint_response.usage;
    let lint_cost = client.calculate_cost(&lint_usage);
    info!("Lint script generation LLM usage: {}", lint_usage);
    info!("Lint script generation LLM cost: {}", lint_cost);

    // Extract lint script content
    let lint_script_content = extract_script(&lint_response.content)
        .context("Failed to extract lint script content from LLM response")?;

    // Save to the trajectory store directory
    let lint_script_path = trajectory_store.problem_dir().join("lint-script.sh");
    fs::write(&lint_script_path, &lint_script_content).context(format!(
        "Failed to write lint script to {:?}",
        lint_script_path
    ))?;

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&lint_script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&lint_script_path, perms)?;
    }

    info!("Lint script saved to {:?}", lint_script_path);

    // Generate test script
    info!("Generating test script...");
    let test_prompt = get_test_script_user_prompt(&problem.problem_statement, &formatted_files, &file_contents);
    
    // Create a combined prompt with system and user instructions
    let combined_test_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_SCRIPT_SYSTEM_PROMPT, test_prompt
    );

    let test_response = client
        .completion(&combined_test_prompt, script_config.max_tokens, script_config.temperature)
        .await
        .context("Failed to generate test script")?;

    // Track usage
    let test_usage = test_response.usage;
    let test_cost = client.calculate_cost(&test_usage);
    info!("Test script generation LLM usage: {}", test_usage);
    info!("Test script generation LLM cost: {}", test_cost);

    // Extract test script content
    let test_script_content = extract_script(&test_response.content)
        .context("Failed to extract test script content from LLM response")?;

    // Save to the trajectory store directory
    let test_script_path = trajectory_store.problem_dir().join("test-script.sh");
    fs::write(&test_script_path, &test_script_content).context(format!(
        "Failed to write test script to {:?}",
        test_script_path
    ))?;

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&test_script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&test_script_path, perms)?;
    }

    info!("Test script saved to {:?}", test_script_path);

    Ok(())
}
