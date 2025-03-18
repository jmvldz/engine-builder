use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub relevance: RelevanceConfig,
    pub ranking: RankingConfig,
    pub codebase: CodebaseConfig,
    #[serde(default)]
    pub dockerfile: DockerfileConfig,
    #[serde(default)]
    pub scripts: ScriptConfig,
    #[serde(default)]
    pub container: ContainerConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    pub model_type: String,
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub timeout: u64, // in seconds
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

    /// Path to the exclusions config file
    #[serde(default = "default_exclusions_path")]
    pub exclusions_path: String,
}

fn default_exclusions_path() -> String {
    "exclusions.json".to_string()
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
    pub max_retries: usize,
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
            max_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptConfig {
    pub llm: LLMConfig,
    pub max_tokens: usize,
    pub temperature: f64,
    pub max_retries: usize,
}

impl Default for ScriptConfig {
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
            max_retries: 3,
        }
    }
}

// Container configuration for running lint and test containers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContainerConfig {
    pub timeout: u64,  // Timeout for container execution in seconds
    pub parallel: bool, // Whether to run lint and test containers in parallel
    pub remove: bool,  // Whether to remove containers after execution
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            timeout: 300,      // 5 minutes default timeout
            parallel: false,   // Serial execution by default
            remove: true,      // Remove containers by default
        }
    }
}

/// Configuration for observability and tracing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObservabilityConfig {
    pub langfuse: LangfuseConfig,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            langfuse: LangfuseConfig::default(),
        }
    }
}

/// Configuration for Langfuse tracing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LangfuseConfig {
    pub enabled: bool,
    pub host: String,
    pub project_id: String,
    pub secret_key: String,
    pub public_key: String,
    /// Optional trace ID for consistent tracing across runs.
    /// If not provided, the problem_id from CodebaseConfig will be used.
    /// This ensures all traces for the same problem are grouped together.
    pub trace_id: Option<String>,
}

impl Default for LangfuseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "https://us.cloud.langfuse.com".to_string(),
            project_id: "engines-builder".to_string(),
            secret_key: "".to_string(),
            public_key: "".to_string(),
            trace_id: None,
        }
    }
}

impl Config {
    pub fn from_file(path: Option<&str>) -> Result<Self> {
        let path = path.unwrap_or("config.json");
        let file = File::open(path).context(format!("Failed to open config file: {}", path))?;
        let reader = BufReader::new(file);
        let mut config: Self = serde_json::from_reader(reader).context("Failed to parse config file")?;
        
        // If trace_id is not set, use the problem_id as the trace_id
        if config.observability.langfuse.trace_id.is_none() {
            config.observability.langfuse.trace_id = Some(config.codebase.problem_id.clone());
        }
        
        Ok(config)
    }

    pub fn default() -> Self {
        let mut config = Self {
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
                exclusions_path: "exclusions.json".to_string(),
            },
            dockerfile: DockerfileConfig::default(),
            scripts: ScriptConfig::default(),
            container: ContainerConfig::default(),
            observability: ObservabilityConfig::default(),
        };
        
        // Set the trace_id based on the problem_id
        config.observability.langfuse.trace_id = Some(config.codebase.problem_id.clone());
        
        config
    }
}