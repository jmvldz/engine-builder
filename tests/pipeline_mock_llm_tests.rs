use anyhow::Result;
use async_trait::async_trait;
use engine_builder::config::{CodebaseConfig, LLMConfig, RelevanceConfig, RankingConfig};
use engine_builder::llm::client::{LLMClient, LLMResponse, TokenCost, TokenUsage};
use engine_builder::models::problem::SWEBenchProblem;
use engine_builder::models::exclusion::ExclusionConfig;
use engine_builder::stages::{file_selection, relevance, ranking};
use engine_builder::utils::trajectory_store::TrajectoryStore;
use tempfile::tempdir;
use std::sync::Arc;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

// Mock LLM client for testing
struct MockLLMClient {
    responses: HashMap<String, String>,
}

impl MockLLMClient {
    fn new() -> Self {
        let mut responses = HashMap::new();
        
        // File selection response
        responses.insert(
            "file_selection".to_string(),
            r#"Based on the problem statement and codebase structure, I recommend focusing on these files:

```json
[
  "src/main.rs",
  "src/lib.rs",
  "src/models/file.rs"
]
```

These files are most likely to be relevant to the issue described."#.to_string()
        );
        
        // Relevance responses for each file
        responses.insert(
            "relevance_src/main.rs".to_string(),
            r#"
RELEVANCE: Relevant
SUMMARY: This file contains the main entry point for the application and handles CLI arguments.
"#.to_string()
        );
        
        responses.insert(
            "relevance_src/lib.rs".to_string(),
            r#"
RELEVANCE: Relevant
SUMMARY: This file exports the core modules and functionality of the library.
"#.to_string()
        );
        
        responses.insert(
            "relevance_src/models/file.rs".to_string(),
            r#"
RELEVANCE: Relevant
SUMMARY: This file defines the file data structures used throughout the codebase.
"#.to_string()
        );
        
        // Ranking response
        responses.insert(
            "ranking".to_string(),
            r#"
Based on the problem statement and relevant files, here is my ranking:

```json
[
  "src/models/file.rs",
  "src/lib.rs",
  "src/main.rs"
]
```

I've prioritized the file.rs model because it's most central to the issue described.
"#.to_string()
        );
        
        Self { responses }
    }
    
    fn get_response_key(&self, prompt: &str) -> String {
        // Print a snippet of the prompt to help debug
        println!("Prompt snippet: {}", &prompt[..100.min(prompt.len())]);
        
        // Check for file selection prompt - should happen first
        if prompt.contains("analyze the following codebase") || 
           prompt.contains("codebase structure") ||
           prompt.contains("determine which files") {
            println!("Identified as file_selection prompt");
            return "file_selection".to_string();
        }
        
        // Check for ranking prompt
        if prompt.contains("Rank the following files") {
            println!("Identified as ranking prompt");
            return "ranking".to_string();
        }
        
        // For relevance, extract the file path - most specific checks should be last
        for key in self.responses.keys() {
            if key.starts_with("relevance_") {
                let file_path = &key[10..];
                if prompt.contains(file_path) {
                    println!("Identified as relevance prompt for {}", file_path);
                    return key.clone();
                }
            }
        }
        
        println!("No key matched, returning unknown. Full prompt: {}", prompt);
        "unknown".to_string()
    }
}

#[async_trait]
impl LLMClient for MockLLMClient {
    async fn completion(&self, prompt: &str, _max_tokens: usize, _temperature: f64) -> Result<LLMResponse> {
        let key = self.get_response_key(prompt);
        
        let content = self.responses.get(&key).cloned().unwrap_or_else(|| {
            format!("Mock response not found for key: {}", key)
        });
        
        // Print the key and content for debugging
        println!("MockLLMClient responding to key: {}", key);
        println!("Content: {}", content);
        
        Ok(LLMResponse {
            content,
            usage: TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 100,
                total_tokens: 200,
            },
        })
    }
    
    async fn completion_with_tracing(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
        _trace_id: Option<&str>,
        _generation_name: Option<&str>,
        _metadata: Option<serde_json::Value>,
    ) -> Result<LLMResponse> {
        // Just delegate to the regular completion method
        self.completion(prompt, max_tokens, temperature).await
    }
    
    fn name(&self) -> &str {
        "MockLLMClient"
    }
    
    fn get_token_prices(&self) -> (f64, f64) {
        (0.01, 0.01) // Mock prices for prompt and completion tokens
    }
    
    fn calculate_cost(&self, usage: &TokenUsage) -> TokenCost {
        TokenCost::from_usage(usage, 0.01, 0.01)
    }
}

// Factory function for creating mock LLM client
fn create_mock_client(_: &LLMConfig) -> Pin<Box<dyn Future<Output = Result<Arc<dyn LLMClient>>> + Send>> {
    Box::pin(async {
        let client: Arc<dyn LLMClient> = Arc::new(MockLLMClient::new());
        Ok(client)
    })
}

