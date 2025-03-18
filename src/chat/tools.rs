use anyhow::{anyhow, Result};
use crate::config::Config;
use crate::stages::{container, dockerfile, file_selection, ranking, relevance};
use crate::models::problem::SWEBenchProblem;
use std::process::Command;
use std::path::PathBuf;
use tokio::process::Command as AsyncCommand;

/// Represents a tool that can be executed by the LLM
pub trait Tool: Send + Sync {
    /// Get the name of the tool
    fn name(&self) -> &'static str;
    
    /// Get a description of the tool
    fn description(&self) -> &'static str;
    
    /// Execute the tool with the given input
    async fn execute(&self, input: &str, config: &Config) -> Result<String>;
}

/// Get a list of available tool names
pub fn get_available_tools() -> Vec<&'static str> {
    vec![
        "relevance",
        "ranking",
        "pipeline",
        "file_selection",
        "dockerfile",
        "build_image",
        "generate_scripts",
        "run_lint",
        "run_test",
        "run_all",
        "help",
    ]
}

/// Execute a tool by name
pub async fn execute_tool(name: &str, input: &str, config: &Config) -> Result<String> {
    match name {
        "relevance" => execute_relevance(input, config).await,
        "ranking" => execute_ranking(input, config).await,
        "pipeline" => execute_pipeline(input, config).await,
        "file_selection" => execute_file_selection(input, config).await,
        "dockerfile" => execute_dockerfile(input, config).await,
        "build_image" => execute_build_image(input, config).await,
        "generate_scripts" => execute_generate_scripts(input, config).await,
        "run_lint" => execute_run_lint(input, config).await,
        "run_test" => execute_run_test(input, config).await,
        "run_all" => execute_run_all(input, config).await,
        "help" => execute_help(input, config).await,
        _ => Err(anyhow!("Unknown tool: {}", name)),
    }
}

/// Create a problem from input
fn create_problem_from_input(input: &str, config: &Config) -> Result<SWEBenchProblem> {
    // Parse input for problem ID and statement
    let lines: Vec<&str> = input.lines().collect();
    
    let problem_id = if let Some(line) = lines.iter().find(|l| l.starts_with("problem_id:")) {
        line.trim_start_matches("problem_id:").trim().to_string()
    } else {
        config.codebase.problem_id.clone()
    };
    
    let problem_statement = if let Some(line) = lines.iter().find(|l| l.starts_with("problem_statement:")) {
        line.trim_start_matches("problem_statement:").trim().to_string()
    } else {
        config.codebase.problem_statement.clone()
    };
    
    let codebase_path = if let Some(line) = lines.iter().find(|l| l.starts_with("codebase_path:")) {
        PathBuf::from(line.trim_start_matches("codebase_path:").trim())
    } else {
        config.codebase.path.clone()
    };
    
    // Load exclusion config
    let exclusion_config = match crate::models::exclusion::ExclusionConfig::from_file(&config.codebase.exclusions_path) {
        Ok(loaded_config) => loaded_config,
        Err(_) => crate::models::exclusion::ExclusionConfig::default(),
    };
    
    Ok(SWEBenchProblem::new(problem_id, problem_statement)
        .with_codebase_path(&codebase_path)
        .with_exclusion_config(exclusion_config))
}

/// Execute the relevance command
async fn execute_relevance(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Run relevance assessment
    relevance::process_codebase(config.relevance.clone(), &config.codebase, problem).await?;
    
    Ok("Relevance assessment completed successfully.".to_string())
}

/// Execute the ranking command
async fn execute_ranking(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Run file ranking
    ranking::process_rankings(config.ranking.clone(), problem).await?;
    
    Ok("File ranking completed successfully.".to_string())
}

/// Execute the pipeline command
async fn execute_pipeline(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Run file selection
    file_selection::process_file_selection(
        config.relevance.clone(),
        &config.codebase,
        problem.clone(),
    ).await?;
    
    // Process relevance
    relevance::process_codebase(config.relevance.clone(), &config.codebase, problem.clone()).await?;
    
    // Run file ranking
    ranking::process_rankings(config.ranking.clone(), problem.clone()).await?;
    
    // Generate scripts
    crate::stages::scripts::generate_scripts_from_ranking(
        config.ranking.clone(),
        config.scripts.clone(),
        problem.clone(),
    ).await?;
    
    // Generate Dockerfile
    dockerfile::generate_dockerfile(config.ranking.clone(), problem).await?;
    
    Ok("Full pipeline completed successfully.".to_string())
}

/// Execute the file selection command
async fn execute_file_selection(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Run file selection
    file_selection::process_file_selection(
        config.relevance.clone(),
        &config.codebase,
        problem,
    ).await?;
    
    Ok("File selection completed successfully.".to_string())
}

