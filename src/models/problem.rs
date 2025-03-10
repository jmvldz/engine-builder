use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use walkdir::{WalkDir, DirEntry};

use super::file::CodebaseFile;

/// Represents a problem from the SWE-bench dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SWEBenchProblem {
    /// Unique identifier for the problem
    pub id: String,
    
    /// The problem statement (usually a GitHub issue)
    pub problem_statement: String,
    
    /// Additional metadata about the problem
    pub metadata: HashMap<String, String>,
    
    /// Cache of files in the codebase (lazy-loaded)
    #[serde(skip)]
    file_cache: HashMap<String, CodebaseFile>,
    
    /// Codebase root path (not serialized)
    #[serde(skip)]
    codebase_path: Option<PathBuf>,
    
    /// File extensions to include (not serialized)
    #[serde(skip)]
    pub include_extensions: Vec<String>,
    
    /// Directories to exclude (not serialized)
    #[serde(skip)]
    pub exclude_dirs: Vec<String>,
    
    /// Cached file paths (not serialized)
    #[serde(skip)]
    cached_paths: Vec<String>,
}

impl SWEBenchProblem {
    /// Create a new SWE-bench problem
    pub fn new(id: String, problem_statement: String) -> Self {
        Self {
            id,
            problem_statement,
            metadata: HashMap::new(),
            file_cache: HashMap::new(),
            codebase_path: None,
            include_extensions: Vec::new(),
            exclude_dirs: Vec::new(),
            cached_paths: Vec::new(),
        }
    }
    
    /// Set the codebase path
    pub fn with_codebase_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.codebase_path = Some(path.as_ref().to_path_buf());
        self
    }
    
    /// Set extensions to include
    pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
        self.include_extensions = extensions;
        self
    }
    
    /// Set directories to exclude
    pub fn with_exclude_dirs(mut self, dirs: Vec<String>) -> Self {
        self.exclude_dirs = dirs;
        self
    }
    
    /// Initialize the problem by scanning the codebase
    pub fn initialize(&mut self) -> Result<()> {
        if self.codebase_path.is_none() {
            return Ok(());
        }
        
        let codebase_path = self.codebase_path.as_ref().unwrap();
        
        // Scan for files
        let mut paths = Vec::new();
        
        for entry in WalkDir::new(codebase_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| !self.should_exclude(e))
        {
            if entry.file_type().is_file() {
                if let Ok(path) = entry.path().strip_prefix(codebase_path) {
                    if let Some(path_str) = path.to_str() {
                        paths.push(path_str.to_string());
                    }
                }
            }
        }
        
        self.cached_paths = paths;
        Ok(())
    }
    
    /// Check if a directory entry should be excluded
    fn should_exclude(&self, entry: &DirEntry) -> bool {
        // Skip hidden files and directories
        if entry.file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
        {
            return true;
        }
        
        // Check if this entry or any of its parent directories are in the exclude list
        let path = entry.path();
        for ancestor in path.ancestors() {
            if let Some(dir_name) = ancestor.file_name() {
                if let Some(dir_str) = dir_name.to_str() {
                    if self.exclude_dirs.contains(&dir_str.to_string()) {
                        return true;
                    }
                }
            }
        }
        
        // Check file extension
        if entry.file_type().is_file() && !self.include_extensions.is_empty() {
            if let Some(extension) = entry.path().extension() {
                if let Some(ext_str) = extension.to_str() {
                    return !self.include_extensions.contains(&ext_str.to_string());
                }
            }
            return true; // No extension means exclude
        }
        
        false
    }
    
    /// Get all file paths in the codebase
    pub fn all_file_paths(&self) -> Vec<String> {
        self.cached_paths.clone()
    }
    
    /// Get a specific file from the codebase
    pub fn get_file(&mut self, path: &str) -> Result<&CodebaseFile> {
        if !self.file_cache.contains_key(path) {
            let content = if let Some(codebase_path) = &self.codebase_path {
                let full_path = codebase_path.join(path);
                fs::read_to_string(&full_path)
                    .context(format!("Failed to read file: {:?}", full_path))?
            } else {
                return Err(anyhow::anyhow!("Codebase path not set"));
            };
            
            let file = CodebaseFile::new(path.to_string(), content);
            self.file_cache.insert(path.to_string(), file);
        }
        
        Ok(self.file_cache.get(path).unwrap())
    }
}