use anyhow::Result;
use async_trait::async_trait;
use engine_builder::config::{CodebaseConfig, Config, LLMConfig, RankingConfig, RelevanceConfig};
use engine_builder::llm::client::{LLMClient, LLMResponse, TokenCost, TokenUsage};
use engine_builder::models::exclusion::ExclusionConfig;
use engine_builder::models::file::FilePatternSelection;
use engine_builder::models::problem::SWEBenchProblem;
use engine_builder::models::relevance::{RelevanceDecision, RelevanceStatus};
use engine_builder::stages::ranking;
use engine_builder::utils::trajectory_store::TrajectoryStore;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tempfile::tempdir;

// Mock LLM client for testing
struct MockLLMClient {
    responses: HashMap<String, String>,
}

impl MockLLMClient {
    fn new() -> Self {
        let mut responses = HashMap::new();
        
        // For ranking, use a response with a JSON array
        responses.insert(
            "ranking".to_string(),
            r#"Based on the relevance assessments, here's the ranked list of files:

```json
[
  "src/models/file.rs",
  "src/lib.rs",
  "src/main.rs"
]
```

Explanation: I've ranked the files based on their importance to the problem."#.to_string(),
        );
        
        Self { responses }
    }
}

#[async_trait]
impl LLMClient for MockLLMClient {
    async fn completion(&self, _prompt: &str, _max_tokens: usize, _temperature: f64) -> Result<LLMResponse> {
        // For the ranking test, always return the ranking response
        let content = self.responses.get("ranking").cloned().unwrap_or_else(|| {
            "Unknown response".to_string()
        });
        
        Ok(LLMResponse {
            content,
            usage: TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 100,
                total_tokens: 200,
            },
        })
    }
    
    fn name(&self) -> &str {
        "MockLLMClient"
    }
    
    fn get_token_prices(&self) -> (f64, f64) {
        (0.01, 0.01) // Mock prices
    }
    
    fn calculate_cost(&self, usage: &TokenUsage) -> TokenCost {
        TokenCost::from_usage(usage, 0.01, 0.01)
    }
}

// Factory function for creating mock LLM client
#[allow(dead_code)]
fn create_mock_client(_: &LLMConfig) -> Pin<Box<dyn Future<Output = Result<Arc<dyn LLMClient>>> + Send>> {
    Box::pin(async {
        let client: Arc<dyn LLMClient> = Arc::new(MockLLMClient::new());
        Ok(client)
    })
}

