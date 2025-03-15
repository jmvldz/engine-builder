use engine_builder::config::{Config, LLMConfig, ContainerConfig};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_config_default() {
    let default_config = Config::default();
    
    // Check default values for various components
    assert_eq!(default_config.codebase.path.to_str().unwrap(), ".");
    assert_eq!(default_config.codebase.problem_id, "custom_problem");
    assert_eq!(default_config.codebase.exclusions_path, "exclusions.json");
    
    // Check default LLM values
    assert_eq!(default_config.relevance.llm.model_type, "anthropic");
    assert_eq!(default_config.relevance.llm.model, "claude-3-sonnet-20240229");
    
    // Check default container config
    assert_eq!(default_config.container.timeout, 300);
    assert_eq!(default_config.container.parallel, false);
    assert_eq!(default_config.container.remove, true);
}

#[test]
fn test_llm_config_fields() {
    let llm_config = LLMConfig {
        model_type: "openai".to_string(),
        model: "gpt-4".to_string(),
        api_key: "test-key".to_string(),
        base_url: Some("https://api.test.com".to_string()),
        timeout: 60,
        max_retries: 5,
    };
    
    assert_eq!(llm_config.model_type, "openai");
    assert_eq!(llm_config.model, "gpt-4");
    assert_eq!(llm_config.api_key, "test-key");
    assert_eq!(llm_config.base_url, Some("https://api.test.com".to_string()));
    assert_eq!(llm_config.timeout, 60);
    assert_eq!(llm_config.max_retries, 5);
}

#[test]
fn test_container_config_default() {
    let container_config = ContainerConfig::default();
    
    assert_eq!(container_config.timeout, 300);
    assert_eq!(container_config.parallel, false);
    assert_eq!(container_config.remove, true);
}

#[test]
fn test_config_from_file() {
    // Create a temporary directory for our test file
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("test_config.json");
    
    // Create a minimal test config file
    let config_json = r#"{
        "relevance": {
            "llm": {
                "model_type": "test_model_type",
                "model": "test_model",
                "api_key": "test_api_key",
                "timeout": 10,
                "max_retries": 2
            },
            "max_workers": 4,
            "max_tokens": 1000,
            "timeout": 100.0,
            "max_file_tokens": 10000,
            "trajectory_store_dir": "test_dir"
        },
        "ranking": {
            "llm": {
                "model_type": "test_model_type",
                "model": "test_model",
                "api_key": "test_api_key",
                "timeout": 10,
                "max_retries": 2
            },
            "num_rankings": 2,
            "max_workers": 2,
            "max_tokens": 1000,
            "temperature": 0.5,
            "trajectory_store_dir": "test_dir"
        },
        "codebase": {
            "path": "test_path",
            "problem_id": "test_problem",
            "problem_statement": "test statement",
            "exclusions_path": "test_exclusions.json"
        }
    }"#;
    
    let mut file = File::create(&config_path).unwrap();
    file.write_all(config_json.as_bytes()).unwrap();
    
    // Test loading the config
    let config = Config::from_file(Some(config_path.to_str().unwrap())).unwrap();
    
    // Verify the loaded config values
    assert_eq!(config.relevance.llm.model_type, "test_model_type");
    assert_eq!(config.relevance.llm.model, "test_model");
    assert_eq!(config.relevance.llm.api_key, "test_api_key");
    assert_eq!(config.relevance.max_workers, 4);
    
    assert_eq!(config.codebase.path.to_str().unwrap(), "test_path");
    assert_eq!(config.codebase.problem_id, "test_problem");
    assert_eq!(config.codebase.problem_statement, "test statement");
    
    // Clean up the temporary directory
    temp_dir.close().unwrap();
}

// Test error handling for file not found
#[test]
fn test_config_from_nonexistent_file() {
    let result = Config::from_file(Some("nonexistent_file.json"));
    assert!(result.is_err());
    
    // Verify error message contains the filename
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("nonexistent_file.json"));
}