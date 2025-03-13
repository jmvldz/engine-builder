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

#[test]
fn test_git_directory_excluded_in_file_scan() {
    // Create a mock codebase root path
    let root_path = PathBuf::from("/mock/codebase");
    
    // Create mock file entries
    let mock_entries = vec![
        // Normal directories and files
        MockDirEntry::new(root_path.join("src"), true),
        MockDirEntry::new(root_path.join("src/main.rs"), false),
        MockDirEntry::new(root_path.join("docs"), true),
        MockDirEntry::new(root_path.join("docs/README.md"), false),
        
        // .git directory and files
        MockDirEntry::new(root_path.join(".git"), true),
        MockDirEntry::new(root_path.join(".git/config"), false),
        MockDirEntry::new(root_path.join(".git/HEAD"), false),
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
    
    // Test that normal files are included
    assert!(file_paths_set.contains("src/main.rs"), "src/main.rs should be included");
    assert!(file_paths_set.contains("docs/README.md"), "docs/README.md should be included");
    
    // Test that .git files are excluded
    assert!(!file_paths_set.contains(".git/config"), ".git/config should be excluded");
    assert!(!file_paths_set.contains(".git/HEAD"), ".git/HEAD should be excluded");
    assert!(!file_paths_set.contains(".git/objects/somehash"), ".git/objects/somehash should be excluded");
    
    // Check that no file path starts with .git/
    for path in &file_paths_set {
        assert!(!path.starts_with(".git/"), "No file should start with .git/");
    }
    
    // Since we're using a mock approach, we can't generate a tree to verify it doesn't contain .git entries
    // That would require actual filesystem access, which we're avoiding in this test
}