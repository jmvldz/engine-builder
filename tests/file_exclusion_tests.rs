use std::collections::HashSet;
use std::path::{Path, PathBuf};

use engine_builder::models::exclusion::ExclusionConfig;

// Mock DirEntry and FileType for testing without accessing the file system
struct MockDirEntry {
    path: PathBuf,
    is_dir: bool,
}

impl MockDirEntry {
    fn new<P: AsRef<Path>>(path: P, is_dir: bool) -> Self {
        MockDirEntry {
            path: path.as_ref().to_path_buf(),
            is_dir,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn file_type(&self) -> MockFileType {
        MockFileType { is_dir: self.is_dir }
    }
}

struct MockFileType {
    is_dir: bool,
}

impl MockFileType {
    fn is_dir(&self) -> bool {
        self.is_dir
    }
}

#[tokio::test]
async fn test_git_directory_exclusion() {
    // Create a mock codebase root path
    let root_path = PathBuf::from("/mock/codebase");
    
    // Create mock file entries
    let mock_entries = vec![
        // Normal directories and files
        MockDirEntry::new(root_path.join("src"), true),
        MockDirEntry::new(root_path.join("src/main.rs"), false),
        MockDirEntry::new(root_path.join("docs"), true),
        MockDirEntry::new(root_path.join("docs/readme.md"), false),
        
        // .git directory and files
        MockDirEntry::new(root_path.join(".git"), true),
        MockDirEntry::new(root_path.join(".git/config"), false),
        MockDirEntry::new(root_path.join(".git/objects"), true),
        MockDirEntry::new(root_path.join(".git/objects/somehash"), false),
    ];
    
    // Create exclusion config
    let exclusion_config = ExclusionConfig::default();
    
    // Apply the exclusion filter to the mock files
    let filtered_files: Vec<String> = mock_entries.iter()
        .filter(|entry| !entry.file_type().is_dir()) // Filter out directories
        .filter(|entry| !exclusion_config.should_exclude(entry.path())) // Apply exclusion filter
        .filter_map(|entry| {
            entry.path().strip_prefix(&root_path).ok() // Remove the root path prefix
                .and_then(|rel_path| rel_path.to_str()) // Convert to string
                .map(|s| s.to_string()) // Create owned string
        })
        .collect();
    
    // Create a set of found file paths for easier lookup
    let file_paths_set: HashSet<String> = filtered_files.into_iter().collect();
    
    // Verify that we found the expected normal files
    assert!(file_paths_set.contains("src/main.rs"), "Should contain src/main.rs");
    assert!(file_paths_set.contains("docs/readme.md"), "Should contain docs/readme.md");
    
    // Verify that .git files are not included
    assert!(!file_paths_set.contains(".git/config"), "Should not contain .git/config");
    assert!(!file_paths_set.contains(".git/objects/somehash"), "Should not contain .git/objects/somehash");
    
    // Verify no path starts with .git/
    assert!(file_paths_set.iter().all(|path| !path.starts_with(".git/")), 
            "No paths should start with .git/");
}

#[tokio::test]
#[ignore]
async fn test_gitignore_exclusion() {
    // Create a mock codebase root path
    let root_path = PathBuf::from("/mock/codebase");
    
    // Create mock file entries
    let mock_entries = vec![
        // Regular directories and files
        MockDirEntry::new(root_path.join("src"), true),
        MockDirEntry::new(root_path.join("src/main.rs"), false),
        MockDirEntry::new(root_path.join("README.md"), false),
        
        // Files that should be excluded by gitignore patterns
        MockDirEntry::new(root_path.join("target"), true),
        MockDirEntry::new(root_path.join("target/debug.log"), false),
        MockDirEntry::new(root_path.join("node_modules"), true),
        MockDirEntry::new(root_path.join("node_modules/package.json"), false),
        MockDirEntry::new(root_path.join("application.log"), false),
    ];
    
    // Create a custom exclusion config for testing gitignore patterns
    let exclusion_config = ExclusionConfig::default();
    // TODO: In a real implementation, we would need to add a way to parse the mock gitignore content
    // For now, we'll rely on the default exclusion config which should already exclude these patterns
    
    // Apply the exclusion filter to the mock files
    let filtered_files: Vec<String> = mock_entries.iter()
        .filter(|entry| !entry.file_type().is_dir()) // Filter out directories
        .filter(|entry| !exclusion_config.should_exclude(entry.path())) // Apply exclusion filter
        .filter_map(|entry| {
            entry.path().strip_prefix(&root_path).ok() // Remove the root path prefix
                .and_then(|rel_path| rel_path.to_str()) // Convert to string
                .map(|s| s.to_string()) // Create owned string
        })
        .collect();
    
    // Create a set of found file paths for easier lookup
    let file_paths_set: HashSet<String> = filtered_files.into_iter().collect();
    
    // Verify that we found the expected normal files
    assert!(file_paths_set.contains("src/main.rs"), "Should contain src/main.rs");
    assert!(file_paths_set.contains("README.md"), "Should contain README.md");
    
    // Verify that files from .gitignore are excluded
    assert!(!file_paths_set.contains("target/debug.log"), "Should not contain target/debug.log");
    assert!(!file_paths_set.contains("node_modules/package.json"), "Should not contain node_modules/package.json");
    assert!(!file_paths_set.contains("application.log"), "Should not contain application.log");
    
    // Verify no paths start with the ignored directory prefixes
    assert!(file_paths_set.iter().all(|path| !path.starts_with("target/")), 
            "No paths should start with target/");
    assert!(file_paths_set.iter().all(|path| !path.starts_with("node_modules/")), 
            "No paths should start with node_modules/");
}