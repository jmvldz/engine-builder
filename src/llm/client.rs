use async_trait::async_trait;
use anyhow::Result;

use crate::config::LLMConfig;
use crate::llm::openai::OpenAIClient;
use crate::llm::anthropic::AnthropicClient;

/// A trait for LLM clients
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Generate a completion from the LLM
    async fn completion(&self, prompt: &str, max_tokens: usize, temperature: f64) -> Result<String>;
    
    /// Get the name of the LLM client
    fn name(&self) -> &str;
}

/// Create an LLM client from a configuration
pub fn create_client(config: &LLMConfig) -> Result<Box<dyn LLMClient>> {
    match config.model_type.as_str() {
        "openai" => {
            let client = OpenAIClient::new(config)?;
            Ok(Box::new(client))
        },
        "anthropic" => {
            let client = AnthropicClient::new(config)?;
            Ok(Box::new(client))
        },
        _ => Err(anyhow::anyhow!("Unsupported LLM type: {}", config.model_type)),
    }
}