use anyhow::Result;
use async_trait::async_trait;
use log;
use std::fmt;

use crate::config::LLMConfig;
use crate::llm::anthropic::AnthropicClient;
use crate::llm::openai::OpenAIClient;

/// Common structure for token usage tracking across different LLMs
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl fmt::Display for TokenUsage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Prompt tokens: {}, Completion tokens: {}, Total tokens: {}",
            self.prompt_tokens, self.completion_tokens, self.total_tokens
        )
    }
}

/// Cost calculation for token usage
#[derive(Debug, Clone)]
pub struct TokenCost {
    pub prompt_cost: f64,
    pub completion_cost: f64,
    pub total_cost: f64,
}

impl TokenCost {
    /// Calculate cost from token usage and per-token rates
    pub fn from_usage(
        usage: &TokenUsage,
        prompt_price_per_1k: f64,
        completion_price_per_1k: f64,
    ) -> Self {
        let prompt_cost = (usage.prompt_tokens as f64 / 1000.0) * prompt_price_per_1k;
        let completion_cost = (usage.completion_tokens as f64 / 1000.0) * completion_price_per_1k;

        TokenCost {
            prompt_cost,
            completion_cost,
            total_cost: prompt_cost + completion_cost,
        }
    }

    /// Format as USD currency
    pub fn as_usd(&self) -> String {
        format!("${:.4}", self.total_cost)
    }
}

impl fmt::Display for TokenCost {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Cost: ${:.4} (Prompt: ${:.4}, Completion: ${:.4})",
            self.total_cost, self.prompt_cost, self.completion_cost
        )
    }
}

/// Response from an LLM request
pub struct LLMResponse {
    pub content: String,
    pub usage: TokenUsage,
}

/// A trait for LLM clients
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// Generate a completion from the LLM
    async fn completion(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
    ) -> Result<LLMResponse>;

    /// Get the name of the LLM client
    fn name(&self) -> &str;

    /// Get the cost per 1K tokens for prompt and completion
    fn get_token_prices(&self) -> (f64, f64);

    /// Fetch the latest pricing data from the provider API
    async fn fetch_pricing_data(&self) -> Result<()> {
        // Default implementation does nothing
        // Providers should override this method to fetch pricing
        Ok(())
    }

    /// Calculate cost from token usage
    fn calculate_cost(&self, usage: &TokenUsage) -> TokenCost {
        let (prompt_price, completion_price) = self.get_token_prices();
        TokenCost::from_usage(usage, prompt_price, completion_price)
    }
}

/// Create an LLM client from a configuration and fetch pricing data
pub async fn create_client(config: &LLMConfig) -> Result<Box<dyn LLMClient>> {
    let client: Box<dyn LLMClient> = match config.model_type.as_str() {
        "openai" => {
            let client = OpenAIClient::new(config)?;
            Box::new(client)
        }
        "anthropic" => {
            let client = AnthropicClient::new(config)?;
            Box::new(client)
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unsupported LLM type: {}",
                config.model_type
            ))
        }
    };

    // Fetch pricing data after creating the client
    if let Err(e) = client.fetch_pricing_data().await {
        log::warn!(
            "Failed to fetch pricing data: {}. Using fallback pricing.",
            e
        );
    }

    Ok(client)
}
