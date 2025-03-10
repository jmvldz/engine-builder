use async_trait::async_trait;
use anyhow::{Result, Context};
use reqwest::{Client, header};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use crate::config::LLMConfig;
use crate::llm::client::LLMClient;

/// OpenAI API response for chat completions
#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    prompt_tokens: usize,
    #[allow(dead_code)]
    completion_tokens: usize,
    #[allow(dead_code)]
    total_tokens: usize,
}

/// A client for the OpenAI API
pub struct OpenAIClient {
    client: Client,
    config: LLMConfig,
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
        })
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn completion(&self, prompt: &str, max_tokens: usize, temperature: f64) -> Result<String> {
        let base_url = self.config.base_url.as_deref().unwrap_or("https://api.openai.com/v1");
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
        
        let response = self.client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await
                .context("Failed to read error response from OpenAI API")?;
            return Err(anyhow::anyhow!("OpenAI API error ({}): {}", status, error_text));
        }
        
        let response_data: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;
        
        if response_data.choices.is_empty() {
            return Err(anyhow::anyhow!("OpenAI API returned no choices"));
        }
        
        let content = response_data.choices[0].message.content
            .clone()
            .ok_or_else(|| anyhow::anyhow!("OpenAI API returned null content"))?;
        
        Ok(content)
    }
    
    fn name(&self) -> &str {
        "openai"
    }
}