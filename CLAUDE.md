# Engine Builder - Claude Assistance Guidelines

## Build Commands
- Build: `cargo build --release`
- Run: `cargo run --release -- -c config.json pipeline`
- Test all: `cargo test`
- Test single: `cargo test test_name`
- Test file: `cargo test --test file_name`

## Logging
- Set log level with environment variable: `RUST_LOG=info|debug|trace`
- Example: `RUST_LOG=debug cargo run --release -- -c config.json relevance`

## Code Style Guidelines
- Use 4-space indentation
- Follow standard Rust naming conventions (snake_case for variables/functions, CamelCase for types)
- Group imports by external crates, then internal modules
- Use anyhow::Result for public API error handling and thiserror::Error for internal errors
- Document public functions and structs with doc comments
- Prefer .clone() over .to_string() for String conversions
- Use descriptive variable names and follow builder pattern for complex objects
- Handle errors with context using the anyhow::Context trait
- Implement Default trait for configuration structs
- Use async/await for async operations with tokio runtime