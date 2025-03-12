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
            exclude_dirs: Vec::new(),
            cached_paths: Vec::new(),
        }
    }
    
    /// Set the codebase path
    pub fn with_codebase_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.codebase_path = Some(path.as_ref().to_path_buf());
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
        
        false
    }
    
    /// Get all file paths in the codebase
    pub fn all_file_paths(&self) -> Vec<String> {
        self.cached_paths.clone()
    }
    
    /// Generate a tree-like representation of the codebase
    pub fn generate_tree(&self) -> String {
        if self.codebase_path.is_none() {
            return String::new();
        }
        
        let _codebase_path = self.codebase_path.as_ref().unwrap();
        let mut result = String::new();
        
        // Create a map of directories to their files and subdirectories
        let mut dir_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        
        for path in &self.cached_paths {
            let parent = path.rfind('/').map_or("", |i| &path[0..i]);
            dir_map.entry(parent.to_string())
                .or_default()
                .push(path.clone());
        }
        
        // Function to print a directory tree recursively
        fn print_dir(
            dir: &str,
            dir_map: &std::collections::HashMap<String, Vec<String>>,
            result: &mut String,
            prefix: &str,
            is_last: bool,
        ) {
            // Print directory name
            let dir_name = if dir.is_empty() {
                "."
            } else {
                dir.split('/').last().unwrap_or(dir)
            };
            
            let branch = if is_last { "└── " } else { "├── " };
            result.push_str(&format!("{}{}{}/\n", prefix, branch, dir_name));
            
            // Prepare the prefix for children
            let child_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };
            
            // Get files and subdirectories in this directory
            let mut entries = Vec::new();
            if let Some(files) = dir_map.get(dir) {
                for file in files {
                    if file.starts_with(dir) && file != dir {
                        let rel_path = if dir.is_empty() {
                            file.clone()
                        } else {
                            file[dir.len() + 1..].to_string()
                        };
                        
                        if !rel_path.contains('/') {
                            // This is a file
                            entries.push((rel_path, false));
                        } else {
                            // This is a subdirectory
                            let subdir = rel_path.split('/').next().unwrap_or("");
                            let _full_subdir = if dir.is_empty() {
                                subdir.to_string()
                            } else {
                                format!("{}/{}", dir, subdir)
                            };
                            
                            // Only add the directory if we haven't added it already
                            if !entries.iter().any(|(name, _)| name == subdir) {
                                entries.push((subdir.to_string(), true));
                            }
                        }
                    }
                }
            }
            
            // Sort entries: directories first, then files
            entries.sort_by(|a, b| {
                match (a.1, b.1) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.0.cmp(&b.0),
                }
            });
            
            // Print entries
            for (i, (name, is_dir)) in entries.iter().enumerate() {
                let is_last_entry = i == entries.len() - 1;
                
                if *is_dir {
                    let full_path = if dir.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", dir, name)
                    };
                    
                    print_dir(&full_path, dir_map, result, &child_prefix, is_last_entry);
                } else {
                    let branch = if is_last_entry { "└── " } else { "├── " };
                    result.push_str(&format!("{}{}{}\n", child_prefix, branch, name));
                }
            }
        }
        
        // Start with the root directory
        print_dir("", &dir_map, &mut result, "", true);
        
        result
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