// This test demonstrates the end-to-end compatibility between stages
// by creating mocked files at each stage and verifying that the next stage
// can correctly consume the output from the previous stage
#[tokio::test]
async fn test_end_to_end_pipeline_compatibility() -> Result<()> {
    // Override the LLM client creation function with a wrapper that adapts to the expected signature
    engine_builder::llm::client::set_client_factory(|_llm_config: &LLMConfig| {
        Box::pin(async {
            let client: Arc<dyn LLMClient> = Arc::new(MockLLMClient::new());
            Ok(client)
        })
    });
    // Create temporary directories and configs
    let temp_dir = tempdir()?;
    let temp_path = temp_dir.path().to_string_lossy().to_string();
    let codebase_dir = tempdir()?;
    
    // Mock codebase files
    let mock_files = [
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub mod models;\n"),
        ("src/models/file.rs", "pub struct File {}\n"),
    ];
    
    for (path, content) in &mock_files {
        let file_path = codebase_dir.path().join(path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file_path, content)?;
    }
    
    // Create problem
    let problem = SWEBenchProblem::new(
        "e2e_test".to_string(),
        "Test problem statement".to_string(),
    )
    .with_codebase_path(codebase_dir.path())
    .with_exclusion_config(ExclusionConfig::default());
    
    // Create configs
    let global_config = Config {
        anthropic_api_key: "dummy_key".to_string(),
        model: "test-model".to_string(),
        relevance: RelevanceConfig {
            model: Some("test-model".to_string()),
            max_tokens: 1000,
            max_file_tokens: 10000,
            max_workers: 4,
            timeout: 30.0,
        },
        ranking: RankingConfig {
            model: Some("test-model".to_string()),
            max_tokens: 1000,
            num_rankings: 1,
            max_workers: 4,
            temperature: 0.0,
        },
        output_path: Some(temp_path.clone()),
        codebase: CodebaseConfig {
            path: codebase_dir.path().to_path_buf(),
            exclusions_path: "exclusions.json".to_string(),
            problem_id: "e2e_test".to_string(),
            problem_statement: "Test problem statement".to_string(),
        },
        dockerfile: Default::default(),
        scripts: Default::default(),
        container: Default::default(),
        observability: Default::default(),
    };
    
    // Extract configs from global config
    let _relevance_config = global_config.relevance.clone();
    let _codebase_config = global_config.codebase.clone();
    
    // Create a trajectory store using the trajectory directory from global config
    let trajectory_dir = temp_path.clone();
    let store = TrajectoryStore::new(&trajectory_dir, &problem)?;
    
    // Ensure the problem directory exists
    let prob_dir = store.problem_dir();
    std::fs::create_dir_all(&prob_dir)?;
    
    // Stage 1: File Selection
    // Create mock file selection results
    
    // 1. Save mock codebase tree response
    let tree_response = r#"Based on the problem statement and codebase structure, I recommend focusing on these files:

```json
[
  "src/main.rs",
  "src/lib.rs",
  "src/models/file.rs"
]
```

These files are most likely to be relevant to the issue described."#;
    
    // Use the problem directory from the store
    std::fs::create_dir_all(&prob_dir)?;
    
    std::fs::write(
        prob_dir.join("codebase_tree_response.txt"),
        tree_response,
    )?;
    
    // 2. Save file patterns directly
    let file_patterns = FilePatternSelection::new(vec![
        "src/main.rs".to_string(),
        "src/lib.rs".to_string(),
        "src/models/file.rs".to_string(),
    ]);
    
    // Create the file patterns directory structure
    std::fs::create_dir_all(&prob_dir)?;
    
    // Save file patterns directly to the expected path
    let file_patterns_path = prob_dir.join("file_patterns.json");
    let file_patterns_json = serde_json::to_string_pretty(&file_patterns)?;
    std::fs::write(&file_patterns_path, file_patterns_json)?;
    
    // Stage 2: Relevance
    // Create mock relevance decisions
    let mut relevance_decisions = std::collections::HashMap::new();
    
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
    
    // Write the relevance decisions to the expected path
    let relevance_decisions_path = store.relevance_decisions_path();
    std::fs::write(
        &relevance_decisions_path,
        serde_json::to_string_pretty(&relevance_decisions)?,
    )?;
    
    // Stage 3: Run the ranking stage using the mock output from the previous stages
    let mut problem_instance = problem.clone();
    problem_instance.initialize()?;
    
    // Run ranking - this should be able to read relevance decisions and produce ranking
    ranking::process_rankings(&global_config, problem_instance).await?;
    
    // Verify that ranking was created
    assert!(store.ranking_exists());
    
    // Load the ranking to verify it contains the expected files
    let ranking = store.load_ranking()?;
    
    // Verify the ranked files
    assert_eq!(ranking.ranked_files.len(), 3);
    
    // The paths should match what we defined in relevance
    let ranked_paths: Vec<_> = ranking.ranked_files.iter().map(|f| &f.path).collect();
    assert!(ranked_paths.contains(&&"src/main.rs".to_string()));
    assert!(ranked_paths.contains(&&"src/lib.rs".to_string()));
    assert!(ranked_paths.contains(&&"src/models/file.rs".to_string()));
    
    // Cleanup
    temp_dir.close()?;
    codebase_dir.close()?;
    
    Ok(())
}
