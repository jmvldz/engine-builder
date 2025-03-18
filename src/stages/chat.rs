use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
// No log imports needed
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use serde_json;
use std::io;
use std::time::Duration;

use crate::config::LLMConfig;
use crate::llm::client::{create_client, LLMClient};

pub struct ChatApp {
    input: String,
    messages: Vec<ChatMessage>,
    llm_client: Box<dyn LLMClient>,
}

pub enum ChatMessage {
    User(String),
    Assistant(String),
    System(String),
}

// Helper struct for parsing tool calls
struct ToolCall {
    tool: String,
    args: String,
}

impl ChatApp {
    pub async fn new(llm_config: &LLMConfig) -> Result<Self> {
        let llm_client = create_client(llm_config).await?;
        
        Ok(Self {
            input: String::new(),
            messages: vec![ChatMessage::System(
                "Welcome to the chat interface. Type your message and press Enter to send.".to_string(),
            )],
            llm_client,
        })
    }
    
    // Function to process a tool call from the LLM
    async fn process_tool_call(&mut self, tool_name: &str, _args: &str) -> Result<String> {
        match tool_name {
            "file_selection" => {
                // Implementation for file selection command
                self.messages.push(ChatMessage::System("Running file selection...".to_string()));
                // Actual implementation would call the file_selection module
                Ok("File selection completed.".to_string())
            }
            "relevance" => {
                // Implementation for relevance command
                self.messages.push(ChatMessage::System("Running relevance assessment...".to_string()));
                // Actual implementation would call the relevance module
                Ok("Relevance assessment completed.".to_string())
            }
            "ranking" => {
                // Implementation for ranking command
                self.messages.push(ChatMessage::System("Running file ranking...".to_string()));
                // Actual implementation would call the ranking module
                Ok("File ranking completed.".to_string())
            }
            "dockerfile" => {
                // Implementation for dockerfile command
                self.messages.push(ChatMessage::System("Generating Dockerfile...".to_string()));
                // Actual implementation would call the dockerfile module
                Ok("Dockerfile generation completed.".to_string())
            }
            "build_image" => {
                // Implementation for build_image command
                self.messages.push(ChatMessage::System("Building Docker image...".to_string()));
                // Actual implementation would call the dockerfile module
                Ok("Docker image build completed.".to_string())
            }
            "run_lint" => {
                // Implementation for run_lint command
                self.messages.push(ChatMessage::System("Running lint container...".to_string()));
                // Actual implementation would call the container module
                Ok("Lint container execution completed.".to_string())
            }
            "run_test" => {
                // Implementation for run_test command
                self.messages.push(ChatMessage::System("Running test container...".to_string()));
                // Actual implementation would call the container module
                Ok("Test container execution completed.".to_string())
            }
            // Add more tools as needed
            _ => Ok(format!("Unknown tool: {}", tool_name)),
        }
    }
    
    // Parse a tool call from the LLM response
    fn parse_tool_call(&self, response: &str) -> Option<ToolCall> {
        // Check if the response contains a tool call
        if let Some(start) = response.find("```tool") {
            if let Some(end) = response[start..].find("```") {
                let tool_json = &response[start + 7..start + end].trim();
                
                // Parse the JSON
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(tool_json) {
                    if let (Some(tool), Some(args)) = (
                        json.get("tool").and_then(|t| t.as_str()),
                        json.get("args").and_then(|a| a.as_str()),
                    ) {
                        return Some(ToolCall {
                            tool: tool.to_string(),
                            args: args.to_string(),
                        });
                    }
                }
            }
        }
        
        None
    }
    
