# LangFuse Integration

Engine Builder includes integration with [LangFuse](https://langfuse.com/) for tracing and observability of LLM operations.

## Configuration

LangFuse is configured in the `observability` section of your `config.json` file:

```json
"observability": {
  "langfuse": {
    "enabled": true,
    "host": "https://us.cloud.langfuse.com",
    "project_id": "engines-builder",
    "secret_key": "key_here",
    "public_key": "key_here",
    "trace_id": null  // When null, will use problem_id as the trace_id
  }
}
```

## Trace ID Behavior

The `trace_id` field determines how traces are grouped in LangFuse:

- When `trace_id` is `null` (default): The system will automatically use the `problem_id` from the `codebase` section as the trace ID. This ensures that all operations for the same problem are grouped together in LangFuse, even across multiple runs.

- When `trace_id` is explicitly set: The system will use the provided value as the trace ID.

This behavior allows you to have consistent tracing across multiple commands that use the same configuration file, making it easier to analyze the full lifecycle of a problem solution.

## Environment Variables

You can also configure LangFuse using environment variables:

- `LANGFUSE_SECRET_KEY`: Your LangFuse secret key
- `LANGFUSE_PUBLIC_KEY`: Your LangFuse public key
- `LANGFUSE_HOST`: The LangFuse API host (defaults to "https://us.cloud.langfuse.com")
- `LANGFUSE_PROJECT_ID`: Your LangFuse project ID (defaults to "engines-builder")

Environment variables take precedence over config file settings.

## Disabling LangFuse

To disable LangFuse tracing, set `enabled` to `false` in your config file:

```json
"langfuse": {
  "enabled": false,
  ...
}
```