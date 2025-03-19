use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub anthropic_api_key: String,
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
    #[serde(default)]
    pub output_path: Option<String>,
}

/// Common model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub model: String,
    pub timeout: u64, // in seconds
    pub max_retries: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model: "claude-3-sonnet-20240229".to_string(),
            timeout: 60,
            max_retries: 3,
        }
    }
}

/// Legacy LLMConfig structure for compatibility with LLM client code
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
    pub model: ModelConfig,
    pub max_workers: usize,
    pub max_tokens: usize,
    pub timeout: f64,
    pub max_file_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingConfig {
    pub model: ModelConfig,
    pub num_rankings: usize,
    pub max_workers: usize,
    pub max_tokens: usize,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerfileConfig {
    pub model: ModelConfig,
    pub max_tokens: usize,
    pub temperature: f64,
    pub max_retries: usize,
}

impl Default for DockerfileConfig {
    fn default() -> Self {
        Self {
            model: ModelConfig {
                model: "claude-3-opus-20240229".to_string(),
                timeout: 60,
                max_retries: 3,
            },
            max_tokens: 4096,
            temperature: 0.0,
            max_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptConfig {
    pub model: ModelConfig,
    pub max_tokens: usize,
    pub temperature: f64,
    pub max_retries: usize,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            model: ModelConfig {
                model: "claude-3-opus-20240229".to_string(),
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
        let config = serde_json::from_reader(reader).context("Failed to parse config file")?;
        Ok(config)
    }

    pub fn default() -> Self {
        Self {
            anthropic_api_key: "".to_string(),
            relevance: RelevanceConfig {
                model: ModelConfig::default(),
                max_workers: 256,
                max_tokens: 4096,
                timeout: 1800.0,
                max_file_tokens: 100_000,
            },
            ranking: RankingConfig {
                model: ModelConfig::default(),
                num_rankings: 3,
                max_workers: 4,
                max_tokens: 4096,
                temperature: 0.0,
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
            output_path: Some(".engines".to_string()),
        }
    }
    
    /// Convert a ModelConfig to the LLMConfig format needed by LLM clients
    pub fn to_llm_config(&self, model_config: &ModelConfig) -> LLMConfig {
        LLMConfig {
            model_type: "anthropic".to_string(),
            model: model_config.model.clone(),
            api_key: self.anthropic_api_key.clone(),
            base_url: None,
            timeout: model_config.timeout,
            max_retries: model_config.max_retries,
        }
    }
    
    /// Get the output directory path
    pub fn get_output_dir(&self) -> String {
        self.output_path.clone().unwrap_or_else(|| ".engines".to_string())
    }
    
    /// Get the trajectory store directory for a given problem ID
    pub fn get_trajectory_dir(&self, problem_id: &str) -> String {
        format!("{}/trajectories/{}", self.get_output_dir(), problem_id)
    }
    
    /// Get the Dockerfile path for a given problem
    pub fn get_dockerfile_path(&self, problem_id: &str) -> String {
        format!("{}/dockerfiles/{}/Dockerfile", self.get_output_dir(), problem_id)
    }
    
    /// Get the scripts directory for a given problem
    pub fn get_scripts_dir(&self, problem_id: &str) -> String {
        format!("{}/scripts/{}", self.get_output_dir(), problem_id)
    }
}
