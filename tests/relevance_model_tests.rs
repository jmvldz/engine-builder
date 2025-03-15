use engine_builder::models::relevance::{RelevanceDecision, RelevanceStatus};
use serde_json;

#[test]
fn test_relevance_status_serialization() {
    // Test serialization of RelevanceStatus
    let relevant = RelevanceStatus::Relevant;
    let serialized = serde_json::to_string(&relevant).unwrap();
    assert_eq!(serialized, "\"Relevant\"");
    
    let not_relevant = RelevanceStatus::NotRelevant;
    let serialized = serde_json::to_string(&not_relevant).unwrap();
    assert_eq!(serialized, "\"NotRelevant\"");
    
    let parse_error = RelevanceStatus::ParseError;
    let serialized = serde_json::to_string(&parse_error).unwrap();
    assert_eq!(serialized, "\"ParseError\"");
    
    // Test deserialization
    let deserialized: RelevanceStatus = serde_json::from_str("\"Relevant\"").unwrap();
    assert_eq!(deserialized, RelevanceStatus::Relevant);
    
    let deserialized: RelevanceStatus = serde_json::from_str("\"NotRelevant\"").unwrap();
    assert_eq!(deserialized, RelevanceStatus::NotRelevant);
    
    let deserialized: RelevanceStatus = serde_json::from_str("\"ParseError\"").unwrap();
    assert_eq!(deserialized, RelevanceStatus::ParseError);
}

#[test]
fn test_relevance_decision_relevant() {
    let message = "The file contains core functionality".to_string();
    let summary = "This file defines the main data structures".to_string();
    
    let decision = RelevanceDecision::relevant(message.clone(), summary.clone());
    
    assert_eq!(decision.message, message);
    assert_eq!(decision.status, RelevanceStatus::Relevant);
    assert_eq!(decision.summary, Some(summary.clone()));
    assert!(decision.is_relevant());
    
    // Test serialization
    let serialized = serde_json::to_string(&decision).unwrap();
    let deserialized: RelevanceDecision = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(deserialized.message, message);
    assert_eq!(deserialized.status, RelevanceStatus::Relevant);
    assert_eq!(deserialized.summary, Some(summary));
    assert!(deserialized.is_relevant());
}

#[test]
fn test_relevance_decision_not_relevant() {
    let message = "The file is a test utility and not relevant".to_string();
    
    let decision = RelevanceDecision::not_relevant(message.clone());
    
    assert_eq!(decision.message, message);
    assert_eq!(decision.status, RelevanceStatus::NotRelevant);
    assert_eq!(decision.summary, None);
    assert!(!decision.is_relevant());
    
    // Test serialization
    let serialized = serde_json::to_string(&decision).unwrap();
    let deserialized: RelevanceDecision = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(deserialized.message, message);
    assert_eq!(deserialized.status, RelevanceStatus::NotRelevant);
    assert_eq!(deserialized.summary, None);
    assert!(!deserialized.is_relevant());
}

#[test]
fn test_relevance_decision_parse_error() {
    let message = "Could not parse the LLM response".to_string();
    
    let decision = RelevanceDecision::parse_error(message.clone());
    
    assert_eq!(decision.message, message);
    assert_eq!(decision.status, RelevanceStatus::ParseError);
    assert_eq!(decision.summary, None);
    assert!(!decision.is_relevant());
    
    // Test serialization
    let serialized = serde_json::to_string(&decision).unwrap();
    let deserialized: RelevanceDecision = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(deserialized.message, message);
    assert_eq!(deserialized.status, RelevanceStatus::ParseError);
    assert_eq!(deserialized.summary, None);
    assert!(!deserialized.is_relevant());
}