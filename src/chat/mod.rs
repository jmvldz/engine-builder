use anyhow::{Context, Result};
use std::io;
use tokio::sync::mpsc;
use crate::config::{Config, LLMConfig};
use crate::models::problem::SWEBenchProblem;
use crate::llm::client::create_client;

pub mod tools;
pub mod ui;

/// Structure for chat messages
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Configuration for the chat session
#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub llm_config: LLMConfig,
    pub max_tokens: usize,
    pub temperature: f64,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            llm_config: LLMConfig {
                model_type: "anthropic".to_string(),
                model: "claude-3-sonnet-20240229".to_string(),
                api_key: "".to_string(),
                base_url: None,
                timeout: 30,
                max_retries: 3,
            },
            max_tokens: 4096,
            temperature: 0.7,
        }
    }
}

/// Starts a chat session with the configured LLM
pub async fn start_chat(config: ChatConfig) -> Result<()> {
    let llm_client = create_client(&config.llm_config)
        .await
        .context("Failed to create LLM client")?;
    
    log::info!("Starting chat with {}/{}", &config.llm_config.model_type, &config.llm_config.model);
    
    // Create channels for communication between UI and chat processing
    let (ui_tx, ui_rx) = mpsc::channel::<ChatMessage>(100);
    let (input_tx, mut input_rx) = mpsc::channel::<String>(10);
    
    // Load the application config for tool execution
    let app_config = Config::from_file(None).unwrap_or_else(|_| Config::default());
    
    // Create a default problem for tool execution
    let problem = SWEBenchProblem::new(
        app_config.codebase.problem_id.clone(),
        app_config.codebase.problem_statement.clone(),
    )
    .with_codebase_path(&app_config.codebase.path);
    
    // Keep track of the conversation history
    let mut history = Vec::new();
    
    // Add initial system message
    let system_message = ChatMessage {
        role: "system".to_string(),
        content: create_system_prompt(),
    };
    
    history.push(system_message.clone());
    
    // Send welcome message to UI
    let welcome_message = ChatMessage {
        role: "assistant".to_string(),
        content: format!(
            "Welcome to the Engine Builder Chat Interface!\n\nI'm using the {} model.\n\nHow can I help you today? Type 'help' for available commands.",
            &config.llm_config.model
        ),
    };
    
    let _ = ui_tx.send(welcome_message.clone()).await;
    history.push(welcome_message);
    
    // Spawn UI task
    let ui_handle = tokio::spawn(ui::run_chat_ui(ui_rx, input_tx));
    
    // Main chat loop
    while let Some(input) = input_rx.recv().await {
        if input.trim().eq_ignore_ascii_case("exit") {
            break;
        }
        
        // Add user message to history
        let user_message = ChatMessage {
            role: "user".to_string(),
            content: input.clone(),
        };
        history.push(user_message);
        
        // Handle built-in commands
        if input.trim().eq_ignore_ascii_case("help") {
            let help_message = ChatMessage {
                role: "assistant".to_string(),
                content: "Available Tools:\n".to_string() + 
                    &tools::get_tools().iter()
                        .map(|t| format!("- {} - {}", t.name, t.description))
                        .collect::<Vec<_>>()
                        .join("\n"),
            };
            
            let _ = ui_tx.send(help_message.clone()).await;
            history.push(help_message);
            continue;
        }
        
        // Create "thinking" message
        let thinking_message = ChatMessage {
            role: "assistant".to_string(),
            content: "Thinking...".to_string(),
        };
        let _ = ui_tx.send(thinking_message).await;
        
        // Create prompt from history
        let prompt = create_prompt(&history);
        
        // Get response from LLM
        match llm_client.completion(&prompt, config.max_tokens, config.temperature).await {
            Ok(response) => {
                // Check if the response contains a tool call
                if let Some((tool_name, params)) = tools::parse_tool_call(&response.content) {
                    // Execute the tool
                    let tool_call_message = ChatMessage {
                        role: "assistant".to_string(),
                        content: format!("I'll run the '{}' command for you...", tool_name),
                    };
                    let _ = ui_tx.send(tool_call_message.clone()).await;
                    history.push(tool_call_message);
                    
                    match tools::execute_tool(&tool_name, &params, &app_config, &problem).await {
                        Ok(result) => {
                            // Create tool result message
                            let result_message = ChatMessage {
                                role: "assistant".to_string(),
                                content: format!(
                                    "Result: {} - {}",
                                    if result.success { "SUCCESS" } else { "FAILED" },
                                    result.output
                                ),
                            };
                            
                            let _ = ui_tx.send(result_message.clone()).await;
                            history.push(result_message);
                        }
                        Err(e) => {
                            // Create error message
                            let error_message = ChatMessage {
                                role: "assistant".to_string(),
                                content: format!("Error executing tool: {}", e),
                            };
                            
                            let _ = ui_tx.send(error_message.clone()).await;
                            history.push(error_message);
                        }
                    }
                } else {
                    // Regular response
                    let response_message = ChatMessage {
                        role: "assistant".to_string(),
                        content: response.content.clone(),
                    };
                    
                    let _ = ui_tx.send(response_message.clone()).await;
                    history.push(response_message);
                }
            },
            Err(e) => {
                // Send error message
                let error_message = ChatMessage {
                    role: "assistant".to_string(),
                    content: format!("Error getting response: {}", e),
                };
                
                let _ = ui_tx.send(error_message.clone()).await;
                history.push(error_message);
            }
        }
    }
    
    // Abort UI task when chat ends
    ui_handle.abort();
    
    log::info!("Chat session ended");
    Ok(())
}

/// Create the system prompt with tool descriptions
fn create_system_prompt() -> String {
    let mut prompt = String::new();
    
    prompt.push_str("You are a helpful assistant with access to all command-line tools from engine-builder.\n");
    prompt.push_str("You can use the following tools to help the user with their tasks:\n\n");
    
    // Add tool descriptions
    for tool in tools::get_tools() {
        prompt.push_str(&format!("- {}: {}\n", tool.name, tool.description));
        
        // Add parameter descriptions if any
        if !tool.parameters.is_empty() {
            prompt.push_str("  Parameters:\n");
            for (name, param) in &tool.parameters {
                let default_str = param.default.as_ref()
                    .map(|d| format!(" (default: {})", d))
                    .unwrap_or_else(|| "".to_string());
                
                prompt.push_str(&format!("  - {}: {}{}\n", name, param.description, default_str));
            }
        }
        
        prompt.push_str("\n");
    }
    
    prompt.push_str("\nTo use a tool, use the format: TOOL: tool_name(param1=value1, param2=value2)\n");
    prompt.push_str("For example: TOOL: build_image(tag=\"my-image\")\n\n");
    prompt.push_str("You should always provide a brief explanation before using a tool, and explain the results after.\n\n");
    
    prompt
}

/// Create a prompt from the conversation history
fn create_prompt(history: &[ChatMessage]) -> String {
    // This implementation works for both Anthropic and OpenAI models
    let mut prompt = String::new();
    
    // Add the conversation history
    for message in history {
        match message.role.as_str() {
            "system" => {
                prompt.push_str(&format!("System: {}\n\n", message.content));
            }
            "user" => {
                prompt.push_str(&format!("Human: {}\n\n", message.content));
            }
            "assistant" => {
                prompt.push_str(&format!("Assistant: {}\n\n", message.content));
            }
            _ => {}
        }
    }
    
    // Add the final prompt for the assistant
    prompt.push_str("Assistant: ");
    
    prompt
}