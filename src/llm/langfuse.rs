use anyhow::{Context, Result};
use log::{debug, warn};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Langfuse API configuration
const DEFAULT_API_URL: &str = "https://us.cloud.langfuse.com";
const API_PATH: &str = "/api/public";

// Langfuse trace types and models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    pub problem_id: Option<String>,
    pub file_path: Option<String>,
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageMetadata {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub prompt_cost: Option<f64>,
    pub completion_cost: Option<f64>,
    pub total_cost: Option<f64>,
}

// Event wrapper for batch ingestion
#[derive(Debug, Clone, Serialize)]
struct IngestionEvent {
    id: String,
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
    body: serde_json::Value,
}

// We're using serde_json::Value directly instead of TraceBody

// We use serde_json::Value directly for all event bodies to match Langfuse API

// Batch request
#[derive(Debug, Serialize)]
struct BatchRequest {
    batch: Vec<serde_json::Value>,
}

/// Langfuse client for sending observability data
#[derive(Clone)]
pub struct LangfuseClient {
    client: Client,
    base_url: String,
    secret_key: String,
    public_key: String,
    enabled: bool,
    pub trace_id: Option<String>,
}

impl Default for LangfuseClient {
    fn default() -> Self {
        Self {
            client: Client::new(),
            base_url: DEFAULT_API_URL.to_string(),
            secret_key: String::new(),
            public_key: String::new(),
            enabled: false,
            trace_id: None,
        }
    }
}

impl LangfuseClient {
    /// Create a new Langfuse client
    pub fn new(
        base_url: Option<String>,
        secret_key: Option<String>,
        public_key: Option<String>,
        project_id: Option<String>,
        enabled: Option<bool>,
        trace_id: Option<String>,
    ) -> Result<Self> {
        // Try environment variables if not provided directly
        let secret_key = secret_key
            .or_else(|| env::var("LANGFUSE_SECRET_KEY").ok())
            .unwrap_or_default();

        let public_key = public_key
            .or_else(|| env::var("LANGFUSE_PUBLIC_KEY").ok())
            .unwrap_or_default();

        let base_url = base_url
            .or_else(|| env::var("LANGFUSE_HOST").ok())
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());

        let project_name = project_id
            .or_else(|| env::var("LANGFUSE_PROJECT_ID").ok())
            .unwrap_or_else(|| "engines-builder".to_string());

        // Check if required keys are available and explicitly enabled
        let has_credentials = !secret_key.is_empty() && !public_key.is_empty();
        let is_enabled = enabled.unwrap_or(has_credentials);

        // Only enable if both conditions are met
        let enabled = is_enabled && has_credentials;

        if !enabled {
            if is_enabled && !has_credentials {
                warn!("Langfuse tracing is enabled but missing credentials. Set secret_key and public_key in config.json or as environment variables.");
            } else if !is_enabled {
                debug!("Langfuse tracing is disabled by configuration.");
            }
        } else {
            debug!("Langfuse tracing enabled for project: {}", project_name);
        }

        // Create the client
        let client = Client::builder()
            .build()
            .context("Failed to create HTTP client for Langfuse")?;

