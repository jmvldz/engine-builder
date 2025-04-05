use anyhow::Result;
use engine_builder::models::file::FilePatternSelection;
use engine_builder::models::problem::SWEBenchProblem;
use engine_builder::models::ranking::{
    ProblemContext, RankedCodebaseFile, RelevantFileDataForPrompt,
};
use engine_builder::models::relevance::{RelevanceDecision, RelevanceStatus};
use engine_builder::utils::trajectory_store::TrajectoryStore;
use std::collections::HashMap;
use tempfile::tempdir;

fn create_test_problem() -> SWEBenchProblem {
    SWEBenchProblem::new(
        "test_pipeline".to_string(),
        "Test problem for pipeline integration".to_string(),
    )
}

/// Create a mock TrajectoryStore with the necessary files for testing the pipeline
fn setup_mock_pipeline(store: &TrajectoryStore) -> Result<()> {
    // 1. Mock file selection output (file_patterns.json)
    let file_patterns = FilePatternSelection::new(vec![
        "src/main.rs".to_string(),
        "src/lib.rs".to_string(),
        "src/models/*.rs".to_string(),
    ]);

    // Save the file patterns to the trajectory store directory
    let problem_dir = store.problem_dir();
    std::fs::create_dir_all(&problem_dir)?;
    std::fs::write(
        problem_dir.join("file_patterns.json"),
        serde_json::to_string_pretty(&file_patterns)?,
    )?;

    // 2. Create mock relevance decisions
    let mut relevance_decisions = HashMap::new();

    relevance_decisions.insert(
        "src/main.rs".to_string(),
        RelevanceDecision {
            message: "Main file is relevant".to_string(),
            status: RelevanceStatus::Relevant,
            summary: Some("Contains the main entry point".to_string()),
        },
    );

    relevance_decisions.insert(
        "src/lib.rs".to_string(),
        RelevanceDecision {
            message: "Library file is relevant".to_string(),
            status: RelevanceStatus::Relevant,
            summary: Some("Contains core functionality".to_string()),
        },
    );

    relevance_decisions.insert(
        "src/models/file.rs".to_string(),
        RelevanceDecision {
            message: "Model file is relevant".to_string(),
            status: RelevanceStatus::Relevant,
            summary: Some("Defines file structures".to_string()),
        },
    );

    // Save the relevance decisions
    std::fs::write(
        store.relevance_decisions_path(),
        serde_json::to_string_pretty(&relevance_decisions)?,
    )?;

    Ok(())
}

#[test]
fn test_file_selection_to_relevance_format_compatibility() -> Result<()> {
    let temp_dir = tempdir()?;
    let problem = create_test_problem();

    // Create a trajectory store
    let store = TrajectoryStore::new(&temp_dir, &problem)?;

    // Setup mock file patterns (output of file selection)
    let file_patterns = FilePatternSelection::new(vec![
        "src/main.rs".to_string(),
        "src/lib.rs".to_string(),
        "src/models/*.rs".to_string(),
    ]);

    // Save to trajectory store
    let problem_dir = store.problem_dir();
    std::fs::write(
        problem_dir.join("file_patterns.json"),
        serde_json::to_string_pretty(&file_patterns)?,
    )?;

    // Save mock LLM response at the path where the relevance stage expects it
    let tree_response = r#"Based on the problem statement and codebase structure, I recommend focusing on these files:

```json
[
  "src/main.rs",
  "src/lib.rs",
  "src/models/*.rs"
]
```

These files are most likely to be relevant to the issue described."#;

    std::fs::write(
        problem_dir.join("codebase_tree_response.txt"),
        tree_response,
    )?;

    // Verify by loading the file back (simulating what relevance stage would do)
    let file_patterns_json = std::fs::read_to_string(problem_dir.join("file_patterns.json"))?;
    let loaded_patterns: FilePatternSelection = serde_json::from_str(&file_patterns_json)?;

    // Check if the patterns match
    assert_eq!(loaded_patterns.patterns.len(), file_patterns.patterns.len());
    for (i, pattern) in file_patterns.patterns.iter().enumerate() {
        assert_eq!(&loaded_patterns.patterns[i], pattern);
    }

    // Verify the pattern matching works as expected
    assert!(loaded_patterns.matches("src/main.rs"));
    assert!(loaded_patterns.matches("src/models/file.rs"));
    assert!(!loaded_patterns.matches("src/utils/helper.rs"));

    // Cleanup
    temp_dir.close()?;

    Ok(())
}

