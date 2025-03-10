
/// Count the number of tokens in a string
/// This is a very simplistic implementation - in a real app,
/// you'd use a proper tokenizer library for your model type.
pub fn count_tokens(text: &str) -> usize {
    // Simple whitespace-based tokenization for demonstration
    // In a real implementation, you'd use a tiktoken or similar library
    
    // Avoid counting sequential whitespace as multiple tokens
    let mut prev_was_space = true;
    let mut count = 0;
    
    for c in text.chars() {
        if c.is_whitespace() {
            if !prev_was_space {
                count += 1;
                prev_was_space = true;
            }
        } else {
            if prev_was_space {
                prev_was_space = false;
            }
        }
    }
    
    // Add one more token if the text doesn't end with whitespace
    if !prev_was_space {
        count += 1;
    }
    
    // This is a very approximate count - would need model-specific tokenization
    count
}

/// Count tokens with fallback for empty strings
pub fn count_tokens_with_fallback(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        count_tokens(text)
    }
}