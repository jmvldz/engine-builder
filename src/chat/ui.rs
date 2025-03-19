use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{self},
    execute,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap, Clear, List, ListItem},
    layout::{Layout, Constraint, Direction, Rect},
    style::{Style, Color},
    text::Line,
    Terminal,
};
use std::{io, time::Duration, collections::VecDeque};
use tokio::sync::mpsc;

use crate::chat::ChatMessage;

/// App structure to hold UI state
pub struct ChatApp {
    /// Chat message history as formatted strings
    pub output_lines: VecDeque<String>,
    /// Original chat messages
    pub messages: Vec<ChatMessage>,
    /// Input text
    pub input: String,
    /// Cursor position in input
    pub cursor_position: usize,
    /// Command channel sender
    pub tx: mpsc::Sender<String>,
    /// Is app running
    pub running: bool,
    /// Show help
    pub show_help: bool,
    /// Current working directory
    pub cwd: String,
}

impl ChatApp {
    /// Create a new chat app
    pub fn new(tx: mpsc::Sender<String>) -> Self {
        // Get the current working directory
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "/unknown".to_string());
            
        Self {
            output_lines: VecDeque::new(),
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            tx,
            running: true,
            show_help: false,
            cwd,
        }
    }

    /// Handle input events
    pub async fn handle_events(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press {
                    match key.code {
                        // Quit application on Ctrl+C or Ctrl+Q
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.running = false;
                        }
                        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.running = false;
                        }
                        
                        // Show/hide help on F1 or Ctrl+H
                        KeyCode::F(1) => {
                            self.show_help = !self.show_help;
                        }
                        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.show_help = !self.show_help;
                        }
                        
                        // Send message on Enter (if not empty)
                        KeyCode::Enter if key.modifiers.is_empty() => {
                            if !self.input.is_empty() {
                                let input_text = self.input.clone();
                                if input_text.trim().eq_ignore_ascii_case("exit") {
                                    self.running = false;
                                } else if input_text.trim().eq_ignore_ascii_case("/help") {
                                    // Toggle help display
                                    self.show_help = !self.show_help;
                                    // Clear input
                                    self.input.clear();
                                    self.cursor_position = 0;
                                } else {
                                    // Send input to chat handler
                                    if let Err(e) = self.tx.send(input_text.clone()).await {
                                        log::error!("Failed to send user input: {}", e);
                                        // Add error message to local history
                                        self.messages.push(ChatMessage {
                                            role: "system".to_string(),
                                            content: format!("Error: Failed to send message: {}", e),
                                        });
                                    }
                                    
                                    // Add user message to local history
                                    self.messages.push(ChatMessage {
                                        role: "user".to_string(),
                                        content: input_text,
                                    });
                                    
                                    // Clear input
                                    self.input.clear();
                                    self.cursor_position = 0;
                                }
                            }
                        }
                        
                        // Handle cursor movement
                        KeyCode::Left => {
                            self.move_cursor_left();
                        }
                        KeyCode::Right => {
                            self.move_cursor_right();
                        }
                        KeyCode::Home => {
                            self.cursor_position = 0;
                        }
                        KeyCode::End => {
                            self.cursor_position = self.input.len();
                        }
                        
                        // Handle text modification
                        KeyCode::Backspace => {
                            self.delete_char();
                        }
                        KeyCode::Delete => {
                            self.delete_char_forward();
                        }
                        KeyCode::Char(c) => {
                            self.insert_char(c);
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
    
    /// Move cursor left
    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }
    
    /// Move cursor right
    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }
    
    /// Delete character at cursor
    fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.input.remove(self.cursor_position);
        }
    }
    
    /// Delete character after cursor
    fn delete_char_forward(&mut self) {
        if self.cursor_position < self.input.len() {
            self.input.remove(self.cursor_position);
        }
    }
    
    /// Insert character at cursor
    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    /// Render the UI
    pub fn render(&mut self, frame: &mut Frame) {
        // Create layout with header and main content area
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),  // Static header (7 lines)
                Constraint::Min(5),     // Main content area (fills available space)
            ])
            .split(frame.size());
        
        // Draw header
        self.render_header(frame, main_chunks[0]);
        
        // Create layout for output and input within the main content area
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),     // Output area (fills available space)
                Constraint::Length(3),  // Fixed input box height
            ])
            .split(main_chunks[1]);
        
        // Draw chat history
        self.render_messages(frame, content_chunks[0]);
        
        // Draw input area
        self.render_input(frame, content_chunks[1]);
        
        // Draw help popup if requested
        if self.show_help {
            self.render_help(frame);
        }
    }
    
    /// Render static header
    fn render_header(&self, frame: &mut Frame, area: Rect) {
        // Create header text
        let cwd_line = format!("│   cwd: {:<36} │", self.cwd);
        let header_text = vec![
            "╭────────────────────────────────────────────╮",
            "│ ✻ Welcome to Engine Builder!              │",
            "│                                            │",
            "│   /help for help                           │",
            "│                                            │",
            &cwd_line,
            "╰────────────────────────────────────────────╯",
        ];
        
        // Create a paragraph from the header text
        let header_widget = Paragraph::new(header_text.join("\n"))
            .style(Style::default().fg(Color::Cyan));
        
        frame.render_widget(header_widget, area);
    }
    
    /// Render input area
    fn render_input(&mut self, frame: &mut Frame, area: Rect) {
        // Create a block for input with border
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded);
        
        let inner_area = input_block.inner(area);
        frame.render_widget(input_block, area);
        
        // Create input text with cursor
        let input_text = format!("> {}", self.input);
        
        // Calculate visible portion of input
        let scroll_offset = if self.cursor_position + 2 >= inner_area.width as usize {
            self.cursor_position + 2 - inner_area.width as usize + 1
        } else {
            0
        };
        
        let visible_text = if input_text.len() > scroll_offset {
            &input_text[scroll_offset..]
        } else {
            ""
        };
        
        let visible_chars = visible_text.chars().take(inner_area.width as usize).collect::<String>();
        
        // Create text widget
        let input_paragraph = Paragraph::new(visible_chars)
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(input_paragraph, inner_area);
        
        // Draw cursor at current position
        let cursor_x = if self.cursor_position + 2 >= scroll_offset {
            (self.cursor_position + 2 - scroll_offset) as u16
        } else {
            0
        };
        
        frame.set_cursor(
            inner_area.x + cursor_x.min(inner_area.width - 1),
            inner_area.y
        );
    }
    
    /// Render chat message history
    fn render_messages(&self, frame: &mut Frame, area: Rect) {
        // Create a block for output with border
        let output_block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded);
        
        let inner_area = output_block.inner(area);
        frame.render_widget(output_block, area);
        
        // Convert output lines to ListItems with proper text formatting
        let items: Vec<ListItem> = self.output_lines.iter()
            .map(|line| {
                // Create a ListItem with proper text wrapping
                ListItem::new(line.clone())
            })
            .collect();
        
        // Create a list widget for output
        let output_list = List::new(items)
            .style(Style::default());
        
        // Render the list widget
        frame.render_widget(output_list, inner_area);
    }
    
    /// Render help popup
    fn render_help(&self, frame: &mut Frame) {
        let area = centered_rect(60, 60, frame.size());
        
        // Draw a clear background
        frame.render_widget(Clear, area);
        
        // Draw a block around the help text
        let block = Block::default()
            .title("Help")
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .style(Style::default().bg(Color::DarkGray));
        
        let inner_area = block.inner(area);
        frame.render_widget(block, area);
        
        // Create the help text
        let help_text = vec![
            "Engine Builder Chat Interface",
            "",
            "Keyboard Shortcuts:",
            "  Enter      - Send message",
            "  Ctrl+C     - Quit application",
            "  Ctrl+Q     - Quit application",
            "  F1/Ctrl+H  - Toggle help",
            "",
            "Commands:",
            "  help       - Show tool information",
            "  exit       - Quit application",
            "",
            "Tools can be used with: TOOL: tool_name(param=value)",
        ];
        
        let paragraph = Paragraph::new(help_text.join("\n"))
            .style(Style::default().fg(Color::White))
            .block(Block::default())
            .wrap(Wrap { trim: false });
        
        frame.render_widget(paragraph, inner_area);
    }
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Run the chat UI
pub async fn run_chat_ui(
    rx: mpsc::Receiver<ChatMessage>,
    tx: mpsc::Sender<String>,
) -> Result<()> {
    // Set up terminal
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state
    let mut app = ChatApp::new(tx);
    
    // We don't need an internal channel anymore, removed
    
    // Create a channel for collecting messages from background tasks
    let mut user_input_rx = rx;
        
    // Main UI loop
    while app.running {
        // Non-blocking check for new messages
        if let Ok(message) = user_input_rx.try_recv() {
            // Add message to history
            app.messages.push(message.clone());
            
            // Format and add to output lines
            let prefix = match message.role.as_str() {
                "user" => "> ",
                "assistant" => "⏺ ",
                "system" => "! ",
                _ => "? ",
            };
            
            // Add the message with prefix to output lines
            // Format the message content with spaces between words
            let formatted_content = message.content.split_whitespace().collect::<Vec<&str>>().join(" ");
            app.output_lines.push_back(format!("{}{}", prefix, formatted_content));
            
            // Force terminal to redraw
            terminal.autoresize()?;
        }
        
        // Draw UI
        terminal.draw(|f| app.render(f))?;
        
        // Handle user input and events
        app.handle_events().await?;
        
        // Redraw UI after handling events to ensure viewport is updated
        terminal.draw(|f| app.render(f))?;
    }
    
    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(terminal.backend_mut(), terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    
    Ok(())
}
