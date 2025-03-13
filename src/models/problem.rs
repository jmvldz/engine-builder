use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};
use log::{info, debug};
use serde::{Deserialize, Serialize};
use walkdir::{WalkDir, DirEntry};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::file::CodebaseFile;
use super::exclusion::ExclusionConfig;

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
    
    /// Exclusion config (not serialized)
    #[serde(skip)]
    pub exclusion_config: ExclusionConfig,
    
    /// Cached file paths (not serialized)
    #[serde(skip)]
    cached_paths: Vec<String>,
    
    /// Gitignore patterns (not serialized)
    #[serde(skip)]
    gitignore: Option<Gitignore>,
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
            exclusion_config: ExclusionConfig::default(),
            cached_paths: Vec::new(),
            gitignore: None,
        }
    }
    
    /// Set the codebase path
    pub fn with_codebase_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.codebase_path = Some(path.as_ref().to_path_buf());
        self
    }
    
    /// Set exclusion config
    pub fn with_exclusion_config(mut self, config: ExclusionConfig) -> Self {
        self.exclusion_config = config;
        self
    }
    
    /// Initialize the problem by scanning the codebase
    pub fn initialize(&mut self) -> Result<()> {
        if self.codebase_path.is_none() {
            return Ok(());
        }
        
        let codebase_path = self.codebase_path.as_ref().unwrap();
        
        info!("Starting file tree traversal at: {:?}", codebase_path);
        
        // Load gitignore file if it exists
        let gitignore_path = codebase_path.join(".gitignore");
        if gitignore_path.exists() {
            info!("Found .gitignore file at: {:?}", gitignore_path);
            match self.load_gitignore(&gitignore_path, codebase_path) {
                Ok(gitignore) => {
                    info!("Successfully loaded .gitignore patterns");
                    self.gitignore = Some(gitignore);
                },
                Err(e) => {
                    info!("Failed to load .gitignore: {:?}", e);
                }
            }
        } else {
            info!("No .gitignore file found at: {:?}", gitignore_path);
        }
        
        // Scan for files
        let mut paths = Vec::new();
        let mut file_count = 0;
        let mut dir_count = 0;
        let mut excluded_count = 0;
        
        for entry in WalkDir::new(codebase_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| {
                if let Ok(entry) = e {
                    if entry.file_type().is_dir() {
                        debug!("Exploring directory: {:?}", entry.path());
                        dir_count += 1;
                    }
                    Some(entry)
                } else {
                    info!("Error accessing path: {:?}", e);
                    None
                }
            })
            .filter(|e| {
                let should_include = !self.should_exclude(e);
                if !should_include {
                    debug!("Excluding path: {:?}", e.path());
                    excluded_count += 1;
                }
                should_include
            })
        {
            if entry.file_type().is_file() {
                debug!("Found file: {:?}", entry.path());
                file_count += 1;
                
                if let Ok(path) = entry.path().strip_prefix(codebase_path) {
                    if let Some(path_str) = path.to_str() {
                        paths.push(path_str.to_string());
                    }
                }
            }
        }
        
        self.cached_paths = paths;
        info!("File tree traversal complete: {} directories, {} files processed, {} paths excluded", 
              dir_count, file_count, excluded_count);
        
        Ok(())
    }
    
    /// Load gitignore patterns from a .gitignore file
    fn load_gitignore(&self, gitignore_path: &Path, codebase_path: &Path) -> Result<Gitignore> {
        let mut builder = GitignoreBuilder::new(codebase_path);
        
        info!("Loading gitignore from path: {:?}", gitignore_path);
        
        // Read the gitignore file content for debugging
        if let Ok(content) = std::fs::read_to_string(gitignore_path) {
            info!("Gitignore content:\n{}", content);
        }
        
        // GitignoreBuilder.add returns Option<()>, where None means success
        match builder.add(gitignore_path) {
            Some(err) => {
                info!("Failed to add gitignore file: {}", err);
                return Err(anyhow::anyhow!("Failed to add gitignore file: {}", err));
            },
            None => {
                info!("Successfully added gitignore file");
            },
        }
        
        // builder.build() returns Result<Gitignore, ignore::Error>
        let gitignore = match builder.build() {
            Ok(gitignore) => {
                info!("Successfully built gitignore");
                gitignore
            },
            Err(e) => {
                info!("Failed to build gitignore: {}", e);
                return Err(anyhow::anyhow!("Failed to build gitignore: {}", e));
            }
        };
        
        // Test that the gitignore patterns work correctly
        let test_paths = vec![
            "target/test.txt",
            "node_modules/file.js",
            "example.log",
            "src/main.rs",
        ];
        
        for test_path in test_paths {
            let path = codebase_path.join(test_path);
            let is_dir = path.is_dir();
            let match_result = gitignore.matched(&path, is_dir);
            info!("Testing gitignore match for {}: {:?}", test_path, match_result);
        }
            
        Ok(gitignore)
    }
    
    /// Check if a directory entry should be excluded
    pub fn should_exclude(&self, entry: &DirEntry) -> bool {
        let path = entry.path();
        
        // Apply pattern-based exclusions first
        if self.exclusion_config.should_exclude(path) {
            debug!("Excluding path based on exclusion patterns: {:?}", path);
            return true;
        }
        
        // Check if file matches gitignore patterns
        if let Some(gitignore) = &self.gitignore {
            let is_match = gitignore.matched(path, entry.file_type().is_dir());
            if is_match.is_ignore() {
                debug!("Excluding due to .gitignore match: {:?}", path);
                return true;
            }
        }
        
        // Skip hidden files and directories (if not already excluded by gitignore or patterns)
        if entry.file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
        {
            debug!("Excluding hidden file/directory: {:?}", path);
            return true;
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
            info!("Cannot generate tree: codebase path not set");
            return String::new();
        }
        
        let codebase_path = self.codebase_path.as_ref().unwrap();
        info!("Generating tree representation for codebase at: {:?}", codebase_path);
        info!("Total files to include in tree: {}", self.cached_paths.len());
        
        let mut result = String::new();
        
        // Create a set of all directories based on file paths
        let mut all_dirs = std::collections::HashSet::new();
        
        // First gather all directories from file paths
        for path in &self.cached_paths {
            let mut parts = path.split('/').collect::<Vec<_>>();
            parts.pop(); // Remove the filename
            
            // Add all parent directories
            let mut current_path = String::new();
            for part in parts {
                if !current_path.is_empty() {
                    current_path.push('/');
                }
                current_path.push_str(part);
                all_dirs.insert(current_path.clone());
            }
        }
        
        // Also traverse filesystem to find all directories, including empty ones
        if let Some(codebase_path) = &self.codebase_path {
            use walkdir::WalkDir;
            
            for entry in WalkDir::new(codebase_path)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| !self.should_exclude(e) && e.file_type().is_dir())
            {
                if let Ok(rel_path) = entry.path().strip_prefix(codebase_path) {
                    if let Some(path_str) = rel_path.to_str() {
                        if !path_str.is_empty() {
                            all_dirs.insert(path_str.to_string());
                        }
                    }
                }
            }
        }
        
        info!("Found {} total directories", all_dirs.len());
        
        // Create a map of directories to their files
        let mut files_by_dir: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        
        // Add files to their respective directories
        for path in &self.cached_paths {
            let parent = path.rfind('/').map_or("", |i| &path[0..i]);
            info!("Adding file to tree: {} (parent directory: {})", path, parent);
            files_by_dir.entry(parent.to_string())
                .or_default()
                .push(path.clone());
        }
        
        // Create a map of parent directories to their direct subdirectories
        let mut subdirs_by_dir: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        
        // Map each directory to its parent
        for dir in &all_dirs {
            if let Some(last_slash) = dir.rfind('/') {
                let parent = &dir[0..last_slash];
                subdirs_by_dir.entry(parent.to_string())
                    .or_default()
                    .push(dir.clone());
            } else {
                // Top-level directory, add to root
                subdirs_by_dir.entry(String::new())
                    .or_default()
                    .push(dir.clone());
            }
        }
        
        // Function to print a directory tree recursively
        fn print_dir(
            dir: &str,
            files_by_dir: &std::collections::HashMap<String, Vec<String>>,
            subdirs_by_dir: &std::collections::HashMap<String, Vec<String>>,
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
            
            info!("Building tree for directory: {}", dir_name);
            
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
            
            // Add subdirectories
            if let Some(subdirs) = subdirs_by_dir.get(dir) {
                for subdir in subdirs {
                    let name = if let Some(last_slash) = subdir.rfind('/') {
                        &subdir[last_slash + 1..]
                    } else {
                        subdir
                    };
                    
                    entries.push((name.to_string(), true));
                }
            }
            
            // Add files
            if let Some(files) = files_by_dir.get(dir) {
                for file in files {
                    if file.starts_with(dir) && file != dir {
                        let rel_path = if dir.is_empty() {
                            file.clone()
                        } else if file.len() > dir.len() + 1 && file.as_bytes()[dir.len()] == b'/' {
                            file[dir.len() + 1..].to_string()
                        } else {
                            continue; // Not directly under this directory
                        };
                        
                        if !rel_path.contains('/') {
                            // This is a file
                            entries.push((rel_path, false));
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
                    
                    print_dir(&full_path, files_by_dir, subdirs_by_dir, result, &child_prefix, is_last_entry);
                } else {
                    let branch = if is_last_entry { "└── " } else { "├── " };
                    result.push_str(&format!("{}{}{}\n", child_prefix, branch, name));
                }
            }
        }
        
        // Start with the root directory
        print_dir("", &files_by_dir, &subdirs_by_dir, &mut result, "", true);
        
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