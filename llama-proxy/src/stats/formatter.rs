//! Stats formatting for different output formats

use super::RequestMetrics;
use crate::config::StatsFormat;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Format metrics according to the configured format
pub fn format_metrics(metrics: &RequestMetrics, format: StatsFormat) -> String {
    match format {
        StatsFormat::Pretty => format_pretty(metrics),
        StatsFormat::Json => format_json(metrics),
        StatsFormat::Compact => format_compact(metrics),
    }
}

/// Render a token count for display. `None` is the absence of a measurement
/// and prints `n/a`; a measured zero prints `0`.
fn token_cell(tokens: Option<u64>) -> String {
    tokens.map_or_else(|| "n/a".to_string(), |t| t.to_string())
}

/// Inner width of the pretty box: the frame is `┌` + BOX_INNER `─` + `┐` and
/// every content row is `│` + content padded to BOX_INNER + `│`. Padding is
/// measured in terminal display cells (`unicode-width`), not chars: a
/// `{:width$}` spec counts chars, so each CJK value pushed its right border
/// past the frame (a CJK model name spanned 74 cells against 68).
const BOX_INNER: usize = 66;

/// One frame row: `│` + content padded to BOX_INNER display cells + `│`.
fn box_row(content: &str) -> String {
    let pad = BOX_INNER.saturating_sub(UnicodeWidthStr::width(content));
    format!("│{}{}│\n", content, " ".repeat(pad))
}

/// A labelled row: `│ Label: value<padding>│`, label padded as historically
/// (" Model: ", " Time:  ") and value fitted to the remaining cells.
fn labeled_row(label: &str, value: &str) -> String {
    let head = format!(" {:<6} ", label);
    let fit = BOX_INNER - head.chars().count();
    box_row(&format!("{head}{}", truncate_to_width(value, fit)))
}

/// Identity rows: model, timestamp, optional client/conversation ids.
fn pretty_identity_lines(m: &RequestMetrics) -> String {
    let mut out = labeled_row("Model:", &m.model);
    out.push_str(&labeled_row(
        "Time:",
        &m.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    ));
    if let Some(client) = &m.client_id {
        out.push_str(&labeled_row("Client:", client));
    }
    if let Some(conv) = &m.conversation_id {
        out.push_str(&labeled_row("Conv:", conv));
    }
    out
}

/// Performance rows. A prefill/decode split is only real when the backend
/// reported one; otherwise show the throughput a single wall-clock duration
/// can actually support rather than inventing a split.
fn pretty_performance_lines(m: &RequestMetrics) -> String {
    if m.has_timing_split {
        box_row(&format!(
            "   Prompt Processing: {:8.2} tokens/sec ({:7.1}ms)",
            m.prompt_tps, m.prompt_ms
        )) + &box_row(&format!(
            "   Generation:        {:8.2} tokens/sec ({:7.1}ms)",
            m.generation_tps, m.generation_ms
        ))
    } else {
        box_row(&format!("   Total throughput:  {:8.2} tokens/sec", m.total_tps))
            + &box_row("   (backend reported no prefill/decode split)")
    }
}

/// Token rows: input/output/total plus the optional reasoning line.
fn pretty_tokens_lines(m: &RequestMetrics) -> String {
    let mut out = box_row(&format!(
        "   Input: {:>6} │ Output: {:>6} │ Total: {:>6}",
        token_cell(m.prompt_tokens),
        token_cell(m.completion_tokens),
        m.total_tokens
    ));
    if let Some(r) = m.reasoning_tokens {
        out.push_str(&box_row(&format!("   Reasoning Tokens: {r}")));
    }
    out
}

/// Closing rows: context, finish reason, stream state, duration, concurrency.
fn pretty_footer_lines(m: &RequestMetrics) -> String {
    let context_str = match (m.context_used, m.context_total, m.context_percent) {
        (Some(used), Some(total), Some(pct)) => format!("{used}/{total} ({pct:.1}%)"),
        (Some(used), Some(total), None) => format!("{used}/{total}"),
        _ => "N/A".to_string(),
    };
    let mut out = labeled_row("Context:", &context_str);
    out.push_str(&labeled_row("Finish:", &m.finish_reason));
    // A client that hung up is normal traffic, not a defect worth a box line;
    // an incomplete answer always is, whatever caused it.
    if m.streaming {
        let badge = match m.stream_end {
            Some("truncated") => Some("TRUNCATED (backend ended without a completion event)"),
            Some("stalled") => Some("STALLED (stream timed out before completion)"),
            Some("client_gone") => None,
            _ => Some("ok"),
        };
        if let Some(badge) = badge {
            out.push_str(&labeled_row("Stream:", badge));
        }
    }
    out.push_str(&labeled_row("Duration:", &format!("{:.1}ms", m.duration_ms)));
    if let Some(c) = m.concurrent_requests {
        out.push_str(&labeled_row("Concurrent:", &c.to_string()));
    }
    out
}

