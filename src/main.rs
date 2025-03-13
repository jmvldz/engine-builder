use anyhow::Result;
use clap::Parser;
use engine_builder::config::Config;
use engine_builder::models::exclusion::ExclusionConfig;
use engine_builder::models::problem::SWEBenchProblem;
use engine_builder::stages::{dockerfile, file_selection, ranking, relevance};
use log::info;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short = 'f', long)]
    config_path: Option<String>,

    /// Path to the codebase to analyze
    #[arg(short, long)]
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
    /// Run file relevance assessment
    Relevance,
    /// Run file ranking
    Ranking,
    /// Run full pipeline (relevance, ranking, and dockerfile generation)
    Pipeline,
    /// Run only the file selection step
    FileSelection,
    /// Generate a test-focused Dockerfile for running tests based on the ranked files
    Dockerfile,
    /// Build a Docker image from the generated Dockerfile
    BuildImage {
        /// Tag name for the Docker image
        #[arg(short, long, default_value = "engine-builder-test")]
        tag: String,
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
    env_logger::init();

    info!("Starting engine-builder. To adjust log level, set RUST_LOG=info, RUST_LOG=debug or RUST_LOG=trace");

    let cli = Cli::parse();
    let mut config = Config::from_file(cli.config_path.as_deref())?;

    // Update codebase path if provided
    if let Some(path) = &cli.codebase_path {
        config.codebase.path = path.clone();
    }

    // Create problem from CLI and config
    let problem = create_problem(&cli, &config);

    match cli.command {
        Command::Relevance => {
            info!("Running relevance assessment");
            relevance::process_codebase(config.relevance, &config.codebase, problem.clone())
                .await?;
        }
        Command::Ranking => {
            info!("Running file ranking");
            ranking::process_rankings(config.ranking.clone(), problem.clone()).await?;
        }
        Command::Pipeline => {
            info!("Running full pipeline");
            relevance::process_codebase(config.relevance, &config.codebase, problem.clone())
                .await?;
            ranking::process_rankings(config.ranking.clone(), problem.clone()).await?;
            info!("Generating test-focused Dockerfile based on ranked files");
            dockerfile::generate_dockerfile(config.ranking, problem.clone()).await?;
        }
        Command::FileSelection => {
            info!("Running file selection process");
            file_selection::process_file_selection(
                config.relevance,
                &config.codebase,
                problem.clone(),
            )
            .await?;
        }
        Command::Dockerfile => {
            info!("Generating test-focused Dockerfile based on ranked files");
            dockerfile::generate_dockerfile(config.ranking, problem.clone()).await?;
        }
        Command::BuildImage { tag } => {
            info!("Building Docker image with tag: {}", tag);
            dockerfile::build_docker_image(&config.ranking, &problem, &tag)?;
        }
    }

    Ok(())
}
