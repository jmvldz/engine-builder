use engine_builder::utils::token_counter::{count_tokens, count_tokens_with_fallback};

#[test]
fn test_count_tokens_empty_string() {
    let empty_text = "";
    assert_eq!(count_tokens(empty_text), 0);
}

#[test]
fn test_count_tokens_single_token() {
    let text = "hello";
    assert_eq!(count_tokens(text), 1);
}

#[test]
fn test_count_tokens_multiple_tokens() {
    let text = "hello world";
    assert_eq!(count_tokens(text), 2);
}

#[test]
fn test_count_tokens_multiple_spaces() {
    let text = "hello   world"; // Multiple spaces
    assert_eq!(count_tokens(text), 2);
}

#[test]
fn test_count_tokens_newlines() {
    let text = "hello\nworld";
    assert_eq!(count_tokens(text), 2);
}

#[test]
fn test_count_tokens_mixed_whitespace() {
    let text = "hello\n  world\t  test";
    assert_eq!(count_tokens(text), 3);
}

#[test]
fn test_count_tokens_trailing_space() {
    let text = "hello world ";
    assert_eq!(count_tokens(text), 2);
}

#[test]
fn test_count_tokens_leading_space() {
    let text = " hello world";
    assert_eq!(count_tokens(text), 2);
}

#[test]
fn test_count_tokens_with_fallback_empty() {
    let empty_text = "";
    assert_eq!(count_tokens_with_fallback(empty_text), 0);
}

#[test]
fn test_count_tokens_with_fallback_normal() {
    let text = "hello world";
    assert_eq!(count_tokens_with_fallback(text), 2);
}
