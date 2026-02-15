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

    #[test]
    fn test_format_compact() {
        let mut m = RequestMetrics::new();
        m.model = "test-model".to_string();
        m.prompt_tokens = 100;
        m.completion_tokens = 50;
        m.generation_tps = 42.5;
        m.generation_ms = 1176.0;
        m.streaming = true;
        m.finish_reason = "stop".to_string();
        m.duration_ms = 1200.0;

        let output = format_compact(&m);
        assert!(output.contains("test-model"));
        assert!(output.contains("100/50"));
        assert!(output.contains("stream"));
    }
}
