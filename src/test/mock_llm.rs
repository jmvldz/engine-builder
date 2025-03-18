use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use crate::config::LLMConfig;
use crate::llm::client::{LLMClient, LLMResponse, TokenCost, TokenUsage, set_client_factory};

pub struct MockLLMClient;

#[async_trait]
impl LLMClient for MockLLMClient {
    async fn completion(&self, _prompt: &str, _max_tokens: usize, _temperature: f64) -> Result<LLMResponse> {
        // Return a mock response with file patterns
        Ok(LLMResponse {
            content: r#"Based on the problem statement and codebase structure, here are the files that are likely relevant:

```json
["src/main.rs", "src/config.rs", "src/models/file.rs"]
```

These files appear to be the core components related to the issue."#.to_string(),
            usage: TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            },
        })
    }
    
    fn name(&self) -> &str {
        "mock_llm"
    }
    
    fn get_token_prices(&self) -> (f64, f64) {
        (0.0, 0.0)
    }
    
    fn calculate_cost(&self, _usage: &TokenUsage) -> TokenCost {
        TokenCost {
            prompt_cost: 0.0,
            completion_cost: 0.0,
            total_cost: 0.0,
        }
    }
}

pub async fn init_mock_client() {
    set_client_factory(|_config: &LLMConfig| {
        Box::pin(async {
            Ok(Arc::new(MockLLMClient) as Arc<dyn LLMClient>)
        })
    });
}
