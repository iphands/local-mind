//! Stats formatting for different output formats

use super::RequestMetrics;
use crate::config::StatsFormat;

/// Format metrics according to the configured format
pub fn format_metrics(metrics: &RequestMetrics, format: StatsFormat) -> String {
    match format {
        StatsFormat::Pretty => format_pretty(metrics),
        StatsFormat::Json => format_json(metrics),
        StatsFormat::Compact => format_compact(metrics),
    }
}

/// Pretty box format for terminal output
fn format_pretty(m: &RequestMetrics) -> String {
    let context_str = match (m.context_used, m.context_total, m.context_percent) {
        (Some(used), Some(total), Some(pct)) => {
            format!("{}/{} ({:.1}%)", used, total, pct)
        }
        (Some(used), Some(total), None) => {
            format!("{}/{}", used, total)
        }
        _ => "N/A".to_string(),
    };

    let client_str = m
        .client_id
        .as_ref()
        .map(|c| format!("Client: {}", truncate(c, 48)));

    let conv_str = m
        .conversation_id
        .as_ref()
        .map(|c| format!("Conv: {}", truncate(c, 50)));

    let extra_lines = match (&client_str, &conv_str) {
        (Some(client), Some(conv)) => {
            format!("│ {:60}│\n│ {:60}│\n", client, conv)
        }
        (Some(client), None) => {
            format!("│ {:60}│\n", client)
        }
        (None, Some(conv)) => {
            format!("│ {:60}│\n", conv)
        }
        (None, None) => String::new(),
    };

    format!(
        r#"┌──────────────────────────────────────────────────────────────────┐
│ LLM Request Metrics                                              │
├──────────────────────────────────────────────────────────────────┤
│ Model: {:56}│
│ Time:  {:56}│
{}├──────────────────────────────────────────────────────────────────┤
│ Performance                                                      │
│   Prompt Processing: {:8.2} tokens/sec ({:7.1}ms)                │
│   Generation:        {:8.2} tokens/sec ({:7.1}ms)                │
├──────────────────────────────────────────────────────────────────┤
│ Tokens                                                           │
│   Input: {:6} │ Output: {:6} │ Total: {:6}                   │
├──────────────────────────────────────────────────────────────────┤
│ Context: {:54}│
│ Finish: {:56}│
│ Duration: {:54.1}ms│
└──────────────────────────────────────────────────────────────────┘
"#,
        truncate(&m.model, 56),
        m.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
        extra_lines,
        m.prompt_tps,
        m.prompt_ms,
        m.generation_tps,
        m.generation_ms,
        m.prompt_tokens,
        m.completion_tokens,
        m.total_tokens,
        context_str,
        m.finish_reason,
        m.duration_ms,
    )
}

/// JSON format for structured logging
fn format_json(m: &RequestMetrics) -> String {
    serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string())
}

/// Compact single-line format
fn format_compact(m: &RequestMetrics) -> String {
    let context_str = match (m.context_used, m.context_total) {
        (Some(used), Some(total)) => format!("ctx:{}/{}", used, total),
        _ => "ctx:N/A".to_string(),
    };

    format!(
        "[{}] model={} tokens={}/{} tps={:.1}/{:.1}ms={} {} finish={} dur={:.1}ms",
        m.timestamp.format("%H:%M:%S"),
        m.model,
        m.prompt_tokens,
        m.completion_tokens,
        m.generation_tps,
        m.generation_ms,
        context_str,
        if m.streaming { "stream" } else { "sync" },
        m.finish_reason,
        m.duration_ms
    )
}