    // Send message to LLM with tool information
    async fn send_to_llm(&mut self, user_input: &str) -> Result<()> {
        // Create a system prompt that informs the LLM about available tools
        let system_prompt = r#"
        You are an AI assistant with access to the following tools:
        
        - file_selection: Run the file selection process to identify potentially relevant files
        - relevance: Run the relevance assessment to evaluate file relevance
        - ranking: Run the file ranking to prioritize relevant files
        - dockerfile: Generate a test-focused Dockerfile
        - build_image: Build a Docker image from the generated Dockerfile
        - run_lint: Run lint script in a Docker container
        - run_test: Run test script in a Docker container
        
        To use a tool, respond with:
        ```tool
        {
            "tool": "tool_name",
            "args": "arguments for the tool"
        }
        ```
        
        Otherwise, respond directly to the user's query.
        "#;
        
        // In a real implementation, you would need to modify the LLM client to support system prompts
        // For now, we'll just prepend it to the user input
        let full_prompt = format!("{}\n\nUser: {}", system_prompt, user_input);
        
        match self.llm_client.completion(&full_prompt, 2048, 0.7).await {
            Ok(response) => {
                // Parse the response to check for tool calls
                if let Some(tool_call) = self.parse_tool_call(&response.content) {
                    let tool_result = self.process_tool_call(&tool_call.tool, &tool_call.args).await?;
                    self.messages.push(ChatMessage::System(tool_result));
                } else {
                    self.messages.push(ChatMessage::Assistant(response.content));
                }
            }
            Err(e) => {
                self.messages.push(ChatMessage::System(format!("Error: {}", e)));
            }
        }
        
        Ok(())
    }
    
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        
        // Main loop
        let result = self.run_app(&mut terminal).await;
        
        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;
        
        result
    }
    
    async fn run_app<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        loop {
            terminal.draw(|f| self.ui::<B>(f))?;
            
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Enter => {
                                if !self.input.is_empty() {
                                    let user_input = std::mem::take(&mut self.input);
                                    self.messages.push(ChatMessage::User(user_input.clone()));
                                    
                                    // Process command if it starts with "/"
                                    if user_input.starts_with('/') {
                                        if user_input == "/quit" || user_input == "/exit" {
                                            return Ok(());
                                        }
                                        // Handle other commands here
                                        self.messages.push(ChatMessage::System(
                                            format!("Command not implemented: {}", user_input),
                                        ));
                                    } else {
                                        // Send to LLM
                                        if let Err(e) = self.send_to_llm(&user_input).await {
                                            self.messages.push(ChatMessage::System(format!("Error: {}", e)));
                                        }
                                    }
                                }
                            }
                            KeyCode::Char(c) => {
                                self.input.push(c);
                            }
                            KeyCode::Backspace => {
                                self.input.pop();
                            }
                            KeyCode::Esc => {
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    
    fn ui(&self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(3),
            ])
            .split(f.size());
        
        // Chat history
        let messages: Vec<ratatui::text::Line> = self
            .messages
            .iter()
            .map(|m| match m {
                ChatMessage::User(msg) => {
                    ratatui::text::Line::from(vec![
                        Span::styled("You: ", Style::default().fg(Color::Green)),
                        Span::raw(msg),
                    ])
                }
                ChatMessage::Assistant(msg) => {
                    ratatui::text::Line::from(vec![
                        Span::styled("Assistant: ", Style::default().fg(Color::Blue)),
                        Span::raw(msg),
                    ])
                }
                ChatMessage::System(msg) => {
                    ratatui::text::Line::from(vec![
                        Span::styled("System: ", Style::default().fg(Color::Yellow)),
                        Span::raw(msg),
                    ])
                }
            })
            .collect();
        
        let chat_history = Paragraph::new(messages)
            .wrap(Wrap { trim: true })
            .scroll((
                (self.messages.len() as u16).saturating_sub(chunks[0].height.saturating_sub(2)),
                0
            ));
        
        f.render_widget(chat_history, chunks[0]);
        
        // Input box
        let input = Paragraph::new(self.input.as_str())
            .style(Style::default())
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Input")
                .border_style(Style::default().fg(Color::Blue)));
        
        f.render_widget(input, chunks[1]);
        
        // Cursor position
        f.set_cursor(
            chunks[1].x + self.input.len() as u16 + 1,
            chunks[1].y + 1,
        );
    }
}
