use h_core::Message;
use serde_json::{Map, Value};

/// Apply Anthropic prompt cache control to a message array.
///
/// Anthropic's cached prefixes allow you to mark parts of the prompt as
/// cacheable, reducing costs for repeated content like system prompts,
/// tool definitions, and conversation history.
///
/// Strategy:
/// - Cache the system prompt (always)
/// - Cache the last 2 user turns (prefix of conversation)
/// - Never cache the final assistant message (it changes each turn)
pub fn apply_cache_control(messages: &mut Vec<Value>) {
    if messages.is_empty() {
        return;
    }

    // Find cache points: system prompt + alternating user/assistant pairs
    // We cache the prefix that's likely to repeat across API calls
    let len = messages.len();

    // Cache the last user message's content (before any tool calls)
    // This is the conversation prefix that will be reused
    let mut cache_point = None;
    for i in (0..len).rev() {
        if let Some(role) = messages[i].get("role").and_then(|v| v.as_str()) {
            if role == "user" {
                cache_point = Some(i);
                break;
            }
        }
    }

    // Also cache the second-to-last user message if there are enough messages
    let mut second_cache_point = None;
    if let Some(first) = cache_point {
        for i in (0..first).rev() {
            if let Some(role) = messages[i].get("role").and_then(|v| v.as_str()) {
                if role == "user" {
                    second_cache_point = Some(i);
                    break;
                }
            }
        }
    }

    // Apply cache_control to the identified messages
    let cache_indices: Vec<usize> = [second_cache_point, cache_point]
        .into_iter()
        .flatten()
        .collect();
    for idx in cache_indices {
        if let Some(content) = messages[idx].get_mut("content") {
            if content.is_string() {
                // Convert plain string to content block with cache control
                let text = content.as_str().unwrap_or("").to_string();
                *content = Value::Array(vec![Value::Object(Map::from_iter([
                    ("type".to_string(), Value::String("text".to_string())),
                    ("text".to_string(), Value::String(text)),
                    (
                        "cache_control".to_string(),
                        Value::Object(Map::from_iter([(
                            "type".to_string(),
                            Value::String("ephemeral".to_string()),
                        )])),
                    ),
                ]))]);
            } else if let Some(arr) = content.as_array_mut() {
                // Add cache_control to the last content block
                if let Some(last) = arr.last_mut() {
                    if let Some(obj) = last.as_object_mut() {
                        obj.insert(
                            "cache_control".to_string(),
                            Value::Object(Map::from_iter([(
                                "type".to_string(),
                                Value::String("ephemeral".to_string()),
                            )])),
                        );
                    }
                }
            }
        }
    }
}

/// Build a cache-aware system prompt block for Anthropic.
pub fn build_cached_system_block(text: &str) -> Value {
    Value::Array(vec![Value::Object(Map::from_iter([
        ("type".to_string(), Value::String("text".to_string())),
        ("text".to_string(), Value::String(text.to_string())),
        (
            "cache_control".to_string(),
            Value::Object(Map::from_iter([(
                "type".to_string(),
                Value::String("ephemeral".to_string()),
            )])),
        ),
    ]))])
}

/// Convert messages to Anthropic format with cache control applied.
pub fn messages_to_anthropic_cached(messages: &[&Message]) -> Vec<Value> {
    let mut result: Vec<Value> = messages
        .iter()
        .map(|m| {
            let mut obj = Map::new();
            obj.insert(
                "role".to_string(),
                Value::String(m.role.to_string()),
            );
            if let Some(ref content) = m.content {
                if let Some(text) = content.as_text() {
                    obj.insert("content".to_string(), Value::String(text.to_string()));
                }
            }
            if let Some(ref tool_calls) = m.tool_calls {
                let text_block = Value::Object(Map::from_iter([
                    ("type".to_string(), Value::String("text".to_string())),
                    (
                        "text".to_string(),
                        Value::String(
                            m.content
                                .as_ref()
                                .and_then(|c| c.as_text())
                                .unwrap_or("")
                                .to_string(),
                        ),
                    ),
                ]));
                let tool_blocks: Vec<Value> = tool_calls.iter().map(|tc| {
                    Value::Object(Map::from_iter([
                        ("type".to_string(), Value::String("tool_use".to_string())),
                        ("id".to_string(), Value::String(tc.id.clone())),
                        ("name".to_string(), Value::String(tc.function.name.clone())),
                        (
                            "input".to_string(),
                            serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(Value::Object(Map::new())),
                        ),
                    ]))
                }).collect();
                let mut blocks = vec![text_block];
                blocks.extend(tool_blocks);
                obj.insert("content".to_string(), Value::Array(blocks));
            }
            if let Some(ref tool_call_id) = m.tool_call_id {
                obj.insert(
                    "tool_result".to_string(),
                    Value::String(tool_call_id.clone()),
                );
            }
            Value::Object(obj)
        })
        .collect();

    apply_cache_control(&mut result);
    result
}

/// Check if a model supports prompt caching.
pub fn supports_caching(model_id: &str) -> bool {
    let m = model_id.to_lowercase();
    m.contains("claude")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_cached_system_block() {
        let block = build_cached_system_block("You are helpful");
        let arr = block.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let obj = arr[0].as_object().unwrap();
        assert_eq!(obj.get("type").unwrap().as_str().unwrap(), "text");
        assert_eq!(obj.get("text").unwrap().as_str().unwrap(), "You are helpful");
        assert!(obj.contains_key("cache_control"));
    }

    #[test]
    fn test_supports_caching() {
        assert!(supports_caching("claude-sonnet-4-6"));
        assert!(supports_caching("claude-opus-4-6"));
        assert!(!supports_caching("gpt-4o"));
        assert!(!supports_caching("gpt-4o-mini"));
    }

    #[test]
    fn test_apply_cache_control_empty() {
        let mut msgs: Vec<Value> = vec![];
        apply_cache_control(&mut msgs);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_apply_cache_control_single_message() {
        let mut msgs = vec![Value::Object(Map::from_iter([
            ("role".to_string(), Value::String("user".to_string())),
            ("content".to_string(), Value::String("Hello".to_string())),
        ]))];
        apply_cache_control(&mut msgs);
        // Content should be converted to array with cache_control
        let content = &msgs[0]["content"];
        assert!(content.is_array());
        assert!(content[0].get("cache_control").is_some());
    }

    #[test]
    fn test_apply_cache_control_multiple_messages() {
        let mut msgs = vec![
            Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                ("content".to_string(), Value::String("First".to_string())),
            ])),
            Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("assistant".to_string())),
                ("content".to_string(), Value::String("Reply".to_string())),
            ])),
            Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("user".to_string())),
                ("content".to_string(), Value::String("Second".to_string())),
            ])),
        ];
        apply_cache_control(&mut msgs);
        // Last user message should have cache_control
        let last_content = &msgs[2]["content"];
        assert!(last_content.is_array());
        assert!(last_content[0].get("cache_control").is_some());
    }

    #[test]
    fn test_messages_to_anthropic_cached() {
        let messages = vec![
            Message::user("Hello"),
            Message::assistant("Hi there"),
        ];
        let refs: Vec<&Message> = messages.iter().collect();
        let result = messages_to_anthropic_cached(&refs);
        assert_eq!(result.len(), 2);
    }
}
