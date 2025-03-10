# Building and Running CodeMonkeys-rs

Here are step-by-step instructions for building, configuring, and running the CodeMonkeys-rs code:

## 1. Prerequisites

Make sure you have Rust and Cargo installed. If not, install them using [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 2. Building the Project

```bash
# Navigate to the project directory
cd codemonkeys-rs

# Build the project
cargo build --release
```

## 3. Configuration

Create and configure your `config.json` file:

```bash
# Copy the example configuration file
cp config.json.example config.json

# Edit the configuration file with your favorite editor
vim config.json  # or nano, code, etc.
```

### Key Configuration Items:

1. **API Keys**:
   - Set `relevance.llm.api_key` to your OpenAI API key
   - Set `ranking.llm.api_key` to your Anthropic API key

2. **Codebase Path**:
   - Set `codebase.path` to the absolute path of the codebase you want to analyze
   - Example: `"/Users/username/projects/my-project"`

3. **Problem Statement**:
   - Set `codebase.problem_statement` to describe what you're looking for
   - Example: `"Find files related to the user authentication system"`

4. **File Extensions**:
   - Set `codebase.include_extensions` to specify which file types to analyze
   - Example: `["js", "ts", "jsx", "tsx"]` for a JavaScript/TypeScript project

5. **Excluded Directories**:
   - Set `codebase.exclude_dirs` to skip directories like tests, docs, etc.
   - Example: `["node_modules", "dist", "build", "tests"]` 

## 4. Running the Tool

### Run the Full Pipeline

```bash
# Using the config.json in the current directory
cargo run --release -- pipeline

# Using a specific config file
cargo run --release -- -c /path/to/config.json pipeline
```

### Run Individual Stages

To run only the relevance assessment:
```bash
cargo run --release -- relevance
```

To run only the ranking (after running relevance):
```bash
cargo run --release -- ranking
```

### Command-line Overrides

You can override config settings via command line:

```bash
# Override the codebase path
cargo run --release -- -b /path/to/codebase pipeline

# Override the problem statement
cargo run --release -- -s "Fix the login page redirect issue" pipeline

# Override the problem ID (for trajectory storage)
cargo run --release -- -p "login-redirect-bug" pipeline
```

## 5. Viewing Results

Results are stored in the trajectory directory specified in your config (default: `./data/trajectories`):

```bash
# Create the trajectory directory if it doesn't exist
mkdir -p ./data/trajectories

# After running, view the results
ls -la ./data/trajectories/<problem_id>/

# View relevance decisions
cat ./data/trajectories/<problem_id>/relevance_decisions.json | jq

# View the final ranking
cat ./data/trajectories/<problem_id>/ranking.json | jq
```

## 6. Troubleshooting

If you encounter issues:

1. **API Key Errors**: Double-check your API keys are correct in config.json
2. **File Access Errors**: Ensure the codebase path is correct and accessible
3. **Logging Information**: Set the `RUST_LOG` environment variable for more detailed logs:
   ```bash
   RUST_LOG=debug cargo run --release -- pipeline
   ```

## 7. Performance Tips

- Adjust `relevance.max_workers` based on your CPU cores and API rate limits
- For large codebases, be selective with your `include_extensions` to reduce processing time
- Use a specific `problem_statement` to help the LLM focus on relevant files