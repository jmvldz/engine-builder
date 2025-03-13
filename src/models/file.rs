use glob::Pattern;
use serde::{Deserialize, Serialize};

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
        // Normalize file_path by removing "./" prefix if it exists
        let normalized_path = file_path.strip_prefix("./").unwrap_or(file_path);

        for pattern in &self.patterns {
            // Normalize pattern by removing "./" prefix if it exists
            let normalized_pattern = pattern.strip_prefix("./").unwrap_or(pattern);

            // Check for exact file match
            if normalized_pattern == normalized_path {
                return true;
            }

            // Check if the file is in a specified directory
            if normalized_pattern.ends_with('/') && normalized_path.starts_with(normalized_pattern)
            {
                return true;
            }

            // Check for glob pattern match
            if normalized_pattern.contains('*') {
                if let Ok(glob_pattern) = Pattern::new(normalized_pattern) {
                    if glob_pattern.matches(normalized_path) {
                        return true;
                    }
                }
            }
        }

        false
    }
}
