use async_trait::async_trait;
use anyhow::{Result, Context};
use reqwest::{Client, header};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use log::debug;

use crate::config::LLMConfig;
use crate::llm::client::LLMClient;

/// Anthropic API response for chat completions
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicContent>,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    #[serde(default)]
    input_tokens: usize,
    #[allow(dead_code)]
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
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn completion(&self, prompt: &str, max_tokens: usize, temperature: f64) -> Result<String> {
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
        
        Ok(text_content)
    }
    
    fn name(&self) -> &str {
        "anthropic"
    }
}