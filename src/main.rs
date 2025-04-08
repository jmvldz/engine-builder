use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use engine_builder::config::Config;
use engine_builder::llm::langfuse;
use engine_builder::models::exclusion::ExclusionConfig;
use engine_builder::models::problem::SWEBenchProblem;
use engine_builder::stages::{container, dockerfile, file_selection, ranking, relevance};
use log::{info, warn};
use std::env;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the config file. If not provided, will look for ~/.engines.config.json or ./config.json
    #[arg(
        short = 'c',
        long,
        help = "Path to the config file. If not provided, will look for ~/.engines.config.json or ./config.json"
    )]
    config_path: Option<String>,

    /// Path to the codebase to analyze
    #[arg(short = 'd', long)]
    codebase_path: Option<PathBuf>,

    /// Problem ID for trajectory storage
    #[arg(short = 'i', long)]
    problem_id: Option<String>,

    /// Problem statement or prompt
    #[arg(short = 'p', long)]
    problem_statement: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run full pipeline (file selection, relevance, ranking, scripts, and dockerfile generation)
    Pipeline,
    /// Run only the file selection step (first stage of pipeline)
    FileSelection,
    /// Run file relevance assessment (second stage of pipeline)
    Relevance,
    /// Run file ranking (third stage of pipeline)
    Ranking,
    /// Generate lint and test scripts based on ranked files (fourth stage of pipeline)
    GenerateScripts,
    /// Generate a test-focused Dockerfile based on ranked files (fifth stage of pipeline)
    Dockerfile,
    /// Generate an overview document of all reasoning across stages
    Overview,
    /// Build a Docker image from the generated Dockerfile
    BuildImage {
        /// Tag name for the Docker image
        #[arg(short, long, default_value = "engine-builder-test")]
        tag: String,
    },
    /// Run lint script in a Docker container
    RunLint {
        /// Tag name for the Docker image
        #[arg(short, long, default_value = "engine-builder-test")]
        tag: String,
    },
    /// Run test script in a Docker container
    RunTest {
        /// Tag name for the Docker image
        #[arg(short, long, default_value = "engine-builder-test")]
        tag: String,
    },
    /// Run both lint and test scripts in Docker containers
    RunAll {
        /// Tag name for the Docker image
        #[arg(short, long, default_value = "engine-builder-test")]
        tag: String,

        /// Run in parallel mode (both containers at once)
        #[arg(short, long)]
        parallel: bool,
    },
    /// Start an interactive chat session with the configured LLM
    Chat {
        /// Which LLM configuration to use (relevance, ranking, dockerfile, scripts)
        #[arg(short, long, default_value = "relevance")]
        config_type: String,

        /// Temperature for LLM responses (0.0-1.0)
        #[arg(short, long)]
        temperature: Option<f64>,
    },
}