/// Execute the dockerfile command
async fn execute_dockerfile(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Generate Dockerfile
    dockerfile::generate_dockerfile(config.ranking.clone(), problem).await?;
    
    Ok("Dockerfile generation completed successfully.".to_string())
}

/// Execute the build image command
async fn execute_build_image(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Parse tag from input
    let tag = if let Some(line) = input.lines().find(|l| l.starts_with("tag:")) {
        line.trim_start_matches("tag:").trim().to_string()
    } else {
        "engine-builder-test".to_string()
    };
    
    // Build Docker image
    dockerfile::build_docker_image(&config.ranking, &problem, &tag, config.dockerfile.max_retries).await?;
    
    Ok(format!("Docker image built successfully with tag: {}", tag))
}

/// Execute the generate scripts command
async fn execute_generate_scripts(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Generate scripts
    crate::stages::scripts::generate_scripts_from_ranking(
        config.ranking.clone(),
        config.scripts.clone(),
        problem,
    ).await?;
    
    Ok("Scripts generated successfully.".to_string())
}

/// Execute the run lint command
async fn execute_run_lint(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Parse tag from input
    let tag = if let Some(line) = input.lines().find(|l| l.starts_with("tag:")) {
        line.trim_start_matches("tag:").trim().to_string()
    } else {
        "engine-builder-test".to_string()
    };
    
    // Run lint container
    let result = container::run_lint_container(&problem, &tag, &config.container).await?;
    
    // Format result
    let status = if result.success { "SUCCESS" } else { "FAILED" };
    Ok(format!("Lint container execution complete\nExit code: {}\nStatus: {}", result.exit_code, status))
}

/// Execute the run test command
async fn execute_run_test(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Parse tag from input
    let tag = if let Some(line) = input.lines().find(|l| l.starts_with("tag:")) {
        line.trim_start_matches("tag:").trim().to_string()
    } else {
        "engine-builder-test".to_string()
    };
    
    // Run test container
    let result = container::run_test_container(&problem, &tag, &config.container).await?;
    
    // Format result
    let status = if result.success { "SUCCESS" } else { "FAILED" };
    Ok(format!("Test container execution complete\nExit code: {}\nStatus: {}", result.exit_code, status))
}

/// Execute the run all command
async fn execute_run_all(input: &str, config: &Config) -> Result<String> {
    let problem = create_problem_from_input(input, config)?;
    
    // Parse tag and parallel flag from input
    let tag = if let Some(line) = input.lines().find(|l| l.starts_with("tag:")) {
        line.trim_start_matches("tag:").trim().to_string()
    } else {
        "engine-builder-test".to_string()
    };
    
    let parallel = input.lines().any(|l| l.trim() == "parallel: true");
    
    // Override parallel flag if specified
    let mut container_config = config.container.clone();
    if parallel {
        container_config.parallel = true;
    }
    
    // Run containers
    let (lint_result, test_result) = container::run_containers(
        &problem,
        &tag,
        &container_config,
    ).await?;
    
    // Format results
    let lint_status = if lint_result.success { "SUCCESS" } else { "FAILED" };
    let test_status = if test_result.success { "SUCCESS" } else { "FAILED" };
    
    Ok(format!(
        "Container execution summary:\nLint container: {} (exit code: {})\nTest container: {} (exit code: {})",
        lint_status, lint_result.exit_code,
        test_status, test_result.exit_code
    ))
}

/// Execute the help command
async fn execute_help(_input: &str, _config: &Config) -> Result<String> {
    Ok(format!(
        "Available commands:\n\n\
        - relevance: Run relevance assessment on the codebase\n\
        - ranking: Run file ranking based on relevance assessments\n\
        - pipeline: Run the full pipeline (file selection, relevance, ranking, scripts, dockerfile)\n\
        - file_selection: Run only the file selection step\n\
        - dockerfile: Generate a Dockerfile based on ranked files\n\
        - build_image: Build a Docker image from the generated Dockerfile\n\
        - generate_scripts: Generate lint and test scripts based on ranked files\n\
        - run_lint: Run the lint script in a Docker container\n\
        - run_test: Run the test script in a Docker container\n\
        - run_all: Run both lint and test scripts in Docker containers\n\
        - help: Show this help message\n\n\
        For each command, you can specify:\n\
        - problem_id: Custom problem ID\n\
        - problem_statement: Custom problem statement\n\
        - codebase_path: Custom path to the codebase\n\
        For build_image, run_lint, run_test, and run_all, you can also specify:\n\
        - tag: Custom Docker image tag\n\
        For run_all, you can also specify:\n\
        - parallel: true/false (whether to run containers in parallel)"
    ))
}