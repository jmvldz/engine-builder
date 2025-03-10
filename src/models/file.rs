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