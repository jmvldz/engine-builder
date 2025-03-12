use std::fs::{self, File};
use std::io::Write;
use std::collections::HashSet;
use tempfile::tempdir;

use codemonkeys_rs::models::problem::SWEBenchProblem;
use codemonkeys_rs::models::exclusion::ExclusionConfig;

#[tokio::test]
async fn test_exclusion_patterns() {
    // Create a temporary directory structure
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();
    
    // Create normal directories and files
    fs::create_dir_all(temp_path.join("src")).expect("Failed to create src dir");
    fs::create_dir_all(temp_path.join("docs")).expect("Failed to create docs dir");
    fs::create_dir_all(temp_path.join("assets")).expect("Failed to create assets dir");
    fs::create_dir_all(temp_path.join("node_modules")).expect("Failed to create node_modules dir");
    fs::create_dir_all(temp_path.join(".git")).expect("Failed to create .git dir");
    fs::create_dir_all(temp_path.join(".vscode")).expect("Failed to create .vscode dir");
    
    // Create source files
    let mut src_file = File::create(temp_path.join("src/main.rs")).expect("Failed to create src file");
    src_file.write_all(b"fn main() {}").expect("Failed to write to src file");
    
    // Create documentation files
    let mut docs_file = File::create(temp_path.join("docs/readme.md")).expect("Failed to create docs file");
    docs_file.write_all(b"# Documentation").expect("Failed to write to docs file");
    
    // Create various files that should be excluded
    // Image file
    let mut image_file = File::create(temp_path.join("assets/logo.png")).expect("Failed to create image file");
    image_file.write_all(b"fake png data").expect("Failed to write to image file");
    
    // Audio file
    let mut audio_file = File::create(temp_path.join("assets/sound.mp3")).expect("Failed to create audio file");
    audio_file.write_all(b"fake mp3 data").expect("Failed to write to audio file");
    
    // Document file
    let mut doc_file = File::create(temp_path.join("docs/manual.pdf")).expect("Failed to create pdf file");
    doc_file.write_all(b"fake pdf data").expect("Failed to write to pdf file");
    
    // Minified file
    let mut min_file = File::create(temp_path.join("src/script.min.js")).expect("Failed to create minified file");
    min_file.write_all(b"console.log('minified')").expect("Failed to write to minified file");
    
    // Package lock file
    let mut lock_file = File::create(temp_path.join("package-lock.json")).expect("Failed to create lock file");
    lock_file.write_all(b"{}").expect("Failed to write to lock file");
    
    // Git config file
    let mut git_config = File::create(temp_path.join(".git/config")).expect("Failed to create git config file");
    git_config.write_all(b"[core]").expect("Failed to write to git config file");
    
    // VS Code settings file
    let mut vscode_file = File::create(temp_path.join(".vscode/settings.json")).expect("Failed to create vscode file");
    vscode_file.write_all(b"{}").expect("Failed to write to vscode file");
    
    // Node modules file
    let mut node_file = File::create(temp_path.join("node_modules/package.json")).expect("Failed to create node_modules file");
    node_file.write_all(b"{}").expect("Failed to write to node_modules file");
    
    // Normal file that shouldn't be excluded
    let mut normal_file = File::create(temp_path.join("README.md")).expect("Failed to create README file");
    normal_file.write_all(b"# Project").expect("Failed to write to README file");
    
    // Initialize the problem with the temp directory and exclusion patterns
    let exclusion_config = ExclusionConfig::default();
    let mut problem = SWEBenchProblem::new("test-exclusion".to_string(), "Testing exclusion patterns".to_string())
        .with_codebase_path(temp_path)
        .with_exclusion_config(exclusion_config);
    
    // Initialize the problem (which scans the codebase)
    problem.initialize().expect("Failed to initialize problem");
    
    // Get all file paths found during scan
    let file_paths = problem.all_file_paths();
    let file_paths_set: HashSet<String> = file_paths.into_iter().collect();
    
    // Verify that we found the expected normal files
    assert!(file_paths_set.contains("src/main.rs"), "Should contain src/main.rs");
    assert!(file_paths_set.contains("docs/readme.md"), "Should contain docs/readme.md");
    assert!(file_paths_set.contains("README.md"), "Should contain README.md");
    
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