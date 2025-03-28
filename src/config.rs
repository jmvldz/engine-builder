use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub anthropic_api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub relevance: RelevanceConfig,
    #[serde(default)]
    pub ranking: RankingConfig,
    pub codebase: CodebaseConfig,
    #[serde(default)]
    pub dockerfile: DockerfileConfig,
    #[serde(default)]
    pub scripts: ScriptConfig,
    #[serde(default)]
    pub chat: ChatConfig,
    #[serde(default)]
    pub container: ContainerConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub output_path: Option<String>,
}

fn default_model() -> String {
    "claude-3-7-sonnet-20250219".to_string()
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
    #[serde(default = "default_codebase_path")]
    pub path: PathBuf,

    /// ID for the problem (used for trajectory storage)
    pub problem_id: String,

    /// Custom problem statement/prompt
    pub problem_statement: String,

    /// Path to the exclusions config file
    #[serde(default = "default_exclusions_path")]
    pub exclusions_path: String,
}

fn default_codebase_path() -> PathBuf {
    PathBuf::from(".")
}

fn default_exclusions_path() -> String {
    "exclusions.json".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RelevanceConfig {
    pub model: Option<String>,
    #[serde(default = "default_max_workers")]
    pub max_workers: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_relevance_timeout")]
    pub timeout: f64,
    #[serde(default = "default_max_file_tokens")]
    pub max_file_tokens: usize,
}

fn default_max_workers() -> usize { 8 }
fn default_max_tokens() -> usize { 4096 }
fn default_relevance_timeout() -> f64 { 1800.0 }
fn default_max_file_tokens() -> usize { 100_000 }

impl Default for RelevanceConfig {
    fn default() -> Self {
        Self {
            model: None,
            max_workers: default_max_workers(),
            max_tokens: default_max_tokens(),
            timeout: default_relevance_timeout(),
            max_file_tokens: default_max_file_tokens(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RankingConfig {
    pub model: Option<String>,
    #[serde(default = "default_ranking_max_workers")]
    pub max_workers: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
}
fn default_ranking_max_workers() -> usize { 4 }
fn default_temperature() -> f64 { 0.0 }

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            model: None,
            max_workers: default_ranking_max_workers(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerfileConfig {
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
}

fn default_max_retries() -> usize { 3 }

impl Default for DockerfileConfig {
    fn default() -> Self {
        Self {
            model: None,
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            max_retries: default_max_retries(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptConfig {
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            model: None,
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            max_retries: default_max_retries(),
        }
    }
}

// Chat interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatConfig {
    #[serde(default = "default_chat_model")]
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_chat_temperature")]
    pub temperature: f64,
}

fn default_chat_temperature() -> f64 { 0.7 }

fn default_chat_model() -> Option<String> {
    Some("claude-3-7-sonnet-20250219".to_string())
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            model: default_chat_model(),
            max_tokens: default_max_tokens(),
            temperature: default_chat_temperature(),
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
        use log::{info, debug};
        
        // If a specific path is provided via command line, use that
        if let Some(path_str) = path {
            debug!("Attempting to load config from command-line specified path: {}", path_str);
            let file = File::open(path_str).context(format!("Failed to open config file: {}", path_str))?;
            let reader = BufReader::new(file);
            let config = serde_json::from_reader(reader).context("Failed to parse config file")?;
            info!("Loaded configuration from: {}", path_str);
            return Ok(config);
        }
        
        // Try to find config in home directory first (.engines.config.json)
        if let Ok(home_dir) = std::env::var("HOME") {
            let home_config_path = format!("{}/.engines.config.json", home_dir);
            debug!("Checking for config in home directory: {}", home_config_path);
            if let Ok(file) = File::open(&home_config_path) {
                let reader = BufReader::new(file);
                match serde_json::from_reader(reader) {
                    Ok(config) => {
                        info!("Loaded configuration from home directory: {}", home_config_path);
                        return Ok(config);
                    },
                    Err(e) => debug!("Failed to parse home directory config: {}", e)
                }
            } else {
                debug!("No config found in home directory");
            }
        }
        
        // Try to find config in current directory (config.json)
        debug!("Checking for config in current directory: config.json");
        if let Ok(file) = File::open("config.json") {
            let reader = BufReader::new(file);
            match serde_json::from_reader(reader) {
                Ok(config) => {
                    info!("Loaded configuration from current directory: config.json");
                    return Ok(config);
                },
                Err(e) => debug!("Failed to parse current directory config: {}", e)
            }
        } else {
            debug!("No config found in current directory");
        }
        
        // If no config file found, return an error
        Err(anyhow::anyhow!("No config file found. Expected either ~/.engines.config.json, ./config.json, or config path provided via -c flag"))
    }

    pub fn default() -> Self {
        Self {
            anthropic_api_key: "".to_string(),
            model: default_model(),
            relevance: RelevanceConfig::default(),
            ranking: RankingConfig::default(),
            codebase: CodebaseConfig {
                path: PathBuf::from("."),
                problem_id: "custom_problem".to_string(),
                problem_statement: "Please analyze this codebase".to_string(),
                exclusions_path: "exclusions.json".to_string(),
            },
            dockerfile: DockerfileConfig::default(),
            scripts: ScriptConfig::default(),
            chat: ChatConfig::default(),
            container: ContainerConfig::default(),
            observability: ObservabilityConfig::default(),
            output_path: Some(".engines".to_string()),
        }
    }
    
    /// Get the effective model name for a stage
    pub fn get_model_for_stage(&self, stage_model: &Option<String>) -> String {
        match stage_model {
            Some(model) => model.clone(),
            None => self.model.clone(),
        }
    }
    
    /// Convert to the LLMConfig format needed by LLM clients
    pub fn to_llm_config(&self, stage_model: &Option<String>) -> LLMConfig {
        let model = self.get_model_for_stage(stage_model);
        
        LLMConfig {
            model_type: "anthropic".to_string(),
            model,
            api_key: self.anthropic_api_key.clone(),
            base_url: None,
            timeout: 60, // Fixed default timeout
            max_retries: 3, // Fixed default max retries
        }
    }
    
    /// Get the output directory path
    pub fn get_output_dir(&self) -> String {
        self.output_path.clone().unwrap_or_else(|| ".engines".to_string())
    }
    
    /// Get the trajectory store directory (shared across all problems)
    pub fn get_trajectory_dir(&self, _problem_id: &str) -> String {
        self.get_output_dir()
    }
    
    /// Get the Dockerfile path for a given problem
    pub fn get_dockerfile_path(&self, _problem_id: &str) -> String {
        format!("{}/Dockerfile", self.get_output_dir())
    }
    
    /// Get the scripts directory for a given problem
    pub fn get_scripts_dir(&self, _problem_id: &str) -> String {
        self.get_output_dir()
    }
}