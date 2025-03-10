# CodeMonkeys-rs

A Rust implementation of the file identification and ranking modules from the [CodeMonkeys](https://github.com/princeton-nlp/SWE-bench) project.

## Overview

CodeMonkeys-rs analyzes a codebase to identify and rank files that are relevant to a given problem statement. It operates in two stages:

1. **Relevance Assessment**: Evaluates each file in the codebase to determine if it's relevant to the problem.
2. **File Ranking**: Ranks the relevant files to prioritize which files are most likely to need editing.

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

### Command-line Arguments

- `-c, --config-path`: Path to the configuration file (default: `config.json`)
- `-b, --codebase-path`: Path to the codebase to analyze (overrides config)
- `-p, --problem-id`: Custom problem ID for trajectory storage (overrides config)
- `-s, --problem-statement`: Custom problem statement (overrides config)

## Results

Results are stored in the trajectory store directory specified in the config:

- Relevance decisions: `$TRAJECTORY_STORE_DIR/$PROBLEM_ID/relevance_decisions.json`
- File rankings: `$TRAJECTORY_STORE_DIR/$PROBLEM_ID/ranking.json`

## License

This project is licensed under the same license as the original CodeMonkeys project.