use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub relevance: RelevanceConfig,
    pub ranking: RankingConfig,
    pub codebase: CodebaseConfig,
    #[serde(default)]
    pub dockerfile: DockerfileConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    pub model_type: String,
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub timeout: u64,  // in seconds
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseConfig {
    /// Path to the codebase root directory
    pub path: PathBuf,
    
    /// ID for the problem (used for trajectory storage)
    pub problem_id: String,
    
    /// Custom problem statement/prompt
    pub problem_statement: String,
    
    /// Directories to exclude (e.g. ["tests", "docs"])
    pub exclude_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevanceConfig {
    pub llm: LLMConfig,
    pub max_workers: usize,
    pub max_tokens: usize,
    pub timeout: f64,
    pub max_file_tokens: usize,
    pub trajectory_store_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    pub llm: LLMConfig,
    pub num_rankings: usize,
    pub max_workers: usize,
    pub max_tokens: usize,
    pub temperature: f64,
    pub trajectory_store_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerfileConfig {
    pub llm: LLMConfig,
    pub max_tokens: usize,
    pub temperature: f64,
    pub output_path: String,
}

impl Default for DockerfileConfig {
    fn default() -> Self {
        Self {
            llm: LLMConfig {
                model_type: "anthropic".to_string(),
                model: "claude-3-opus-20240229".to_string(),
                api_key: "".to_string(),
                base_url: None,
                timeout: 60,
                max_retries: 3,
            },
            max_tokens: 4096,
            temperature: 0.0,
            output_path: "Dockerfile".to_string(),
        }
    }
}

impl Config {
    pub fn from_file(path: Option<&str>) -> Result<Self> {
        let path = path.unwrap_or("config.json");
        let file = File::open(path).context(format!("Failed to open config file: {}", path))?;
        let reader = BufReader::new(file);
        let config = serde_json::from_reader(reader).context("Failed to parse config file")?;
        Ok(config)
    }
    
    pub fn default() -> Self {
        Self {
            relevance: RelevanceConfig {
                llm: LLMConfig {
                    model_type: "anthropic".to_string(),
                    model: "claude-3-sonnet-20240229".to_string(),
                    api_key: "".to_string(),
                    base_url: None,
                    timeout: 30,
                    max_retries: 3,
                },
                max_workers: 256,
                max_tokens: 4096,
                timeout: 1800.0,
                max_file_tokens: 100_000,
                trajectory_store_dir: "data/trajectories".to_string(),
            },
            ranking: RankingConfig {
                llm: LLMConfig {
                    model_type: "anthropic".to_string(),
                    model: "claude-3-sonnet-20240229".to_string(),
                    api_key: "".to_string(),
                    base_url: None,
                    timeout: 30,
                    max_retries: 3,
                },
                num_rankings: 3,
                max_workers: 4,
                max_tokens: 4096,
                temperature: 0.0,
                trajectory_store_dir: "data/trajectories".to_string(),
            },
            codebase: CodebaseConfig {
                path: PathBuf::from("."),
                problem_id: "custom_problem".to_string(),
                problem_statement: "Please analyze this codebase".to_string(),
                exclude_dirs: vec!["tests".to_string(), "docs".to_string()],
            },
            dockerfile: DockerfileConfig::default(),
        }
    }
}