/// Pretty box format for terminal output
fn format_pretty(m: &RequestMetrics) -> String {
    let separator = format!("├{}┤\n", "─".repeat(BOX_INNER));
    let mut out = format!("┌{}┐\n", "─".repeat(BOX_INNER));
    out.push_str(&box_row(" LLM Request Metrics"));
    out.push_str(&separator);
    out.push_str(&pretty_identity_lines(m));
    out.push_str(&separator);
    out.push_str(&box_row(" Performance"));
    out.push_str(&pretty_performance_lines(m));
    out.push_str(&separator);
    out.push_str(&box_row(" Tokens"));
    out.push_str(&pretty_tokens_lines(m));
    out.push_str(&separator);
    out.push_str(&pretty_footer_lines(m));
    out.push_str(&format!("└{}┘\n", "─".repeat(BOX_INNER)));
    out
}

/// JSON format for structured logging
fn format_json(m: &RequestMetrics) -> String {
    serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string())
}

/// Compact single-line format
fn format_compact(m: &RequestMetrics) -> String {
    let context_str = match (m.context_used, m.context_total) {
        (Some(used), Some(total)) => format!("ctx={}/{}", used, total),
        _ => "ctx=null".to_string(),
    };

    let group_str = m.group_name.as_ref().map(|g| format!(" group={}", g)).unwrap_or_default();

    let concurrent_str = m
        .concurrent_requests
        .map(|c| format!(" concurrent={}", c))
        .unwrap_or_default();

    // Only claim a prefill/decode split when the backend actually reported one:
    // llama.cpp's `timings`, or vLLM's `metrics` (which needs the server started
    // with --enable-per-request-metrics; without it the field is null). Otherwise
    // print total throughput rather than two numbers that look measured and are not.
    let tps_str = if m.has_timing_split {
        format!("tps={:.2}/{:.2}", m.prompt_tps, m.generation_tps)
    } else {
        format!("tps={:.2}tot", m.total_tps)
    };

    // Queue time is the honest signal for "is concurrency hurting?" -- it is time
    // the request sat waiting to be scheduled, so it rises only under real
    // saturation, whereas a falling per-stream decode rate does not by itself
    // mean the server is overloaded. Shown only when the backend reports it and
    // it is not trivially zero.
    let queue_str = m
        .queue_ms
        .filter(|q| *q >= 1.0)
        .map(|q| format!(" queue={:.0}ms", q))
        .unwrap_or_default();

    format!(
        "model={}{} tokens={}/{} {} {} {} finish={} dur={:.1}ms{}{}",
        m.model,
        group_str,
        token_cell(m.prompt_tokens),
        token_cell(m.completion_tokens),
        tps_str,
        context_str,
        if m.streaming {
            match m.stream_end {
                Some("truncated") => "stream=trunc",
                Some("stalled") => "stream=stalled",
                Some("client_gone") => "stream=gone",
                Some("ok") => "stream=ok",
                // Requests whose stream end was never observed (legacy/other
                // paths) keep the historical bare marker.
                _ if m.stream_incomplete => "stream=trunc",
                _ => "stream=ok",
            }
        } else {
            "sync"
        },
        m.finish_reason,
        m.duration_ms,
        queue_str,
        concurrent_str
    )
}

