use anyhow::Result;
use clap::Parser;
use codemonkeys_rs::config::Config;
use codemonkeys_rs::stages::{relevance, ranking};
use codemonkeys_rs::models::problem::SWEBenchProblem;
use log::info;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    config_path: Option<String>,
    
    /// Path to the codebase to analyze
    #[arg(short, long)]
    codebase_path: Option<PathBuf>,
    
    /// Problem ID for trajectory storage
    #[arg(short, long)]
    problem_id: Option<String>,
    
    /// Problem statement or prompt
    #[arg(short, long)]
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
    /// Run full pipeline (relevance then ranking)
    Pipeline,
}

/// Create a problem from the CLI args and config
fn create_problem(cli: &Cli, config: &Config) -> SWEBenchProblem {
    let problem_id = cli.problem_id.clone().unwrap_or_else(|| config.codebase.problem_id.clone());
    let problem_statement = cli.problem_statement.clone().unwrap_or_else(|| config.codebase.problem_statement.clone());
    
    SWEBenchProblem::new(problem_id, problem_statement)
        .with_codebase_path(&config.codebase.path)
        .with_extensions(config.codebase.include_extensions.clone())
        .with_exclude_dirs(config.codebase.exclude_dirs.clone())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
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
            relevance::process_codebase(config.relevance, &config.codebase, problem.clone()).await?;
        },
        Command::Ranking => {
            info!("Running file ranking");
            ranking::process_rankings(config.ranking, problem.clone()).await?;
        },
        Command::Pipeline => {
            info!("Running full pipeline");
            relevance::process_codebase(config.relevance, &config.codebase, problem.clone()).await?;
            ranking::process_rankings(config.ranking, problem.clone()).await?;
        }
    }
    
    Ok(())
}