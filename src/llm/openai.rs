use anyhow::{Context, Result};
use async_trait::async_trait;
use log::debug;
use reqwest::{header, Client};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use crate::config::LLMConfig;
use crate::llm::client::{LLMClient, LLMResponse, TokenUsage};

/// OpenAI API response for chat completions
#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

use std::collections::HashMap;
use std::sync::RwLock;

/// OpenAI pricing response structure
#[derive(Debug, Deserialize)]
struct OpenAIPricingResponse {
    #[allow(dead_code)]
    data: Vec<OpenAIPricingModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAIPricingModel {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    pricing: Option<OpenAIModelPricing>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelPricing {
    #[allow(dead_code)]
    input: Option<f64>,
    #[allow(dead_code)]
    output: Option<f64>,
}

/// A client for the OpenAI API
pub struct OpenAIClient {
    client: Client,
    config: LLMConfig,
    pricing_cache: RwLock<HashMap<String, (f64, f64)>>,
}

impl OpenAIClient {
    /// Create a new OpenAI client
    pub fn new(config: &LLMConfig) -> Result<Self> {
        let mut headers = header::HeaderMap::new();

        // Add the API key header
        let auth_value = format!("Bearer {}", config.api_key);
        let auth_header = header::HeaderValue::from_str(&auth_value)
            .context("Failed to create Authorization header")?;
        headers.insert(header::AUTHORIZATION, auth_header);

        // Add content-type header
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        // Create the client
        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(config.timeout))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            config: config.clone(),
            pricing_cache: RwLock::new(HashMap::new()),
        })
    }

    /// Get token pricing for the configured model - fallback to static values if not in cache
    fn get_model_pricing(&self) -> (f64, f64) {
        // Try to get from cache first
        if let Some(pricing) = self.pricing_cache.read().unwrap().get(&self.config.model) {
            return *pricing;
        }

        // Fallback to hardcoded pricing if not in cache
        match self.config.model.as_str() {
            "gpt-4" => (0.03, 0.06),
            "gpt-4-32k" => (0.06, 0.12),
            "gpt-4-turbo" | "gpt-4-1106-preview" | "gpt-4-0125-preview" => (0.01, 0.03),
            "gpt-4o" | "gpt-4o-2024-05-13" => (0.005, 0.015),
            "gpt-3.5-turbo" | "gpt-3.5-turbo-1106" => (0.0015, 0.002),
            _ => {
                debug!(
                    "Unknown model pricing for {}, using GPT-4 pricing",
                    self.config.model
                );
                (0.03, 0.06)
            }
        }
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn completion(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f64,
    ) -> Result<LLMResponse> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/chat/completions", base_url);

        let request_body = json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens,
            "temperature": temperature,
        });

        let response = self
            .client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .context("Failed to read error response from OpenAI API")?;
            return Err(anyhow::anyhow!(
                "OpenAI API error ({}): {}",
                status,
                error_text
            ));
        }

        let response_data: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;

        if response_data.choices.is_empty() {
            return Err(anyhow::anyhow!("OpenAI API returned no choices"));
        }

        let content = response_data.choices[0]
            .message
            .content
            .clone()
            .ok_or_else(|| anyhow::anyhow!("OpenAI API returned null content"))?;

        // Extract usage information
        let usage = if let Some(api_usage) = response_data.usage {
            TokenUsage {
                prompt_tokens: api_usage.prompt_tokens,
                completion_tokens: api_usage.completion_tokens,
                total_tokens: api_usage.total_tokens,
            }
        } else {
            // Fallback if API doesn't return usage
            debug!("No usage information returned from OpenAI API");
            TokenUsage::default()
        };

        Ok(LLMResponse { content, usage })
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn get_token_prices(&self) -> (f64, f64) {
        self.get_model_pricing()
    }

    async fn fetch_pricing_data(&self) -> Result<()> {
        debug!("Fetching OpenAI pricing data");

        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");
        let url = format!("{}/models", base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch OpenAI models for pricing")?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .context("Failed to read error response from OpenAI API")?;
            debug!("Error fetching OpenAI pricing: {}", error_text);
            return Ok(()); // Continue with hardcoded pricing
        }

        // OpenAI doesn't expose pricing in their API directly
        // We would need to parse from their pricing page or use another source
        // For now, we'll populate the cache with the static values for the current model

        let model_name = self.config.model.clone();
        let pricing = self.get_model_pricing(); // Get hardcoded pricing

        // Update cache
        let mut pricing_cache = self.pricing_cache.write().unwrap();
        pricing_cache.insert(model_name, pricing);

        debug!("Updated pricing cache for OpenAI model");
        Ok(())
    }
}
