use engine_builder::models::ranking::{
    FileRanking, ProblemContext, RankedCodebaseFile, RelevantFileDataForPrompt,
};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_file_ranking_serialization() {
    let ranking = FileRanking {
        message: "These files are ranked by relevance".to_string(),
        ranking: vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "Cargo.toml".to_string(),
        ],
    };

    // Serialize to JSON
    let serialized = serde_json::to_string(&ranking).unwrap();

    // Deserialize back
    let deserialized: FileRanking = serde_json::from_str(&serialized).unwrap();

    // Verify
    assert_eq!(deserialized.message, ranking.message);
    assert_eq!(deserialized.ranking.len(), 3);
    assert_eq!(deserialized.ranking[0], "src/main.rs");
    assert_eq!(deserialized.ranking[1], "src/lib.rs");
    assert_eq!(deserialized.ranking[2], "Cargo.toml");
}

#[test]
fn test_ranked_codebase_file() {
    let file = RankedCodebaseFile {
        path: "src/main.rs".to_string(),
        tokens: 1024,
    };

    assert_eq!(file.path, "src/main.rs");
    assert_eq!(file.tokens, 1024);
}

#[test]
fn test_relevant_file_data_for_prompt() {
    let file_data = RelevantFileDataForPrompt {
        path: "src/config.rs".to_string(),
        summary: "Configuration handling for the application".to_string(),
        token_count: 256,
    };

    assert_eq!(file_data.path, "src/config.rs");
    assert_eq!(
        file_data.summary,
        "Configuration handling for the application"
    );
    assert_eq!(file_data.token_count, 256);

    // Test serialization
    let serialized = serde_json::to_string(&file_data).unwrap();
    let deserialized: RelevantFileDataForPrompt = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.path, file_data.path);
    assert_eq!(deserialized.summary, file_data.summary);
    assert_eq!(deserialized.token_count, file_data.token_count);
}

#[test]
fn test_problem_context() {
    // Create test rankings
    let ranking1 = FileRanking {
        message: "Ranking 1".to_string(),
        ranking: vec!["file1.rs".to_string(), "file2.rs".to_string()],
    };

    let ranking2 = FileRanking {
        message: "Ranking 2".to_string(),
        ranking: vec!["file2.rs".to_string(), "file3.rs".to_string()],
    };

    // Create ranked files
    let ranked_files = vec![
        RankedCodebaseFile {
            path: "file1.rs".to_string(),
            tokens: 100,
        },
        RankedCodebaseFile {
            path: "file2.rs".to_string(),
            tokens: 200,
        },
        RankedCodebaseFile {
            path: "file3.rs".to_string(),
            tokens: 300,
        },
    ];

    // Create usage data
    let mut usage1 = HashMap::new();
    usage1.insert("prompt_tokens".to_string(), json!(1000));

    let mut usage2 = HashMap::new();
    usage2.insert("completion_tokens".to_string(), json!(500));

    // Create problem context
    let context = ProblemContext {
        model_rankings: vec![ranking1, ranking2],
        ranked_files: ranked_files,
        prompt_caching_usages: vec![usage1, usage2],
    };

    // Test serialization
    let serialized = serde_json::to_string(&context).unwrap();
    let deserialized: ProblemContext = serde_json::from_str(&serialized).unwrap();

    // Verify
    assert_eq!(deserialized.model_rankings.len(), 2);
    assert_eq!(deserialized.model_rankings[0].message, "Ranking 1");
    assert_eq!(deserialized.model_rankings[1].message, "Ranking 2");

    assert_eq!(deserialized.ranked_files.len(), 3);
    assert_eq!(deserialized.ranked_files[0].path, "file1.rs");
    assert_eq!(deserialized.ranked_files[0].tokens, 100);

    assert_eq!(deserialized.prompt_caching_usages.len(), 2);
}