#[test]
fn test_relevance_to_ranking_format_compatibility() -> Result<()> {
    let temp_dir = tempdir()?;
    let problem = create_test_problem();

    // Create a trajectory store
    let store = TrajectoryStore::new(&temp_dir, &problem)?;

    // Setup mock relevance decisions (output of relevance stage)
    let mut relevance_decisions = HashMap::new();

    relevance_decisions.insert(
        "src/main.rs".to_string(),
        RelevanceDecision {
            message: "Main file is relevant".to_string(),
            status: RelevanceStatus::Relevant,
            summary: Some("Contains the main entry point".to_string()),
        },
    );

    relevance_decisions.insert(
        "src/lib.rs".to_string(),
        RelevanceDecision {
            message: "Library file is relevant".to_string(),
            status: RelevanceStatus::Relevant,
            summary: Some("Contains core functionality".to_string()),
        },
    );

    relevance_decisions.insert(
        "src/models/irrelevant.rs".to_string(),
        RelevanceDecision {
            message: "This file is not relevant".to_string(),
            status: RelevanceStatus::NotRelevant,
            summary: None,
        },
    );

    // Save relevance decisions
    std::fs::write(
        store.relevance_decisions_path(),
        serde_json::to_string_pretty(&relevance_decisions)?,
    )?;

    // Load decisions (simulating what ranking stage would do)
    let loaded_decisions = store.load_relevance_decisions()?;

    // Check if decisions match
    assert_eq!(loaded_decisions.len(), relevance_decisions.len());

    // Simulate filtering to relevant files only (as ranking stage would do)
    let relevant_files: Vec<RelevantFileDataForPrompt> = loaded_decisions
        .iter()
        .filter(|(_, decision)| decision.status == RelevanceStatus::Relevant)
        .map(|(path, decision)| RelevantFileDataForPrompt {
            path: path.clone(),
            summary: decision.summary.clone().unwrap_or_default(),
            token_count: 100, // Mock token count
        })
        .collect();

    // Verify relevant file count
    assert_eq!(relevant_files.len(), 2);

    // Verify relevant files have proper data
    for file in &relevant_files {
        assert!(file.path == "src/main.rs" || file.path == "src/lib.rs");
        assert!(!file.summary.is_empty());
        assert!(file.token_count > 0);
    }

    // Cleanup
    temp_dir.close()?;

    Ok(())
}

#[test]
fn test_full_pipeline_format_compatibility() -> Result<()> {
    let temp_dir = tempdir()?;
    let problem = create_test_problem();

    // Create a trajectory store
    let store = TrajectoryStore::new(&temp_dir, &problem)?;

    // Setup mock data for all stages
    setup_mock_pipeline(&store)?;

    // 1. Validate File Selection output
    let file_patterns_json =
        std::fs::read_to_string(store.problem_dir().join("file_patterns.json"))?;
    let file_patterns: FilePatternSelection = serde_json::from_str(&file_patterns_json)?;

    assert!(!file_patterns.patterns.is_empty());

    // 2. Validate Relevance Decisions
    let relevance_decisions = store.load_relevance_decisions()?;
    assert!(!relevance_decisions.is_empty());

    // 3. Simulate Ranking stage processing
    let relevant_files: Vec<RelevantFileDataForPrompt> = relevance_decisions
        .iter()
        .filter(|(_, decision)| decision.status == RelevanceStatus::Relevant)
        .map(|(path, decision)| RelevantFileDataForPrompt {
            path: path.clone(),
            summary: decision.summary.clone().unwrap_or_default(),
            token_count: 100, // Mock token count
        })
        .collect();

    // Check we have all the relevant files correctly filtered
    assert_eq!(relevant_files.len(), 3);

    // 4. Generate mock ranking output
    let ranked_files: Vec<RankedCodebaseFile> = relevant_files
        .iter()
        .map(|file| RankedCodebaseFile {
            path: file.path.clone(),
            tokens: file.token_count,
        })
        .collect();

    // Create a problem context
    let context = ProblemContext {
        model_rankings: vec![],
        ranked_files,
        prompt_caching_usages: vec![],
    };

    // Save the ranking
    store.save_ranking(context)?;

    // Verify ranking exists
    assert!(store.ranking_exists());

    // Load ranking to validate
    let loaded_ranking = store.load_ranking()?;
    assert_eq!(loaded_ranking.ranked_files.len(), 3);

    // Cleanup
    temp_dir.close()?;

    Ok(())
}

#[test]
fn test_stage_dependency_checking() -> Result<()> {
    let temp_dir = tempdir()?;
    let problem = create_test_problem();

    // Create a trajectory store
    let store = TrajectoryStore::new(&temp_dir, &problem)?;

    // Test that ranking checks for relevance decisions
    let relevance_path = store.relevance_decisions_path();
    assert!(!relevance_path.exists());

    // Create file patterns file but not relevance decisions
    std::fs::create_dir_all(store.problem_dir())?;
    let file_patterns = FilePatternSelection::new(vec!["src/main.rs".to_string()]);
    std::fs::write(
        store.problem_dir().join("file_patterns.json"),
        serde_json::to_string_pretty(&file_patterns)?,
    )?;

    // Verify file_patterns.json exists but not relevance_decisions.json
    assert!(store.problem_dir().join("file_patterns.json").exists());
    assert!(!store.relevance_decisions_path().exists());

    // Now create relevance decisions but not ranking
    let mut relevance_decisions = HashMap::new();
    relevance_decisions.insert(
        "src/main.rs".to_string(),
        RelevanceDecision::relevant("Message".to_string(), "Summary".to_string()),
    );
    std::fs::write(
        store.relevance_decisions_path(),
        serde_json::to_string_pretty(&relevance_decisions)?,
    )?;

    // Verify ranking doesn't exist yet
    assert!(!store.ranking_exists());

    // Cleanup
    temp_dir.close()?;

    Ok(())
}
