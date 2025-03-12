# CodeMonkeys-rs

A Rust implementation of the file identification and ranking modules from the [CodeMonkeys](https://github.com/princeton-nlp/SWE-bench) project.

## Overview

CodeMonkeys-rs analyzes a codebase to identify and rank files that are relevant to a given problem statement. It operates in three stages:

1. **Relevance Assessment**: Evaluates each file in the codebase to determine if it's relevant to the problem.
2. **File Ranking**: Ranks the relevant files to prioritize which files are most likely to need editing.
3. **Dockerfile Generation**: Creates a Dockerfile based on the ranked files to containerize the application.

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/codemonkeys-rs.git
cd codemonkeys-rs

# Build the project
cargo build --release
```

## Configuration

Create a `config.json` file based on the provided `config.json.example`:

```bash
cp config.json.example config.json
```

Edit the configuration file to set your:
- API keys for OpenAI and/or Anthropic
- Path to your codebase
- Custom problem statement
- File extensions to include
- Directories to exclude

## Usage

### Running the Full Pipeline

```bash
cargo run --release -- -c path/to/config.json pipeline
```

### Running Individual Stages

For relevance assessment only:
```bash
cargo run --release -- -c path/to/config.json relevance
```

For ranking only (requires relevance to have been run first):
```bash
cargo run --release -- -c path/to/config.json ranking
```

For file selection only (subset of relevance that only selects files to process):
```bash
cargo run --release -- -c path/to/config.json file-selection
```

For Dockerfile generation (requires ranking to have been run first):
```bash
cargo run --release -- -c path/to/config.json dockerfile
```

### Command-line Arguments

- `-c, --config-path`: Path to the configuration file (default: `config.json`)
- `-b, --codebase-path`: Path to the codebase to analyze (overrides config)
- `-p, --problem-id`: Custom problem ID for trajectory storage (overrides config)
- `-s, --problem-statement`: Custom problem statement (overrides config)

### Logging

The application uses the Rust `log` crate with an `env_logger` backend. You can control the log level by setting the `RUST_LOG` environment variable:

```bash
# Display only error and warning messages
RUST_LOG=warn cargo run --release -- -c config.json pipeline

# Display info, warning, and error messages (default if RUST_LOG is not set)
RUST_LOG=info cargo run --release -- -c config.json pipeline

# Display detailed debug logs, including file tree traversal
RUST_LOG=debug cargo run --release -- -c config.json pipeline

# Display all log messages, including trace
RUST_LOG=trace cargo run --release -- -c config.json pipeline
```

File tree traversal logs are shown at the INFO level, including:
- Files and directories being explored
- Files being excluded
- Files being added to the codebase analysis

## Results

Results are stored in the trajectory store directory specified in the config:

- Relevance decisions: `$TRAJECTORY_STORE_DIR/$PROBLEM_ID/relevance_decisions.json`
- File rankings: `$TRAJECTORY_STORE_DIR/$PROBLEM_ID/ranking.json`
- Dockerfiles: `data/dockerfiles/$PROBLEM_ID_Dockerfile` and `./Dockerfile`

## License

This project is licensed under the same license as the original CodeMonkeys project.