use std::collections::HashSet;
use std::path::{Path, PathBuf};

use codemonkeys_rs::models::exclusion::ExclusionConfig;

// Custom struct to mock a DirEntry for testing
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

// Mock file type for testing
struct MockFileType {
    is_dir: bool,
}

impl MockFileType {
    fn is_dir(&self) -> bool {
        self.is_dir
    }
}

#[tokio::test]
async fn test_exclusion_patterns() {
    // Create a mock codebase root path
    let root_path = PathBuf::from("/mock/codebase");
    
    // Create a list of mock file entries
    let mock_entries = vec![
        // Regular files and directories
        MockDirEntry::new(root_path.join("src"), true),
        MockDirEntry::new(root_path.join("src/main.rs"), false),
        MockDirEntry::new(root_path.join("docs"), true),
        MockDirEntry::new(root_path.join("docs/readme.md"), false),
        MockDirEntry::new(root_path.join("README.md"), false),
        
        // Excluded files by extension
        MockDirEntry::new(root_path.join("assets"), true),
        MockDirEntry::new(root_path.join("assets/logo.png"), false),
        MockDirEntry::new(root_path.join("assets/sound.mp3"), false),
        MockDirEntry::new(root_path.join("docs/manual.pdf"), false),
        MockDirEntry::new(root_path.join("src/script.min.js"), false),
        
        // Excluded files by name
        MockDirEntry::new(root_path.join("package-lock.json"), false),
        
        // Excluded directories and their files
        MockDirEntry::new(root_path.join(".git"), true),
        MockDirEntry::new(root_path.join(".git/config"), false),
        MockDirEntry::new(root_path.join(".vscode"), true),
        MockDirEntry::new(root_path.join(".vscode/settings.json"), false),
        MockDirEntry::new(root_path.join("node_modules"), true),
        MockDirEntry::new(root_path.join("node_modules/package.json"), false),
    ];
    
    // Create a test-specific exclusion config that doesn't exclude the test directory
    let mut exclusion_config = ExclusionConfig::default();
    // Make sure our test-specific config doesn't include "tests" in directories_to_skip
    exclusion_config.directories_to_skip.retain(|dir| dir != "tests");
    
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
    assert!(
        file_paths_set.contains("src/main.rs"), 
        "Should contain src/main.rs. Found paths: {:?}", file_paths_set
    );
    assert!(
        file_paths_set.contains("docs/readme.md"), 
        "Should contain docs/readme.md. Found paths: {:?}", file_paths_set
    );
    assert!(
        file_paths_set.contains("README.md"), 
        "Should contain README.md. Found paths: {:?}", file_paths_set
    );
    
    // Verify files with excluded extensions are not included
    assert!(!file_paths_set.contains("assets/logo.png"), "Should not contain assets/logo.png");
    assert!(!file_paths_set.contains("assets/sound.mp3"), "Should not contain assets/sound.mp3");
    assert!(!file_paths_set.contains("docs/manual.pdf"), "Should not contain docs/manual.pdf");
    assert!(!file_paths_set.contains("src/script.min.js"), "Should not contain src/script.min.js");
    
    // Verify excluded files by name are not included
    assert!(!file_paths_set.contains("package-lock.json"), "Should not contain package-lock.json");
    
    // Verify files in excluded directories are not included
    assert!(!file_paths_set.contains(".git/config"), "Should not contain .git/config");
    assert!(!file_paths_set.contains(".vscode/settings.json"), "Should not contain .vscode/settings.json");
    assert!(!file_paths_set.contains("node_modules/package.json"), "Should not contain node_modules/package.json");
    
    // Verify no paths start with the excluded directory prefixes
    assert!(file_paths_set.iter().all(|path| !path.starts_with(".git/")), "No paths should start with .git/");
    assert!(file_paths_set.iter().all(|path| !path.starts_with(".vscode/")), "No paths should start with .vscode/");
    assert!(file_paths_set.iter().all(|path| !path.starts_with("node_modules/")), "No paths should start with node_modules/");
    assert!(file_paths_set.iter().all(|path| !path.starts_with("assets/")), "No paths should start with assets/");

    // Verify that excluded extensions are truly excluded
    assert!(file_paths_set.iter().all(|path| !path.ends_with(".png")), "No paths should end with .png");
    assert!(file_paths_set.iter().all(|path| !path.ends_with(".mp3")), "No paths should end with .mp3");
    assert!(file_paths_set.iter().all(|path| !path.ends_with(".pdf")), "No paths should end with .pdf");
    assert!(file_paths_set.iter().all(|path| !path.contains(".min.js")), "No paths should contain .min.js");
}