/// Truncate a string to `max_cells` terminal display cells with an ellipsis.
/// Never splits a character: a 2-cell character is kept only if both of its
/// cells fit the remaining budget; below 3 cells the ellipsis cannot shrink.
fn truncate_to_width(s: &str, max_cells: usize) -> String {
    if UnicodeWidthStr::width(s) <= max_cells {
        return s.to_string();
    }
    let budget = max_cells.saturating_sub(3);
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        used += w;
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_metrics() -> RequestMetrics {
        let mut m = RequestMetrics::new();
        m.model = "test-model".to_string();
        m.prompt_tokens = Some(100);
        m.completion_tokens = Some(50);
        m.total_tokens = 150;
        m.prompt_tps = 200.5;
        m.generation_tps = 42.5;
        m.prompt_ms = 500.0;
        m.generation_ms = 1176.0;
        m.has_timing_split = true; // llama.cpp-style backend
        m.total_tps = 125.0;
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

    /// A backend that reports no `timings` (vLLM) must not be shown a
    /// prefill/decode split -- only total throughput.
    #[test]
    fn test_format_compact_without_timing_split() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        m.prompt_tps = 0.0;
        m.generation_tps = 0.0;
        m.total_tps = 125.0;

        let output = format_compact(&m);
        assert!(output.contains("tps=125.00tot"), "got: {output}");
        assert!(!output.contains("/0.00"), "must not print a fabricated split: {output}");
    }

    /// Regression: prompt/generation TPS were once derived from a hardcoded
    /// 20%/80% split of the wall clock, which invented figures like 113,344
    /// tok/s of prefill on a server measured at ~14.5k. Nothing may reintroduce
    /// a split that the backend did not report.
    #[test]
    fn test_no_split_is_invented_from_duration() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        m.prompt_tps = 0.0;
        m.generation_tps = 0.0;
        m.prompt_tokens = Some(39240);
        m.completion_tokens = Some(146);
        m.total_tokens = 39386;
        m.duration_ms = 1731.0;
        m.total_tps = (m.total_tokens as f64 / m.duration_ms) * 1000.0;

        let output = format_compact(&m);
        // 39240 / (0.2 * 1731ms) * 1000 == 113344.89 -- the old fabricated value
        assert!(!output.contains("113344"), "reintroduced the 20/80 estimate: {output}");
        assert!(output.contains("tps=22753.32tot"), "got: {output}");
    }

    #[test]
    fn test_format_compact_sync() {
        let mut m = create_test_metrics();
        m.streaming = false;

        let output = format_compact(&m);
        assert!(output.contains("sync"));
    }

    #[test]
    fn test_stream_end_badges_are_key_value() {
        let cases = [
            (Some("ok"), "stream=ok"),
            (Some("truncated"), "stream=trunc"),
            (Some("stalled"), "stream=stalled"),
            (Some("client_gone"), "stream=gone"),
        ];
        for (end, expected) in cases {
            let mut m = create_test_metrics();
            m.stream_end = end;
            let output = format_compact(&m);
            assert!(output.contains(expected), "{end:?} -> {output}");
            assert!(!output.contains("stream "), "must stay one field: {output}");
        }
    }

    #[test]
    fn test_pretty_reports_truncation() {
        let mut m = create_test_metrics();
        m.stream_end = Some("truncated");
        m.stream_incomplete = true;
        let output = format_pretty(&m);
        assert!(output.contains("TRUNCATED"), "got: {output}");

        let mut m = create_test_metrics();
        m.stream_end = Some("client_gone");
        let output = format_pretty(&m);
        assert!(!output.contains("Stream:"), "a client hangup is not a defect line: {output}");
    }

    #[test]
    fn test_format_compact_with_context() {
        let mut m = create_test_metrics();
        m.context_used = Some(100);
        m.context_total = Some(4096);

        let output = format_compact(&m);
        assert!(output.contains("ctx=100/4096"));
    }

    #[test]
    fn test_format_compact_no_context() {
        let m = create_test_metrics();

        let output = format_compact(&m);
        assert!(output.contains("ctx=null"));
    }

    /// An absent token count must read `n/a`, never a fabricated 0.
    #[test]
    fn absent_token_counts_print_n_a() {
        let mut m = create_test_metrics();
        m.prompt_tokens = None;
        m.completion_tokens = None;

        let compact = format_compact(&m);
        assert!(compact.contains("tokens=n/a/n/a"), "got: {compact}");

        let pretty = format_pretty(&m);
        assert!(pretty.contains("Input:    n/a"), "got: {pretty}");
        assert!(pretty.contains("Output:    n/a"), "got: {pretty}");
    }

    /// Some(0) is a measured zero and must still print 0.
    #[test]
    fn measured_zero_token_counts_print_zero() {
        let mut m = create_test_metrics();
        m.prompt_tokens = Some(0);
        m.completion_tokens = Some(0);

        let compact = format_compact(&m);
        assert!(compact.contains("tokens=0/0"), "got: {compact}");
        assert!(!compact.contains("n/a"), "got: {compact}");
    }

    #[test]
    fn test_format_compact_with_group() {
        let mut m = create_test_metrics();
        m.group_name = Some("opus".to_string());

        let output = format_compact(&m);
        assert!(output.contains("group=opus"));
        assert!(output.contains("test-model"));
    }

    #[test]
    fn test_format_compact_no_group() {
        let m = create_test_metrics();

        let output = format_compact(&m);
        // Should not contain "group=" when group_name is None
        assert!(!output.contains("group="));
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
        let result = truncate_to_width("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_exact() {
        let result = truncate_to_width("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate_to_width("hello world this is long", 10);
        assert_eq!(result, "hello w...");
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn test_truncate_very_short() {
        let result = truncate_to_width("hi", 2);
        assert_eq!(result, "hi");
    }

    #[test]
    fn test_truncate_empty() {
        let result = truncate_to_width("", 10);
        assert_eq!(result, "");
    }

    /// A model name with multibyte characters used to be sliced on a byte index,
    /// which panicked the whole formatter when the cut landed mid-character.
    /// Sweeping every width is stronger than one hand-picked index: it covers
    /// whatever width a caller (or a future box redesign) picks.
    #[test]
    fn test_truncate_multibyte_never_panics() {
        for name in [
            "模型-🌍-very-long-model-name-that-exceeds-the-limit-by-a-lot",
            "aaaaaaaaaaaa模型bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb模型-very-long-model-name",
        ] {
            for width in 1..=name.chars().count() + 5 {
                let out = truncate_to_width(name, width);
                // "..." is the floor: below width 3 the ellipsis cannot shrink.
                assert!(
                    out.chars().count() <= width.max(3),
                    "width {width} produced {} chars: {out:?}",
                    out.chars().count()
                );
                assert!(
                    UnicodeWidthStr::width(out.as_str()) <= width.max(3),
                    "width {width} produced {} cells: {out:?}",
                    UnicodeWidthStr::width(out.as_str())
                );
            }
        }

        // The path that actually shipped the panic: format_pretty truncates the
        // model name at a fixed width.
        let mut m = create_test_metrics();
        m.model = "aaaaaaaaaaaa模型bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb模型-very-long-model-name".to_string();
        let _ = format_pretty(&m);
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
        m.prompt_tps = 123.45; // Prompt processing TPS
        m.generation_tps = 150.00; // Generation TPS
        m.has_timing_split = true;
        m.finish_reason = "length".to_string();
        m.duration_ms = 600.0;

        let output = format_compact(&m);
        // Format: tps={:.2}/{:.2} -> prompt_tps/generation_tps
        assert!(output.contains("123.45"));
        assert!(output.contains("150.00"));
        assert!(output.contains("length"));
        assert!(output.contains("600"));
    }

    /// RED at baseline 57b2d2b (probe captured in .omo/evidence/big-fix/task92.txt):
    /// the box padded with `{:w$}` specs that count CHARS, so the CJK model row
    /// spanned 74 display cells (Client 69, Finish 69, even the ASCII Tokens row
    /// 70) against the 68-cell frame. Every row must occupy the frame exactly,
    /// measured in display cells.
    #[test]
    fn pretty_rows_all_share_the_frame_width() {
        let mut m = create_test_metrics();
        m.model = "本地大模型-Qwen3-14B-量化版".to_string();
        m.client_id = Some("客户标识-中文-identifier".to_string());
        m.conversation_id = Some("会话-abc".to_string());
        m.finish_reason = "停止done".to_string();
        m.context_used = Some(100);
        m.context_total = Some(4096);
        m.context_percent = Some(2.44);
        m.reasoning_tokens = Some(10);
        m.concurrent_requests = Some(2);
        m.stream_end = Some("truncated");
        m.stream_incomplete = true;

        let frame = BOX_INNER + 2;
        for line in format_pretty(&m).lines() {
            let cells = UnicodeWidthStr::width(line);
            assert_eq!(cells, frame, "row {line:?} spans {cells} cells");
        }
    }

    /// The cut lands on display cells, not chars: a 2-cell character is kept
    /// only when both of its cells fit the remaining budget.
    #[test]
    fn truncate_to_width_cuts_on_display_cells() {
        assert_eq!(truncate_to_width("模型模型模型", 8), "模型...");
        assert_eq!(truncate_to_width("模型模型模型", 7), "模型...");
        assert_eq!(truncate_to_width("模型模型模型", 4), "...");
        for max_cells in 3..=40 {
            let out = truncate_to_width("本地大模型-Qwen3-14B-量化版很长", max_cells);
            assert!(
                UnicodeWidthStr::width(out.as_str()) <= max_cells.max(3),
                "{max_cells} cells -> {out:?}"
            );
        }
    }
}
