use crate::engine::commands::LlmResponse;

pub fn parse_llm_output(raw_text: &str) -> Result<LlmResponse, String> {
    // 1. Strip Gemma chat template artifacts
    let cleaned = raw_text.replace("<start_of_turn>", "").replace("<end_of_turn>", "");
    
    // 2. Find the first '{' and its matching '}' to extract the first complete JSON object.
    //    (The model sometimes hallucinates extra text/tokens after the first JSON.)
    let start = cleaned.find('{').ok_or("No opening brace found in LLM output")?;
    let rest = &cleaned[start..];
    let mut depth = 0i32;
    let mut end = 0;
    for (i, ch) in rest.char_indices() {
        if ch == '{' { depth += 1; }
        else if ch == '}' { depth -= 1; }
        if depth == 0 {
            end = i + 1; // include the closing brace
            break;
        }
    }
    if depth != 0 {
        return Err("Unmatched braces in LLM output".to_string());
    }
    
    let json_str = &rest[..end];
    
    // 3. Parse into our struct
    serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse LLM JSON: {} | Raw: {}", e, json_str))
}