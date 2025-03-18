use anyhow::Result;
use colored::Colorize;
use std::io::{self, Write};

/// Handles the UI for the chat interface
pub struct ChatUI {
    input_width: usize,
}

impl ChatUI {
    /// Create a new chat UI
    pub fn new() -> Self {
        Self {
            input_width: 80, // Default width for input box
        }
    }
    
    /// Read input from the user with a nice box around the input field
    pub fn read_input(&self, prefix: &str) -> Result<String> {
        // Print the prefix
        print!("{}: ", prefix.bright_green());
        io::stdout().flush()?;
        
        // Draw the top border of the box
        println!("\n┌{}┐", "─".repeat(self.input_width));
        
        // Print the left border
        print!("│ ");
        io::stdout().flush()?;
        
        // Read the input
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        // Trim the newline
        let input = input.trim_end().to_string();
        
        // Draw the bottom border of the box
        println!("└{}┘", "─".repeat(self.input_width));
        
        Ok(input)
    }
    
    /// Display a message from the assistant
    pub fn display_message(&self, sender: &str, message: &str) {
        println!("{}: {}", sender.bright_blue(), message);
    }
    
    /// Display a tool execution result
    pub fn display_tool_result(&self, tool_name: &str, result: &str) {
        println!("\n{} {}", "Tool Execution:".yellow(), tool_name.bright_yellow());
        println!("{}", "─".repeat(self.input_width));
        println!("{}", result);
        println!("{}\n", "─".repeat(self.input_width));
    }
}