use engine_builder::models::exclusion::ExclusionConfig;
use engine_builder::models::problem::SWEBenchProblem;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_problem_new() {
    let id = "test_problem_1".to_string();
    let statement = "This is a test problem".to_string();
    
    let problem = SWEBenchProblem::new(id.clone(), statement.clone());
    
    assert_eq!(problem.id, id);
    assert_eq!(problem.problem_statement, statement);
    assert!(problem.metadata.is_empty());
}

#[test]
fn test_problem_with_codebase_path() {
    let problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_codebase_path("/test/path");
    
    assert!(problem.get_codebase_path().is_some());
    assert_eq!(problem.get_codebase_path().unwrap(), &PathBuf::from("/test/path"));
}

#[test]
fn test_problem_with_exclusion_config() {
    let config = ExclusionConfig::default();
    let problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_exclusion_config(config);
    
    // Just verify we can set the exclusion config - detailed exclusion tests are in other test files
    assert!(problem.exclusion_config.files_to_skip.len() > 0); // Default exclusion config has some entries
}

#[test]
fn test_initialize_empty_directory() {
    let temp_dir = tempdir().unwrap();
    
    let mut problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_codebase_path(temp_dir.path());
    
    // Initialize should work on an empty directory
    problem.initialize().unwrap();
    
    // No files should be found
    assert_eq!(problem.all_file_paths().len(), 0);
    
    temp_dir.close().unwrap();
}

#[test]
fn test_initialize_with_files() {
    let temp_dir = tempdir().unwrap();
    
    // Create a few test files
    let file1_path = temp_dir.path().join("file1.txt");
    let file2_path = temp_dir.path().join("file2.rs");
    let nested_dir = temp_dir.path().join("nested");
    fs::create_dir(&nested_dir).unwrap();
    let file3_path = nested_dir.join("file3.rs");
    
    File::create(&file1_path).unwrap().write_all(b"Test content 1").unwrap();
    File::create(&file2_path).unwrap().write_all(b"Test content 2").unwrap();
    File::create(&file3_path).unwrap().write_all(b"Test content 3").unwrap();
    
    let mut problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_codebase_path(temp_dir.path());
    
    problem.initialize().unwrap();
    
    // Should find 3 files
    let paths = problem.all_file_paths();
    assert_eq!(paths.len(), 3);
    
    // Verify all paths are found
    assert!(paths.contains(&"file1.txt".to_string()));
    assert!(paths.contains(&"file2.rs".to_string()));
    assert!(paths.contains(&"nested/file3.rs".to_string()));
    
    temp_dir.close().unwrap();
}

#[test]
fn test_initialize_with_gitignore() {
    let temp_dir = tempdir().unwrap();
    println!("Temp directory created at: {:?}", temp_dir.path());
    
    // Create a .gitignore file
    let gitignore_path = temp_dir.path().join(".gitignore");
    let mut gitignore_file = File::create(gitignore_path).unwrap();
    gitignore_file.write_all(b"*.log\nnode_modules/\n").unwrap();
    println!("Created .gitignore with content: *.log and node_modules/");
    
    // Create test files
    let file1_path = temp_dir.path().join("file1.txt");
    let file2_path = temp_dir.path().join("file2.log"); // Should be ignored
    let node_modules_dir = temp_dir.path().join("node_modules");
    fs::create_dir(&node_modules_dir).unwrap();
    let file3_path = node_modules_dir.join("file3.js"); // Should be ignored
    
    File::create(&file1_path).unwrap().write_all(b"Test content 1").unwrap();
    File::create(&file2_path).unwrap().write_all(b"Test content 2").unwrap();
    File::create(&file3_path).unwrap().write_all(b"Test content 3").unwrap();
    
    println!("Created test files:");
    println!("  - file1.txt (should be included)");
    println!("  - file2.log (should be excluded by gitignore)");
    println!("  - node_modules/file3.js (should be excluded by gitignore)");
    
    let mut problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_codebase_path(temp_dir.path());
    
    problem.initialize().unwrap();
    
    // Should only find file1.txt, the others should be excluded by gitignore
    let paths = problem.all_file_paths();
    println!("Found paths: {:?}", paths);
    
    // Filter out .gitignore from the paths for the assertion
    let filtered_paths: Vec<String> = paths.iter()
        .filter(|p| *p != ".gitignore")
        .cloned()
        .collect();
    
    println!("Filtered paths (without .gitignore): {:?}", filtered_paths);
    assert_eq!(filtered_paths.len(), 1);
    assert!(filtered_paths.contains(&"file1.txt".to_string()));
    
    temp_dir.close().unwrap();
}

#[test]
fn test_list_files_in_directory() {
    let temp_dir = tempdir().unwrap();
    
    // Create directory structure
    let dir1 = temp_dir.path().join("dir1");
    let dir2 = temp_dir.path().join("dir2");
    fs::create_dir(&dir1).unwrap();
    fs::create_dir(&dir2).unwrap();
    
    // Create files
    File::create(dir1.join("file1.txt")).unwrap().write_all(b"Test 1").unwrap();
    File::create(dir1.join("file2.txt")).unwrap().write_all(b"Test 2").unwrap();
    File::create(dir2.join("file3.txt")).unwrap().write_all(b"Test 3").unwrap();
    
    let mut problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_codebase_path(temp_dir.path());
    
    problem.initialize().unwrap();
    
    // Test listing files in dir1
    let dir1_files = problem.list_files_in_directory("dir1");
    assert_eq!(dir1_files.len(), 2);
    assert!(dir1_files.contains(&"dir1/file1.txt".to_string()));
    assert!(dir1_files.contains(&"dir1/file2.txt".to_string()));
    
    // Test listing files in dir2
    let dir2_files = problem.list_files_in_directory("dir2");
    assert_eq!(dir2_files.len(), 1);
    assert!(dir2_files.contains(&"dir2/file3.txt".to_string()));
    
    // Test listing files in nonexistent directory
    let nonexistent_files = problem.list_files_in_directory("nonexistent");
    assert_eq!(nonexistent_files.len(), 0);
    
    temp_dir.close().unwrap();
}

#[test]
fn test_get_file() {
    let temp_dir = tempdir().unwrap();
    
    // Create a test file
    let file_path = temp_dir.path().join("test.txt");
    File::create(&file_path).unwrap().write_all(b"Test content").unwrap();
    
    let mut problem = SWEBenchProblem::new("test_id".to_string(), "test statement".to_string())
        .with_codebase_path(temp_dir.path());
    
    problem.initialize().unwrap();
    
    // Get the file
    let file = problem.get_file("test.txt").unwrap();
    
    // Verify file content
    assert_eq!(file.content, "Test content");
    assert_eq!(file.path, "test.txt");
    
    temp_dir.close().unwrap();
}
