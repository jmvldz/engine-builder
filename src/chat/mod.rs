use anyhow::Result;
use crate::config::{Config, LLMConfig};
use crate::llm::client::{LLMClient, LLMResponse};
use std::io::{self, Write};
use colored::Colorize;
use tokio::sync::mpsc;

mod tools;
mod ui;

pub use tools::Tool;
pub use ui::ChatUI;

/// Represents a message in the chat conversation
#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
    Assistant(String),
    System(String),
    Tool {
        name: String,
        input: String,
        output: String,
    },
}

/// Manages the chat conversation with the LLM
pub struct ChatSession {
    config: Config,
    llm_client: Box<dyn LLMClient>,
    messages: Vec<ChatMessage>,
    ui: ChatUI,
}

impl ChatSession {
    /// Create a new chat session with the given configuration
    pub async fn new(config: Config) -> Result<Self> {
        // Create LLM client from config
        let llm_client = crate::llm::client::create_client(&config.relevance.llm).await?;
        
        // Initialize UI
        let ui = ChatUI::new();
        
        // Initialize with system message
        let messages = vec![
            ChatMessage::System(format!(
                "You are an assistant helping with the engine-builder CLI. \
                You have access to various commands that you can run to help the user. \
                The available commands are: {}",
                tools::get_available_tools().join(", ")
            )),
        ];
        
        Ok(Self {
            config,
            llm_client,
            messages,
            ui,
        })
    }
    
    /// Start the chat session
    pub async fn start(&mut self) -> Result<()> {
        println!("{}", "Starting chat session with LLM...".green());
        println!("Type {} to exit the chat", "exit".yellow());
        
        // Display welcome message
        println!("\n{}\n", "Welcome to the engine-builder chat interface! I'm here to help you with your tasks. You can ask me questions or request me to perform actions using the available CLI commands.".bright_blue());
        
        loop {
            // Get user input
            let user_input = self.ui.read_input("You")?;
            
            // Check if user wants to exit
            if user_input.trim().to_lowercase() == "exit" {
                println!("{}", "Exiting chat session...".yellow());
                break;
            }
            
            // Add user message to conversation
            self.messages.push(ChatMessage::User(user_input.clone()));
            
            // Process user input and get LLM response
            let response = self.process_message(&user_input).await?;
            
            // Display assistant response
            self.ui.display_message("Assistant", &response);
            
            // Add assistant message to conversation
            self.messages.push(ChatMessage::Assistant(response));
        }
        
        Ok(())
    }
    
    /// Process a message and get the LLM response
    async fn process_message(&self, message: &str) -> Result<String> {
        // Convert messages to prompt format
        let prompt = self.format_messages_for_llm();
        
        // Get response from LLM
        let response = self.llm_client.completion(
            &prompt,
            self.config.relevance.max_tokens,
            0.7, // Temperature
        ).await?;
        
        // Parse the response for tool calls
        let response_content = response.content.clone();
        if let Some((tool_name, tool_input)) = self.parse_tool_call(&response_content) {
            // Execute the tool
            match self.execute_tool(&tool_name, &tool_input).await {
                Ok(tool_output) => {
                    // Display tool execution result
                    self.ui.display_tool_result(&tool_name, &tool_output);
                    
                    // Process the tool output with the LLM to get a final response
                    let tool_prompt = format!(
                        "{}\n\nTool: {}\nInput: {}\nOutput: {}\n\nAssistant: ",
                        prompt, tool_name, tool_input, tool_output
                    );
                    
                    let final_response = self.llm_client.completion(
                        &tool_prompt,
                        self.config.relevance.max_tokens,
                        0.7, // Temperature
                    ).await?;
                    
                    Ok(final_response.content)
                }
                Err(e) => {
                    // Return error message
                    Ok(format!("I tried to execute the tool '{}', but encountered an error: {}", tool_name, e))
                }
            }
        } else {
            // No tool call detected, return the original response
            Ok(response_content)
        }
    }
    
    /// Format messages for the LLM prompt
    fn format_messages_for_llm(&self) -> String {
        let mut prompt = String::new();
        
        for message in &self.messages {
            match message {
                ChatMessage::User(content) => {
                    prompt.push_str(&format!("Human: {}\n\n", content));
                }
                ChatMessage::Assistant(content) => {
                    prompt.push_str(&format!("Assistant: {}\n\n", content));
                }
                ChatMessage::System(content) => {
                    prompt.push_str(&format!("System: {}\n\n", content));
                }
                ChatMessage::Tool { name, input, output } => {
                    prompt.push_str(&format!("Tool: {}\nInput: {}\nOutput: {}\n\n", name, input, output));
                }
            }
        }
        
        // Add final prompt for assistant response
        prompt.push_str("Assistant: ");
        
        prompt
    }
    
    /// Execute a tool and add the result to the conversation
    async fn execute_tool(&mut self, tool_name: &str, input: &str) -> Result<String> {
        // Find and execute the tool
        let output = tools::execute_tool(tool_name, input, &self.config).await?;
        
        // Add tool execution to conversation
        self.messages.push(ChatMessage::Tool {
            name: tool_name.to_string(),
            input: input.to_string(),
            output: output.clone(),
        });
        
        Ok(output)
    }
    
    /// Parse a response for tool calls
    fn parse_tool_call(&self, response: &str) -> Option<(String, String)> {
        // Look for patterns like "I'll use the X tool" or "Let me execute X"
        let tool_patterns = [
            r"(?i)I'll use the (\w+) tool",
            r"(?i)Let me execute (\w+)",
            r"(?i)I'll run the (\w+) command",
            r"(?i)Using the (\w+) tool",
            r"(?i)Let me use the (\w+) tool",
            r"(?i)I'll call the (\w+) tool",
            r"(?i)I'll use (\w+) to",
        ];
        
        for pattern in tool_patterns {
            if let Some(captures) = regex::Regex::new(pattern).ok()?.captures(response) {
                if let Some(tool_match) = captures.get(1) {
                    let tool_name = tool_match.as_str().to_lowercase();
                    
                    // Check if this is a valid tool
                    if tools::get_available_tools().iter().any(|&t| t.to_lowercase() == tool_name) {
                        // Extract the input for the tool
                        // This is a simple heuristic - we take everything after the tool mention
                        let tool_mention_end = tool_match.end();
                        if tool_mention_end < response.len() {
                            let input = response[tool_mention_end..].trim().to_string();
                            return Some((tool_name, input));
                        }
                    }
                }
            }
        }
        
        // Also look for explicit tool call format
        let tool_call_pattern = r"(?i)Tool: (\w+)\s+Input: (.+?)(?:\n|$)";
        if let Some(captures) = regex::Regex::new(tool_call_pattern).ok()?.captures(response) {
            if let (Some(tool_match), Some(input_match)) = (captures.get(1), captures.get(2)) {
                let tool_name = tool_match.as_str().to_lowercase();
                let input = input_match.as_str().trim().to_string();
                
                // Check if this is a valid tool
                if tools::get_available_tools().iter().any(|&t| t.to_lowercase() == tool_name) {
                    return Some((tool_name, input));
                }
            }
        }
        
        None
    }
}