        Ok(Self {
            client,
            base_url,
            secret_key,
            public_key,
            enabled,
            trace_id,
        })
    }

    /// Create a new Langfuse client with explicit API keys
    pub fn with_credentials(
        secret_key: &str,
        public_key: &str,
        project_id: &str,
        base_url: Option<&str>,
        enabled: Option<bool>,
        trace_id: Option<&str>,
    ) -> Result<Self> {
        Self::new(
            base_url.map(|s| s.to_string()),
            Some(secret_key.to_string()),
            Some(public_key.to_string()),
            Some(project_id.to_string()),
            enabled,
            trace_id.map(|s| s.to_string()),
        )
    }

    /// Get the current timestamp in ISO 8601 format
    fn current_timestamp() -> String {
        let now = chrono::Utc::now();
        now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    /// Get timestamp in milliseconds
    fn timestamp_ms() -> u64 {
        let start = SystemTime::now();
        let since_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        since_epoch.as_millis() as u64
    }

    /// Format timestamp to ISO 8601
    fn format_timestamp(timestamp_ms: u64) -> String {
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64)
            .unwrap_or_else(|| chrono::Utc::now());
        dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    /// Create a new trace
    pub async fn create_trace(
        &self,
        name: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<String> {
        if !self.enabled {
            return Ok(self
                .trace_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string()));
        }

        let trace_id = self
            .trace_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let event_id = Uuid::new_v4().to_string();
        let timestamp = Self::current_timestamp();

        // Create trace body as JSON to match Langfuse API spec
        let trace_body = json!({
            "id": trace_id.clone(),
            "name": name.to_string(),
            "timestamp": timestamp.clone(),
            "metadata": metadata
        });

        let event = IngestionEvent {
            id: event_id,
            timestamp,
            event_type: "trace-create".to_string(),
            body: trace_body,
        };

        let batch = BatchRequest {
            batch: vec![serde_json::to_value(event)?],
        };

        let url = format!("{}{}/ingestion", self.base_url, API_PATH);

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.public_key, Some(&self.secret_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&batch)
            .send()
            .await;

        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Failed to read error response".to_string());
                    warn!("Langfuse API error ({}): {}", status, error_text);
                } else {
                    debug!("Created Langfuse trace: {}", trace_id);
                }
            }
            Err(e) => {
                warn!("Failed to send trace to Langfuse: {}", e);
            }
        }

        Ok(trace_id)
    }

    /// Log a generation event
    pub async fn log_generation(
        &self,
        trace_id: &str,
        name: &str,
        model: &str,
        prompt: &str,
        completion: &str,
        token_usage: &crate::llm::client::TokenUsage,
        token_cost: Option<&crate::llm::client::TokenCost>,
        metadata: Option<serde_json::Value>,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<String> {
        if !self.enabled {
            return Ok(Uuid::new_v4().to_string());
        }

        let observation_id = Uuid::new_v4().to_string();
        let event_id = Uuid::new_v4().to_string();
        let now = Self::timestamp_ms();

        let start_time_ms = start_time.unwrap_or(now - 1000); // Default to 1 second ago
        let end_time_ms = end_time.unwrap_or(now);

        let start_time_iso = Self::format_timestamp(start_time_ms);
        let end_time_iso = Self::format_timestamp(end_time_ms);

        // Build usage object according to Langfuse spec
        let usage_details = json!({
            "prompt_tokens": token_usage.prompt_tokens,
            "completion_tokens": token_usage.completion_tokens,
            "total_tokens": token_usage.total_tokens
        });

        // Build cost details
        let cost_details = if let Some(cost) = token_cost {
            json!({
                "prompt_cost": cost.prompt_cost,
                "completion_cost": cost.completion_cost,
                "total_cost": cost.total_cost
            })
        } else {
            json!(null)
        };

        // Parse prompt and completion as JSON if they are valid JSON strings
        let input_value = match serde_json::from_str::<serde_json::Value>(prompt) {
            Ok(json_value) => json_value,
            Err(_) => json!(prompt),
        };

        let output_value = match serde_json::from_str::<serde_json::Value>(completion) {
            Ok(json_value) => json_value,
            Err(_) => json!(completion),
        };

        // Create the generation body according to the Langfuse API spec
        let generation_body = json!({
            "id": observation_id.clone(),
            "traceId": trace_id.to_string(),
            "type": "generation",
            "name": name.to_string(),
            "startTime": start_time_iso,
            "endTime": end_time_iso,
            "metadata": metadata,
            "input": input_value,
            "output": output_value,
            "level": "DEFAULT",
            "model": model.to_string(),
            "usageDetails": usage_details,
            "costDetails": cost_details
        });

        let event = IngestionEvent {
            id: event_id,
            timestamp: Self::current_timestamp(),
            event_type: "generation-create".to_string(),
            body: generation_body,
        };

        let batch = BatchRequest {
            batch: vec![serde_json::to_value(event)?],
        };

        let url = format!("{}{}/ingestion", self.base_url, API_PATH);

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.public_key, Some(&self.secret_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&batch)
            .send()
            .await;

        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Failed to read error response".to_string());
                    warn!("Langfuse API error ({}): {}", status, error_text);
                } else {
                    debug!("Logged generation to Langfuse: {}", observation_id);
                }
            }
            Err(e) => {
                warn!("Failed to send generation to Langfuse: {}", e);
            }
        }

        Ok(observation_id)
    }

    /// Log an event
    pub async fn log_event(
        &self,
        trace_id: &str,
        name: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<String> {
        if !self.enabled {
            return Ok(Uuid::new_v4().to_string());
        }

        let observation_id = Uuid::new_v4().to_string();
        let event_id = Uuid::new_v4().to_string();
        let timestamp = Self::current_timestamp();

        // Create event body as JSON to match Langfuse API spec
        let event_body = json!({
            "id": observation_id.clone(),
            "traceId": trace_id.to_string(),
            "type": "event",
            "name": name.to_string(),
            "startTime": timestamp.clone(),
            "endTime": timestamp.clone(),
            "metadata": metadata,
            "level": "DEFAULT"
        });

        let event = IngestionEvent {
            id: event_id,
            timestamp,
            event_type: "event-create".to_string(),
            body: event_body,
        };

        let batch = BatchRequest {
            batch: vec![serde_json::to_value(event)?],
        };

        let url = format!("{}{}/ingestion", self.base_url, API_PATH);

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.public_key, Some(&self.secret_key))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&batch)
            .send()
            .await;

        match response {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Failed to read error response".to_string());
                    warn!("Langfuse API error ({}): {}", status, error_text);
                } else {
                    debug!("Logged event to Langfuse: {}", observation_id);
                }
            }
            Err(e) => {
                warn!("Failed to send event to Langfuse: {}", e);
            }
        }

        Ok(observation_id)
    }
}

