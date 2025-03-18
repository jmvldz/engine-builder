use anyhow::{Context, Result};
use log::{info, warn};
use regex::Regex;
use std::fs;

use crate::config::{RankingConfig, RelevanceConfig, ScriptConfig};
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
    
    // First check if we have a Dockerfile
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    let dockerfile_content = if dockerfile_path.exists() {
        fs::read_to_string(&dockerfile_path).context(format!(
            "Failed to read Dockerfile at {:?}",
            dockerfile_path
        ))?
    } else {
        String::new()
    };

    // Generate setup script first, including Dockerfile content for context if available
    info!("Generating setup script...");
    let mut setup_prompt = get_setup_script_user_prompt(&problem.problem_statement, &formatted_files, &file_contents);
    
    // Add Dockerfile content to the setup script prompt if available
    if !dockerfile_content.is_empty() {
        setup_prompt = format!(
            "{}\n\nDockerfile (for context):\n<dockerfile>\n{}\n</dockerfile>\n\nPlease create a setup script that complements this Dockerfile by handling package installations, environment variables, and other setup that might change frequently.",
            setup_prompt,
            dockerfile_content
        );
    }
    
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

    // Save to the trajectory store directory
    let setup_script_path = trajectory_store.problem_dir().join("setup-script.sh");
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

/// Generate lint and test scripts based on ranking data
pub async fn generate_scripts_from_ranking(
    config: RankingConfig,
    script_config: ScriptConfig,
    mut problem: SWEBenchProblem,
) -> Result<()> {
    info!("Starting script generation from ranking data");

    // Create a trajectory store for this problem
    let trajectory_store =
        TrajectoryStore::new(&config.trajectory_store_dir, &problem).context(format!(
            "Failed to create trajectory store for problem: {}",
            problem.id
        ))?;

    // Check if ranking exists
    if !trajectory_store.ranking_exists() {
        return Err(anyhow!(
            "No ranking found for problem: {}. Run the ranking step first.",
            problem.id
        ));
    }

    // Load ranking
    let ranking = trajectory_store.load_ranking().context(format!(
        "Failed to load ranking for problem: {}",
        problem.id
    ))?;

    // Get ranked files (limit to top 5 files to avoid context overflow)
    let max_files = 5; // Limiting to top 5 files
    let ranked_files = ranking
        .ranked_files
        .iter()
        .take(max_files)
        .cloned()
        .collect::<Vec<_>>();

    if ranked_files.is_empty() {
        return Err(anyhow!(
            "No ranked files found for problem: {}",
            problem.id
        ));
    }

    info!("Using top {} ranked files", ranked_files.len());
    for file in &ranked_files {
        info!("Ranked file: {}", file.path);
    }

    // Load file contents
    let mut file_contents = Vec::new();

    for file in &ranked_files {
        match problem.get_file(&file.path) {
            Ok(file_data) => {
                file_contents.push((file.path.clone(), file_data.content.clone()));
            }
            Err(e) => {
                warn!("Failed to read file {}: {}", file.path, e);
            }
        }
    }

    // Create LLM client
    let client = create_client(&script_config.llm)
        .await
        .context("Failed to create LLM client")?;

    // First check if we have a Dockerfile
    let dockerfile_path = trajectory_store.problem_dir().join("Dockerfile");
    let dockerfile_content = if dockerfile_path.exists() {
        fs::read_to_string(&dockerfile_path).context(format!(
            "Failed to read Dockerfile at {:?}",
            dockerfile_path
        ))?
    } else {
        String::new()
    };

    // Generate setup script first, including Dockerfile content for context if available
    info!("Generating setup script...");
    let mut setup_prompt = get_setup_script_user_prompt(&problem.problem_statement, &ranked_files, &file_contents);
    
    // Add Dockerfile content to the setup script prompt if available
    if !dockerfile_content.is_empty() {
        setup_prompt = format!(
            "{}\n\nDockerfile (for context):\n<dockerfile>\n{}\n</dockerfile>\n\nPlease create a setup script that complements this Dockerfile by handling package installations, environment variables, and other setup that might change frequently.",
            setup_prompt,
            dockerfile_content
        );
    }
    
    // Create a combined prompt with system and user instructions
    let combined_setup_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        SETUP_SCRIPT_SYSTEM_PROMPT, setup_prompt
    );

    // Add tracing metadata for setup script
    let setup_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "setup_script_from_ranking",
        "temperature": script_config.temperature,
        "num_files": ranked_files.len(),
    });

    let setup_response = client
        .completion_with_tracing(
            &combined_setup_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("setup_script_ranking_{}", problem.id)),
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

    // Save to the trajectory store directory
    let setup_script_path = trajectory_store.problem_dir().join("setup-script.sh");
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
    let mut lint_prompt = get_lint_script_user_prompt(&problem.problem_statement, &ranked_files, &file_contents);
    lint_prompt.push_str(&additional_context);
    
    // Create a combined prompt with system and user instructions
    let combined_lint_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        LINT_SCRIPT_SYSTEM_PROMPT, lint_prompt
    );

    // Add tracing metadata for lint script
    let lint_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "lint_script_from_ranking",
        "temperature": script_config.temperature,
        "num_files": ranked_files.len(),
    });

    let lint_response = client
        .completion_with_tracing(
            &combined_lint_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("lint_script_ranking_{}", problem.id)),
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
    let mut test_prompt = get_test_script_user_prompt(&problem.problem_statement, &ranked_files, &file_contents);
    test_prompt.push_str(&additional_context);
    
    // Create a combined prompt with system and user instructions
    let combined_test_prompt = format!(
        "System instructions:\n{}\n\nUser request:\n{}",
        TEST_SCRIPT_SYSTEM_PROMPT, test_prompt
    );

    // Add tracing metadata for test script
    let test_metadata = serde_json::json!({
        "problem_id": problem.id,
        "stage": "test_script_from_ranking",
        "temperature": script_config.temperature,
        "num_files": ranked_files.len(),
    });

    let test_response = client
        .completion_with_tracing(
            &combined_test_prompt,
            script_config.max_tokens,
            script_config.temperature,
            None, // Auto-generate trace ID
            Some(&format!("test_script_ranking_{}", problem.id)),
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
