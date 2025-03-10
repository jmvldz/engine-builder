use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// A ranked file in the codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedCodebaseFile {
    /// Path to the file, relative to the codebase root
    pub path: String,
    
    /// Token count of the file
    pub tokens: usize,
}

/// A ranking of files by relevance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRanking {
    /// The full message from the LLM
    pub message: String,
    
    /// The ranked list of file paths
    pub ranking: Vec<String>,
}

/// Data about a file for inclusion in a prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevantFileDataForPrompt {
    /// Path to the file, relative to the codebase root
    pub path: String,
    
    /// A summary of why the file is relevant
    pub summary: String,
    
    /// Token count of the file
    pub token_count: usize,
}

/// The context for a problem, including ranked files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemContext {
    /// The rankings from the model
    pub model_rankings: Vec<FileRanking>,
    
    /// The final ranked list of files
    pub ranked_files: Vec<RankedCodebaseFile>,
    
    /// Usage data from the LLM API
    pub prompt_caching_usages: Vec<HashMap<String, serde_json::Value>>,
}