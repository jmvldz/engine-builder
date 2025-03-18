use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap, Clear},
    layout::{Layout, Constraint, Direction, Rect},
    style::{Style, Color},
};
use std::{io, time::Duration};
use tokio::sync::mpsc;

use crate::chat::ChatMessage;

/// App structure to hold UI state
pub struct ChatApp {
    /// Chat message history
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
}

impl ChatApp {
    /// Create a new chat app
    pub fn new(tx: mpsc::Sender<String>) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            tx,
            running: true,
            show_help: false,
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
        // Create layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(3),
            ])
            .split(frame.size());
        
        // Draw chat history
        self.render_messages(frame, chunks[0]);
        
        // Draw input area
        self.render_input(frame, chunks[1]);
        
        // Draw help popup if requested
        if self.show_help {
            self.render_help(frame);
        }
    }
    
    /// Render input area
    fn render_input(&mut self, frame: &mut Frame, area: Rect) {
        // Create a block for input
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title("Input");
        
        let inner_area = input_block.inner(area);
        frame.render_widget(input_block, area);
        
        // Create input text with cursor
        let input_text = self.input.clone();
        
        // Calculate visible portion of input
        let scroll_offset = if self.cursor_position >= inner_area.width as usize {
            self.cursor_position - inner_area.width as usize + 1
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
        let input_paragraph = Paragraph::new(visible_chars);
        frame.render_widget(input_paragraph, inner_area);
        
        // Draw cursor at current position
        let cursor_x = if self.cursor_position >= scroll_offset {
            (self.cursor_position - scroll_offset) as u16
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
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title("Chat History");
        
        let inner_area = block.inner(area);
        frame.render_widget(block, area);
        
        // Format all messages as a single text
        let mut formatted_text = String::new();
        
        for message in &self.messages {
            let header = match message.role.as_str() {
                "user" => "\n[You]: ",
                "assistant" => "\n[Assistant]: ",
                "system" => "\n[System]: ",
                _ => "\n[Unknown]: ",
            };
            
            formatted_text.push_str(header);
            formatted_text.push_str(&message.content);
            formatted_text.push_str("\n");
        }
        
        // Create a paragraph from the formatted text
        let paragraph = Paragraph::new(formatted_text)
            .style(Style::default())
            .wrap(Wrap { trim: false })
            .scroll((0, 0));
        
        frame.render_widget(paragraph, inner_area);
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
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
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
            app.messages.push(message);
        }
        
        // Draw UI
        terminal.draw(|f| app.render(f))?;
        
        // Handle user input and events
        app.handle_events().await?;
    }
    
    // Restore terminal
    terminal::disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    
    Ok(())
}