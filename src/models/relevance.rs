use serde::{Deserialize, Serialize};

/// The status of a relevance decision
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelevanceStatus {
    /// The file is relevant to the problem
    Relevant,
    
    /// The file is not relevant to the problem
    NotRelevant,
    
    /// There was an error parsing the LLM response
    ParseError,
}

/// The decision about whether a file is relevant to a problem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevanceDecision {
    /// The full message from the LLM
    pub message: String,
    
    /// The status of the decision
    pub status: RelevanceStatus,
    
    /// A summary of why the file is relevant (only if status is Relevant)
    pub summary: Option<String>,
}

impl RelevanceDecision {
    /// Create a new relevance decision for a relevant file
    pub fn relevant(message: String, summary: String) -> Self {
        Self {
            message,
            status: RelevanceStatus::Relevant,
            summary: Some(summary),
        }
    }
    
    /// Create a new relevance decision for an irrelevant file
    pub fn not_relevant(message: String) -> Self {
        Self {
            message,
            status: RelevanceStatus::NotRelevant,
            summary: None,
        }
    }
    
    /// Create a new relevance decision for a parsing error
    pub fn parse_error(message: String) -> Self {
        Self {
            message,
            status: RelevanceStatus::ParseError,
            summary: None,
        }
    }
    
    /// Check if the file is relevant
    pub fn is_relevant(&self) -> bool {
        self.status == RelevanceStatus::Relevant
    }
}