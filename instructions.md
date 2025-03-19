# Instructions: Engine Builder Terminal UI

## Overview
This document provides instructions for implementing a **Terminal User Interface (TUI)** for the Engine Builder project using **Rust**, **Ratatui**, and **Crossterm**. The provided code creates a structured terminal UI that mimics a chat-style command interface, featuring:

- A **static header** displaying the welcome message and current working directory.
- A **scrollable terminal-style output** that retains previous commands and responses.
- A **fixed input box** at the bottom that allows the user to type commands.
- A simple **event-driven architecture** that handles input, updates the UI, and responds to commands.

This UI serves as the foundation for an interactive engine-building assistant where users can execute commands and receive responses in a structured format.

---

## Code Implementation
Below is the complete Rust code to implement the terminal UI:

```rust
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
    widgets::{Block, Borders, Paragraph, List, ListItem},
    layout::{Constraint, Direction, Layout},
    style::{Style, Color},
    text::{Span, Spans},
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute, terminal,
};
use std::io::{self, stdout};
use std::collections::VecDeque;

struct AppState {
    input: String,          // Holds current input text
    output: VecDeque<String>, // Holds terminal-like output history
}

fn main() -> io::Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    
    // App state
    let mut app = AppState {
        input: String::new(),
        output: VecDeque::new(),
    };

    // Static header text
    let header_text = vec![
        "╭────────────────────────────────────────────╮",
        "│ ✻ Welcome to Engine Builder!              │",
        "│                                            │",
        "│   /help for help                           │",
        "│                                            │",
        "│   cwd: /Users/josh/Code/engine-builder     │",
        "╰────────────────────────────────────────────╯",
    ];

    loop {
        // Draw UI
        terminal.draw(|frame| {
            let size = frame.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_text.len() as u16), // Static header
                    Constraint::Min(1),  // Output area (fills up available space)
                    Constraint::Length(3), // Fixed input box height
                ])
                .split(size);

            // Render header
            let header_widget = Paragraph::new(header_text.join("\n"))
                .style(Style::default().fg(Color::Cyan));
            frame.render_widget(header_widget, chunks[0]);

            // Render output (as a scrollable list)
            let output_items: Vec<ListItem> = app.output.iter()
                .map(|line| ListItem::new(Spans::from(Span::raw(line.clone()))))
                .collect();
            let output_widget = List::new(output_items).block(Block::default().borders(Borders::ALL));
            frame.render_widget(output_widget, chunks[1]);

            // Render input box
            let input_widget = Paragraph::new(format!("> {}", app.input))
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(input_widget, chunks[2]);
        })?;

        // Handle input events
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Enter => {
                        if !app.input.is_empty() {
                            // Move input to output history
                            app.output.push_back(format!("> {}", app.input));
                            // Simulate response output
                            app.output.push_back("⏺ Hello! How can I help you with the engine-builder project today?".to_string());
                            app.input.clear(); // Clear input box
                        }
                    }
                    KeyCode::Backspace => { app.input.pop(); }
                    KeyCode::Char(c) => { app.input.push(c); }
                    KeyCode::Esc => break, // Exit on ESC
                    _ => {}
                }
            }
        }
    }

    // Cleanup
    terminal::disable_raw_mode()?;
    execute!(terminal::LeaveAlternateScreen)?;
    Ok(())
}
```

---

## How It Works
1. **Static Header**
   - Displays "Welcome to Engine Builder!" and the current working directory (hardcoded, but can be dynamically retrieved).
   - Uses a **Ratatui Paragraph widget** to render the header with a cyan-colored style.

2. **Scrollable Output (Like a Terminal)**
   - Stores **past inputs and responses** in `output: VecDeque<String>`.
   - Uses a **Ratatui List widget** to render terminal-style scrolling output.
   - When the user **presses Enter**, the input is moved to the output area.
   - Simulated response output is appended for demonstration purposes.

3. **Fixed Input Box**
   - Styled using **Ratatui's Paragraph widget**.
   - Always remains at the **bottom of the screen**.
   - Displays `> user_input` to indicate the active prompt.
   - Clears the input after submission while keeping the box in place.

4. **Event-Driven Input Handling**
   - Listens for user key presses (Crossterm `event::poll`).
   - Supports typing, backspace, and **Enter for submission**.
   - **ESC key exits** the program cleanly.

---

## Next Steps
### ✅ Dynamically Fetch the Current Directory
Replace the hardcoded directory in the header with:
```rust
let cwd = std::env::current_dir().unwrap().display().to_string();
```

### ✅ Execute Real Commands Instead of Simulated Responses
Modify the `Enter` key event to **execute system commands** and capture output.

### ✅ Implement Scrollback for Large Output History
Use keyboard input (e.g., Arrow keys) to scroll through the `output` list.

---

## Summary
This code provides a **fully functional terminal UI** that simulates a command-based chat or CLI tool. It ensures:
- **A structured layout with a clean input box**.
- **Terminal-style output history**.
- **Smooth user interaction with key event handling**.

This serves as a strong foundation for building a **more advanced engine-building assistant** with real command execution, autocompletion, and enhanced interactivity.

**Ready for implementation! 🚀**


