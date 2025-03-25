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

## Codebase structure
.
├── CLAUDE.md
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── README.md
├── config.example.json
├── config.json
├── data
│   └── trajectories
│       └── flexprice
├── exclusions.json
├── src
│   ├── chat
│   │   ├── mod.rs
│   │   ├── tools.rs
│   │   └── ui.rs
│   ├── config.rs
│   ├── lib.rs
│   ├── llm
│   │   ├── anthropic.rs
│   │   ├── client.rs
│   │   ├── langfuse.rs
│   │   ├── mod.rs
│   │   ├── openai.rs
│   │   └── prompts.rs
│   ├── main.rs
│   ├── models
│   │   ├── dockerfile.rs
│   │   ├── exclusion.rs
│   │   ├── file.rs
│   │   ├── mod.rs
│   │   ├── problem.rs
│   │   ├── ranking.rs
│   │   └── relevance.rs
│   ├── stages
│   │   ├── container.rs
│   │   ├── dockerfile.rs
│   │   ├── file_selection.rs
│   │   ├── mod.rs
│   │   ├── ranking.rs
│   │   ├── relevance.rs
│   │   └── scripts.rs
│   └── utils
│       ├── json_utils.rs
│       ├── mod.rs
│       ├── token_counter.rs
│       └── trajectory_store.rs
└── tests
    ├── config_tests.rs
    ├── dockerfile_tests.rs
    ├── exclusion_file_tests.rs
    ├── exclusion_pattern_tests.rs
    ├── exclusion_unit_tests.rs
    ├── file_exclusion_tests.rs
    ├── git_dir_exclusion_test.rs
    ├── json_utils_tests.rs
    ├── pipeline_e2e_integration_tests.rs
    ├── pipeline_integration_tests.rs
    ├── pipeline_mock_llm_tests.rs
    ├── problem_model_tests.rs
    ├── ranking_model_tests.rs
    ├── relevance_model_tests.rs
    ├── token_counter_tests.rs
    └── trajectory_store_tests.rs

