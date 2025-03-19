use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::Config;
use crate::models::problem::SWEBenchProblem;
use crate::stages;

/// Structure to represent a tool that can be called by the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: HashMap<String, ToolParameter>,
    pub required_parameters: Vec<String>,
}

/// Structure to represent a parameter for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    pub name: String,
    pub description: String,
    pub parameter_type: String,
    pub default: Option<String>,
}

/// Result of a tool execution
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

/// Get a list of all available tools
pub fn get_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "relevance".to_string(),
            description: "Run file relevance assessment".to_string(),
            parameters: HashMap::new(),
            required_parameters: vec![],
        },
        Tool {
            name: "ranking".to_string(),
            description: "Run file ranking".to_string(),
            parameters: HashMap::new(),
            required_parameters: vec![],
        },
        Tool {
            name: "pipeline".to_string(),
            description: "Run full pipeline (relevance and dockerfile generation)".to_string(),
            parameters: HashMap::new(),
            required_parameters: vec![],
        },
        Tool {
            name: "file_selection".to_string(),
            description: "Run only the file selection step".to_string(),
            parameters: HashMap::new(),
            required_parameters: vec![],
        },
        Tool {
            name: "dockerfile".to_string(),
            description: "Generate a test-focused Dockerfile for running tests based on relevant files".to_string(),
            parameters: HashMap::new(),
            required_parameters: vec![],
        },
        Tool {
            name: "build_image".to_string(),
            description: "Build a Docker image from the generated Dockerfile".to_string(),
            parameters: {
                let mut params = HashMap::new();
                params.insert(
                    "tag".to_string(),
                    ToolParameter {
                        name: "tag".to_string(),
                        description: "Tag name for the Docker image".to_string(),
                        parameter_type: "string".to_string(),
                        default: Some("engine-builder-test".to_string()),
                    },
                );
                params
            },
            required_parameters: vec![],
        },
        Tool {
            name: "generate_scripts".to_string(),
            description: "Generate lint and test scripts based on relevant files".to_string(),
            parameters: HashMap::new(),
            required_parameters: vec![],
        },
        Tool {
            name: "run_lint".to_string(),
            description: "Run lint script in a Docker container".to_string(),
            parameters: {
                let mut params = HashMap::new();
                params.insert(
                    "tag".to_string(),
                    ToolParameter {
                        name: "tag".to_string(),
                        description: "Tag name for the Docker image".to_string(),
                        parameter_type: "string".to_string(),
                        default: Some("engine-builder-test".to_string()),
                    },
                );
                params
            },
            required_parameters: vec![],
        },
        Tool {
            name: "run_test".to_string(),
            description: "Run test script in a Docker container".to_string(),
            parameters: {
                let mut params = HashMap::new();
                params.insert(
                    "tag".to_string(),
                    ToolParameter {
                        name: "tag".to_string(),
                        description: "Tag name for the Docker image".to_string(),
                        parameter_type: "string".to_string(),
                        default: Some("engine-builder-test".to_string()),
                    },
                );
                params
            },
            required_parameters: vec![],
        },
        Tool {
            name: "run_all".to_string(),
            description: "Run both lint and test scripts in Docker containers".to_string(),
            parameters: {
                let mut params = HashMap::new();
                params.insert(
                    "tag".to_string(),
                    ToolParameter {
                        name: "tag".to_string(),
                        description: "Tag name for the Docker image".to_string(),
                        parameter_type: "string".to_string(),
                        default: Some("engine-builder-test".to_string()),
                    },
                );
                params.insert(
                    "parallel".to_string(),
                    ToolParameter {
                        name: "parallel".to_string(),
                        description: "Run in parallel mode (both containers at once)".to_string(),
                        parameter_type: "boolean".to_string(),
                        default: Some("false".to_string()),
                    },
                );
                params
            },
            required_parameters: vec![],
        },
    ]
}

/// Parse tool call from the LLM response
pub fn parse_tool_call(response: &str) -> Option<(String, HashMap<String, String>)> {
    // This is a simple implementation. It assumes the model will wrap the tool call in markers
    if let Some(start) = response.find("TOOL:") {
        if let Some(end) = response[start..].find("\n") {
            let tool_call = &response[start + 5..start + end].trim();
            
            // Parse tool name and parameters
            if let Some(open_paren) = tool_call.find('(') {
                if let Some(close_paren) = tool_call.find(')') {
                    let tool_name = tool_call[..open_paren].trim().to_string();
                    let params_str = &tool_call[open_paren + 1..close_paren];
                    
                    // Parse parameters
                    let mut params = HashMap::new();
                    for param in params_str.split(',') {
                        if let Some(eq) = param.find('=') {
                            let key = param[..eq].trim().to_string();
                            let value = param[eq + 1..].trim().to_string();
                            
                            // Remove quotes if present
                            let value = if value.starts_with('"') && value.ends_with('"') {
                                value[1..value.len() - 1].to_string()
                            } else {
                                value
                            };
                            
                            params.insert(key, value);
                        }
                    }
                    
                    return Some((tool_name, params));
                }
            }
        }
    }
    
    None
}

