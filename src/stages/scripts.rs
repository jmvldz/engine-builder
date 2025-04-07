use anyhow::{Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::llm::client::{create_client, TokenCost};
use crate::llm::prompts::{
    get_lint_script_user_prompt, get_setup_script_user_prompt, get_test_script_error_user_prompt,
    get_test_script_user_prompt, LINT_SCRIPT_SYSTEM_PROMPT, SETUP_SCRIPT_SYSTEM_PROMPT, 
    TEST_SCRIPT_ERROR_SYSTEM_PROMPT, TEST_SCRIPT_SYSTEM_PROMPT,
};
use crate::models::problem::SWEBenchProblem;
use crate::models::ranking::RankedCodebaseFile;
use crate::models::relevance::RelevanceStatus;
use crate::utils::trajectory_store::TrajectoryStore;
use std::ops::Add;

// Add implementation for Add trait for TokenCost
impl Add for TokenCost {
    type Output = TokenCost;

    fn add(self, other: TokenCost) -> TokenCost {
        TokenCost {
            prompt_cost: self.prompt_cost + other.prompt_cost,
            completion_cost: self.completion_cost + other.completion_cost,
            total_cost: self.total_cost + other.total_cost,
        }
    }
}

/// Generate scripts from ranking results
pub async fn generate_scripts_from_ranking(
    config: &Config,
    problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting script generation from ranking data");

    // Create a trajectory store for this problem
    let trajectory_dir = config.get_trajectory_dir(&problem.id);

    // Create a trajectory store
    let trajectory_store = TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
        "Failed to create trajectory store for problem: {}",
        problem.id
    ))?;

    // Check if ranking exists
    if !trajectory_store.ranking_exists() {
        return Err(anyhow::anyhow!(
            "Ranking not found for problem: {}. Run ranking step first.",
            problem.id
        ));
    }

    // Load the ranking
    let ranking_context = trajectory_store.load_ranking().context(format!(
        "Failed to load ranking for problem: {}",
        problem.id
    ))?;

    // Extract ranked files
    let ranked_files = ranking_context.ranked_files;

    if ranked_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No ranked files found for problem: {}",
            problem.id
        ));
    }

    info!("Found {} ranked files", ranked_files.len());

    // Call the script generation function
    generate_scripts(config, problem).await
}

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
pub async fn generate_scripts(config: &Config, mut problem: SWEBenchProblem) -> Result<()> {
    info!("Starting script generation from relevance data");

    // Get trajectory and scripts directories
    let trajectory_dir = config.get_trajectory_dir(&problem.id);
    let scripts_dir = config.get_scripts_dir(&problem.id);

    // Create the scripts directory
    std::fs::create_dir_all(&scripts_dir).context(format!(
        "Failed to create scripts directory: {}",
        scripts_dir
    ))?;

    let trajectory_store = TrajectoryStore::new(&trajectory_dir, &problem).context(format!(
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
    let all_decisions = trajectory_store
        .load_all_relevance_decisions()
        .context(format!(
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

    // Create LLM config using the config's to_llm_config method
    let llm_config = config.to_llm_config(&config.scripts.model);

    // Create LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    // Generate setup script
    info!("Generating setup script...");
    // Create a Vec of RankedCodebaseFile from formatted_files
    let ranked_files: Vec<RankedCodebaseFile> = formatted_files
        .iter()
        .map(|path| RankedCodebaseFile {
            path: path.clone(),
            tokens: 0, // We don't need actual token counts here
        })
        .collect();

    // Prepare file_contents in the right format (path, content) without summaries
    let file_contents_for_prompt: Vec<(String, String)> = file_contents
        .iter()
        .map(|(path, _, content)| (path.clone(), content.clone()))
        .collect();

    let setup_prompt = get_setup_script_user_prompt(
        &problem.problem_statement,
        &ranked_files,
        &file_contents_for_prompt,
    );

    // Create a combined prompt with system and user instructions
    let combined_setup_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        SETUP_SCRIPT_SYSTEM_PROMPT, setup_prompt
    );

    // Add tracing metadata for setup script
    let setup_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "setup_script_generation",
        "temperature": config.scripts.temperature,
        "num_files": formatted_files.len(),
    });

    let setup_response = client
        .completion_with_tracing(
            &combined_setup_prompt,
            config.scripts.max_tokens,
            config.scripts.temperature,
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

    // Save setup script reasoning
    let metadata = serde_json::json!({
        "model": config.scripts.model,
        "tokens": setup_usage.total_tokens,
        "temperature": config.scripts.temperature
    });

    crate::stages::overview::save_reasoning(
        config,
        &problem,
        "setup_script",
        "",
        &setup_response.content,
        Some(metadata),
    )
    .context("Failed to save setup script reasoning to structured storage")?;

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
    let mut lint_prompt = get_lint_script_user_prompt(
        &problem.problem_statement,
        &ranked_files,
        &file_contents_for_prompt,
    );
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
        "temperature": config.scripts.temperature,
        "num_files": formatted_files.len(),
    });

    let lint_response = client
        .completion_with_tracing(
            &combined_lint_prompt,
            config.scripts.max_tokens,
            config.scripts.temperature,
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

    // Save lint script reasoning
    let metadata = serde_json::json!({
        "model": config.scripts.model,
        "tokens": lint_usage.total_tokens,
        "temperature": config.scripts.temperature
    });

    crate::stages::overview::save_reasoning(
        config,
        &problem,
        "lint_script",
        "",
        &lint_response.content,
        Some(metadata),
    )
    .context("Failed to save lint script reasoning to structured storage")?;

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
    let mut test_prompt = get_test_script_user_prompt(
        &problem.problem_statement,
        &ranked_files,
        &file_contents_for_prompt,
    );
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
        "temperature": config.scripts.temperature,
        "num_files": formatted_files.len(),
    });

    let test_response = client
        .completion_with_tracing(
            &combined_test_prompt,
            config.scripts.max_tokens,
            config.scripts.temperature,
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

    // Save test script reasoning
    let metadata = serde_json::json!({
        "model": config.scripts.model,
        "tokens": test_usage.total_tokens,
        "temperature": config.scripts.temperature
    });

    crate::stages::overview::save_reasoning(
        config,
        &problem,
        "test_script",
        "",
        &test_response.content,
        Some(metadata),
    )
    .context("Failed to save test script reasoning to structured storage")?;

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
    let first_test = if let Some(line) = test_script_content.lines().find(|line| {
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
        "temperature": config.scripts.temperature,
        "num_files": formatted_files.len(),
    });

    let single_test_response = client
        .completion_with_tracing(
            &combined_single_test_prompt,
            config.scripts.max_tokens,
            config.scripts.temperature,
            None, // Auto-generate trace ID
            Some(&format!("single_test_script_{}", problem.id)),
            Some(single_test_metadata),
        )
        .await
        .context("Failed to generate single test script")?;

    // Track usage
    let single_test_usage = single_test_response.usage;
    let single_test_cost = client.calculate_cost(&single_test_usage);
    info!(
        "Single test script generation LLM usage: {}",
        single_test_usage
    );
    info!(
        "Single test script generation LLM cost: {}",
        single_test_cost
    );

    // Save single test script reasoning
    let metadata = serde_json::json!({
        "model": config.scripts.model,
        "tokens": single_test_usage.total_tokens,
        "temperature": config.scripts.temperature
    });

    crate::stages::overview::save_reasoning(
        config,
        &problem,
        "single_test_script",
        "",
        &single_test_response.content,
        Some(metadata),
    )
    .context("Failed to save single test script reasoning to structured storage")?;

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

    // No need to save copies since scripts are already in the trajectory store directory

    // Calculate total usage and cost
    let total_usage = crate::llm::client::TokenUsage {
        prompt_tokens: setup_usage.prompt_tokens
            + lint_usage.prompt_tokens
            + test_usage.prompt_tokens
            + single_test_usage.prompt_tokens,
        completion_tokens: setup_usage.completion_tokens
            + lint_usage.completion_tokens
            + test_usage.completion_tokens
            + single_test_usage.completion_tokens,
        total_tokens: setup_usage.total_tokens
            + lint_usage.total_tokens
            + test_usage.total_tokens
            + single_test_usage.total_tokens,
    };
    let total_cost = setup_cost + lint_cost + test_cost + single_test_cost;
    info!("Total script generation LLM usage: {}", total_usage);
    info!("Total script generation LLM cost: {}", total_cost);

    info!("Script generation completed");
    Ok(())
}

