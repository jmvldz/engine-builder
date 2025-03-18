use anyhow::Result;
use async_trait::async_trait;
use log::{self, debug};
use std::fmt;
use std::sync::{Arc, Once};

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
    
    /// Generate a completion with Langfuse tracing
    async fn completion_with_tracing(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
        trace_id: Option<&str>,
        generation_name: Option<&str>,
        metadata: Option<serde_json::Value>,
    ) -> Result<LLMResponse> {
        use crate::llm::langfuse;
        use std::time::{Instant, SystemTime, UNIX_EPOCH};
        
        // Get the current timestamp in milliseconds
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
            
        // Record start time for duration measurement - currently calculated using SystemTime instead
        let _instant_start = Instant::now();
        
        // Create a new trace if one wasn't provided
        let (_owned_trace_id, trace_id_str) = match trace_id {
            Some(id) => (None, id.to_string()),
            None => {
                // Check if metadata contains problem_id to use as trace_id
                let problem_id = if let Some(meta) = &metadata {
                    meta.get("problem_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                };
                
                // Create a new trace for this completion
                let trace_name = generation_name.unwrap_or("llm_completion");
                match langfuse::get_tracer() {
                    Ok(tracer) => {
                        // If we have a problem_id, use it as the trace_id
                        if let Some(id) = problem_id {
                            debug!("Using problem_id as trace_id: {}", id);
                            (None, id)
                        } else {
                            // Otherwise create a new trace
                            match tracer.create_trace(trace_name, metadata.clone()).await {
                                Ok(id) => {
                                    let id_str = id.clone();
                                    (Some(id), id_str)
                                },
                                Err(_) => (None, String::new()),
                            }
                        }
                    },
                    Err(_) => (None, String::new()),
                }
            }
        };
        
        // Call the regular completion method
        let result = self.completion(prompt, max_tokens, temperature).await;
        
        // Get the end timestamp
        let end_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
            
        // Log to Langfuse if enabled and we have a valid trace ID
        if !trace_id_str.is_empty() {
            if let Ok(response) = &result {
                if let Ok(tracer) = langfuse::get_tracer() {
                    let gen_name = generation_name.unwrap_or("llm_generation");
                    let cost = self.calculate_cost(&response.usage);
                    
                    // Create JSON for prompt and completion
                    let input_json = serde_json::json!(prompt);
                    let output_json = serde_json::json!(response.content);
                    
                    // Log the generation
                    let _ = tracer.log_generation(
                        &trace_id_str,
                        gen_name,
                        self.name(),
                        &serde_json::to_string(&input_json).unwrap_or_else(|_| prompt.to_string()),
                        &serde_json::to_string(&output_json).unwrap_or_else(|_| response.content.clone()),
                        &response.usage,
                        Some(&cost),
                        metadata,
                        Some(start_time),
                        Some(end_time),
                    ).await;
                }
            }
        }
        
        result
    }

    /// Get the name of the LLM client
    fn name(&self) -> &str {
        "unknown"
    }

    /// Get the cost per 1K tokens for prompt and completion
    fn get_token_prices(&self) -> (f64, f64) {
        (0.01, 0.01) // Default prices
    }

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

// Default client factory function
async fn default_client_factory(config: &LLMConfig) -> Result<Box<dyn LLMClient>> {
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

// Type for an async client factory function
type AsyncClientFactory = fn(&LLMConfig) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Arc<dyn LLMClient>>> + Send>>;

// Global state for client factory
static INIT: Once = Once::new();
static mut ASYNC_CLIENT_FACTORY: Option<AsyncClientFactory> = None;

/// Set a custom async client factory function for testing
pub fn set_client_factory(factory: AsyncClientFactory) {
    unsafe {
        INIT.call_once(|| {});
        ASYNC_CLIENT_FACTORY = Some(factory);
    }
}

/// Create an LLM client from a configuration and fetch pricing data
pub async fn create_client(config: &LLMConfig) -> Result<Box<dyn LLMClient>> {
    // Check if we have a custom factory
    unsafe {
        if let Some(factory) = ASYNC_CLIENT_FACTORY {
            let arc_client = factory(config).await?;
            
            // Convert Arc<dyn LLMClient> to Box<dyn LLMClient>
            // This is a bit of a hack, but needed for compatibility with existing code
            struct ArcWrapper {
                inner: Arc<dyn LLMClient>,
            }
            
            #[async_trait]
            impl LLMClient for ArcWrapper {
                fn name(&self) -> &str {
                    self.inner.name()
                }
                
                fn get_token_prices(&self) -> (f64, f64) {
                    self.inner.get_token_prices()
                }
                
                async fn completion(&self, prompt: &str, max_tokens: usize, temperature: f64) -> Result<LLMResponse> {
                    self.inner.completion(prompt, max_tokens, temperature).await
                }
                
                async fn completion_with_tracing(
                    &self,
                    prompt: &str,
                    max_tokens: usize,
                    temperature: f64,
                    trace_id: Option<&str>,
                    generation_name: Option<&str>,
                    metadata: Option<serde_json::Value>,
                ) -> Result<LLMResponse> {
                    self.inner.completion_with_tracing(
                        prompt,
                        max_tokens,
                        temperature,
                        trace_id,
                        generation_name,
                        metadata,
                    ).await
                }
                
                async fn fetch_pricing_data(&self) -> Result<()> {
                    self.inner.fetch_pricing_data().await
                }
                
                fn calculate_cost(&self, usage: &TokenUsage) -> TokenCost {
                    self.inner.calculate_cost(usage)
                }
            }
            
            return Ok(Box::new(ArcWrapper { inner: arc_client }));
        }
    }
    
    // Otherwise use the default factory
    default_client_factory(config).await
}