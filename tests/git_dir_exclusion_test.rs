use std::fs::{self, File};
use std::io::Write;
use std::collections::HashSet;
use tempfile::tempdir;

use codemonkeys_rs::models::problem::SWEBenchProblem;

#[test]
fn test_git_directory_excluded_in_file_scan() {
    // Create a temporary directory structure
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    // Create a mock codebase structure
    fs::create_dir_all(temp_path.join("src")).expect("Failed to create src dir");
    fs::create_dir_all(temp_path.join("docs")).expect("Failed to create docs dir");
    fs::create_dir_all(temp_path.join(".git/objects")).expect("Failed to create .git dir");
    
    // Create some files in each directory
    let mut src_file = File::create(temp_path.join("src/main.rs")).expect("Failed to create src file");
    src_file.write_all(b"fn main() {}").expect("Failed to write src file");
    
    let mut docs_file = File::create(temp_path.join("docs/README.md")).expect("Failed to create docs file");
    docs_file.write_all(b"# Documentation").expect("Failed to write docs file");
    
    // Create some files in .git to ensure they're excluded
    let mut git_config = File::create(temp_path.join(".git/config")).expect("Failed to create .git config");
    git_config.write_all(b"[core]\n\trepositoryformatversion = 0").expect("Failed to write .git config");
    
    let mut git_head = File::create(temp_path.join(".git/HEAD")).expect("Failed to create .git HEAD");
    git_head.write_all(b"ref: refs/heads/main").expect("Failed to write .git HEAD");
    
    let mut git_object = File::create(temp_path.join(".git/objects/somehash")).expect("Failed to create .git object");
    git_object.write_all(b"object data").expect("Failed to write .git object");
    
    // Initialize our problem with the mock codebase
    let mut problem = SWEBenchProblem::new("test-problem".to_string(), "Test problem".to_string())
        .with_codebase_path(temp_path);
    
    problem.initialize().expect("Failed to initialize problem");
    
    // Get all file paths found during scan
    let file_paths = problem.all_file_paths();
    
    // Convert to a HashSet for easier searching
    let file_paths_set: HashSet<String> = file_paths.into_iter().collect();
    
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
    
    // Generate a tree and ensure it doesn't contain .git entries
    let tree = problem.generate_tree();
    assert!(!tree.contains(".git"), "Tree should not contain .git entries");
}