/// Update a test script based on error output from a failed test run
pub async fn update_test_script_from_error(
    config: &Config,
    problem: &SWEBenchProblem,
    test_script_path: &Path,
    error_output: &[String],
    attempt: usize,
) -> Result<String> {
    // Read the current test script
    let test_script_content = fs::read_to_string(test_script_path)
        .context(format!("Failed to read test script at {:?}", test_script_path))?;

    // Format error output as a single string
    let error_output_str = error_output.join("\n");

    // Create LLM config
    let llm_config = crate::config::LLMConfig {
        model_type: "anthropic".to_string(),
        model: config.scripts.model.clone().unwrap_or_else(|| config.model.clone()),
        api_key: config.anthropic_api_key.clone(),
        base_url: None,
        timeout: 60,
        max_retries: 3,
    };

    // Create LLM client
    let client = create_client(&llm_config)
        .await
        .context("Failed to create LLM client")?;

    // Generate the user prompt for the LLM
    let user_prompt = get_test_script_error_user_prompt(
        &problem.problem_statement,
        &test_script_content,
        &error_output_str,
    );

    // Combine with system prompt
    let combined_error_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_SCRIPT_ERROR_SYSTEM_PROMPT, user_prompt
    );

    // Send the request to the LLM
    let llm_response = client
        .completion_with_tracing(
            &combined_error_prompt,
            config.scripts.max_tokens,
            config.scripts.temperature,
            None,
            Some(&format!("test_script_error_{}", problem.id)),
            None,
        )
        .await
        .context("Failed to get test script fix from LLM")?;

    // Extract the full LLM response
    let full_llm_response = llm_response.content.clone();

    // Save the reasoning to a file
    let reasoning_path = test_script_path
        .with_file_name(format!("test_script_error_reasoning_{}.md", problem.id));

    fs::write(&reasoning_path, &full_llm_response).context(format!(
        "Failed to write test script error reasoning to {:?}",
        reasoning_path
    ))?;

    // Save structured reasoning
    let metadata = serde_json::json!({
        "model": config.scripts.model,
        "tokens": llm_response.usage.total_tokens,
        "temperature": config.scripts.temperature,
        "attempt": attempt
    });

    crate::stages::overview::save_reasoning(
        config,
        problem,
        "test_script_error",
        &format!("_{}", attempt),
        &full_llm_response,
        Some(metadata),
    )
    .context("Failed to save test script error reasoning to structured storage")?;

    info!("Saved test script error reasoning to {:?}", reasoning_path);

    // Try to extract the test script content
    match extract_script(&full_llm_response) {
        Ok(content) => Ok(content),
        Err(_) => {
            // If we can't extract a code block, return the original script
            warn!("Could not extract updated test script from LLM response, using original");
            Ok(test_script_content)
        }
    }
}
