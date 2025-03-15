# engine-builder

A Rust implementation of the file identification and ranking modules from the [SWE-bench](https://github.com/princeton-nlp/SWE-bench) project.

## Overview

engine-builder analyzes a codebase to identify and rank files that are relevant to a given problem statement. It operates in three stages:

1. **Relevance Assessment**: Evaluates each file in the codebase to determine if it's relevant to the problem.
2. **File Ranking**: Ranks the relevant files to prioritize which files are most likely to need editing.
3. **Script Generation**: Creates lint and test scripts customized for the problem.
4. **Dockerfile Generation**: Creates a Dockerfile based on the ranked files to containerize the application.
5. **Container Execution**: Runs the lint and test scripts in Docker containers, with optional parallel execution.

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/engine-builder.git
cd engine-builder

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

**Important**: These stages must be run in sequence. The pipeline command automatically handles this sequence for you.

1. First, run file selection:
```bash
cargo run --release -- -c path/to/config.json file-selection
```

2. Next, run relevance assessment:
```bash
cargo run --release -- -c path/to/config.json relevance
```

3. Then run ranking:
```bash
cargo run --release -- -c path/to/config.json ranking
```

4. After ranking, you can generate scripts:
```bash
cargo run --release -- -c path/to/config.json generate-scripts
```

5. And generate a Dockerfile:
```bash
cargo run --release -- -c path/to/config.json dockerfile
```

For building a Docker image:
```bash
cargo run --release -- -c path/to/config.json build-image --tag my-custom-tag
```

For running lint and test scripts in containers:
```bash
# Run just the lint script
cargo run --release -- -c path/to/config.json run-lint --tag my-custom-tag

# Run just the test script
cargo run --release -- -c path/to/config.json run-test --tag my-custom-tag

# Run both lint and test scripts sequentially
cargo run --release -- -c path/to/config.json run-all --tag my-custom-tag

# Run both lint and test scripts in parallel
cargo run --release -- -c path/to/config.json run-all --tag my-custom-tag --parallel
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

This project is licensed under the same license as the original SWE-bench project.