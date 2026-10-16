//! Stats formatting for different output formats

use super::RequestMetrics;
use crate::config::StatsFormat;
use std::borrow::Cow;
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

/// A measured rate in a 6-cell numeric slot, or `n/a` -- deliberately the same absence
/// marker the token cells already use, so the line speaks one spelling.
///
/// Width-neutral on purpose. `{:>6.2}` only holds values below 1000, so any real prefill
/// rate overflows this cell -- llama.cpp's cache-inflated figure is documented at 880k
/// tok/s. That is a PRE-EXISTING defect of `tps={:>6.2}/{:>6.2}`, not something this
/// introduced; T6 replaces it with the k-unit form pinned by the golden. Never clamp
/// silently here: a fabricated ceiling on a measured rate is the exact sin this plan
/// exists to prevent.
fn tps_side(v: Option<f64>) -> Cow<'static, str> {
    match v {
        Some(v) if v > 0.0 => Cow::Owned(format!("{v:>6.2}")),
        _ => Cow::Borrowed("   n/a"),
    }
}

/// A backend duration in a 7-cell slot, `n/a` on its own `0.0 == not reported` sentinel.
/// Without this a one-sided split printed `n/a tokens/sec (    0.0ms)` -- an honest
/// absence sitting beside a zero meaning the same thing, and two conventions in adjacent
/// columns is worse than either alone.
fn ms_side(v: f64) -> Cow<'static, str> {
    if v > 0.0 {
        Cow::Owned(format!("{v:>7.1}"))
    } else {
        Cow::Borrowed("    n/a")
    }
}

/// The wall-bounded floor in an 8-cell slot. `0.0` here means "never computed"
/// (no counted output, or no wall clock) rather than "decoded instantly", so it
/// gets the same `n/a` as everywhere else: `0.00 tokens/sec (wall-bounded)` is
/// the lie `tps>=0.00` was on the compact line.
fn floor_side(v: f64) -> Cow<'static, str> {
    if v > 0.0 {
        Cow::Owned(format!("{v:>8.2}"))
    } else {
        Cow::Borrowed("     n/a")
    }
}

/// Which rate tier a sample renders in: `1` = backend-measured decode,
/// `2` = the wall-bounded floor, `3` = nothing to measure at all.
///
/// ONE predicate, shared by `format_compact`, `format_pretty` and the alignment
/// test's row bucketing. When that test re-derived the tier from
/// `has_timing_split` it silently disagreed with the renderer as soon as the
/// gate moved onto the rates -- and "all three tiers appear in the fixture"
/// could then pass without the formatter ever rendering what the test believed
/// it had seen.
fn tps_tier(m: &RequestMetrics) -> u8 {
    // `is_some_and(|v| v > 0.0)`, not `is_some()`: `tps_side` prints a value only
    // above zero, so a directly-built `Some(0.0)` would enter tier 1 and emit the
    // exact half-row this gate exists to prevent (`tps=200.50/   n/a`). The
    // collector cannot produce that value -- `finite_rate_opt` rejects it -- but
    // the tier is a rendering decision, and this is the same second lock
    // `has_throughput_signal` keeps in collector.rs.
    if m.generation_tps.is_some_and(|v| v > 0.0) {
        1
    } else if m.decode_tps_min > 0.0 {
        2
    } else {
        3
    }
}

