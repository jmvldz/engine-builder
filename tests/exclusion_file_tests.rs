use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;

use engine_builder::models::exclusion::ExclusionConfig;

#[test]
fn test_loading_exclusion_file() {
    // Create a temporary directory
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create a test exclusion file
    let exclusion_file_path = temp_path.join("test_exclusions.json");
    let exclusion_content = r#"
    {
        "extensions_to_skip": [".png", ".jpg", ".pdf"],
        "files_to_skip": ["package-lock.json", ".DS_Store"],
        "directories_to_skip": [".git", "node_modules", "dist"]
    }
    "#;

    let mut file = File::create(&exclusion_file_path).expect("Failed to create exclusion file");
    file.write_all(exclusion_content.as_bytes())
        .expect("Failed to write to exclusion file");

    // Load the exclusion config
    let exclusion_config = ExclusionConfig::from_file(exclusion_file_path.to_str().unwrap())
        .expect("Failed to load exclusion config");

    // Verify the loaded config
    assert_eq!(
        exclusion_config.extensions_to_skip.len(),
        3,
        "Should have 3 extensions to skip"
    );
    assert!(
        exclusion_config
            .extensions_to_skip
            .contains(&".png".to_string()),
        "Should contain .png"
    );
    assert!(
        exclusion_config
            .extensions_to_skip
            .contains(&".jpg".to_string()),
        "Should contain .jpg"
    );
    assert!(
        exclusion_config
            .extensions_to_skip
            .contains(&".pdf".to_string()),
        "Should contain .pdf"
    );

    assert_eq!(
        exclusion_config.files_to_skip.len(),
        2,
        "Should have 2 files to skip"
    );
    assert!(
        exclusion_config
            .files_to_skip
            .contains(&"package-lock.json".to_string()),
        "Should contain package-lock.json"
    );
    assert!(
        exclusion_config
            .files_to_skip
            .contains(&".DS_Store".to_string()),
        "Should contain .DS_Store"
    );

    assert_eq!(
        exclusion_config.directories_to_skip.len(),
        3,
        "Should have 3 directories to skip"
    );
    assert!(
        exclusion_config
            .directories_to_skip
            .contains(&".git".to_string()),
        "Should contain .git"
    );
    assert!(
        exclusion_config
            .directories_to_skip
            .contains(&"node_modules".to_string()),
        "Should contain node_modules"
    );
    assert!(
        exclusion_config
            .directories_to_skip
            .contains(&"dist".to_string()),
        "Should contain dist"
    );

    // Test the exclusion patterns
    let test_cases = [
        (Path::new("image.png"), true),
        (Path::new("document.pdf"), true),
        (Path::new("code.js"), false),
        (Path::new("package-lock.json"), true),
        (Path::new(".DS_Store"), true),
        (Path::new("README.md"), false),
        (Path::new(".git/config"), true),
        (Path::new("node_modules/package.json"), true),
        (Path::new("dist/bundle.js"), true),
        (Path::new("src/main.js"), false),
    ];

    for (path, expected) in test_cases {
        assert_eq!(
            exclusion_config.should_exclude(path),
            expected,
            "Path {:?} should{} be excluded",
            path,
            if expected { "" } else { " not" }
        );
    }
}

#[test]
fn test_invalid_exclusion_file() {
    // Create a temporary directory
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Create an invalid exclusion file
    let exclusion_file_path = temp_path.join("invalid_exclusions.json");
    let exclusion_content = r#"
        not valid json
    "#;

    let mut file = File::create(&exclusion_file_path).expect("Failed to create exclusion file");
    file.write_all(exclusion_content.as_bytes())
        .expect("Failed to write to exclusion file");

    // Try to load the exclusion config
    let result = ExclusionConfig::from_file(exclusion_file_path.to_str().unwrap());

    // Verify the result is an error
    assert!(result.is_err(), "Loading invalid JSON should fail");
}

#[test]
fn test_nonexistent_exclusion_file() {
    // Try to load a non-existent exclusion config
    let result = ExclusionConfig::from_file("nonexistent_file.json");

    // Verify the result is an error
    assert!(result.is_err(), "Loading non-existent file should fail");
}
