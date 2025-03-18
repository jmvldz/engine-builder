use anyhow::{Context, Result};
use std::io::{self, Write};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
    ExecutableCommand,
    cursor,
    style::{Color, SetForegroundColor, Print, ResetColor},
};
use tokio::sync::mpsc;
use crate::config::{Config, LLMConfig};
use crate::models::problem::SWEBenchProblem;
use crate::llm::client::create_client;

pub mod tools;

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
    
    println!("Starting chat with {}/{}", &config.llm_config.model_type, &config.llm_config.model);
    println!("Type 'exit' or press Ctrl+C to end the session");
    println!("For a list of available commands, type 'help'");
    
    // Create channel for passing user input to the chat loop
    let (tx, mut rx) = mpsc::channel::<String>(10);
    
    // Load the application config for tool execution
    let app_config = Config::from_file(None).unwrap_or_else(|_| Config::default());
    
    // Create a default problem for tool execution
    let problem = SWEBenchProblem::new(
        app_config.codebase.problem_id.clone(),
        app_config.codebase.problem_statement.clone(),
    )
    .with_codebase_path(&app_config.codebase.path);
    
    // Spawn a separate task for handling user input
    let input_handle = tokio::spawn(async move {
        loop {
            match get_user_input_with_box("You: ") {
                Ok(input) => {
                    if tx.send(input.clone()).await.is_err() {
                        break;
                    }
                    
                    if input.trim().eq_ignore_ascii_case("exit") {
                        break;
                    }
                },
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    break;
                }
            }
        }
    });
    
    // Keep track of the conversation history
    let mut history = Vec::new();
    
    // Add initial system message
    history.push(ChatMessage {
        role: "system".to_string(),
        content: create_system_prompt(),
    });
    
    // Main chat loop
    while let Some(input) = rx.recv().await {
        if input.trim().eq_ignore_ascii_case("exit") {
            break;
        }
        
        // Add user message to history
        history.push(ChatMessage {
            role: "user".to_string(),
            content: input.clone(),
        });
        
        // Handle built-in commands
        if input.trim().eq_ignore_ascii_case("help") {
            print_help();
            history.push(ChatMessage {
                role: "assistant".to_string(),
                content: "I've displayed the help information above.".to_string(),
            });
            continue;
        }
        
        // Create prompt from history
        let prompt = create_prompt(&history);
        
        // Get response from LLM
        match llm_client.completion(&prompt, config.max_tokens, config.temperature).await {
            Ok(response) => {
                // Check if the response contains a tool call
                if let Some((tool_name, params)) = tools::parse_tool_call(&response.content) {
                    // Execute the tool
                    print_assistant_message(&format!("I'll run the '{}' command for you...", tool_name));
                    
                    match tools::execute_tool(&tool_name, &params, &app_config, &problem).await {
                        Ok(result) => {
                            // Print the tool result
                            if result.success {
                                print_tool_success(&result.output);
                            } else {
                                print_tool_error(&result.output);
                            }
                            
                            // Add the tool call and result to the history
                            history.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: format!(
                                    "I'll run the '{}' command for you.\n\nResult: {}",
                                    tool_name,
                                    result.output
                                ),
                            });
                        }
                        Err(e) => {
                            print_tool_error(&format!("Error executing tool: {}", e));
                            
                            history.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: format!(
                                    "I tried to run the '{}' command but encountered an error: {}",
                                    tool_name, e
                                ),
                            });
                        }
                    }
                } else {
                    // Regular response
                    print_assistant_message(&response.content);
                    
                    // Add assistant response to history
                    history.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: response.content.clone(),
                    });
                }
            },
            Err(e) => {
                eprintln!("Error getting response: {}", e);
            }
        }
    }
    
    // Wait for the input handling task to complete
    let _ = input_handle.await;
    
    println!("Chat session ended.");
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

/// Print available commands
fn print_help() {
    println!("\n--- Available Commands ---");
    println!("help    - Show this help message");
    println!("exit    - End the chat session");
    println!("");
    println!("--- Available Tools ---");
    
    for tool in tools::get_tools() {
        println!("{} - {}", tool.name, tool.description);
    }
    
    println!("-------------------------\n");
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

/// Display the assistant's message with formatting
fn print_assistant_message(message: &str) {
    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Cyan));
    println!("\nAssistant:");
    let _ = stdout.execute(ResetColor);
    println!("{}\n", message);
    let _ = stdout.flush();
}

/// Display a tool success message
fn print_tool_success(message: &str) {
    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Green));
    println!("\n✓ Success:");
    let _ = stdout.execute(ResetColor);
    println!("{}\n", message);
    let _ = stdout.flush();
}

/// Display a tool error message
fn print_tool_error(message: &str) {
    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Red));
    println!("\n✗ Error:");
    let _ = stdout.execute(ResetColor);
    println!("{}\n", message);
    let _ = stdout.flush();
}

/// Get user input with a nice box around it
fn get_user_input_with_box(prompt: &str) -> Result<String> {
    let mut stdout = io::stdout();
    let mut input = String::new();
    let min_width = 80;
    
    // Enable raw mode for better control over terminal
    enable_raw_mode()?;
    
    // Print the box top
    stdout.execute(Print("\n┌"))?;
    for _ in 0..min_width {
        stdout.execute(Print("─"))?;
    }
    stdout.execute(Print("┐\n"))?;
    
    // Print the prompt line (with prompt)
    stdout.execute(Print("│ "))?;
    stdout.execute(SetForegroundColor(Color::Green))?;
    stdout.execute(Print(prompt))?;
    stdout.execute(ResetColor)?;
    
    // Initialize cursor position after prompt
    let prompt_len = prompt.len();
    
    // Event handling loop
    loop {
        // Clear line after the prompt
        stdout.execute(cursor::SavePosition)?;
        
        // Calculate the remaining space
        let remaining_space = min_width - (prompt_len + input.len() + 3); // +3 for "│ " and "│" at end
        
        // Print input and padding
        stdout.execute(Print(&input))?;
        for _ in 0..remaining_space {
            stdout.execute(Print(" "))?;
        }
        stdout.execute(Print("│"))?;
        
        // Restore cursor position to continue editing
        stdout.execute(cursor::RestorePosition)?;
        stdout.execute(cursor::MoveRight(input.len() as u16))?;
        stdout.flush()?;
        
        // Handle keyboard input
        if let Event::Key(key_event) = event::read()? {
            if key_event.kind == KeyEventKind::Press {
                match key_event.code {
                    KeyCode::Enter => {
                        break;
                    }
                    KeyCode::Char(c) => {
                        input.push(c);
                        // Print the character
                        stdout.execute(Print(c.to_string()))?;
                    }
                    KeyCode::Backspace => {
                        if !input.is_empty() {
                            input.pop();
                            // Move left, print space, move left again to erase
                            stdout.execute(cursor::MoveLeft(1))?;
                            stdout.execute(Print(" "))?;
                            stdout.execute(cursor::MoveLeft(1))?;
                        }
                    }
                    KeyCode::Esc => {
                        input = "exit".to_string();
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    
    // Print the box bottom
    stdout.execute(cursor::MoveToColumn(0))?;
    stdout.execute(cursor::MoveDown(1))?;
    stdout.execute(Print("└"))?;
    for _ in 0..min_width {
        stdout.execute(Print("─"))?;
    }
    stdout.execute(Print("┘\n"))?;
    
    // Disable raw mode to return to normal terminal behavior
    disable_raw_mode()?;
    
    Ok(input)
}