/// Performance rows. A prefill/decode split is only real when the backend
/// reported one. With none, name both derived numbers for what they are: a
/// wall-bounded floor on decode and a cost rate. Neither is a speed, and
/// neither is invented -- the invented 20/80 split this replaced is gone.
///
/// Gated on the DECODE side (via [`tps_tier`]) for the same reason as the
/// compact line, and pretty is the DEFAULT format, so leaving this on
/// `has_timing_split` would have the box print a *measured* `Prompt Processing`
/// row for a half split -- an impressive prefill figure under a "measured"
/// heading, `Generation: n/a` beneath it, and no floor anywhere in the format
/// most operators actually read. A half split is not a split.
fn pretty_performance_lines(m: &RequestMetrics) -> String {
    if tps_tier(m) == 1 {
        box_row(&format!(
            "   Prompt Processing: {:>8} tokens/sec ({}ms)",
            tps_side(m.prompt_tps),
            ms_side(m.prompt_ms)
        )) + &box_row(&format!(
            "   Generation:        {:>8} tokens/sec ({}ms)",
            tps_side(m.generation_tps),
            ms_side(m.generation_ms)
        ))
    } else {
        let mut out = String::new();
        // A measured prefill with no measured decode is HALF a split: it stays in
        // the box, as its own labelled row and not as a split, sitting beside the
        // bound that actually speaks to decode. Dropping it entirely would delete
        // the only backend measurement the sample carries.
        let prefill_measured = m.prompt_tps.is_some_and(|v| v > 0.0);
        if prefill_measured {
            out.push_str(&box_row(&format!(
                "   Prompt Processing: {:>8} tokens/sec ({}ms)",
                tps_side(m.prompt_tps),
                ms_side(m.prompt_ms)
            )));
        }
        out.push_str(&box_row(&format!(
            "   Decode (floor):    {} tokens/sec (wall-bounded)",
            floor_side(m.decode_tps_min)
        )));
        out.push_str(&box_row(&format!(
            "   Work rate:         {:8.2} tokens/sec (all tokens/wall)",
            m.total_tps
        )));
        out.push_str(&box_row(if prefill_measured {
            "   (no backend decode rate: prefill measured, floor is a bound)"
        } else {
            "   (no backend split: decode is a floor, work is cost)"
        }));
        out
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

/// A free-form field is text the proxy did not author: the model name and
/// `finish_reason` arrive verbatim from the backend, `group_name` from config. Two
/// things must be true before one may enter a single-line record, and neither is
/// style:
///   * no control character may survive. A `\n` in a backend `stop_reason` splits one
///     log record into two, which is the forgery class the InfluxDB line protocol
///     already sanitizes against; U+2028/2029 are line separators that `char::is_control`
///     does NOT report, so they are named explicitly.
///   * no `=` may survive. The head is located by probing for `ctx=`, `tok=`,
///     `stream=`, `finish=` — a free-form value carrying one of those needles makes the
///     probe ambiguous, so a backend string can forge a column. Control-char stripping
///     alone does not close this.
///   * the BYTE count is capped, not just the cell count. `truncate_to_width` bounds
///     cells, and a cell bound does not bound bytes: U+200B, ZWJ/ZWNJ and combining
///     marks are zero cells, so a backend echoing `"\u{200b}" * 10_000_000` as the model
///     name is under the cell limit, sails through truncation untouched, and writes
///     tens of megabytes into one log record. The model is remotely writable through
///     the model the backend echoes, so the cap is on chars as well.
const MAX_FREE_FORM_CHARS: usize = 64;

/// Cap on the `finish=` value in DISPLAY CELLS. See `format_compact`: it exists to
/// bound a log write, not to align a column — `finish=` is the last head cell, so its
/// width cannot move the head, and every legitimate finish reason fits well inside it.
const MAX_FINISH_CELLS: usize = 24;

fn free_form(value: &str) -> String {
    value
        .chars()
        .take(MAX_FREE_FORM_CHARS)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') || c == '=' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Pad to `cells` TERMINAL CELLS on the right. Never `{:<N}` on text: a format count
/// is in chars, so the 8-char CJK model that is 16 cells wide would gain 8 spaces and
/// shove every column after it — the bug `box_row` already fixed for `pretty`.
fn pad_cells(s: &str, cells: usize) -> String {
    match cells.checked_sub(UnicodeWidthStr::width(s)) {
        Some(shortfall) => format!("{}{}", s, " ".repeat(shortfall)),
        None => s.to_string(),
    }
}

/// Render one `n/a`-able count for a `{:>6}`-shaped slot. `None` is the absence of a
/// measurement and prints `n/a`; a measured zero prints `0`.
fn count_cell(count: Option<u64>) -> String {
    count.map_or_else(|| "n/a".to_string(), |n| n.to_string())
}

/// The rate group: ONE width for all three tiers, so the line cannot betray which
/// tier it rendered. Which tier you see is a fact about the backend, not the request.
///
/// Built as two fixed-width fields rather than three hand-spaced literals. A
/// hand-spaced tier string is 22, 23 or 24 cells depending on how many spaces someone
/// typed (`tg>=` is inherently one wider than `tg=`, and `pp=…k` is one wider than
/// `pp=…n/a`), and only the first of those is the contract's 23. Fixing the widths
/// makes every tier land on 23 by construction AND pins the decode number's right edge,
/// so `tg=  195.5` and `tg>= 146.3` stack in a wall of mixed-tier lines.
///
/// `pp` gets 12 cells and `tg` 10 because the two fields span very different ranges:
/// prefill legitimately runs 50 → 580,000 tok/s (a full prefix-cache hit is left
/// un-discounted by design), and the kilo suffix is what keeps that four-order spread
/// inside a fixed cell without clamping. `tg` must hold `10000.0`, which is what
/// 9999.99 rounds to at one decimal — so it needs 7, not 6.
fn rate_group(m: &RequestMetrics) -> String {
    // `pp` prints ONLY on tier 1. A half split (backend-reported prefill, no usable
    // decode rate) is tier 2, and printing its cache-inflated prefill there put a huge
    // figure in the column the eye lands on while the column that matters stayed empty
    // — the exact failure `tps_tier` exists to prevent. Absent, not zero.
    match tps_tier(m) {
        1 => format!(
            "pp={:>8.3}k tg={:>7.1}",
            m.prompt_tps.unwrap_or(0.0) / 1000.0,
            m.generation_tps.unwrap_or(0.0)
        ),
        // The absent `pp` side gives up one cell (11, not tier 1's 12) so the floor
        // number gets the SAME 7 cells tier 1 gives a measured decode: `{:>6.1}` renders
        // 10000.0 -- which 9999.95 rounds up to -- as 7 chars, making the group 24 and
        // moving `dur=` off its pinned cell. The `n/a` sits in whitespace either way.
        2 => format!("pp=     n/a tg>={:>7.1}", m.decode_tps_min),
        3 => "pp=     n/a tg=     n/a".to_string(),
        _ => unreachable!("tps_tier returns 1..=3"),
    }
}

/// Compact single-line format: a pinned-width head followed by a free-form tail.
/// A cell is columnar only if its width is constant, so no conditional cell may sit
/// inside the head — `group=`, `work=`, `queue=`, `concurrent=` and `cache=` are all
/// present-or-absent and therefore live in the tail, `cache=` last because a cell that
/// appears only on cache hits must sit after everything that has to line up.
///
/// The widths are the contract's (`HEAD`/`head_offsets` in the test module); changing
/// one means changing the plan's table and the tracker in the same commit, never here.
fn format_compact(m: &RequestMetrics) -> String {
    // Truncated THEN padded, both in display cells: a 50-cell GGUF name is what shifts
    // every column today, and padding a 46-cell value to 16 is a no-op.
    let model = pad_cells(&truncate_to_width(&free_form(&m.model), 16), 16);

    // `dur` is the one cell that saturates. It is the proxy's own wall clock, not a
    // backend-reported quantity, and 7 digits is ~2.8h — past that a request is stuck,
    // and a column that shifts for a stuck request teaches nothing. `ctx`/`tok` and the
    // rates are backend-reported and NEVER clamped: an out-of-band figure must widen the
    // line rather than be silently rounded into a number the engine never reported.
    // `f64::min` returns the OTHER operand when one is NaN, so a NaN wall clock would
    // otherwise print as the 9999999 sentinel — a plausible 2.8h request where nothing is
    // known. Non-finite renders as `NaN`/`inf` (still 7 cells) rather than a lie.
    let dur_value = if m.duration_ms.is_finite() {
        m.duration_ms.min(9_999_999.0)
    } else {
        f64::NAN
    };
    let dur = format!("dur={:>7.0}ms", dur_value);
    let ctx = format!("ctx={:>6}/{:>6}", count_cell(m.context_used), count_cell(m.context_total));
    let tok = format!(
        "tok={:>6}/{:>6}",
        count_cell(m.prompt_tokens),
        count_cell(m.completion_tokens)
    );

    // Always labelled now, including non-streaming: bare `sync` was a column with no
    // key, and `stream=sync` keeps every existing `grep sync` working.
    let badge = if m.streaming {
        match m.stream_end {
            Some("truncated") => "trunc",
            Some("stalled") => "stalled",
            Some("client_gone") => "gone",
            Some("ok") => "ok",
            _ if m.stream_incomplete => "trunc",
            _ => "ok",
        }
    } else {
        "sync"
    };
    let stream = format!("stream={}", pad_cells(badge, 7));

    // `n/a`, never a blank cell: an empty `finish=` reads as a missing column.
    // `tool_calls` fills the 10 cells exactly and `content_filter` (14) overflows BY
    // DESIGN — a rare diagnostic value is never clamped into invisibility. The cap is
    // purely a log-flood bound: this is verbatim backend text and the widest value any
    // backend emits is 14 cells, so 24 cuts junk and nothing legitimate.
    let finish_value = free_form(&m.finish_reason);
    let finish_value = match finish_value.trim() {
        "" => "n/a".to_string(),
        "unknown" => "n/a".to_string(),
        _ => truncate_to_width(&finish_value, MAX_FINISH_CELLS),
    };
    let finish = format!("finish={}", pad_cells(&finish_value, 10));

    let head = [format!("model={model}"), rate_group(m), dur, ctx, tok, stream, finish].join(" ");

    let mut tail: Vec<String> = Vec::new();
    if let Some(g) = &m.group_name {
        tail.push(format!("group={}", free_form(g)));
    }
    // `work=` is the cost rate: (prompt+completion)/wall. On a long-context agent turn
    // with a short completion it reads in the thousands while decode crawls in the
    // tens, so it is the least trustworthy number on the line and sits furthest out.
    // Only tiers 2 and 3 have one; tier 1's figures are the measured split.
    if tps_tier(m) != 1 && m.total_tps > 0.0 {
        tail.push(format!("work={:.2}", m.total_tps));
    }
    if let Some(q) = m.queue_ms.filter(|q| *q >= 1.0) {
        tail.push(format!("queue={q:.0}ms"));
    }
    if let Some(c) = m.concurrent_requests {
        tail.push(format!("concurrent={c}"));
    }
    if let Some(c) = m.cached_tokens.filter(|c| *c > 0) {
        tail.push(format!("cache={c}"));
    }

    // The head pads its last cell; the line must not end in it, or awk/column
    // consumption breaks on every request whose tail is thin.
    format!("{head} {}", tail.join(" ")).trim_end().to_string()
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
        m.prompt_tps = Some(200.5);
        m.generation_tps = Some(42.5);
        m.prompt_ms = 500.0;
        m.generation_ms = 1176.0;
        m.has_timing_split = true; // llama.cpp-style backend
        m.total_tps = 125.0;
        // 50 completion tokens over the 1200ms wall clock. Hand-set like every
        // other field here: these tests build a sample, they do not run the
        // extractor.
        m.decode_tps_min = 50.0 / 1.2;
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
        // Pinned head order: model, rate group, dur, ctx, tok, stream, finish.
        assert!(output.contains("pp=   0.201k tg=   42.5"), "got: {output}");
        assert!(output.contains("dur=   1200ms"), "got: {output}");
        assert!(output.contains("tok=   100/    50"), "got: {output}");
        assert!(output.contains("stream=ok"));
        assert!(output.contains("finish=stop"));
    }

    /// A backend that reports no `timings` (vLLM) must not be shown a
    /// prefill/decode split -- and the two derived numbers it does get must be
    /// labelled for what they are: a wall-bounded floor and a cost rate.
    #[test]
    fn test_format_compact_without_timing_split() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        m.prompt_tps = None;
        m.generation_tps = None;
        m.total_tps = 125.0;
        m.decode_tps_min = 50.0 / 1.2;

        let output = format_compact(&m);
        assert!(output.contains("tg>=   41.7"), "got: {output}");
        assert!(output.contains("work=125.00"), "got: {output}");
        assert!(!output.contains("/0.00"), "must not print a fabricated split: {output}");
        // The old key read as a measured speed; it must not come back.
        assert!(!output.contains("tot"), "retired `tot` key is back: {output}");
    }

    /// The floor branch needs a counted completion. Without one there is no
    /// decode rate to bound at all -- print `n/a`, never a zero that reads as
    /// "instantly slow".
    #[test]
    fn compact_without_counted_output_prints_no_floor() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        // A backend that reports no split reports NO rates: collector.rs derives
        // `has_timing_split` as `prompt_tps.is_some() || generation_tps.is_some()`,
        // so rates-plus-false-flag is a state the only writer of that field cannot
        // produce. Leaving the inherited rates in place made this test pass against
        // the tier gate's blind spot rather than against behaviour.
        m.prompt_tps = None;
        m.generation_tps = None;
        m.completion_tokens = None;
        m.decode_tps_min = 0.0;
        m.total_tps = 125.0;
        assert_eq!(
            m.has_timing_split,
            m.prompt_tps.is_some() || m.generation_tps.is_some(),
            "fixture desynced from the collector's own derivation (collector.rs:210)"
        );

        let output = format_compact(&m);
        assert!(output.contains("tg=     n/a"), "got: {output}");
        assert!(output.contains("work=125.00"), "got: {output}");
        assert!(!output.contains("tg>="), "a zero is not a floor: {output}");
    }

    /// The case the decode-side gate exists for, and the one the old
    /// `has_timing_split` gate got wrong: a HALF split. llama.cpp with a warm
    /// prefix cache yields a cache-inflated prefill rate and no usable
    /// `predicted_per_second` (`finite_rate_opt` maps the 0.0 to `None`), so
    /// `has_timing_split` is TRUE -- collector.rs:210 derives it with `||` -- while
    /// decode stays unmeasured. The old gate printed `tps=88000.00/   n/a`: a huge
    /// figure in the scan column and nothing in the half that matters.
    #[test]
    fn compact_half_split_falls_to_the_floor_tier() {
        let mut m = create_test_metrics();
        m.has_timing_split = true; // as the collector derives it from the prefill rate
        m.prompt_tps = Some(88_000.0); // cache-inflated, and genuinely backend-reported
        m.generation_tps = None; // the column the operator scans is empty

        let out = format_compact(&m);
        assert!(out.contains("tg>="), "must bound the missing quantity: {out}");
        assert!(out.contains("41.7"), "floor value missing: {out}");
        assert!(
            !out.contains("88000") && !out.contains("88,000") && !out.contains("88.000"),
            "prefill printed where decode belongs: {out}"
        );
    }

    /// A backend that measured decode but sent no `usage` block still gets its
    /// measurement shown. `tps=n/a` would be a false claim about the backend's
    /// silence, and the floor is strictly worse than the number it would replace:
    /// `decode_tps_min`'s denominator carries prefill, network and fake-mode
    /// synthesis, so it understates -- and `tps=n/a work=` is the exact shape of the
    /// abandoned-stream class, which this line must not absorb.
    #[test]
    fn compact_measured_decode_beats_the_floor_without_usage_counts() {
        let mut m = create_test_metrics();
        m.completion_tokens = None; // no usage block at all
        m.decode_tps_min = 0.0; // so no floor exists either

        let out = format_compact(&m);
        assert!(out.contains("pp=   0.201k tg=   42.5"), "got: {out}");
        assert!(!out.contains(">="), "a bound replaced a measurement: {out}");
        assert!(!out.contains("work="), "tier 3 leaked into tier 1: {out}");
    }

    /// Tier 1 wins outright: a backend-split line carries neither the floor nor
    /// the cost rate, so the two tiers can never be confused for each other.
    #[test]
    fn compact_split_path_prints_neither_floor_nor_work() {
        let m = create_test_metrics();
        assert!(m.has_timing_split);

        let output = format_compact(&m);
        assert!(output.contains("pp=   0.201k tg=   42.5"), "got: {output}");
        assert!(!output.contains("work="), "tier 3 leaked into a tier-1 line: {output}");
        assert!(!output.contains(">="), "tier 2 leaked into a tier-1 line: {output}");
    }

    /// The cache cell is the reader's discount on `work=`. Absent and measured
    /// zero are both "nothing to discount" and print nothing.
    #[test]
    fn compact_cache_cell_appears_only_when_nonzero() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        // Clear the rates too, or the decode gate silently promotes this row to
        // tier 1 -- where every assertion below still passes, but `cache=` is no
        // longer being checked beside the `work=` it exists to discount.
        m.prompt_tps = None;
        m.generation_tps = None;
        m.cached_tokens = None;
        assert!(!format_compact(&m).contains("cache="), "None must print nothing");

        m.cached_tokens = Some(0);
        assert!(
            !format_compact(&m).contains("cache="),
            "a measured zero adds nothing and buries the real ones"
        );

        m.cached_tokens = Some(4096);
        assert!(format_compact(&m).contains("cache=4096"), "got: {}", format_compact(&m));
    }

    /// Regression: prompt/generation TPS were once derived from a hardcoded
    /// 20%/80% split of the wall clock, which invented figures like 113,344
    /// tok/s of prefill on a server measured at ~14.5k. Nothing may reintroduce
    /// a split that the backend did not report.
    #[test]
    fn test_no_split_is_invented_from_duration() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        m.prompt_tps = None;
        m.generation_tps = None;
        m.prompt_tokens = Some(39240);
        m.completion_tokens = Some(146);
        m.total_tokens = 39386;
        m.duration_ms = 1731.0;
        m.total_tps = (m.total_tokens as f64 / m.duration_ms) * 1000.0;
        m.decode_tps_min = (m.completion_tokens.unwrap() as f64 / m.duration_ms) * 1000.0;

        let output = format_compact(&m);
        // 39240 / (0.2 * 1731ms) * 1000 == 113344.89 -- the old fabricated value
        assert!(!output.contains("113344"), "reintroduced the 20/80 estimate: {output}");
        // The cost rate is still there (it reconciles with sum(total_tokens)),
        // but under a key that cannot be read as a speed, next to the honest
        // floor: 146 tokens over a 1731ms wall is "at least 84", not "6000".
        assert!(output.contains("work=22753.32"), "got: {output}");
        assert!(output.contains("tg>=   84.3"), "got: {output}");
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
        assert!(output.contains("ctx=   100/  4096"), "got: {output}");
    }

    #[test]
    fn test_format_compact_no_context() {
        let m = create_test_metrics();

        let output = format_compact(&m);
        assert!(output.contains("ctx=   n/a/   n/a"), "got: {output}");
    }

    /// An absent token count must read `n/a`, never a fabricated 0.
    #[test]
    fn absent_token_counts_print_n_a() {
        let mut m = create_test_metrics();
        m.prompt_tokens = None;
        m.completion_tokens = None;

        let compact = format_compact(&m);
        assert!(compact.contains("tok=   n/a/   n/a"), "got: {compact}");

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
        assert!(compact.contains("tok=     0/     0"), "got: {compact}");
        assert!(!compact.contains("tok=   n/a"), "got: {compact}");
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
        m.prompt_tps = Some(123.45); // Prompt processing TPS
        m.generation_tps = Some(150.00); // Generation TPS
        m.has_timing_split = true;
        m.finish_reason = "length".to_string();
        m.duration_ms = 600.0;

        let output = format_compact(&m);
        // pref/decode are k tok/s and raw tok/s respectively, so the raw figures
        // appear only through those two cells.
        assert!(output.contains("pp=   0.123k"), "got: {output}");
        assert!(output.contains("tg=  150.0"), "got: {output}");
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

        // The no-split branch has its own two value rows, and they carry longer
        // labels than the split rows. Same frame, same rule -- with an 11-char
        // `{:8.2}` value so the budget is proven at the wide end, not just for
        // the 7-char values a healthy server happens to produce.
        m.has_timing_split = false;
        // Rates must go with the flag: `tps_tier` is what selects these rows now,
        // so inherited rates would re-measure the split rows and leave the wide
        // floor/work labels unproven against the frame.
        m.prompt_tps = None;
        m.generation_tps = None;
        m.total_tps = 12345678.99;
        m.decode_tps_min = 12345678.99;
        for line in format_pretty(&m).lines() {
            let cells = UnicodeWidthStr::width(line);
            assert_eq!(cells, frame, "no-split row {line:?} spans {cells} cells");
        }

        // The third shape the box can take: a HALF split, where a measured prefill
        // row joins the floor branch. It carries the split rows' label width into
        // rows the frame test had never seen together.
        m.prompt_tps = Some(12345678.99);
        for line in format_pretty(&m).lines() {
            let cells = UnicodeWidthStr::width(line);
            assert_eq!(cells, frame, "half-split row {line:?} spans {cells} cells");
        }
    }

    /// With no backend split the box must name BOTH derived numbers for what
    /// they are. "Total throughput" was the label that let a cost rate read as
    /// engine speed on a wall of logs.
    #[test]
    fn pretty_no_split_names_both_numbers() {
        let mut m = create_test_metrics();
        m.has_timing_split = false;
        m.prompt_tps = None;
        m.generation_tps = None;
        m.total_tps = 6073.63;
        m.decode_tps_min = 146.32;

        let output = format_pretty(&m);
        assert!(output.contains("Decode (floor):"), "got: {output}");
        assert!(output.contains("146.32"), "floor value missing: {output}");
        assert!(output.contains("wall"), "floor must say it is wall-bounded: {output}");
        assert!(output.contains("Work rate:"), "got: {output}");
        assert!(output.contains("6073.63"), "cost rate missing: {output}");
        assert!(!output.contains("Total throughput"), "retired label is back: {output}");
        assert!(
            !output.contains("Prompt Processing"),
            "fabricated split row in the no-split box: {output}"
        );
    }

    /// Pretty is the DEFAULT format, so this box -- not the compact line -- is what
    /// most operators see. A half split must not render as a measured split: the old
    /// `has_timing_split` gate printed a `Prompt Processing` row beside
    /// `Generation: n/a` and then never entered the floor branch at all, so the
    /// default format had no bound on decode to show while an inflated prefill figure
    /// sat under a "measured" heading. The measured prefill stays (it is real); it
    /// just stops posing as a split.
    #[test]
    fn pretty_half_split_shows_the_floor_and_keeps_the_measured_prefill() {
        let mut m = create_test_metrics();
        m.has_timing_split = true; // as collector.rs:210 derives it from the prefill rate
        m.prompt_tps = Some(88_000.0);
        m.generation_tps = None;
        m.generation_ms = 0.0; // 0.0 == never reported, so it must print n/a too

        let output = format_pretty(&m);
        assert!(output.contains("Prompt Processing"), "prefill dropped: {output}");
        assert!(output.contains("88000"), "measured prefill value lost: {output}");
        assert!(output.contains("Decode (floor):"), "floor unreachable: {output}");
        assert!(output.contains("Work rate:"), "cost rate missing: {output}");
        assert!(
            !output.contains("Generation:"),
            "a half split rendered itself as a split: {output}"
        );
        assert!(
            !output.contains("(    0.0ms)"),
            "an unreported duration printed a bare zero instead of n/a: {output}"
        );
        assert!(
            output.contains("no backend decode rate"),
            "footer claimed no split at all, but prefill WAS measured: {output}"
        );
    }

    /// Serde picks up the new fields with no manual mapping. The cache counts
    /// are `Option`, so a backend that never mentions its cache stays absent
    /// instead of asserting "nothing was cached" -- while a measured zero still
    /// serializes.
    #[test]
    fn json_format_carries_the_floor_and_omits_absent_cache_fields() {
        let mut m = create_test_metrics();
        let parsed: serde_json::Value = serde_json::from_str(&format_json(&m)).unwrap();
        assert!(parsed.get("decode_tps_min").is_some(), "got: {parsed}");
        assert!(parsed.get("cached_tokens").is_none(), "fabricated zero: {parsed}");
        assert!(parsed.get("cache_creation_tokens").is_none(), "fabricated zero: {parsed}");

        m.cached_tokens = Some(0);
        let parsed: serde_json::Value = serde_json::from_str(&format_json(&m)).unwrap();
        assert_eq!(
            parsed.get("cached_tokens").and_then(|v| v.as_u64()),
            Some(0),
            "a measured zero is still a measurement"
        );
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

    // ---- P03/T2: the columnar contract Plan 01 never had a test for --------------
    //
    // Plan 01 declared the compact line columnar and shipped `contains` assertions,
    // which cannot fail for a ragged line. The property that actually matters is
    // stated here in TERMINAL DISPLAY CELLS (never bytes or `chars()`: the CJK
    // sample below has more bytes than cells, which is the same trap `{:<N}` sprang
    // on the pretty box), and it is stated as ABSOLUTE offsets rather than
    // sample-to-sample agreement -- a layout that is uniformly wrong agrees with
    // itself, so invariance alone is satisfiable and proves nothing.
    //
    // The widths are spelled out from the plan's T6 table instead of being read
    // back off `format_compact`: an expectation derived from the code under test can
    // only ever agree with it. The same table is recorded in
    // `context/plans/03_columnar_stats_honest_prefill_tracker.md`, so a T6 that
    // disagrees with these numbers has to change one of them on purpose, in the
    // open -- not quietly edit the test until it goes green.

    /// "model=" + `{:<16}`, truncated display-cell-wise via `truncate_to_width`.
    const CELL_MODEL: usize = 6 + 16;
    /// ONE width for all three rate tiers. Which tier you see is a fact about the
    /// backend, not about the request, so the line must not betray which it was.
    const CELL_RATE: usize = 23;
    /// "dur=" + `{:>7.0}` + "ms". Seven, not six: a reprompt loop against
    /// `timeout_seconds: 300` overflows six digits and re-breaks everything right.
    const CELL_DUR: usize = 4 + 7 + 2;
    /// "ctx=" + `{:>6}` + "/" + `{:>6}`; an absent side renders `n/a` in 6 cells.
    const CELL_CTX: usize = 4 + 6 + 1 + 6;
    /// "tok=" + `{:>6}` + "/" + `{:>6}`.
    const CELL_TOK: usize = 4 + 6 + 1 + 6;
    /// "stream=" + `{:<7}` -- `stalled` is the widest badge at 7.
    const CELL_STREAM: usize = 7 + 7;
    /// "finish=" + `{:<10}`; `tool_calls` fills it exactly. `content_filter` (14)
    /// overflows BY DESIGN -- a diagnostic field is never silently clamped -- which
    /// is exactly why the invariant pinned below is the START of `finish=`, never
    /// its end.
    const CELL_FINISH: usize = 7 + 10;
    /// Head cells are joined by exactly one space.
    const SEP: usize = 1;

    /// The head, in order, as (label to probe, display width). The rate cell is
    /// probed by POSITION, not by label: tier 1 renders `pp=`/`tg=`, tier 2 `tg>=`
    /// (`work=` is a tail cell, not part of the group), tier 3 `n/a` for both, so the
    /// group has no stable label -- and keeping a 2-char needle out of a line whose
    /// leading field is free-form backend text removes a whole class of probe
    /// false-positives.
    const HEAD: [(&str, usize); 7] = [
        ("model=", CELL_MODEL),
        ("", CELL_RATE),
        ("dur=", CELL_DUR),
        ("ctx=", CELL_CTX),
        ("tok=", CELL_TOK),
        ("stream=", CELL_STREAM),
        ("finish=", CELL_FINISH),
    ];

    /// Total head width, DERIVED from the cells above so the total can never
    /// silently disagree with its own parts.
    const HEAD_CELLS: usize = {
        let mut total = 0;
        let mut i = 0;
        while i < HEAD.len() {
            total += HEAD[i].1;
            if i + 1 < HEAD.len() {
                total += SEP;
            }
            i += 1;
        }
        total
    };

    /// Where each head label must begin, derived from the cell widths.
    fn head_offsets() -> Vec<(&'static str, usize)> {
        let mut out = Vec::new();
        let mut cell = 0usize;
        for (i, (label, width)) in HEAD.iter().enumerate() {
            if !label.is_empty() {
                out.push((*label, cell));
            }
            cell += width;
            if i + 1 < HEAD.len() {
                cell += SEP;
            }
        }
        out
    }

    /// Where a head label was found. Absence and ambiguity are VALUES, not panics:
    /// the contract test collects every mismatch before failing, so a panic here
    /// would destroy the report that makes it useful.
    #[derive(Debug)]
    enum Probe {
        At(usize),
        Absent,
        Ambiguous(usize),
    }

    /// Display-cell offset of `needle`. Bytes would lie: the CJK sample carries more
    /// bytes than cells, the same trap `{:<N}` sprang on the pretty box. A duplicate
    /// needle is `Ambiguous` rather than its first hit, because the free-form
    /// `model=` field can contain any needle and would masquerade as a formatter bug.
    fn probe(line: &str, needle: &str) -> Probe {
        let mut hits = line.match_indices(needle);
        let Some(first) = hits.next() else {
            return Probe::Absent;
        };
        if hits.next().is_some() {
            let n = line.match_indices(needle).count();
            return Probe::Ambiguous(n);
        }
        Probe::At(UnicodeWidthStr::width(&line[..first.0]))
    }

    /// Cells the rate group occupies, derived from where `dur=` actually landed.
    ///
    /// The rate cell is bracketed by TWO separators -- one after the model cell, one
    /// before `dur=` -- so both must come off: `47 - (22 + 2*1) == CELL_RATE`. Shared
    /// by the contract test and the golden control, so this derivation cannot rot on
    /// its own the way a copy in one place did (a single-separator version demanded
    /// `dur=` at 46 while the offsets table demands 47, which no formatter can satisfy).
    fn rate_group_cells(dur_at: usize) -> usize {
        dur_at.saturating_sub(CELL_MODEL + 2 * SEP)
    }

    /// The offsets are pinned over a MAGNITUDE sweep, not only the fixture rows. The
    /// bug class this exists for is a field that fits every healthy value and then grows
    /// one cell at a boundary, moving `dur=`..`finish=` with it -- a wall-bounded floor
    /// of 10000.0 rendered `tg>= 10000.0`, a 24-cell rate group with `dur=` at 48. The
    /// sweep runs to the documented ceilings: prefill 580,000 tok/s on a full
    /// prefix-cache hit, and a decode floor below 100k, past which no engine decodes.
    #[test]
    fn head_offsets_survive_every_rate_magnitude() {
        let mut rows: Vec<(String, RequestMetrics)> = Vec::new();

        for floor in [0.07, 41.67, 999.9, 9_999.9, 9_999.95, 10_000.0, 99_999.9] {
            let mut m = fixture("qwen3", "stop");
            m.completion_tokens = Some(10);
            m.decode_tps_min = floor;
            rows.push((format!("tier2 floor {floor}"), m));
        }
        for prefill in [1.0, 200.5, 14_765.0, 580_000.0, 1_000_000.0, 9_999_999.0] {
            let mut m = fixture("qwen3", "stop");
            m.has_timing_split = true;
            m.prompt_tps = Some(prefill);
            m.generation_tps = Some(42.5);
            rows.push((format!("tier1 prefill {prefill}"), m));
        }
        let mut quiet = fixture("qwen3", "stop");
        quiet.completion_tokens = None;
        quiet.decode_tps_min = 0.0;
        rows.push(("tier3 no counted output".to_string(), quiet));

        for (label, m) in &rows {
            let line = format_compact(m);
            for (needle, want) in head_offsets() {
                match probe(&line, needle) {
                    Probe::At(at) => assert_eq!(at, want, "{label}: `{needle}` at {at}: {line}"),
                    other => panic!("{label}: `{needle}` {other:?} -- {line}"),
                }
            }
        }
    }

    /// `dur` is the one cell that saturates, and the one value that must NOT saturate: a
    /// NaN wall clock is the absence of a measurement, not a 2.8h request. Both branches
    /// must land in 7 cells or `dur=` shifts during the exact incident an operator is
    /// reading the line for -- a wedged backend is the only way to reach this branch.
    #[test]
    fn a_non_finite_or_saturated_wall_keeps_the_dur_cell_at_seven_cells() {
        for (label, wall) in [
            ("finite", 3_144.0),
            ("saturated", 9.9e12),
            ("nan", f64::NAN),
            ("inf", f64::INFINITY),
        ] {
            let mut m = fixture("qwen3", "stop");
            m.duration_ms = wall;
            let line = format_compact(&m);
            for (needle, want) in head_offsets() {
                match probe(&line, needle) {
                    Probe::At(at) => {
                        assert_eq!(at, want, "{label} wall {wall}: `{needle}` at {at}: {line}")
                    }
                    other => panic!("{label} wall {wall}: `{needle}` {other:?} -- {line}"),
                }
            }
        }

        let mut m = fixture("qwen3", "stop");
        m.duration_ms = f64::NAN;
        assert!(
            !format_compact(&m).contains("9999999"),
            "NaN printed as the saturation sentinel"
        );
    }

    /// Backend text is bounded in BYTES, not only in display cells. A zero-width run is
    /// zero cells, so a cell bound alone lets `"\u{200b}" * 100_000` as the echoed model
    /// name -- or a megabyte of `finish_reason` -- through into a single log record.
    #[test]
    fn backend_text_cannot_flood_the_line() {
        let flood = "\u{200b}".repeat(100_000);

        let mut m = fixture("qwen3", "stop");
        m.model = flood.clone();
        let line = format_compact(&m);
        assert!(
            line.chars().count() < 400,
            "model flood leaked {} chars",
            line.chars().count()
        );

        let m = fixture("qwen3", &flood);
        let line = format_compact(&m);
        assert!(
            line.chars().count() < 400,
            "finish flood leaked {} chars",
            line.chars().count()
        );

        let m = fixture("qwen3", &"a".repeat(100_000));
        let line = format_compact(&m);
        for (needle, want) in head_offsets() {
            match probe(&line, needle) {
                Probe::At(at) => assert_eq!(at, want, "long finish: {line}"),
                other => panic!("long finish: `{needle}` {other:?} -- {line}"),
            }
        }
    }

    fn fixture(model: &str, finish_reason: &str) -> RequestMetrics {
        let mut m = RequestMetrics::new();
        m.model = model.to_string();
        m.finish_reason = finish_reason.to_string();
        m
    }

    /// The five rows the contract is checked against. Each carries SEVERAL
    /// adversarial attributes at once, because the property is invariance -- one
    /// line can stress four columns -- and every column has both a narrowest and a
    /// widest representative somewhere in the set (`the_contract_fixture_discriminates`
    /// fails loudly if that ever stops being true).
    ///
    /// `finish_reason` is deliberately hostile in two rows: it is verbatim,
    /// unsanitized backend text (`collector.rs:528`, and Anthropic's `stop_reason`
    /// at `:546`), so a newline in it does not shift a column, it splits the log
    /// line in two. The InfluxDB exporter already sanitizes `"stop\nforged 2"`; the
    /// compact line must not be the last place that learns.
    fn contract_fixtures() -> Vec<(&'static str, RequestMetrics)> {
        let mut rows: Vec<(&'static str, RequestMetrics)> = Vec::new();

        // S1 -- narrowest everything: short ASCII model, no context, no token
        // counts, no optional cells, non-streaming. This is the row the head must
        // not collapse on.
        let mut s1 = fixture("qwen3-7b", "stop");
        s1.has_timing_split = true;
        s1.prompt_tps = Some(53.64);
        s1.generation_tps = Some(42.5);
        s1.duration_ms = 184.0;
        // LOAD-BEARING: the only row exercising the non-streaming shape, which renders
        // `stream=sync` padded into the 7-cell badge field. Do NOT flip this to `true`
        // to silence a red: it is the cheapest edit in the file and would leave
        // CELL_STREAM justified by `stalled` alone.
        s1.streaming = false;
        rows.push(("s1-ascii-min", s1));

        // S2 -- widest IN-BAND everything: a 40-cell model name (forces truncation),
        // 6-digit rates, a 7-digit wall, 6-digit counts (the ceiling of the pinned
        // `{:>6}` fields -- a 1M-token window is out of band and gets its own witness
        // test below), every optional cell present, the longest badge, and an
        // overflowing finish value.
        let mut s2 = fixture("Qwen3-14B-128K-Q3_K_S.gguf-batch-instruct-realigned", "content_filter");
        s2.has_timing_split = true;
        s2.prompt_tps = Some(88_000.0);
        s2.generation_tps = Some(9_999.99);
        s2.duration_ms = 1_234_567.0;
        s2.context_used = Some(999_999);
        s2.context_total = Some(999_999);
        s2.prompt_tokens = Some(999_999);
        s2.completion_tokens = Some(999_999);
        s2.group_name = Some("heavy".to_string());
        s2.cached_tokens = Some(182_400);
        s2.queue_ms = Some(1_234.0);
        s2.concurrent_requests = Some(7);
        s2.streaming = true;
        s2.stream_end = Some("stalled");
        rows.push(("s2-everything-max", s2));

        // S3 -- wide glyphs in BOTH the model and the finish reason, plus the
        // narrowest numbers: padding must be counted in display cells, not chars,
        // in every cell that can carry text.
        let mut s3 = fixture("本地大模型-Qwen3-14B-量化版", "停止done");
        s3.decode_tps_min = 146.32;
        s3.total_tps = 125.0;
        s3.duration_ms = 9.99;
        s3.context_used = Some(1);
        s3.context_total = Some(2_048);
        s3.prompt_tokens = Some(0);
        s3.completion_tokens = Some(0);
        s3.streaming = true;
        s3.stream_end = Some("truncated");
        rows.push(("s3-cjk-tiny", s3));

        // S4 -- the long-context agent turn: a cost rate in the six digits while
        // decode crawls. `cache=` present with `queue=` absent, because an
        // optional cell must never move a head column.
        let mut s4 = fixture("cosmo-6000", "length");
        s4.decode_tps_min = 0.07;
        s4.total_tps = 125_000.0;
        s4.duration_ms = 45_000.0;
        s4.context_used = Some(100_000);
        s4.context_total = Some(262_144);
        s4.prompt_tokens = Some(123_456);
        s4.completion_tokens = Some(7);
        s4.cached_tokens = Some(51_200);
        s4.streaming = true;
        s4.stream_end = Some("ok");
        rows.push(("s4-long-context-cost", s4));

        // S5 -- no counted output at all, one-char model, an absent completion
        // count beside a present prompt count, and a finish value carrying a
        // newline and a space.
        let mut s5 = fixture("a", "stop\nforged 2");
        s5.total_tps = 125.0;
        s5.duration_ms = 0.0;
        s5.prompt_tokens = Some(4_096);
        s5.completion_tokens = None;
        s5.concurrent_requests = Some(1);
        s5.streaming = true;
        s5.stream_end = Some("client_gone");
        rows.push(("s5-no-output-hostile", s5));

        rows
    }

    /// The head columns must be occupied by values that actually differ in width,
    /// or the contract test below is a no-op wearing a costume: a fixture where all
    /// five rows land in the same digit bucket tests nothing, which is Plan 01's
    /// `contains` bug one level up. Runs today, on purpose -- it is what stops a
    /// later edit of `123456` down to `123` from silently deleting the test's power.
    ///
    /// It inspects the INPUT fields, never rendered strings, so it cannot be
    /// satisfied by adjusting an expected value.
    #[test]
    fn the_contract_fixture_discriminates() {
        let rows = contract_fixtures();
        assert!(rows.len() >= 4, "the contract needs >=4 samples");

        let distinct = |vals: Vec<usize>| {
            let mut v = vals;
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        let digits = |v: Option<u64>| v.map_or(0, |n| n.to_string().len());
        let wall_digits = |m: &RequestMetrics| m.duration_ms.max(0.0) as u64;

        let models: Vec<usize> = rows.iter().map(|(_, m)| UnicodeWidthStr::width(m.model.as_str())).collect();
        let walls: Vec<usize> = rows.iter().map(|(_, m)| digits(Some(wall_digits(m)))).collect();
        let ctx_used: Vec<usize> = rows.iter().map(|(_, m)| digits(m.context_used)).collect();
        let ctx_total: Vec<usize> = rows.iter().map(|(_, m)| digits(m.context_total)).collect();
        let prompt: Vec<usize> = rows.iter().map(|(_, m)| digits(m.prompt_tokens)).collect();
        let completion: Vec<usize> = rows.iter().map(|(_, m)| digits(m.completion_tokens)).collect();
        let tiers: Vec<usize> = rows
            .iter()
            // Must be the renderer's own predicate: bucketing these rows by
            // `has_timing_split` let "all three tiers appear" pass on a fixture the
            // formatter would render as two tiers.
            .map(|(_, m)| tps_tier(m) as usize)
            .collect();
        let optionals: Vec<usize> = rows
            .iter()
            .map(|(_, m)| {
                usize::from(m.group_name.is_some())
                    + usize::from(m.cached_tokens.is_some())
                    + usize::from(m.queue_ms.is_some())
                    + usize::from(m.concurrent_requests.is_some())
            })
            .collect();

        for (column, widths) in [
            ("model", &models),
            ("dur", &walls),
            ("ctx used", &ctx_used),
            ("ctx total", &ctx_total),
            ("tok prompt", &prompt),
            ("tok completion", &completion),
            ("tier", &tiers),
            ("optional-cell presence", &optionals),
        ] {
            assert!(
                distinct(widths.clone()) >= 2,
                "column `{column}` never varies across the fixture ({widths:?}): the contract \
                 test would pass without exercising it"
            );
        }

        // The three tiers must all be represented: which tier renders is a fact
        // about the backend, and the head must look identical whichever it was.
        assert_eq!(distinct(tiers), 3, "all three rate tiers must appear in the fixture");

        // No two rows may be twins, or one row is carrying another's coverage.
        let vectors: Vec<(Vec<usize>, Vec<usize>)> = rows
            .iter()
            .map(|(_, m)| {
                (
                    vec![UnicodeWidthStr::width(m.model.as_str())],
                    vec![
                        digits(Some(wall_digits(m))),
                        digits(m.context_used),
                        digits(m.context_total),
                        digits(m.prompt_tokens),
                        digits(m.completion_tokens),
                    ],
                )
            })
            .collect();
        for i in 0..vectors.len() {
            for j in (i + 1)..vectors.len() {
                assert_ne!(
                    vectors[i], vectors[j],
                    "samples {} and {} are width-twins and add no coverage",
                    rows[i].0, rows[j].0
                );
            }
        }
    }

    /// The constants must describe one coherent head, not seven hand-typed numbers
    /// that drifted apart.
    #[test]
    fn head_constants_are_self_consistent() {
        let offsets = head_offsets();
        assert_eq!(
            offsets.first().map(|(_, o)| *o),
            Some(0),
            "the head starts with the model cell"
        );
        assert_eq!(
            offsets.last().map(|(_, o)| *o + CELL_FINISH),
            Some(HEAD_CELLS),
            "`finish=` is the last head cell, so its end is the head width"
        );
        assert_eq!(
            HEAD_CELLS, 129,
            "the head is 123 cells of content plus 6 separators; if this moves, \
             update the table in the plan and the tracker together, never here alone"
        );
    }

    /// Positive control for the table above, and the reason the contract's red means
    /// anything: with every single assertion red, "the formatter is ragged" and "the
    /// expected offsets are simply wrong" look identical, and a permanently-red
    /// contract test gets reconciled by editing its constants. These two literals are
    /// typed out by hand and asserted against `head_offsets()` -- if the table is wrong,
    /// THIS fails today.
    ///
    /// It is a control on the OFFSETS TABLE, not a sample of the output: the rate group
    /// here is the plan table's shape (`{:>9.3}k` / `{:>6.1}`) while the formatter emits
    /// `{:>8.3}k` / `{:>7.1}`. Both total `CELL_RATE` and only the total is asserted, so
    /// do not paste this line into a ticket as an example of what the proxy prints.
    ///
    /// `GOLDEN_CJK` is why the head must be padded in DISPLAY CELLS: its model field
    /// is 8 chars but 16 cells, so `{:<16}` char padding puts every column after it
    /// eight cells out.
    const GOLDEN_ASCII: &str = "model=qwen3-7b         pp=   88.000k tg=9999.9 dur=    184ms ctx=999999/262144 tok=186417/   498 stream=ok      finish=stop      ";

    const GOLDEN_CJK: &str = "model=本地大模型量化版 pp=   88.000k tg=9999.9 dur=    184ms ctx=999999/262144 tok=186417/   498 stream=ok      finish=stop      ";

    #[test]
    fn the_head_offsets_describe_a_buildable_line() {
        for (tag, golden) in [("ascii", GOLDEN_ASCII), ("cjk", GOLDEN_CJK)] {
            assert_eq!(
                UnicodeWidthStr::width(golden),
                HEAD_CELLS,
                "{tag} golden is not the pinned head width"
            );
            for (label, want) in head_offsets() {
                match probe(golden, label) {
                    Probe::At(got) => assert_eq!(got, want, "{tag} golden violates `{label}`"),
                    other => panic!("{tag} golden: `{label}` not uniquely found: {other:?}"),
                }
            }

            // A derivation only the ragged contract test runs is an unexercised
            // assertion -- that is how a one-cell error here survived a fully-red run.
            let found = probe(golden, "dur=");
            let Probe::At(dur_at) = found else {
                panic!("{tag} golden: `dur=` not uniquely found: {found:?}")
            };
            assert_eq!(
                rate_group_cells(dur_at),
                CELL_RATE,
                "{tag} golden: rate-group derivation disagrees with CELL_RATE"
            );
        }
    }

    /// A 1M-token vLLM window (`1048576`) and a prompt of similar size do not fit the
    /// `{:>6}` fields the plan pins, so the head cannot stay aligned for them. That
    /// widening (2 cells of head, against OQ7's width concern) is a human decision, not
    /// this test's. What IS pinned here, and stays pinned however the width question is
    /// answered: an out-of-band number shifts the columns rather than being clamped,
    /// rounded or dropped.
    #[test]
    fn an_over_wide_context_is_never_clamped() {
        let mut m = fixture("cosmo-6000", "stop");
        m.has_timing_split = true;
        m.prompt_tps = Some(13_619.0);
        m.generation_tps = Some(146.2);
        m.duration_ms = 3_144.0;
        m.context_used = Some(1_048_576);
        m.context_total = Some(1_048_576);
        m.prompt_tokens = Some(1_000_000);
        m.completion_tokens = Some(4_096);
        m.streaming = true;
        m.stream_end = Some("ok");

        let line = format_compact(&m);
        for value in ["1048576", "1000000"] {
            assert!(
                line.contains(value),
                "`{value}` must reach the log verbatim rather than be clamped to its field: {line:?}"
            );
        }
    }

    /// The contract Plan 01 shipped without. Its 34 red cells went to zero when
    /// `format_compact` grew the pinned head; it runs in the normal suite now.
    #[test]
    fn compact_head_columns_are_position_invariant() {
        let rows = contract_fixtures();
        let offsets = head_offsets();
        let mut bad: Vec<String> = Vec::new();

        let mut tails: Vec<String> = Vec::new();

        for (name, m) in &rows {
            let line = format_compact(m);

            match probe(&line, "model=") {
                Probe::At(0) => {}
                Probe::At(off) => bad.push(format!("{name}: `model=` must start the line at cell 0, found {off}")),
                Probe::Absent => bad.push(format!("{name}: `model=` absent from {line:?}")),
                Probe::Ambiguous(n) => bad.push(format!(
                    "{name}: `model=` occurs {n} times, so no column can be attributed to it  ({line:?})"
                )),
            }

            for (label, want) in &offsets {
                match probe(&line, label) {
                    Probe::At(got) if got == *want => {}
                    Probe::At(got) => bad.push(format!("{name}: `{label}` starts at cell {got}, expected {want}  ({line:?})")),
                    Probe::Absent => bad.push(format!(
                        "{name}: `{label}` ABSENT -- the head cell is missing, not merely misaligned  ({line:?})"
                    )),
                    Probe::Ambiguous(n) => bad.push(format!(
                        "{name}: `{label}` occurs {n} times; the probe cannot attribute it to a column  ({line:?})"
                    )),
                }
            }

            // The rate group is probed by position, since no label is common to all
            // three tiers. Until the model cell is padded this also absorbs the
            // model's shortfall: it restates the `dur=` assertion above rather than
            // measuring the rate cell independently.
            if let Probe::At(dur) = probe(&line, "dur=") {
                let occupied = rate_group_cells(dur);
                if occupied != CELL_RATE {
                    bad.push(format!(
                        "{name}: the rate group occupies {occupied} cells, expected {CELL_RATE}  ({line:?})"
                    ));
                }
            }

            // Absolute head offsets are blind to everything after `finish=`, so the
            // whole-line properties an operator actually consumes are asserted here:
            // a newline in a backend-supplied field splits one log record into two, a
            // tab expands to a tab stop no width computation can predict, and
            // trailing padding is invisible to every offset above.
            if let Some(ch) = line.chars().find(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')) {
                bad.push(format!(
                    "{name}: control character {ch:?} inside a single-line log record  ({line:?})"
                ));
            }
            if line.ends_with(' ') {
                bad.push(format!(
                    "{name}: trailing whitespace, which breaks awk/column consumption  ({line:?})"
                ));
            }

            // Non-degeneracy: the head is padded identical by design after T6, so the
            // samples prove they differ through the FREE-FORM tail.
            let tail = match line.find("finish=") {
                Some(at) => line[at + "finish=".len()..].to_string(),
                None => line.clone(),
            };
            tails.push(tail);
        }

        let mut unique_tails = tails.clone();
        unique_tails.sort();
        unique_tails.dedup();
        assert!(
            unique_tails.len() == rows.len(),
            "only {} of {} samples rendered a distinct tail -- the fixture is uniform and \
             the contract proves nothing: {tails:?}",
            unique_tails.len(),
            rows.len()
        );

        assert!(
            bad.is_empty(),
            "the compact head is not columnar ({} cell(s) wrong):\n{}",
            bad.len(),
            bad.join("\n")
        );
    }
}