/// Execute a tool based on its name and parameters
pub async fn execute_tool(
    tool_name: &str,
    params: &HashMap<String, String>,
    config: &Config,
    problem: &SWEBenchProblem,
) -> Result<ToolResult> {
    match tool_name {
        "relevance" => {
            let result = stages::relevance::process_codebase(
                config.relevance.clone(),
                &config.codebase,
                problem.clone(),
            )
            .await;
            
            match result {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: "Successfully ran relevance assessment".to_string(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to run relevance assessment: {}", e),
                }),
            }
        }
        "ranking" => {
            let result = stages::ranking::process_rankings(
                config.ranking.clone(),
                problem.clone(),
            )
            .await;
            
            match result {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: "Successfully ran file ranking".to_string(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to run file ranking: {}", e),
                }),
            }
        }
        "pipeline" => {
            // Run file selection
            let result1 = stages::file_selection::process_file_selection(
                config.relevance.clone(),
                &config.codebase,
                problem.clone(),
            )
            .await;
            
            if let Err(e) = result1 {
                return Ok(ToolResult {
                    success: false,
                    output: format!("Failed during file selection: {}", e),
                });
            }
            
            // Process relevance
            let result2 = stages::relevance::process_codebase(
                config.relevance.clone(),
                &config.codebase,
                problem.clone(),
            )
            .await;
            
            if let Err(e) = result2 {
                return Ok(ToolResult {
                    success: false,
                    output: format!("Failed during relevance assessment: {}", e),
                });
            }
            
            // Run ranking
            let result3 = stages::ranking::process_rankings(
                config.ranking.clone(),
                problem.clone(),
            )
            .await;
            
            if let Err(e) = result3 {
                return Ok(ToolResult {
                    success: false,
                    output: format!("Failed during file ranking: {}", e),
                });
            }
            
            // Generate scripts
            let result4 = stages::scripts::generate_scripts_from_ranking(
                config.ranking.clone(),
                config.scripts.clone(),
                problem.clone(),
            )
            .await;
            
            if let Err(e) = result4 {
                return Ok(ToolResult {
                    success: false,
                    output: format!("Failed during script generation: {}", e),
                });
            }
            
            // Generate Dockerfile
            let result5 = stages::dockerfile::generate_dockerfile(
                config.dockerfile.clone(),
                problem.clone(),
            )
            .await;
            
            match result5 {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: "Successfully ran the full pipeline".to_string(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed during Dockerfile generation: {}", e),
                }),
            }
        }
        "file_selection" => {
            let result = stages::file_selection::process_file_selection(
                config.relevance.clone(),
                &config.codebase,
                problem.clone(),
            )
            .await;
            
            match result {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: "Successfully ran file selection".to_string(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to run file selection: {}", e),
                }),
            }
        }
        "dockerfile" => {
            let result = stages::dockerfile::generate_dockerfile(
                config.dockerfile.clone(),
                problem.clone(),
            )
            .await;
            
            match result {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: "Successfully generated Dockerfile".to_string(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to generate Dockerfile: {}", e),
                }),
            }
        }
        "build_image" => {
            let tag = params
                .get("tag")
                .map(|s| s.as_str())
                .unwrap_or("engine-builder-test");
                
            let result = stages::dockerfile::build_docker_image(
                &config.ranking,
                problem,
                tag,
                config.dockerfile.max_retries,
            )
            .await;
            
            match result {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: format!("Successfully built Docker image with tag: {}", tag),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to build Docker image: {}", e),
                }),
            }
        }
        "generate_scripts" => {
            let result = stages::scripts::generate_scripts_from_ranking(
                config.ranking.clone(),
                config.scripts.clone(),
                problem.clone(),
            )
            .await;
            
            match result {
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: "Successfully generated lint and test scripts".to_string(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to generate scripts: {}", e),
                }),
            }
        }
        "run_lint" => {
            let tag = params
                .get("tag")
                .map(|s| s.as_str())
                .unwrap_or("engine-builder-test");
                
            let result = stages::container::run_lint_container(
                problem,
                tag,
                &config.container,
            )
            .await;
            
            match result {
                Ok(container_result) => {
                    let status = if container_result.success {
                        "SUCCESS"
                    } else {
                        "FAILED"
                    };
                    
                    Ok(ToolResult {
                        success: container_result.success,
                        output: format!(
                            "Lint container completed with status: {} (exit code: {})",
                            status,
                            container_result.exit_code
                        ),
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to run lint container: {}", e),
                }),
            }
        }
        "run_test" => {
            let tag = params
                .get("tag")
                .map(|s| s.as_str())
                .unwrap_or("engine-builder-test");
                
            let result = stages::container::run_test_container(
                problem,
                tag,
                &config.container,
            )
            .await;
            
            match result {
                Ok(container_result) => {
                    let status = if container_result.success {
                        "SUCCESS"
                    } else {
                        "FAILED"
                    };
                    
                    Ok(ToolResult {
                        success: container_result.success,
                        output: format!(
                            "Test container completed with status: {} (exit code: {})",
                            status,
                            container_result.exit_code
                        ),
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to run test container: {}", e),
                }),
            }
        }
        "run_all" => {
            let tag = params
                .get("tag")
                .map(|s| s.as_str())
                .unwrap_or("engine-builder-test");
                
            let parallel = params
                .get("parallel")
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(false);
                
            // Clone container config and override parallel flag if specified
            let mut container_config = config.container.clone();
            if parallel {
                container_config.parallel = true;
            }
            
            let result = stages::container::run_containers(
                problem,
                tag,
                &container_config,
            )
            .await;
            
            match result {
                Ok((lint_result, test_result)) => {
                    let lint_status = if lint_result.success {
                        "SUCCESS"
                    } else {
                        "FAILED"
                    };
                    
                    let test_status = if test_result.success {
                        "SUCCESS"
                    } else {
                        "FAILED"
                    };
                    
                    Ok(ToolResult {
                        success: lint_result.success && test_result.success,
                        output: format!(
                            "Container execution summary:\nLint container: {} (exit code: {})\nTest container: {} (exit code: {})",
                            lint_status,
                            lint_result.exit_code,
                            test_status,
                            test_result.exit_code
                        ),
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: format!("Failed to run containers: {}", e),
                }),
            }
        }
        _ => Ok(ToolResult {
            success: false,
            output: format!("Unknown tool: {}", tool_name),
        }),
    }
}