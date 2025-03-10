use async_trait::async_trait;
use anyhow::{Result, Context};
use reqwest::{Client, header};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use log::debug;

use crate::config::LLMConfig;
use crate::llm::client::{LLMClient, LLMResponse, TokenUsage};

/// Anthropic API response for chat completions
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicContent>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(default)]
    text: String,
    #[serde(default)]
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: usize,
    #[serde(default)]
    output_tokens: usize,
}

/// A client for the Anthropic API
pub struct AnthropicClient {
    client: Client,
    config: LLMConfig,
}

impl AnthropicClient {
    /// Create a new Anthropic client
    pub fn new(config: &LLMConfig) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        
        // Add the API key header
        let api_key = format!("{}", config.api_key);
        let api_key_header = header::HeaderValue::from_str(&api_key)
            .context("Failed to create x-api-key header")?;
        headers.insert("x-api-key", api_key_header);
        
        // Add the anthropic-version header
        headers.insert(
            "anthropic-version",
            header::HeaderValue::from_static("2023-06-01"),
        );
        
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
        })
    }
    
    /// Get token pricing for the configured model
    fn get_model_pricing(&self) -> (f64, f64) {
        match self.config.model.as_str() {
            m if m.contains("claude-3-opus") => (0.015, 0.075),
            m if m.contains("claude-3-sonnet") => (0.003, 0.015),
            m if m.contains("claude-3-haiku") => (0.00025, 0.00125),
            m if m.contains("claude-2") => (0.01, 0.03),
            m if m.contains("claude-instant") => (0.0008, 0.0024),
            _ => {
                debug!("Unknown model pricing for {}, using Claude 3 Sonnet pricing", self.config.model);
                (0.003, 0.015)
            }
        }
    }
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn completion(&self, prompt: &str, max_tokens: usize, temperature: f64) -> Result<LLMResponse> {
        let base_url = self.config.base_url.as_deref().unwrap_or("https://api.anthropic.com");
        let url = format!("{}/v1/messages", base_url);
        
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
        
        let response = self.client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await
                .context("Failed to read error response from Anthropic API")?;
            debug!("Anthropic API error: {}", error_text);
            return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, error_text));
        }
        
        let response_text = response.text().await
            .context("Failed to read response text from Anthropic API")?;
        debug!("Anthropic API response: {}", response_text);
        
        let response_data: AnthropicResponse = serde_json::from_str(&response_text)
            .context("Failed to parse Anthropic API response")?;
        
        if response_data.content.is_empty() {
            return Err(anyhow::anyhow!("Anthropic API returned no content"));
        }
        
        // Find the text content
        let text_content = response_data.content.iter()
            .find(|content| content.r#type == "text")
            .map(|content| content.text.clone())
            .unwrap_or_else(|| {
                debug!("No text content found, using first content item");
                response_data.content.first().map(|c| c.text.clone()).unwrap_or_default()
            });
        
        // Extract usage information
        let usage = if let Some(api_usage) = response_data.usage {
            TokenUsage {
                prompt_tokens: api_usage.input_tokens,
                completion_tokens: api_usage.output_tokens,
                total_tokens: api_usage.input_tokens + api_usage.output_tokens,
            }
        } else {
            // Fallback if API doesn't return usage
            debug!("No usage information returned from Anthropic API");
            TokenUsage::default()
        };
        
        Ok(LLMResponse {
            content: text_content,
            usage,
        })
    }
    
    fn name(&self) -> &str {
        "anthropic"
    }
    
    fn get_token_prices(&self) -> (f64, f64) {
        self.get_model_pricing()
    }
}