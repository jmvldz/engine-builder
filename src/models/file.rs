use serde::{Deserialize, Serialize};
use glob::Pattern;

/// Represents a file in the codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseFile {
    /// Path to the file, relative to the codebase root
    pub path: String,
    
    /// Content of the file
    pub content: String,
}

impl CodebaseFile {
    /// Create a new codebase file
    pub fn new(path: String, content: String) -> Self {
        Self { path, content }
    }
    
    /// Get the file extension
    pub fn extension(&self) -> Option<&str> {
        self.path.split('.').last()
    }
    
    /// Check if the file is a Python file
    pub fn is_python(&self) -> bool {
        self.extension() == Some("py")
    }
}

/// Represents a file pattern selection returned by the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePatternSelection {
    /// The list of file patterns to include
    pub patterns: Vec<String>,
}

impl FilePatternSelection {
    /// Create a new file pattern selection
    pub fn new(patterns: Vec<String>) -> Self {
        Self { patterns }
    }
    
    /// Check if a file path matches any of the patterns
    pub fn matches(&self, file_path: &str) -> bool {
        for pattern in &self.patterns {
            // Check for exact file match
            if pattern == file_path {
                return true;
            }
            
            // Check if the file is in a specified directory
            if pattern.ends_with('/') && file_path.starts_with(pattern) {
                return true;
            }
            
            // Check for glob pattern match
            if pattern.contains('*') {
                if let Ok(glob_pattern) = Pattern::new(pattern) {
                    if glob_pattern.matches(file_path) {
                        return true;
                    }
                }
            }
        }
        
        false
    }
}