/// Truncate a string to max length with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_metrics() -> RequestMetrics {
        let mut m = RequestMetrics::new();
        m.model = "test-model".to_string();
        m.prompt_tokens = 100;
        m.completion_tokens = 50;
        m.total_tokens = 150;
        m.prompt_tps = 200.5;
        m.generation_tps = 42.5;
        m.prompt_ms = 500.0;
        m.generation_ms = 1176.0;
        m.streaming = true;
        m.finish_reason = "stop".to_string();
        m.duration_ms = 1200.0;
        m
    }

    #[test]
    fn test_format_compact() {
        let m = create_test_metrics();

        let output = format_compact(&m);
        assert!(output.contains("test-model"));
        assert!(output.contains("100/50"));
        assert!(output.contains("stream"));
        assert!(output.contains("stop"));
        assert!(output.contains("42.5"));
    }

    #[test]
    fn test_format_compact_sync() {
        let mut m = create_test_metrics();
        m.streaming = false;

        let output = format_compact(&m);
        assert!(output.contains("sync"));
    }

    #[test]
    fn test_format_compact_with_context() {
        let mut m = create_test_metrics();
        m.context_used = Some(100);
        m.context_total = Some(4096);

        let output = format_compact(&m);
        assert!(output.contains("ctx:100/4096"));
    }

    #[test]
    fn test_format_compact_no_context() {
        let m = create_test_metrics();

        let output = format_compact(&m);
        assert!(output.contains("ctx:N/A"));
    }

    #[test]
    fn test_format_json() {
        let m = create_test_metrics();

        let output = format_json(&m);
        assert!(output.contains("\"model\":\"test-model\""));
        assert!(output.contains("\"prompt_tokens\":100"));
        assert!(output.contains("\"streaming\":true"));
    }

    #[test]
    fn test_format_pretty_basic() {
        let m = create_test_metrics();

        let output = format_pretty(&m);
        assert!(output.contains("test-model"));
        assert!(output.contains("LLM Request Metrics"));
        assert!(output.contains("200.50"));
        assert!(output.contains("42.50"));
    }

    #[test]
    fn test_format_pretty_with_context() {
        let mut m = create_test_metrics();
        m.context_used = Some(100);
        m.context_total = Some(4096);
        m.context_percent = Some(2.44);

        let output = format_pretty(&m);
        assert!(output.contains("100/4096"));
        assert!(output.contains("2.4%"));
    }

    #[test]
    fn test_format_pretty_no_context() {
        let m = create_test_metrics();

        let output = format_pretty(&m);
        assert!(output.contains("N/A"));
    }

    #[test]
    fn test_format_pretty_with_client_id() {
        let mut m = create_test_metrics();
        m.client_id = Some("client-123".to_string());

        let output = format_pretty(&m);
        assert!(output.contains("client-123"));
    }

    #[test]
    fn test_format_pretty_with_conversation_id() {
        let mut m = create_test_metrics();
        m.conversation_id = Some("conv-456".to_string());

        let output = format_pretty(&m);
        assert!(output.contains("conv-456"));
    }

    #[test]
    fn test_format_pretty_with_both_ids() {
        let mut m = create_test_metrics();
        m.client_id = Some("client-123".to_string());
        m.conversation_id = Some("conv-456".to_string());

        let output = format_pretty(&m);
        assert!(output.contains("client-123"));
        assert!(output.contains("conv-456"));
    }

    #[test]
    fn test_format_metrics_pretty() {
        let m = create_test_metrics();
        let output = format_metrics(&m, StatsFormat::Pretty);
        assert!(output.contains("LLM Request Metrics"));
    }

    #[test]
    fn test_format_metrics_json() {
        let m = create_test_metrics();
        let output = format_metrics(&m, StatsFormat::Json);
        assert!(serde_json::from_str::<serde_json::Value>(&output).is_ok());
    }

    #[test]
    fn test_format_metrics_compact() {
        let m = create_test_metrics();
        let output = format_metrics(&m, StatsFormat::Compact);
        assert!(output.contains("test-model"));
    }

    #[test]
    fn test_truncate_short() {
        let result = truncate("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_exact() {
        let result = truncate("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate("hello world this is long", 10);
        assert_eq!(result, "hello w...");
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn test_truncate_very_short() {
        let result = truncate("hi", 2);
        assert_eq!(result, "hi");
    }

    #[test]
    fn test_truncate_empty() {
        let result = truncate("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_pretty_long_model_name() {
        let mut m = create_test_metrics();
        m.model = "this-is-a-very-long-model-name-that-should-be-truncated-to-fit".to_string();

        let output = format_pretty(&m);
        // Should contain truncated version with ellipsis
        assert!(output.contains("..."));
    }

    #[test]
    fn test_format_pretty_finish_reason_length() {
        let mut m = create_test_metrics();
        m.finish_reason = "tool_calls".to_string();

        let output = format_pretty(&m);
        assert!(output.contains("tool_calls"));
    }

    #[test]
    fn test_format_pretty_context_partial() {
        let mut m = create_test_metrics();
        m.context_used = Some(100);
        m.context_total = Some(4096);
        // No context_percent

        let output = format_pretty(&m);
        assert!(output.contains("100/4096"));
        assert!(!output.contains("2.4%"));
    }

    #[test]
    fn test_format_compact_all_fields() {
        let mut m = create_test_metrics();
        m.generation_tps = 150.0; // This is what format_compact uses for "tps="
        m.generation_ms = 500.0;
        m.finish_reason = "length".to_string();
        m.duration_ms = 600.0;

        let output = format_compact(&m);
        // Format: tps={:.1}/{:.1}ms={} -> generation_tps/generation_ms
        assert!(output.contains("150"));
        assert!(output.contains("500"));
        assert!(output.contains("length"));
        assert!(output.contains("600"));
    }
}
