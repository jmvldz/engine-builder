use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a collection of reasoning information for each pipeline stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewData {
    /// Problem ID this overview relates to
    pub problem_id: String,

    /// Problem statement
    pub problem_statement: String,

    /// Reasoning for file selection stage
    pub file_selection_reasoning: Option<String>,

    /// Reasoning for relevance stage with file paths as keys
    pub relevance_reasoning: HashMap<String, String>,

    /// Reasoning for file ranking
    pub ranking_reasoning: Option<String>,

    /// Reasoning for setup script generation
    pub setup_script_reasoning: Option<String>,

    /// Reasoning for lint script generation
    pub lint_script_reasoning: Option<String>,

    /// Reasoning for test script generation
    pub test_script_reasoning: Option<String>,

    /// Reasoning for single test script generation
    pub single_test_script_reasoning: Option<String>,

    /// Reasoning for Dockerfile generation
    pub dockerfile_reasoning: Option<String>,

    /// Reasoning for any Dockerfile error fixes
    pub dockerfile_error_reasoning: HashMap<String, String>,
    
    /// Reasoning for any test script error fixes
    pub test_script_error_reasoning: HashMap<String, String>,

    /// Metadata about the generation process
    pub metadata: HashMap<String, String>,
}

impl OverviewData {
    /// Create a new empty OverviewData structure
    pub fn new(problem_id: &str, problem_statement: &str) -> Self {
        let mut metadata = HashMap::new();
        metadata.insert("created_at".to_string(), chrono::Utc::now().to_rfc3339());

        Self {
            problem_id: problem_id.to_string(),
            problem_statement: problem_statement.to_string(),
            file_selection_reasoning: None,
            relevance_reasoning: HashMap::new(),
            ranking_reasoning: None,
            setup_script_reasoning: None,
            lint_script_reasoning: None,
            test_script_reasoning: None,
            single_test_script_reasoning: None,
            dockerfile_reasoning: None,
            dockerfile_error_reasoning: HashMap::new(),
            test_script_error_reasoning: HashMap::new(),
            metadata,
        }
    }

    /// Generate a markdown overview document with full detail
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# Project Overview for {}\n\n", self.problem_id));
        md.push_str("## Problem Statement\n\n");
        md.push_str(&format!("{}\n\n", self.problem_statement));

        // Add file selection reasoning if available
        if let Some(reasoning) = &self.file_selection_reasoning {
            md.push_str("## File Selection Strategy\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        // Add relevance reasoning if available
        if !self.relevance_reasoning.is_empty() {
            md.push_str("## File Relevance Analysis\n\n");
            for (file, reasoning) in &self.relevance_reasoning {
                md.push_str(&format!("### {}\n\n", file));
                md.push_str(&format!("{}\n\n", reasoning));
            }
        }

        // Add ranking reasoning if available
        if let Some(reasoning) = &self.ranking_reasoning {
            md.push_str("## File Ranking Strategy\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        // Add script generation reasoning if available
        md.push_str("## Scripts Generation\n\n");

        if let Some(reasoning) = &self.setup_script_reasoning {
            md.push_str("### Setup Script\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        if let Some(reasoning) = &self.lint_script_reasoning {
            md.push_str("### Lint Script\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        if let Some(reasoning) = &self.test_script_reasoning {
            md.push_str("### Test Script\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        if let Some(reasoning) = &self.single_test_script_reasoning {
            md.push_str("### Single Test Script\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        // Add Dockerfile reasoning if available
        if let Some(reasoning) = &self.dockerfile_reasoning {
            md.push_str("## Dockerfile Generation\n\n");
            md.push_str(&format!("{}\n\n", reasoning));
        }

        // Add Dockerfile error reasoning if available
        if !self.dockerfile_error_reasoning.is_empty() {
            md.push_str("## Dockerfile Error Fixes\n\n");

            // Create a sorted vector of attempts to ensure they are in order
            let mut attempts: Vec<(&String, &String)> =
                self.dockerfile_error_reasoning.iter().collect();
            attempts.sort_by(|a, b| {
                a.0.parse::<usize>()
                    .unwrap_or(0)
                    .cmp(&b.0.parse::<usize>().unwrap_or(0))
            });

            for (attempt, reasoning) in attempts {
                md.push_str(&format!("### Attempt {}\n\n", attempt));
                md.push_str(&format!("{}\n\n", reasoning));
            }
        }
        
        // Add Test Script error reasoning if available
        if !self.test_script_error_reasoning.is_empty() {
            md.push_str("## Test Script Error Fixes\n\n");

            // Create a sorted vector of attempts to ensure they are in order
            let mut attempts: Vec<(&String, &String)> =
                self.test_script_error_reasoning.iter().collect();
            attempts.sort_by(|a, b| {
                a.0.parse::<usize>()
                    .unwrap_or(0)
                    .cmp(&b.0.parse::<usize>().unwrap_or(0))
            });

            for (attempt, reasoning) in attempts {
                md.push_str(&format!("### Attempt {}\n\n", attempt));
                md.push_str(&format!("{}\n\n", reasoning));
            }
        }

        md
    }

    /// Generate a summarized markdown overview document using an LLM
    pub async fn to_summarized_markdown(
        &self,
        config: &crate::config::Config,
    ) -> anyhow::Result<String> {
        use crate::llm::client::create_client;
        use anyhow::Context;

        // Create LLM config for summary generation
        let llm_config = config.to_llm_config(&None);

        // Create LLM client
        let client = create_client(&llm_config)
            .await
            .context("Failed to create LLM client for overview summarization")?;

        // Generate the detailed version first
        let detailed_md = self.to_markdown();

        // Create a prompt for the LLM to summarize the detailed overview
        let summary_prompt = format!(
            "You are an expert software engineer tasked with summarizing detailed reasoning about a project build process. \
            I'll provide you with the full reasoning for each step, and I need you to create a concise but \
            comprehensive summary of the build process.

            Here's the full detailed reasoning document:
            
            ```markdown
            {}
            ```
            
            Please create a summarized version that:
            1. Keeps the same section structure
            2. Significantly condenses each section to focus only on key points and decisions
            3. Explains WHY choices were made rather than HOW they were implemented
            4. Highlights important tradeoffs and considerations
            5. Is approximately 1/4 the length of the original document
            
            Format your response as a complete Markdown document.",
            detailed_md
        );

        // Send the request to the LLM
        let llm_response = client
            .completion(
                &summary_prompt,
                4096,
                0.2, // Use a moderate temperature for summarization
            )
            .await
            .context("Failed to get overview summary from LLM")?;

        Ok(llm_response.content)
    }
}