/// Singleton instance of the Langfuse client
pub struct LangfuseTracer {
    client: Arc<LangfuseClient>,
}

impl LangfuseTracer {
    // Create a new Langfuse tracer
    pub fn new() -> Result<Self> {
        let client = LangfuseClient::new(None, None, None, None, None, None)?;
        Ok(Self {
            client: Arc::new(client),
        })
    }

    // Create a new Langfuse tracer with explicit credentials
    pub fn with_credentials(
        secret_key: &str,
        public_key: &str,
        project_id: &str,
        base_url: Option<&str>,
        enabled: Option<bool>,
        trace_id: Option<&str>,
    ) -> Result<Self> {
        let client = LangfuseClient::with_credentials(
            secret_key, public_key, project_id, base_url, enabled, trace_id,
        )?;

        Ok(Self {
            client: Arc::new(client),
        })
    }

    // Get the client
    pub fn client(&self) -> Arc<LangfuseClient> {
        self.client.clone()
    }
}

// Global Langfuse tracer
use std::sync::OnceLock;
static LANGFUSE_TRACER: OnceLock<LangfuseTracer> = OnceLock::new();

/// Initialize the global Langfuse tracer
pub fn init_langfuse(
    secret_key: &str,
    public_key: &str,
    project_id: &str,
    base_url: Option<&str>,
    enabled: Option<bool>,
    trace_id: Option<&str>,
) -> Result<()> {
    let tracer = LangfuseTracer::with_credentials(
        secret_key, public_key, project_id, base_url, enabled, trace_id,
    )?;

    // Set the global tracer
    let _ = LANGFUSE_TRACER.set(tracer);
    Ok(())
}

/// Get the global Langfuse tracer
pub fn get_tracer() -> Result<Arc<LangfuseClient>> {
    match LANGFUSE_TRACER.get() {
        Some(tracer) => Ok(tracer.client()),
        None => {
            // Initialize with defaults if not set yet
            let tracer = LangfuseTracer::new()?;
            let client = tracer.client();
            let _ = LANGFUSE_TRACER.set(tracer);
            Ok(client)
        }
    }
}
