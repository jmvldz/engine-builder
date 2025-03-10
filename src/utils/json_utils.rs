use anyhow::Result;
use regex::Regex;
use serde_json::Value;

/// Extract the last JSON array or object from a string
pub fn extract_last_json(text: &str) -> Result<Vec<String>> {
    // First, try to find a JSON array inside a code block
    // Updated pattern to be more flexible with quoting and formatting
    let re = Regex::new(r"```(?:json)?\s*(\[[\s\S]*?\])\s*```").unwrap();
    
    let json_str = if let Some(captures) = re.captures(text) {
        // Extract just the JSON part (remove the ```)
        captures.get(1).map(|m| m.as_str()).unwrap_or_default().to_string()
    } else {
        // Try to find a JSON array without code blocks
        // Look for the last square bracket pair that might contain a JSON array
        let re = Regex::new(r"\[([\s\S]*?)\]").unwrap();
        
        if let Some(all_matches) = re.captures_iter(text).last() {
            format!("[{}]", all_matches.get(1).map(|m| m.as_str()).unwrap_or_default())
        } else {
            return Err(anyhow::anyhow!("No JSON array found in text"));
        }
    };
    
    // Try to parse the JSON string
    match serde_json::from_str::<Value>(&json_str) {
        Ok(json_value) => {
            // Extract string values from the array
            if let Value::Array(array) = json_value {
                let string_values = array
                    .into_iter()
                    .filter_map(|val| {
                        if let Value::String(s) = val {
                            Some(s)
                        } else {
                            // Try to convert to string if possible
                            val.as_str().map(|s| s.to_string())
                        }
                    })
                    .collect::<Vec<_>>();
                
                if !string_values.is_empty() {
                    return Ok(string_values);
                }
            }
        },
        Err(_) => {
            // If parsing failed, try a more aggressive approach: look for anything that looks like
            // a list of file paths within quotes in the text
            let path_re = Regex::new(r#"["']([^"']+\.[^"']+)["']"#).unwrap();
            let matches: Vec<String> = path_re.captures_iter(text)
                .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
                .collect();
            
            if !matches.is_empty() {
                return Ok(matches);
            }
        }
    }
    
    // If we couldn't extract a valid JSON array or file paths, return an error
    Err(anyhow::anyhow!("Could not extract a valid JSON array or file paths from text"))
}