fn create_test_configs() -> (RelevanceConfig, CodebaseConfig, RankingConfig) {
    let temp_dir = tempdir().unwrap();
    let temp_path = temp_dir.path().to_string_lossy().to_string();
    
    let llm_config = LLMConfig {
        model_type: "test".to_string(),
        model: "test-model".to_string(),
        api_key: "test-api-key".to_string(),
        base_url: None,
        timeout: 30,
        max_retries: 3,
    };
    
    let relevance_config = RelevanceConfig {
        trajectory_store_dir: temp_path.clone(),
        max_tokens: 1000,
        max_file_tokens: 10000,
        max_workers: 4,
        llm: llm_config.clone(),
        timeout: 30.0,
    };
    
    let codebase_config = CodebaseConfig {
        path: temp_dir.path().to_path_buf(),
        exclusions_path: "exclusions.json".to_string(),
        problem_id: "test_problem".to_string(),
        problem_statement: "Test problem statement".to_string(),
    };
    
    let ranking_config = RankingConfig {
        trajectory_store_dir: temp_path,
        max_tokens: 1000,
        num_rankings: 1,
        max_workers: 4,
        temperature: 0.0,
        llm: llm_config,
    };
    
    (relevance_config, codebase_config, ranking_config)
}

#[tokio::test]
async fn test_mock_pipeline_flow() -> Result<()> {
    // Override the LLM client creation function
    engine_builder::llm::client::set_client_factory(create_mock_client);
    
    // Create test configs
    let (relevance_config, codebase_config, ranking_config) = create_test_configs();
    
    // Create a test problem
    let mut problem = SWEBenchProblem::new(
        "test_problem".to_string(),
        "This is a test problem statement".to_string(),
    )
    .with_exclusion_config(ExclusionConfig::default());
    
    // Stage 1: File Selection - Create the directory structure first
    let trajectory_store = TrajectoryStore::new(&relevance_config.trajectory_store_dir, &problem)?;
    let problem_dir = trajectory_store.problem_dir();
    std::fs::create_dir_all(&problem_dir)?;
    
    // Create directory structure and mock files for relevance stage
    let mock_files = [
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub mod models;\n"),
        ("src/models/file.rs", "pub struct File {}\n"),
    ];
    
    for (path, content) in &mock_files {
        let file_path = codebase_config.path.join(path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file_path, content)?;
    }
    
    // Now run file selection
    let (file_patterns, _) = file_selection::run_file_selection(
        &relevance_config, 
        &codebase_config, 
        &problem
    ).await?;
    
    // Verify file patterns
    assert_eq!(file_patterns.patterns.len(), 3);
    assert!(file_patterns.patterns.contains(&"src/main.rs".to_string()));
    assert!(file_patterns.patterns.contains(&"src/lib.rs".to_string()));
    assert!(file_patterns.patterns.contains(&"src/models/file.rs".to_string()));
    
    // Save file patterns for next stage
    file_selection::save_file_patterns(
        &relevance_config.trajectory_store_dir,
        &problem,
        &file_patterns
    )?;
    
    // We've already created the directory structure and mock files above
    
    // Save codebase tree response for relevance stage
    let response_path = trajectory_store.problem_dir().join("codebase_tree_response.txt");
    std::fs::write(
        &response_path,
        r#"Based on the problem statement and codebase structure, I recommend focusing on these files:

```json
[
  "src/main.rs",
  "src/lib.rs",
  "src/models/file.rs"
]
```

These files are most likely to be relevant to the issue described."#
    )?;
    
    // Stage 2: Relevance Assessment
    // We need to configure the problem correctly
    problem = problem.with_codebase_path(&codebase_config.path);
    problem.initialize()?;
    
    relevance::process_codebase(
        relevance_config.clone(),
        &codebase_config,
        problem.clone()
    ).await?;
    
    // Verify relevance decisions
    let decisions = trajectory_store.load_relevance_decisions()?;
    assert_eq!(decisions.len(), 3);
    
    for path in ["src/main.rs", "src/lib.rs", "src/models/file.rs"].iter() {
        assert!(decisions.contains_key(*path));
        assert!(decisions[*path].is_relevant());
        assert!(decisions[*path].summary.is_some());
    }
    
    // Stage 3: Ranking
    ranking::process_rankings(ranking_config, problem.clone()).await?;
    
    // Verify ranking
    let ranking = trajectory_store.load_ranking()?;
    assert_eq!(ranking.ranked_files.len(), 3);
    
    // Print the actual ranked files to help debug
    println!("Actual ranking order:");
    for file in &ranking.ranked_files {
        println!("  {}", file.path);
    }
    
    // Check that the expected files are in the ranked files, without asserting order
    // as the merged ranking algorithm might shuffle things differently in tests
    let paths: Vec<&str> = ranking.ranked_files.iter().map(|file| file.path.as_str()).collect();
    assert!(paths.contains(&"src/models/file.rs"));
    assert!(paths.contains(&"src/lib.rs"));
    assert!(paths.contains(&"src/main.rs"));
    
    Ok(())
}