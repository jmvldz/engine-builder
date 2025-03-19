use anyhow::{Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;
use std::path::Path;

use crate::config::{Config, RankingConfig, RelevanceConfig, ScriptConfig};
use crate::llm::client::create_client;
use crate::llm::prompts::{get_lint_script_user_prompt, get_test_script_user_prompt, 
                         get_setup_script_user_prompt,
                         LINT_SCRIPT_SYSTEM_PROMPT, TEST_SCRIPT_SYSTEM_PROMPT,
                         SETUP_SCRIPT_SYSTEM_PROMPT};
use crate::models::problem::SWEBenchProblem;
use crate::models::relevance::RelevanceStatus;
use crate::utils::trajectory_store::TrajectoryStore;
use anyhow::anyhow;

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
    let config_ref = std::env::var("CONFIG").unwrap_or_default();
    let global_config = Config::from_file(Some(&config_ref)).unwrap_or_default();
    let trajectory_dir = global_config.get_trajectory_dir(&problem.id);
    let scripts_dir = global_config.get_scripts_dir(&problem.id);
    
    // Create the scripts directory
    std::fs::create_dir_all(&scripts_dir).context(format!(
        "Failed to create scripts directory: {}",
        scripts_dir
    ))?;
    
    let trajectory_store =
        TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
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

    // Load file contents
    let mut file_contents = Vec::new();
    for (path, summary) in &relevant_files {
        match problem.get_file(path) {
            Ok(file_data) => {
                file_contents.push((path.clone(), summary.clone(), file_data.content.clone()));
            }
            Err(e) => {
                warn!("Failed to read file {}: {}", path, e);
            }
        }
    }

    // Format files into a string for the prompt
    let formatted_files: Vec<String> = relevant_files
        .iter()
        .map(|(path, summary)| format!("{}:\n{}", path, summary))
        .collect();

    // Create LLM config for Anthropic
    let llm_config = crate::config::LLMConfig {
        model_type: "anthropic".to_string(),
        model: script_config.model.model.clone(),
        api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
        base_url: None,
        timeout: script_config.model.timeout,
        max_retries: script_config.model.max_retries,
    };

    // Create LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    // Generate setup script
    info!("Generating setup script...");
    let setup_prompt = get_setup_script_user_prompt(&problem.problem_statement, &formatted_files, &file_contents);
    
    // Create a combined prompt with system and user instructions
    let combined_setup_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        SETUP_SCRIPT_SYSTEM_PROMPT, setup_prompt
    );

    // Add tracing metadata for setup script
    let setup_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "setup_script_generation",
        "temperature": script_config.temperature,
        "num_files": formatted_files.len(),
    });

    let setup_response = client
        .completion_with_tracing(
            &combined_setup_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("setup_script_{}", problem.id)),
            Some(setup_metadata),
        )
        .await
        .context("Failed to generate setup script")?;

    // Track usage
    let setup_usage = setup_response.usage;
    let setup_cost = client.calculate_cost(&setup_usage);
    info!("Setup script generation LLM usage: {}", setup_usage);
    info!("Setup script generation LLM cost: {}", setup_cost);

    // Extract setup script content
    let setup_script_content = extract_script(&setup_response.content)
        .context("Failed to extract setup script content from LLM response")?;

    // Save to the scripts directory
    let setup_script_path = Path::new(&scripts_dir).join("setup-script.sh");
    fs::write(&setup_script_path, &setup_script_content).context(format!(
        "Failed to write setup script to {:?}",
        setup_script_path
    ))?;

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&setup_script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&setup_script_path, perms)?;
    }

    info!("Setup script saved to {:?}", setup_script_path);

    // Now that we have the setup script, include it in the context for the other scripts
    let additional_context = format!(
        "\n\nSetup Script (for context - already taken care of):\n<setup_script>\n{}\n</setup_script>\n\nYour script should NOT duplicate any setup from the above setup script.",
        setup_script_content
    );

    // Generate lint script
    info!("Generating lint script...");
    let mut lint_prompt = get_lint_script_user_prompt(&problem.problem_statement, &formatted_files, &file_contents);
    lint_prompt.push_str(&additional_context);
    
    // Create a combined prompt with system and user instructions
    let combined_lint_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        LINT_SCRIPT_SYSTEM_PROMPT, lint_prompt
    );

    // Add tracing metadata for lint script
    let lint_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "lint_script_generation",
        "temperature": script_config.temperature,
        "num_files": formatted_files.len(),
    });

    let lint_response = client
        .completion_with_tracing(
            &combined_lint_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("lint_script_{}", problem.id)),
            Some(lint_metadata),
        )
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

    // Save to the scripts directory
    let lint_script_path = Path::new(&scripts_dir).join("lint-script.sh");
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
    let mut test_prompt = get_test_script_user_prompt(&problem.problem_statement, &formatted_files, &file_contents);
    test_prompt.push_str(&additional_context);
    
    // Create a combined prompt with system and user instructions
    let combined_test_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_SCRIPT_SYSTEM_PROMPT, test_prompt
    );

    // Add tracing metadata for test script
    let test_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "test_script_generation",
        "temperature": script_config.temperature,
        "num_files": formatted_files.len(),
    });

    let test_response = client
        .completion_with_tracing(
            &combined_test_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("test_script_{}", problem.id)),
            Some(test_metadata),
        )
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

    // Save to the scripts directory
    let test_script_path = Path::new(&scripts_dir).join("test-script.sh");
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

    // Generate single test script
    info!("Generating single test script...");

    // Extract the first test from the test script to use as the basis
    // for the single test script
    let first_test = if let Some(line) = test_script_content.lines()
        .find(|line| {
            line.contains("function test_") 
                || line.contains("def test_") 
                || line.contains("test() {") 
                || (line.starts_with("test") && line.contains("{"))
        }) {
        line.to_string()
    } else {
        "# First test".to_string()
    };

    // Create a single test script user prompt
    let single_test_prompt = format!(
        "Based on the test script, please create a script to run a single test. The script should:
        
1. Accept a test name as argument
2. Run only that specific test
3. Work in the docker container environment
4. Use the same testing framework as the main test script

For reference, here's the test script:
```sh
{}
```

And here's what looks like a test function: {}

Create a script called 'single-test-script.sh' that runs just one specified test.", 
        test_script_content, first_test
    );

    // Create a combined prompt with system and user instructions
    let combined_single_test_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_SCRIPT_SYSTEM_PROMPT, single_test_prompt
    );

    // Add tracing metadata for single test script
    let single_test_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "single_test_script_generation",
        "temperature": script_config.temperature,
        "num_files": formatted_files.len(),
    });

    let single_test_response = client
        .completion_with_tracing(
            &combined_single_test_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("single_test_script_{}", problem.id)),
            Some(single_test_metadata),
        )
        .await
        .context("Failed to generate single test script")?;

    // Track usage
    let single_test_usage = single_test_response.usage;
    let single_test_cost = client.calculate_cost(&single_test_usage);
    info!("Single test script generation LLM usage: {}", single_test_usage);
    info!("Single test script generation LLM cost: {}", single_test_cost);

    // Extract single test script content
    let single_test_script_content = extract_script(&single_test_response.content)
        .context("Failed to extract single test script content from LLM response")?;

    // Save to the scripts directory
    let single_test_script_path = Path::new(&scripts_dir).join("single-test-script.sh");
    fs::write(&single_test_script_path, &single_test_script_content).context(format!(
        "Failed to write single test script to {:?}",
        single_test_script_path
    ))?;

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&single_test_script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&single_test_script_path, perms)?;
    }

    info!("Single test script saved to {:?}", single_test_script_path);

    // Also save copies to the trajectory store for historical tracking
    fs::copy(&setup_script_path, trajectory_store.problem_dir().join("setup-script.sh"))?;
    fs::copy(&lint_script_path, trajectory_store.problem_dir().join("lint-script.sh"))?;
    fs::copy(&test_script_path, trajectory_store.problem_dir().join("test-script.sh"))?;
    fs::copy(&single_test_script_path, trajectory_store.problem_dir().join("single-test-script.sh"))?;

    // Calculate total usage and cost
    let total_usage = crate::llm::client::TokenUsage {
        prompt_tokens: setup_usage.prompt_tokens + lint_usage.prompt_tokens + test_usage.prompt_tokens + single_test_usage.prompt_tokens,
        completion_tokens: setup_usage.completion_tokens + lint_usage.completion_tokens + test_usage.completion_tokens + single_test_usage.completion_tokens,
        total_tokens: setup_usage.total_tokens + lint_usage.total_tokens + test_usage.total_tokens + single_test_usage.total_tokens,
    };
    let total_cost = setup_cost + lint_cost + test_cost + single_test_cost;
    info!("Total script generation LLM usage: {}", total_usage);
    info!("Total script generation LLM cost: {}", total_cost);

    info!("Script generation completed");
    Ok(())
}