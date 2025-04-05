use engine_builder::models::problem::SWEBenchProblem;
use engine_builder::models::relevance::RelevanceDecision;
use engine_builder::utils::trajectory_store::TrajectoryStore;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

fn create_test_problem() -> SWEBenchProblem {
    SWEBenchProblem::new(
        "test_problem".to_string(),
        "Test problem description".to_string(),
    )
}

#[test]
fn test_trajectory_store_new() {
    let temp_dir = tempdir().unwrap();
    let problem = create_test_problem();

    let store = TrajectoryStore::new(&temp_dir, &problem).unwrap();

    // In the new implementation, we should check that the base directory exists
    assert!(temp_dir.path().exists());
    assert!(temp_dir.path().is_dir());

    // Save something to test functionality
    let decision = RelevanceDecision::relevant("Message".to_string(), "Summary".to_string());
    store
        .save_per_file_relevance_decision("test.rs", decision)
        .unwrap();

    // Cleanup
    temp_dir.close().unwrap();
}

#[test]
fn test_trajectory_store_paths() {
    let temp_dir = tempdir().unwrap();
    let problem = create_test_problem();

    let store = TrajectoryStore::new(&temp_dir, &problem).unwrap();

    // Check path methods
    let expected_problem_dir = temp_dir.path();
    assert_eq!(store.problem_dir(), expected_problem_dir);

    let expected_relevance_path = expected_problem_dir.join("relevance_decisions.json");
    assert_eq!(store.relevance_decisions_path(), expected_relevance_path);

    // Cleanup
    temp_dir.close().unwrap();
}

#[test]
fn test_save_and_load_relevance_decision() {
    let temp_dir = tempdir().unwrap();
    let problem = create_test_problem();

    let store = TrajectoryStore::new(&temp_dir, &problem).unwrap();

    // Create a test relevance decision
    let file_path = "src/main.rs";
    let decision = RelevanceDecision::relevant(
        "Test message".to_string(),
        "This file is relevant".to_string(),
    );

    // Save the decision
    store
        .save_per_file_relevance_decision(file_path, decision.clone())
        .unwrap();

    // Verify file exists
    let decisions_path = store.relevance_decisions_path();
    assert!(decisions_path.exists());

    // Load the decisions
    let loaded_decisions = store.load_relevance_decisions().unwrap();

    // Verify loaded decision matches saved one
    assert_eq!(loaded_decisions.len(), 1);
    assert!(loaded_decisions.contains_key(file_path));

    let loaded_decision = &loaded_decisions[file_path];
    assert_eq!(loaded_decision.message, decision.message);
    assert_eq!(loaded_decision.status, decision.status);
    assert_eq!(loaded_decision.summary, decision.summary);

    // Check if decision exists
    assert!(store.relevance_decision_exists(file_path));
    assert!(!store.relevance_decision_exists("nonexistent.rs"));

    // Cleanup
    temp_dir.close().unwrap();
}

#[test]
fn test_load_nonexistent_relevance_decisions() {
    let temp_dir = tempdir().unwrap();
    let problem = create_test_problem();

    let store = TrajectoryStore::new(&temp_dir, &problem).unwrap();

    // Load decisions when file doesn't exist
    let decisions = store.load_relevance_decisions().unwrap();
    assert!(decisions.is_empty());

    // Cleanup
    temp_dir.close().unwrap();
}

#[test]
fn test_load_all_relevance_decisions() {
    let temp_dir = tempdir().unwrap();
    let problem = create_test_problem();

    let store = TrajectoryStore::new(&temp_dir, &problem).unwrap();

    // Create test decisions
    let mut test_decisions = HashMap::new();
    test_decisions.insert(
        "file1.rs".to_string(),
        RelevanceDecision::relevant("Message 1".to_string(), "Explanation 1".to_string()),
    );
    test_decisions.insert(
        "file2.rs".to_string(),
        RelevanceDecision::relevant("Message 2".to_string(), "Explanation 2".to_string()),
    );

    // Write decisions directly to file
    let decisions_path = store.relevance_decisions_path();
    let json = serde_json::to_string_pretty(&test_decisions).unwrap();

    fs::create_dir_all(decisions_path.parent().unwrap()).unwrap();
    let mut file = File::create(&decisions_path).unwrap();
    file.write_all(json.as_bytes()).unwrap();

    // Test load_all_relevance_decisions
    let loaded = store.load_all_relevance_decisions().unwrap();

    // Verify loaded decisions
    assert_eq!(loaded.len(), 2);
    assert!(loaded.contains_key("file1.rs"));
    assert!(loaded.contains_key("file2.rs"));

    // Cleanup
    temp_dir.close().unwrap();
}
