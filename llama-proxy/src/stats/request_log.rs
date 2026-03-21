//! Request logging formatter

/// Format a request log message in compact format
pub fn format_request_log(request_json: &serde_json::Value) -> String {
    let model = request_json.get("model").and_then(|m| m.as_str()).unwrap_or("unknown");

    let msg_count = request_json
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let is_streaming = request_json.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    let tools_count = request_json.get("tools").and_then(|t| t.as_array()).map(|a| a.len());

    let first_user_msg = extract_first_user_message(request_json);

    let mut parts = vec![format!("model={}", model), format!("msgs={}", msg_count)];

    if is_streaming {
        parts.push("stream".to_string());
    }

    if let Some(count) = tools_count {
        if count > 0 {
            parts.push(format!("tools={}", count));
        }
    }

    if let Some(msg) = first_user_msg {
        parts.push(format!("\"{}\"", msg));
    }

    format!("→ {}", parts.join(" "))
}

/// Extract and format the first user message with truncation
fn extract_first_user_message(request_json: &serde_json::Value) -> Option<String> {
    let messages = request_json.get("messages")?.as_array()?;

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "user" {
            continue;
        }

        let content = extract_message_content(msg)?;
        let normalized = normalize_whitespace(&content);
        return Some(truncate_message(&normalized));
    }

    None
}

/// Extract text content from a message (handles string or array content)
fn extract_message_content(msg: &serde_json::Value) -> Option<String> {
    let content = msg.get("content")?;

    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    if let Some(parts) = content.as_array() {
        let mut texts = Vec::new();
        for part in parts {
            if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    texts.push(text.to_string());
                }
            }
        }
        if !texts.is_empty() {
            return Some(texts.join(" "));
        }
    }

    None
}

/// Convert newlines and tabs to single spaces, collapse multiple spaces
fn normalize_whitespace(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Truncate message according to rules:
/// - If <= 100 chars: show all
/// - If > 100 chars: first 25 + " ... " + last 75
fn truncate_message(s: &str) -> String {
    const MAX_TOTAL: usize = 100;
    const PREFIX_LEN: usize = 25;
    const SUFFIX_LEN: usize = 75;
    const ELLIPSIS: &str = " ... ";

    if s.len() <= MAX_TOTAL {
        return s.to_string();
    }

    let prefix = &s[..PREFIX_LEN.min(s.len())];
    let suffix_start = s.len().saturating_sub(SUFFIX_LEN);
    let suffix = &s[suffix_start..];

    format!("{}{}{}", prefix, ELLIPSIS, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_request_log_basic() {
        let req = json!({
            "model": "qwen3",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true
        });

        let log = format_request_log(&req);
        assert!(log.contains("model=qwen3"));
        assert!(log.contains("msgs=1"));
        assert!(log.contains("stream"));
        assert!(log.contains("\"Hello\""));
    }

    #[test]
    fn test_format_request_log_with_tools() {
        let req = json!({
            "model": "qwen3",
            "messages": [{"role": "user", "content": "Test"}],
            "stream": false,
            "tools": [
                {"type": "function", "function": {"name": "get_weather"}},
                {"type": "function", "function": {"name": "read_file"}}
            ]
        });

        let log = format_request_log(&req);
        assert!(log.contains("tools=2"));
        assert!(!log.contains("stream"));
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("hello\nworld"), "hello world");
        assert_eq!(normalize_whitespace("hello\t\tworld"), "hello world");
        assert_eq!(normalize_whitespace("hello\r\nworld"), "hello world");
        assert_eq!(normalize_whitespace("hello   world"), "hello world");
    }

    #[test]
    fn test_truncate_message_short() {
        let msg = "This is a short message";
        assert_eq!(truncate_message(msg), msg);
    }

    #[test]
    fn test_truncate_message_exactly_100() {
        let msg = "x".repeat(100);
        assert_eq!(truncate_message(&msg).len(), 100);
    }

    #[test]
    fn test_truncate_message_long() {
        let msg = "x".repeat(300);
        let truncated = truncate_message(&msg);
        assert!(truncated.starts_with(&"x".repeat(25)));
        assert!(truncated.contains(" ... "));
        assert!(truncated.ends_with(&"x".repeat(75)));
    }

    #[test]
    fn test_extract_first_user_message() {
        let req = json!({
            "model": "test",
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"}
            ]
        });

        let msg = extract_first_user_message(&req);
        assert_eq!(msg, Some("Hello".to_string()));
    }

    #[test]
    fn test_extract_message_content_array() {
        let msg = json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Part 1"},
                {"type": "text", "text": "Part 2"}
            ]
        });

        let content = extract_message_content(&msg);
        assert_eq!(content, Some("Part 1 Part 2".to_string()));
    }

    #[test]
    fn test_truncate_real_world() {
        let msg = "Can you help me write a Rust function that parses JSON and handles errors gracefully? I need it to work with large files and be memory efficient. The function should also validate the structure of the JSON before processing it further.";
        let truncated = truncate_message(msg);
        assert_eq!(truncated.len(), 105);
        assert!(truncated.contains(" ... "));
    }
}
