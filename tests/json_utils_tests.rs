use engine_builder::utils::json_utils::extract_last_json;

#[test]
fn test_extract_json_from_code_block() {
    let text = r#"Here's some text explaining things.
    
```json
["file1.txt", "file2.rs", "dir/file3.py"]
```

More explanation."#;

    let result = extract_last_json(text).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], "file1.txt");
    assert_eq!(result[1], "file2.rs");
    assert_eq!(result[2], "dir/file3.py");
}

#[test]
fn test_extract_json_without_code_block() {
    let text = r#"Here are some important files: ["file1.txt", "file2.rs", "dir/file3.py"]"#;

    let result = extract_last_json(text).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], "file1.txt");
    assert_eq!(result[1], "file2.rs");
    assert_eq!(result[2], "dir/file3.py");
}

#[test]
fn test_extract_last_json_with_multiple_arrays() {
    let text = r#"First array: ["a", "b", "c"]
    Second array: ["file1.txt", "file2.rs", "dir/file3.py"]"#;

    let result = extract_last_json(text).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], "file1.txt");
    assert_eq!(result[1], "file2.rs");
    assert_eq!(result[2], "dir/file3.py");
}

#[test]
fn test_extract_json_with_formatting() {
    let text = r#"Here's a formatted array:
```json
[
    "file1.txt",
    "file2.rs",
    "dir/file3.py"
]
```"#;

    let result = extract_last_json(text).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], "file1.txt");
    assert_eq!(result[1], "file2.rs");
    assert_eq!(result[2], "dir/file3.py");
}

#[test]
fn test_extract_file_paths_when_json_parse_fails() {
    // This version includes a dummy array that will fail to parse as valid JSON
    let text =
        r#"I recommend these files [dummy array]: "src/main.rs", "src/lib.rs", and "Cargo.toml""#;

    let result = extract_last_json(text).unwrap();
    assert_eq!(result.len(), 3);
    assert!(result.contains(&"src/main.rs".to_string()));
    assert!(result.contains(&"src/lib.rs".to_string()));
    assert!(result.contains(&"Cargo.toml".to_string()));
}

#[test]
fn test_no_json_array_found() {
    let text = "This text doesn't contain any JSON arrays or file paths";

    let result = extract_last_json(text);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No JSON array found"));
}

#[test]
fn test_invalid_json_no_file_paths() {
    let text = r#"This has invalid JSON: [not valid json] and no file paths"#;

    let result = extract_last_json(text);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Could not extract a valid JSON array"));
}