/// Create a problem from the CLI args and config
fn create_problem(cli: &Cli, config: &Config) -> SWEBenchProblem {
    let problem_id = cli
        .problem_id
        .clone()
        .unwrap_or_else(|| config.codebase.problem_id.clone());
    let problem_statement = cli
        .problem_statement
        .clone()
        .unwrap_or_else(|| config.codebase.problem_statement.clone());

    // Load exclusion config if available
    let exclusion_config = match ExclusionConfig::from_file(&config.codebase.exclusions_path) {
        Ok(loaded_config) => {
            info!(
                "Loaded exclusion config from: {}",
                &config.codebase.exclusions_path
            );
            loaded_config
        }
        Err(e) => {
            info!("Using default exclusion config: {}", e);
            ExclusionConfig::default()
        }
    };

    SWEBenchProblem::new(problem_id, problem_statement)
        .with_codebase_path(&config.codebase.path)
        .with_exclusion_config(exclusion_config)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger
    // Log level can be controlled by setting the RUST_LOG environment variable
    // e.g., RUST_LOG=info cargo run --release -- -c config.json pipeline
    // or RUST_LOG=debug for more detailed logs

    // Use a custom logger setup based on the command
    let cli = Cli::parse();

    // Skip file logging when running in test mode
    if cfg!(test) {
        // For tests, use standard logging without any file output
        env_logger::init();
    } else if let Ok(tool_log_path) = std::env::var("ENGINE_BUILDER_TOOL_LOG") {
        // Tool is being executed as part of the chat UI, redirect logs to the specified file
        let log_path = std::path::PathBuf::from(tool_log_path);
        let file = std::fs::File::create(log_path).unwrap();
        env_logger::builder()
            .target(env_logger::Target::Pipe(Box::new(file)))
            .init();
    } else {
        // Normal logging based on command
        match cli.command {
            Command::Chat { .. } => {
                // For chat mode, redirect logs to a file
                let log_path = std::path::PathBuf::from("engine-builder.log");
                let file = std::fs::File::create(log_path).unwrap();
                env_logger::builder()
                    .target(env_logger::Target::Pipe(Box::new(file)))
                    .init();
            }
            _ => {
                // For other commands, use normal logging
                env_logger::init();
            }
        }
    };

    info!("Starting engine-builder. To adjust log level, set RUST_LOG=info, RUST_LOG=debug or RUST_LOG=trace");

    // Use the already parsed CLI args
    let mut config = Config::from_file(cli.config_path.as_deref())?;

    // Check for API key in environment variables if not in config
    if config.anthropic_api_key.is_empty() {
        config.anthropic_api_key = env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        if config.anthropic_api_key.is_empty() {
            warn!("No Anthropic API key found in config or environment variables");
            warn!("Please set ANTHROPIC_API_KEY environment variable or provide it in your configuration file (either ~/.engines.config.json or ./config.json)");
        } else {
            info!("Using Anthropic API key from environment variable");
        }
    }

    // Initialize Langfuse for observability (from config or environment variables)
    let langfuse_enabled = config.observability.langfuse.enabled;
    let langfuse_secret_key = if !config.observability.langfuse.secret_key.is_empty() {
        config.observability.langfuse.secret_key.clone()
    } else {
        env::var("LANGFUSE_SECRET_KEY").unwrap_or_default()
    };

    let langfuse_public_key = if !config.observability.langfuse.public_key.is_empty() {
        config.observability.langfuse.public_key.clone()
    } else {
        env::var("LANGFUSE_PUBLIC_KEY").unwrap_or_default()
    };

    let langfuse_project_id = if !config.observability.langfuse.project_id.is_empty() {
        config.observability.langfuse.project_id.clone()
    } else {
        env::var("LANGFUSE_PROJECT_ID").unwrap_or_else(|_| "engines-builder".to_string())
    };

    let langfuse_host = if !config.observability.langfuse.host.is_empty() {
        config.observability.langfuse.host.clone()
    } else {
        env::var("LANGFUSE_HOST").unwrap_or_else(|_| "https://us.cloud.langfuse.com".to_string())
    };

    // Initialize Langfuse regardless of whether keys are set - the client will handle the enabled state internally
    match langfuse::init_langfuse(
        &langfuse_secret_key,
        &langfuse_public_key,
        &langfuse_project_id,
        Some(&langfuse_host),
        Some(langfuse_enabled),
        config.observability.langfuse.trace_id.as_deref(),
    ) {
        Ok(_) => {
            if langfuse_enabled
                && !langfuse_secret_key.is_empty()
                && !langfuse_public_key.is_empty()
            {
                info!(
                    "Langfuse tracing initialized for project: {}",
                    langfuse_project_id
                );
            }
        }
        Err(e) => warn!("Failed to initialize Langfuse tracing: {}", e),
    }

    // Update codebase path if provided
    if let Some(path) = &cli.codebase_path {
        config.codebase.path = path.clone();
    }

    // Create problem from CLI and config
    let problem = create_problem(&cli, &config);

    match cli.command {
        Command::Relevance => {
            info!("Running relevance assessment");
            relevance::process_codebase(&config, &config.codebase, problem.clone()).await?;
        }
        Command::Ranking => {
            info!("Running file ranking");
            // Verify that relevance assessments have been run
            let trajectory_store = engine_builder::utils::trajectory_store::TrajectoryStore::new(
                &config.get_trajectory_dir(&problem.id),
                &problem,
            )?;

            let relevance_path = trajectory_store.relevance_decisions_path();
            if !relevance_path.exists() {
                info!("Relevance decisions file not found. Ensure you've run the relevance step first with 'cargo run --release -- relevance'");
            }

            ranking::process_rankings(&config, problem.clone()).await?;
        }
        Command::Pipeline => {
            info!("Running full pipeline");

            // Run file selection first to generate codebase_tree_response.txt
            info!("Running file selection process");
            file_selection::process_file_selection(
                &config,
                &config.codebase,
                problem.clone(),
                &config.get_trajectory_dir(&problem.id),
            )
            .await?;

            // Then process relevance using the existing codebase_tree_response.txt
            relevance::process_codebase(&config, &config.codebase, problem.clone()).await?;

            info!("Running file ranking");
            ranking::process_rankings(&config, problem.clone()).await?;
            info!("Generating lint and test scripts based on ranked files");
            engine_builder::stages::scripts::generate_scripts_from_ranking(
                &config,
                problem.clone(),
            )
            .await?;
            info!("Generating test-focused Dockerfile based on ranked files");
            dockerfile::generate_dockerfile(&config, problem.clone()).await?;

            // Finally, generate the overview document with reasoning from all stages
            info!("Generating overview document");
            engine_builder::stages::overview::generate_overview(&config, &problem).await?;
        }
        Command::FileSelection => {
            info!("Running file selection process");
            file_selection::process_file_selection(
                &config,
                &config.codebase,
                problem.clone(),
                &config.get_trajectory_dir(&problem.id),
            )
            .await?;
        }
        Command::Dockerfile => {
            info!("Generating test-focused Dockerfile based on ranked files");
            dockerfile::generate_dockerfile(&config, problem.clone()).await?;
        }
        Command::Overview => {
            info!("Generating overview document for problem: {}", problem.id);
            engine_builder::stages::overview::generate_overview(&config, &problem).await?;
        }
        Command::BuildImage { tag } => {
            info!("Building Docker image with tag: {}", tag);
            dockerfile::build_docker_image(&config, &problem, &tag).await?;
        }
        Command::GenerateScripts => {
            info!("Generating lint and test scripts based on ranked files");
            engine_builder::stages::scripts::generate_scripts_from_ranking(
                &config,
                problem.clone(),
            )
            .await?;
        }
        Command::RunLint { tag } => {
            info!("Running lint container with image tag: {}", tag);
            let result = container::run_lint_container(&problem, &tag, &config.container).await?;

            // Print summary
            println!("\nLint container execution complete");
            println!("Exit code: {}", result.exit_code);
            println!(
                "Status: {}",
                if result.success { "SUCCESS" } else { "FAILED" }
            );

            // Set exit code if container failed
            if !result.success {
                std::process::exit(1);
            }
        }
        Command::RunTest { tag } => {
            info!("Running test container with image tag: {}", tag);
            let result = container::run_test_container(&problem, &tag, &config.container).await?;

            // Print summary
            println!("\nTest container execution complete");
            println!("Exit code: {}", result.exit_code);
            println!(
                "Status: {}",
                if result.success { "SUCCESS" } else { "FAILED" }
            );

            // Set exit code if container failed
            if !result.success {
                std::process::exit(1);
            }
        }
        Command::RunAll { tag, parallel } => {
            info!(
                "Running both lint and test containers with image tag: {}",
                tag
            );

            // Override parallel flag from CLI if provided
            let mut container_config = config.container.clone();
            if parallel {
                container_config.parallel = true;
            }

            let (lint_result, test_result) =
                container::run_containers(&problem, &tag, &container_config).await?;

            // Print summary
            println!("\nContainer execution summary:");
            println!(
                "Lint container: {} (exit code: {})",
                if lint_result.success {
                    "SUCCESS".green()
                } else {
                    "FAILED".red()
                },
                lint_result.exit_code
            );

            println!(
                "Test container: {} (exit code: {})",
                if test_result.success {
                    "SUCCESS".green()
                } else {
                    "FAILED".red()
                },
                test_result.exit_code
            );

            // Set exit code if either container failed
            if !lint_result.success || !test_result.success {
                std::process::exit(1);
            }
        }
        Command::Chat {
            config_type,
            temperature,
        } => {
            info!("Starting chat session with LLM");

            // First check if we have a dedicated chat model config
            let llm_config = if config.chat.model.is_some() {
                info!("Using dedicated chat model configuration");
                config.to_llm_config(&config.chat.model)
            } else {
                // Fall back to the selected config type
                info!("Using {} model configuration for chat", config_type);
                match config_type.to_lowercase().as_str() {
                    "relevance" => config.to_llm_config(&config.relevance.model),
                    "ranking" => config.to_llm_config(&config.ranking.model),
                    "dockerfile" => config.to_llm_config(&config.dockerfile.model),
                    "scripts" => config.to_llm_config(&config.scripts.model),
                    _ => {
                        eprintln!("Invalid config type: {}. Using default chat model: claude-3-7-sonnet-20250219", config_type);
                        // Create a Some with the default model for chat
                        config.to_llm_config(&Some("claude-3-7-sonnet-20250219".to_string()))
                    }
                }
            };

            // Use config temperature unless overridden by command line
            let temp = temperature.unwrap_or(config.chat.temperature);

            // Create chat configuration
            let chat_config = engine_builder::chat::ChatConfig {
                llm_config,
                max_tokens: config.chat.max_tokens,
                temperature: temp,
            };

            // Start the chat session
            engine_builder::chat::start_chat(&config, chat_config).await?;
        }
    }

    Ok(())
}
