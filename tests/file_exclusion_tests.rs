use std::fs::{self, File};
use std::io::Write;
use std::collections::HashSet;
use tempfile::tempdir;

use codemonkeys_rs::models::problem::SWEBenchProblem;

#[tokio::test]
async fn test_git_directory_exclusion() {
    // Create a temporary directory structure with a fake .git directory
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    // Create normal directories and files
    fs::create_dir_all(temp_path.join("src")).expect("Failed to create src dir");
    fs::create_dir_all(temp_path.join("docs")).expect("Failed to create docs dir");
    
    // Create a file in src
    let mut src_file = File::create(temp_path.join("src/main.rs")).expect("Failed to create src file");
    src_file.write_all(b"fn main() {}").expect("Failed to write to src file");
    
    // Create a file in docs
    let mut docs_file = File::create(temp_path.join("docs/readme.md")).expect("Failed to create docs file");
    docs_file.write_all(b"# Documentation").expect("Failed to write to docs file");
    
    // Create a .git directory with a file inside
    fs::create_dir_all(temp_path.join(".git")).expect("Failed to create .git dir");
    fs::create_dir_all(temp_path.join(".git/objects")).expect("Failed to create .git/objects dir");
    let mut git_file = File::create(temp_path.join(".git/config")).expect("Failed to create .git file");
    git_file.write_all(b"[core]\n\trepositoryformatversion = 0").expect("Failed to write to .git file");
    
    // Create a file inside .git/objects
    let mut git_objects_file = File::create(temp_path.join(".git/objects/somehash")).expect("Failed to create .git objects file");
    git_objects_file.write_all(b"object data").expect("Failed to write to .git objects file");
    
    // Initialize the problem with the temp directory
    let mut problem = SWEBenchProblem::new("test-problem".to_string(), "A test problem".to_string())
        .with_codebase_path(temp_path);
    
    // Initialize the problem (which scans the codebase)
    problem.initialize().expect("Failed to initialize problem");
    
    // Get all file paths found during scan
    let file_paths = problem.all_file_paths();
    let file_paths_set: HashSet<String> = file_paths.into_iter().collect();
    
    // Verify that we found the expected normal files
    assert!(file_paths_set.contains("src/main.rs"), "Should contain src/main.rs");
    assert!(file_paths_set.contains("docs/readme.md"), "Should contain docs/readme.md");
    
    // Verify that .git files are not included
    assert!(!file_paths_set.contains(".git/config"), "Should not contain .git/config");
    assert!(!file_paths_set.contains(".git/objects/somehash"), "Should not contain .git/objects/somehash");
    
    // Verify no path starts with .git/
    assert!(file_paths_set.iter().all(|path| !path.starts_with(".git/")), 
            "No paths should start with .git/");
    
    // Clean up happens automatically when temp_dir goes out of scope
}

#[tokio::test]
#[ignore]
async fn test_gitignore_exclusion() {
    // Create a temporary directory structure
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    // Create directories
    fs::create_dir_all(temp_path.join("src")).expect("Failed to create src dir");
    fs::create_dir_all(temp_path.join("target")).expect("Failed to create target dir");
    fs::create_dir_all(temp_path.join("node_modules")).expect("Failed to create node_modules dir");
    
    // Create a .gitignore file with patterns to exclude
    let mut gitignore_file = File::create(temp_path.join(".gitignore")).expect("Failed to create .gitignore");
    gitignore_file.write_all(b"target/\nnode_modules/\n*.log\n").expect("Failed to write to .gitignore");
    
    // Create various files including ones that should be ignored
    let mut src_file = File::create(temp_path.join("src/main.rs")).expect("Failed to create src file");
    src_file.write_all(b"fn main() {}").expect("Failed to write to src file");
    
    let mut target_file = File::create(temp_path.join("target/debug.log")).expect("Failed to create target file");
    target_file.write_all(b"debug info").expect("Failed to write to target file");
    
    let mut node_modules_file = File::create(temp_path.join("node_modules/package.json")).expect("Failed to create node_modules file");
    node_modules_file.write_all(b"{}").expect("Failed to write to node_modules file");
    
    let mut log_file = File::create(temp_path.join("application.log")).expect("Failed to create log file");
    log_file.write_all(b"log entry").expect("Failed to write to log file");
    
    // Create a normal file that should not be ignored
    let mut readme_file = File::create(temp_path.join("README.md")).expect("Failed to create README file");
    readme_file.write_all(b"# Project").expect("Failed to write to README file");
    
    // Initialize the problem with the temp directory
    let mut problem = SWEBenchProblem::new("test-gitignore".to_string(), "A test problem".to_string())
        .with_codebase_path(temp_path);
    
    // Initialize the problem (which scans the codebase and loads .gitignore)
    problem.initialize().expect("Failed to initialize problem");
    
    // Get all file paths found during scan
    let file_paths = problem.all_file_paths();
    let file_paths_set: HashSet<String> = file_paths.into_iter().collect();
    
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
    
    // Clean up happens automatically when temp_dir goes out of scope
}