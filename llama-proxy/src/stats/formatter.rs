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
/// rate overflows this cell -- llama.cpp's cache-inflated figure was recorded raw at
/// 880,447.55 tok/s (since corrected at the source). That is a PRE-EXISTING defect of `tps={:>6.2}/{:>6.2}`, not something this
/// introduced; T6 replaces it with the k-unit form pinned by the golden. Never clamp
/// silently here: a fabricated ceiling on a measured rate is the exact sin this plan
/// exists to prevent.
///
/// The compact line's two rate cells DO clamp (`rate_group_with_clamps`), and that
/// asymmetry with this box is deliberate, not an oversight waiting to be "fixed" toward
/// either side: the compact line's cells carry a declared, human-set supported maximum,
/// every bound value is counted and WARNed, and the raw figure still reaches `json`,
/// InfluxDB and this box. Here nothing is declared and nothing is reported, so clamping
/// would be silent by construction. Operative word: silently.
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

// The rate group's geometry lives here as consts; the reasoning is documented on
// `rate_group_with_clamps`, which is the single site that decides rendering and clamping.

/// The decode value field, in cells, on the tier that prints one: `{:>5.1}`. Both tiers
/// that print a decode rate share this ONE field, which is what makes their ceilings the
/// same number — see [`TG_CELL_CEILING_TPS`].
const DECODE_VALUE_CELLS: usize = 5;
/// The prefill value field, `{:>5.2}k`, so 5 digits plus the unit. Prefill digits print on
/// tier 1 only, but the width is shared with the `n/a` placeholders so every group is one
/// width.
const PP_VALUE_CELLS: usize = 6;

/// What the decode cell **prints when it clamps**: `999.9`, the 5-cell rendering of the 1,000.0
/// fit boundary (`1000.0` is 6 glyphs). Values under 999.95 display identically. The human's
/// stated supported maximum for this deployment, sized at ~4x the highest decode this repo
/// has ever recorded — 247.2 tok/s, from an archived `tps=…/247.16` line; the six
/// `captures/vllm_metrics_*.json` read 10.9–99.2. Note that is TIER-1 evidence: 247.2 is
/// engine-measured decode, while this cell also carries tier 2's DERIVED floor, whose
/// recorded values are orders of magnitude larger (see `rate_group_with_clamps`). Past the
/// ceiling the compact line prints 999.9 and the report says so; the true figure is in
/// `json`, the pretty box and InfluxDB.
const TG_CELL_CEILING_TPS: f64 = 999.9;
/// What the `pp` cell **prints when it clamps**: `{:>5.2}k` of 99,990, and the fit boundary is
/// 99,995 (`99.99` is 5 wide; 99,995 rounds to `100.00`, 6). Values inside the rounding band
/// already display as `99.99k` and are NOT counted as clamps. A statement about THIS deployment (its operator set the
/// supported maximum), NOT about vLLM. The big prefill figures in this repo's records are
/// not the yardstick: 880,447.55 (164,663 prompt tokens, 164,096 of them cached, ≈290x
/// inflation, logged before the correction existed) and the 579,685.47 of the 186,417-token
/// capture (182,400 cached; honest figure 12,491, `collector.rs:569-571`) are BOTH raw and
/// cache-inflated, and the collector corrects both now. Warm CORRECTED prefill here runs
/// 12,770-13,066 (mean 12,858 across four warm repeats of the identical 9074-token request; one
/// T4 live warm run measured 14,257.54). What legitimately reaches past this ceiling is the FULL
/// prefix-cache-hit class, which `collector.rs:595-611` leaves uncorrected by design -- so that class is
/// unbounded and has no recorded maximum (no capture in this repo is a full hit). An
/// off-scale `pp` on a big-context warm server is therefore a legitimate state, not a bug.
/// That trade was made deliberately for a 1-cell shorter line; see `rate_group`.
const PP_CELL_CEILING_TPS: f64 = 99_990.0;

/// One rate the compact line could not print as given: what it received, what it printed,
/// and which cell. Produced by [`rate_group_with_clamps`] and consumed by the exporter
/// layer, which counts and WARNs; `format_compact` itself discards these.
#[derive(Debug, Clone, PartialEq)]
pub struct ClampReport {
    /// `"tg"` (decode) or `"pp"` (prefill) — the cell that bounded the value.
    pub cell: &'static str,
    /// The rate the collector handed over.
    pub actual: f64,
    /// The rate the cell printed instead.
    pub printed: f64,
    /// Tier the line rendered in: 1 = backend-measured, 2 = the wall-bounded floor.
    pub tier: u8,
}

/// Render one rate for its cell, substituting the cell's ceiling rendering when the value
/// would not fit and reporting it. The predicate is **the rendering changed**, not
/// `v > ceiling`: in `(999.9, 1000.0)` the true value already displays as `999.9`, and
/// counting those would inflate the counter with no misrepresentation behind it. So the
/// counter means exactly "the line shows a number you were not given".
///
/// Non-finite values pass through unrendered-as-numbers (`inf`, `NaN`) rather than being
/// replaced by a ceiling: this is `format_compact`'s `dur` rule, and `f64::min` returns
/// the OTHER operand when one is NaN, so a min-based clamp would print the ceiling for a
/// value that is not a quantity at all.
fn fit_rate(
    v: f64,
    ceiling: f64,
    cells: usize,
    render: impl Fn(f64) -> String,
    cell: &'static str,
    tier: u8,
    out: &mut Vec<ClampReport>,
) -> String {
    if !v.is_finite() {
        return render(v);
    }
    let direct = render(v);
    // Numeric renderings are ASCII (digits, `.`, `-`, `inf`, `NaN`), so bytes are cells.
    if direct.len() <= cells {
        return direct;
    }
    out.push(ClampReport {
        cell,
        actual: v,
        printed: ceiling,
        tier,
    });
    render(ceiling)
}

fn render_decode(v: f64) -> String {
    format!("{:>5.1}", v)
}

fn render_prefill(v: f64) -> String {
    format!("{:>5.2}k", v / 1000.0)
}

/// The rate group at the width it rendered, clamps applied. This wrapper exists so the
/// ~40 rendering tests probe the group without threading a report vector; **the clamp
/// decision itself lives in one place**, [`rate_group_with_clamps`], and a caller that
/// needs to know whether a number was bounded must use that one.
pub fn rate_group(m: &RequestMetrics) -> String {
    rate_group_with_clamps(m).0
}

/// The rate group, plus any rate it bounded. One `tps_tier` branch decides rendering and
/// reporting together: a separate predicate reading `RequestMetrics` directly would report
/// a `pp` clamp for a tier-2 request carrying `prompt_tps = Some(500_000)`, where the
/// renderer prints `pp=   n/a` and nothing was bounded — an inflated counter and a WARN
/// describing a line that does not exist.
///
/// **Geometry.** ONE legal width for all three tiers — `CELL_RATE`, 18 cells — reached by
/// construction, never by counting spaces (a hand-spaced tier literal drifts by a cell
/// every time someone types it). Which tier you see is a fact about the backend **and the
/// endpoint shape**, not the request: an Anthropic `/v1/messages` body carries no
/// per-request timing block at all, so it is tier 2 whatever backend serves it, and the
/// line must not betray which tier it rendered.
///
/// **Decode leads**: it is the number an operator scans a wall of lines for, so it is the
/// head's first glyph in every tier. What is shared across tiers is the DECODE VALUE FIELD
/// — `{:>5.1}`, 5 cells, wherever a decode rate prints — not the cell around it: `tg=`
/// (3) makes tier 1's side 8 cells, `tg>=` (4) makes tier 2's 9, and tier 2's constant `pp`
/// placeholder is 8 to tier 1's 9 so the totals agree. The consequences of sharing the
/// value field instead of the cell: both tiers reach the same **999.9** ceiling, decode
/// digits end at cell 8 on tier 1 and 9 on tier 2 (the cross-tier digit stack this file
/// used to guarantee is gone, by decision), `pp=` starts one cell further right on tier 2
/// (nothing probes it positionally — the contract carries the group), and no whitespace
/// field index survives the group: `tg=376.7` is one token, `tg= 51.1` is two, so field
/// count depends on the value's magnitude. Extraction from `dur=` onward still works;
/// inside the group it is matched by pattern, never sliced.
///
/// **Two ceilings, deliberately not provisioned alike.** `pp` is `{:>5.2}k` and stops at
/// **99,990 tok/s**. That is far below the large prefill figures this repo's logs still
/// show — 880,447.55 and 579,685.47 — but both of those are **raw and cache-inflated**, and
/// the collector now corrects them: 880,447.55 is 164,663 prompt tokens with 164,096 cached
/// (≈290x inflation, Plan 01), and 579,685.47 is the archived raw rate of 186,417 tokens at a
/// 321.6 ms ttft with 182,400 cached (honest figure 12,491, `collector.rs:569-571`; the literal is
/// a fixture at `collector.rs:1926`). Neither sizes the ceiling. Warm
/// corrected prefill on this deployment runs 12,770–13,066 (mean 12,858 across four warm
/// repeats; one T4 live warm run measured 14,257.54). The class that legitimately
/// reaches past the ceiling is the **full** prefix-cache hit: `cached == prompt_tokens` is
/// left uncorrected by design (`collector.rs:595-611`), so that class is unbounded and has
/// **no recorded maximum** — no capture in this repo is a full hit. A `pp` clamp is therefore
/// expected on full-cache turns and is a display-space meter, not a fault meter; it is
/// documented as a claim about THIS deployment, not about vLLM. Decode's ceiling is ~4x the
/// highest decode ever recorded (247.2 tok/s, an archived `tps=…/247.16` — tier-1 evidence
/// for a cell that also carries tier 2's derived floor). The tighter cell still lands on the
/// less trustworthy input: `decode_tps_min` is derived (`completion / wall`) and `finite_rate`
/// (`collector.rs:379-385`) passes every finite value through, so a wall containing no
/// generation time produces an enormous floor — a canned test backend rendered `dur=1ms`
/// beside 412 counted tokens, i.e. a wall under 1.5 ms and a floor of roughly 3–8x10⁵ tok/s,
/// some 300–800x past this ceiling. Such a sample is already absurd in `total_tps` (the
/// free-form `work=` tail cell) and in `json`; the source-side plausibility bound is filed at
/// `collector.rs:241`.
///
/// **Past a ceiling the group no longer widens — it clamps and reports.** That reverses this
/// file's previous rule (`a_rate_past_a_cells_field_widens_rather_than_clamps`, whose name is
/// now `rates_past_their_ceiling_clamp_and_report`), and the narrower claim that survives is:
/// the RATE GROUP is bounded, while `ctx=`, `tok=` and `finish=` can still widen the head.
/// Clamping is a human-set, declared maximum on the DISPLAY cell only: `json`, the pretty box
/// and every exporter see the raw value. `ctx=`, `tok=` and `finish=` still widen rather than
/// saturate (`an_over_wide_context_is_never_clamped`), because those are counts and
/// diagnostics, not rates with a stated supported range. Reconcile against `tps_side`: the
/// pretty box still never clamps, and that asymmetry is intentional, not an oversight.
pub fn rate_group_with_clamps(m: &RequestMetrics) -> (String, Vec<ClampReport>) {
    let mut clamps: Vec<ClampReport> = Vec::new();
    // `pp` prints ONLY on tier 1. A half split (backend-reported prefill, no usable decode
    // rate) is tier 2, and printing its cache-inflated prefill there put a huge figure in
    // the column the eye lands on while the column that matters stayed empty — the exact
    // failure `tps_tier` exists to prevent. Absent, not zero. Its `n/a` placeholder is
    // 8 cells to tier 1's 9, because tier 2's `tg>=` label is one cell wider than `tg=`
    // and the placeholder absorbs the difference: that is how all three groups reach
    // `CELL_RATE` (18) with a decode value field that is 5 cells wherever a rate prints.
    let group = match tps_tier(m) {
        1 => {
            let decode = fit_rate(
                m.generation_tps.unwrap_or(0.0),
                TG_CELL_CEILING_TPS,
                DECODE_VALUE_CELLS,
                render_decode,
                "tg",
                1,
                &mut clamps,
            );
            let prefill = fit_rate(
                m.prompt_tps.unwrap_or(0.0),
                PP_CELL_CEILING_TPS,
                PP_VALUE_CELLS,
                render_prefill,
                "pp",
                1,
                &mut clamps,
            );
            format!("tg={} pp={}", decode, prefill)
        }
        2 => {
            // Same 5 value cells as tier 1, so the same 999.9 ceiling, and the `>=` glyph
            // that carries the tier tag is what pushes this side to 9 cells and `pp=` one
            // cell right of where tier 1 puts it. Nothing probes `pp=` positionally (the
            // contract carries the GROUP, `("", CELL_RATE)`); README says to slice the
            // group, not the field.
            let floor = fit_rate(
                m.decode_tps_min,
                TG_CELL_CEILING_TPS,
                DECODE_VALUE_CELLS,
                render_decode,
                "tg",
                2,
                &mut clamps,
            );
            format!("tg>={} pp={:>5}", floor, "n/a")
        }
        // Formatted rather than spelled, so the widths are arithmetic: 3+5, a separator,
        // then 3+6. The sweep's tier-3 row measures the result either way.
        3 => format!("tg={:>5} pp={:>6}", "n/a", "n/a"),
        _ => unreachable!("tps_tier returns 1..=3"),
    };
    (group, clamps)
}

/// Compact single-line format: a pinned-width head followed by a free-form tail.
/// Head order is **rate group, dur, ctx, tok, model, stream, finish** — measured
/// quantities left, identity and diagnostics right, so the cell an operator scans for
/// (decode tok/s) is the head's first glyph. `model=` is the only free-form cell here;
/// everything to its left is therefore immune to a wide-glyph or over-long name.
/// A cell is columnar only if its width is constant, so no conditional cell may sit
/// inside the head — `group=`, `work=`, `queue=`, `concurrent=` and `cache=` are all
/// present-or-absent and therefore live in the tail, `cache=` last because a cell that
/// appears only on cache hits must sit after everything that has to line up.
///
/// The widths are the contract's (`HEAD`/`head_offsets` in the test module); changing
/// one means changing README's `cut -c` table and the Plan 03 tracker's dated rows in the
/// same commit, never here. The plan's own T6 width table is written history — it describes
/// the 2026-10-16 layout, not this one.
fn format_compact(m: &RequestMetrics) -> String {
    // Truncated THEN padded, both in display cells: a 50-cell GGUF name is what shifts
    // every column today, and padding a 46-cell value to 16 is a no-op.
    let model = pad_cells(&truncate_to_width(&free_form(&m.model), 16), 16);

    // `dur` saturates because it is the proxy's own wall clock, not a backend-reported
    // quantity: 7 digits is ~2.8h, past that a request is stuck, and a column that shifts
    // for a stuck request teaches nothing. `ctx`/`tok` are backend-reported counts and
    // NEVER clamped: an out-of-band figure must widen the line rather than be silently
    // rounded into a number the engine never reported. The two RATE cells are the exception
    // and the reason they are allowed one: they carry a declared, human-set supported
    // maximum, and `rate_group_with_clamps` reports every value it bounds so the exporter
    // layer counts and WARNs while `json`/InfluxDB keep the raw figure. A clamp without that
    // report would be exactly the silent rounding this comment used to forbid.
    // `f64::min` returns the OTHER operand when one is NaN, so a NaN wall clock would
    // otherwise print as the 9999999 sentinel — a plausible 2.8h request where nothing is
    // known. Non-finite renders as `NaN`/`inf` (still 7 cells) rather than a lie, and
    // `fit_rate` follows the same rule for the rate cells.
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

    let head = [rate_group(m), dur, ctx, tok, format!("model={model}"), stream, finish].join(" ");

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
        // Pinned head order: rate group, dur, ctx, tok, model, stream, finish.
        assert!(output.contains("tg= 42.5 pp= 0.20k"), "got: {output}");
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
        assert!(output.contains("tg>= 41.7"), "got: {output}");
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
        assert!(output.contains("tg=  n/a"), "got: {output}");
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
        assert!(out.contains("tg= 42.5 pp= 0.20k"), "got: {out}");
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
        assert!(output.contains("tg= 42.5 pp= 0.20k"), "got: {output}");
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
        assert!(output.contains("tg>= 84.3"), "got: {output}");
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
        // Hand-derived from the specs: 123.45/1000 = 0.12345 -> `{:>5.2}` = " 0.12", and
        // 150.0 -> `{:>5.1}` = "150.0" with NO pad left -- this fixture is one of the two
        // that pins the value sitting flush against the label at three integer digits.
        // (It used to assert `pp=  0.123k`, the retired three-decimal form.)
        assert!(output.contains("pp= 0.12k"), "got: {output}");
        assert!(output.contains("tg=150.0"), "got: {output}");
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
    /// ONE width for all three rate tiers: the line must not betray which tier it rendered,
    /// and a clamp means nothing else can widen it either. 18 = the decode side (the 3-cell
    /// `tg=` plus a shared 5-cell value field = 8 on tiers 1 and 3; the 4-cell `tg>=` plus
    /// the SAME value field = 9 on tier 2) + a separator + the prefill side (9 on tier 1 and
    /// tier 3, 8 for tier 2's placeholder, which gives back the cell tier 2's wider label
    /// took). Trimmed from 22 on 2026-11-21: what is shared across tiers is now the VALUE
    /// field, which is why both tiers reach the same 999.9 ceiling; what used to be shared —
    /// the cell around it, and with it the cross-tier digit stack — is not, by decision.
    const CELL_RATE: usize = 18;
    /// The decode cell per tier: its label plus the shared 5-cell value field. These two
    /// differ by exactly the `>=` glyph, and that difference is the whole reason the group
    /// is balanced by the `pp` placeholder rather than by the decode field.
    const TG_SIDE_MEASURED: usize = 3 + DECODE_VALUE_CELLS; // tiers 1, 3
    const TG_SIDE_FLOOR: usize = 4 + DECODE_VALUE_CELLS; // tier 2
    /// The prefill cell per tier. `PP_VALUE_CELLS` already counts the `k`, so tier 1 and the
    /// tier-3 placeholder are both 9; tier 2's placeholder is 8 so its `tg>=` costs the group
    /// nothing. This is also why `pp=` does NOT sit at the same cell on every tier: nothing
    /// probes it positionally (the contract carries the group), and README says to slice the
    /// group rather than the field.
    const PP_SIDE_RATE: usize = 3 + PP_VALUE_CELLS; // tier 1: `{:>5.2}k`
    const PP_SIDE_FLOOR_PLACEHOLDER: usize = 3 + 5; // tier 2: `pp=` + `{:>5}` n/a
    const PP_SIDE_ABSENT_PLACEHOLDER: usize = 3 + 6; // tier 3: `pp=` + `{:>6}` n/a
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
    /// probed by MEASURED WIDTH (`rate_group(m)`), not by label: tier 1 renders
    /// `tg=`/`pp=`, tier 2 `tg>=` (`work=` is a tail cell, not part of the group), tier 3
    /// `n/a` for both, so the group has no stable label -- and it now leads the head, so
    /// no needle in front of it can anchor its left edge either.
    ///
    /// `model=` is the head's only free-form cell, and it sits at position 5: a backend
    /// name carrying `ctx=` or `dur=` makes those probes `Ambiguous`, which is a value the
    /// contract reports rather than a panic. It was equally exposed at position 1; what
    /// the move bought is that the four cells to its left can no longer be shifted by it.
    const HEAD: [(&str, usize); 7] = [
        ("", CELL_RATE),
        ("dur=", CELL_DUR),
        ("ctx=", CELL_CTX),
        ("tok=", CELL_TOK),
        ("model=", CELL_MODEL),
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

    /// Cells the rate group occupies, MEASURED off the renderer's own output. This used
    /// to be a derivation from where `dur=` landed (`dur_at - CELL_MODEL - 2*SEP`), which
    /// restates the `dur=` offset assertion the same tests already run -- and with the
    /// group leading the head the only derivation left is `dur_at - SEP`, a pure
    /// tautology. Expectation comes from the table, width from `rate_group`.
    fn rate_group_cells(m: &RequestMetrics) -> usize {
        UnicodeWidthStr::width(rate_group(m).as_str())
    }

    /// The offsets are pinned over a MAGNITUDE sweep, not only the fixture rows. The bug
    /// class this exists for is a field that fits every healthy value and then grows one cell
    /// at a boundary, moving `dur=`..`finish=` with it.
    ///
    /// **Since 2026-11-21 the sweep crosses each ceiling instead of stopping at it.** Rates
    /// clamp (`rate_group_with_clamps`), so a past-ceiling value is now a GEOMETRY case like
    /// any other: `1,000.0` and `12,345,678.99` must render at exactly `CELL_RATE` or the
    /// columnar contract dies on precisely the traffic it was built for. What the sweep cannot
    /// see is whether a clamp was *reported* -- that is
    /// `rates_past_their_ceiling_clamp_and_report`, which also carries the rounding-boundary
    /// cases (`999.95`, `99,994.9` vs `99,995.0`) and the proof that `json`/pretty keep raw
    /// values. Rows run every digit-count boundary (1, 2 and 3 integer digits) because the
    /// cell's internal pad reaches zero exactly at the widest value, and that is where
    /// whitespace-delimited extraction breaks.
    ///
    /// `generation_tps` gets its own rows because the prefill rows hold it at 42.5 and the
    /// decode rows hold `prompt_tps` pinned, so no row can be blamed for the other field's
    /// overflow. `prefill 1.0` is deliberate: it renders `pp= 0.00k`, the sub-resolution band
    /// `{:>5.2}k` widened from <0.5 to <5 tok/s.
    #[test]
    fn head_offsets_survive_every_rate_magnitude() {
        let mut rows: Vec<(String, RequestMetrics)> = Vec::new();

        for floor in [0.07, 9.9, 41.67, 99.9, 376.7, 999.9, 999.95, 1_000.0] {
            let mut m = fixture("qwen3", "stop");
            m.completion_tokens = Some(10);
            m.decode_tps_min = floor;
            rows.push((format!("tier2 floor {floor}"), m));
        }
        for decode in [0.07, 4.9, 42.5, 376.7, 999.9, 999.95, 1_000.0] {
            let mut m = fixture("qwen3", "stop");
            m.has_timing_split = true;
            m.prompt_tps = Some(14_765.0);
            m.generation_tps = Some(decode);
            rows.push((format!("tier1 decode {decode}"), m));
        }
        for prefill in [
            1.0, 9.9, 200.5, 14_765.0, 99_990.0, 99_994.9, 99_995.0,
            // The worst figure this repo has recorded, which now clamps on the compact line
            // and must still land at CELL_RATE. Its true value is asserted in `json` by the
            // clamp witness.
            880_447.55,
        ] {
            let mut m = fixture("qwen3", "stop");
            m.has_timing_split = true;
            m.prompt_tps = Some(prefill);
            m.generation_tps = Some(42.5);
            rows.push((format!("tier1 prefill {prefill}"), m));
        }
        // A non-finite rate renders PADDED into the narrowed field (`{:>6.1}` of inf is
        // `   inf`), so it cannot widen the group -- the mirror image of the overflow
        // property, and unreachable in production (`finite_rate` maps it to 0.0 first).
        // Pinned on the renderer anyway: a narrow field that truncated instead of padding
        // would shift the head here. Both reachable non-finite paths are covered, one per
        // `tg` side. `NaN` is NOT a width row because it has no width to measure:
        // `tps_tier` gates on `> 0.0` and `NaN > 0.0` is false, so a NaN floor renders
        // tier 3's `n/a` and never reaches `{:>6.1}`. Asserted as a tier fact below.
        let mut inf_floor = fixture("qwen3", "stop");
        inf_floor.completion_tokens = Some(10);
        inf_floor.decode_tps_min = f64::INFINITY;
        rows.push(("tier2 floor inf (pads, cannot widen)".to_string(), inf_floor));

        let mut inf_decode = fixture("qwen3", "stop");
        inf_decode.has_timing_split = true;
        inf_decode.prompt_tps = Some(14_765.0);
        inf_decode.generation_tps = Some(f64::INFINITY);
        rows.push(("tier1 decode inf (pads, cannot widen)".to_string(), inf_decode));
        let mut quiet = fixture("qwen3", "stop");
        quiet.completion_tokens = None;
        quiet.decode_tps_min = 0.0;
        rows.push(("tier3 no counted output".to_string(), quiet));

        for (label, m) in &rows {
            let line = format_compact(m);
            // Independent of the offsets below: the group's own rendered width, so a
            // tier that overflows its field is attributable to the group and not just
            // to the columns it pushed right.
            assert_eq!(
                rate_group_cells(m),
                CELL_RATE,
                "{label}: the rate group renders {} cells, expected {CELL_RATE}: {line}",
                rate_group_cells(m)
            );
            // And the group's TWO HALVES measured, not just its total. This is the
            // property the total cannot see: a tier whose sides went 11+10 still sums to
            // 22, sails through the assert above, and breaks the digit stacking that is
            // the group's entire purpose -- the claim README teaches in prose ("146.3 and
            // 131.0 end their digits on cell 10"). Measured per row, per tier, here.
            let group = rate_group(m);
            // Split on the FIELD boundary, not the first space: the first space is inside
            // the `tg` cell's own padding (`tg>=   0.1`), which is exactly the padding this
            // assert exists to measure.
            let (tg_side, pp_tail) = group
                .split_once(" pp=")
                .expect("rate_group renders its two fields space-separated");
            let pp_side = format!("pp={pp_tail}");
            // PER-TIER, not one flat number. What the tiers share is the value field; their
            // cells differ by exactly the `>=` glyph. A flat bound here would either flag
            // tier 2's correct 9-cell rendering or excuse a tier 1 that quietly grew to 9.
            let tier = tps_tier(m);
            let (want_tg, want_pp) = match tier {
                1 => (TG_SIDE_MEASURED, PP_SIDE_RATE),
                2 => (TG_SIDE_FLOOR, PP_SIDE_FLOOR_PLACEHOLDER),
                _ => (TG_SIDE_MEASURED, PP_SIDE_ABSENT_PLACEHOLDER),
            };
            let tg_cells = UnicodeWidthStr::width(tg_side);
            let pp_cells = pp_side.width();
            assert_eq!(
                tg_cells, want_tg,
                "{label}: tier {tier} renders its decode cell as {tg_cells} cells, not \
                 {want_tg} (its label plus a shared {DECODE_VALUE_CELLS}-cell value field): \
                 {group:?}"
            );
            assert_eq!(
                pp_cells, want_pp,
                "{label}: tier {tier} renders its prefill cell as {pp_cells} cells, not \
                 {want_pp} -- the group balances on this cell, so a drift here moves `dur=`: \
                 {group:?}"
            );
            // And the invariant the flat cell widths CANNOT express: strip the label and the
            // two tiers that print a rate must show the same number of value cells. That is
            // what makes 999.9 one ceiling for both rather than an order of magnitude apart.
            if tier != 3 {
                let label_cells = if tier == 2 { 4 } else { 3 };
                assert_eq!(
                    tg_cells - label_cells,
                    DECODE_VALUE_CELLS,
                    "{label}: tier {tier}'s decode VALUE field is {} cells, not \
                     {DECODE_VALUE_CELLS}, so the tiers no longer share a ceiling: {group:?}",
                    tg_cells - label_cells
                );
            }
            for (needle, want) in head_offsets() {
                match probe(&line, needle) {
                    Probe::At(at) => assert_eq!(at, want, "{label}: `{needle}` at {at}: {line}"),
                    other => panic!("{label}: `{needle}` {other:?} -- {line}"),
                }
            }
        }

        // NaN is read as ABSENCE, not as an enormous rate: both `tps_tier` gates are
        // `> 0.0`, and `NaN > 0.0` is false. So a NaN floor falls to tier 3 (which the
        // sweep pins as a width) and a NaN measured decode cannot buy tier 1 either.
        // Asserted where it is actually a fact -- about the tier, not the cell width --
        // because a row labelled "tier 2 NaN" would have measured tier 3 in silence.
        let mut nan_floor = fixture("qwen3", "stop");
        nan_floor.completion_tokens = Some(10);
        nan_floor.decode_tps_min = f64::NAN;
        assert_eq!(tps_tier(&nan_floor), 3, "a NaN floor must render as absence, never as a rate");
        let mut nan_decode = fixture("qwen3", "stop");
        nan_decode.has_timing_split = true;
        nan_decode.generation_tps = Some(f64::NAN);
        nan_decode.decode_tps_min = 0.0;
        assert_eq!(
            tps_tier(&nan_decode),
            3,
            "a NaN measured decode must not buy tier 1 and print `tg=    NaN`"
        );
    }

    /// Past a rate cell's ceiling the compact line **clamps to the ceiling and reports it**:
    /// the group stays exactly `CELL_RATE`, the cell prints its ceiling, and
    /// [`rate_group_with_clamps`] hands back a [`ClampReport`] naming the value it bounded.
    /// This test replaces `a_rate_past_a_cells_field_widens_rather_than_clamps`, whose
    /// contract a human decision reversed on 2026-11-21 in exchange for a bounded rate group
    /// and a two-cell-shorter line. The losses are named where they were taken: the trip
    /// point moved from a *tested* 9,999.9 (tier 2) and 999,999 (pp) down to 999.9 and 99,990,
    /// and pp's ceiling sits below the cache-inflated prefill figures this repo's logs record
    /// (880,447 raw, ≈290x inflation, corrected at the source now) and below what a FULL
    /// prefix-cache hit can print — that class is left uncorrected by design
    /// (`collector.rs:595-611`) and has no recorded maximum, so a legitimate full-cache turn
    /// can and will clamp.
    ///
    /// Two properties only this shape can test, and both are why the predicate is
    /// **"the rendering changed"** rather than `v > ceiling`:
    ///
    /// * the columns do NOT move. The old witness asserted `dur=` drifted by the width of the
    ///   overflow; here it is pinned at the fixed `CELL_RATE + SEP`, which is the entire
    ///   purchase price of clamping.
    /// * `pp` 99,994.9 and 99,995.0 render the identical `pp=99.99k`, and only the second is
    ///   reported. Counting the first would fill the counter with ordinary formatting
    ///   rounding, and the one question the counter exists to answer -- "is the number on this
    ///   line the number I was given?" -- would stop having a truthful number behind it.
    ///
    /// One case per format string: `{:>5.1}` sits under two different labels, so an edit to
    /// either literal must fail a case of its own.
    #[test]
    fn rates_past_their_ceiling_clamp_and_report() {
        let mut decode = fixture("qwen3", "stop");
        decode.has_timing_split = true;
        decode.prompt_tps = Some(14_765.0);
        decode.generation_tps = Some(1_000.0);

        // Numerically BELOW the 999.9 ceiling, above it once rounded: `{:>5.1}` of 999.95 is
        // `1000.0`, six characters in a five-cell field. This is the case that makes the
        // ceiling a rendering fact rather than a numeric range.
        let mut decode_rounds = fixture("qwen3", "stop");
        decode_rounds.has_timing_split = true;
        decode_rounds.prompt_tps = Some(14_765.0);
        decode_rounds.generation_tps = Some(999.95);

        let mut floor = fixture("qwen3", "stop");
        floor.completion_tokens = Some(10);
        floor.decode_tps_min = 1_000.0;

        let mut floor_rounds = fixture("qwen3", "stop");
        floor_rounds.completion_tokens = Some(10);
        floor_rounds.decode_tps_min = 999.95;

        let mut prefill = fixture("qwen3", "stop");
        prefill.has_timing_split = true;
        prefill.prompt_tps = Some(99_995.0);
        prefill.generation_tps = Some(42.5);

        // Far past a ceiling: 12,345,678.99 in a five-cell field. Nothing pins at a hidden
        // cap and nothing grows -- the digits simply do not reach the compact line, which is
        // exactly what the report and the `json` block at the end of this test are for.
        let mut absurd = fixture("qwen3", "stop");
        absurd.completion_tokens = Some(10);
        absurd.decode_tps_min = 12_345_678.99;

        for (label, m, want_cell, want_printed, want_tier, want_ceiling) in [
            ("tier1 decode 1,000.0", &decode, "tg", "tg=999.9", 1u8, TG_CELL_CEILING_TPS),
            (
                "tier1 decode 999.95 (rounds up)",
                &decode_rounds,
                "tg",
                "tg=999.9",
                1,
                TG_CELL_CEILING_TPS,
            ),
            ("tier2 floor 1,000.0", &floor, "tg", "tg>=999.9", 2, TG_CELL_CEILING_TPS),
            (
                "tier2 floor 999.95 (rounds up)",
                &floor_rounds,
                "tg",
                "tg>=999.9",
                2,
                TG_CELL_CEILING_TPS,
            ),
            ("tier1 prefill 99,995.0", &prefill, "pp", "pp=99.99k", 1, PP_CELL_CEILING_TPS),
            (
                "tier2 floor 12,345,678.99 (far past)",
                &absurd,
                "tg",
                "tg>=999.9",
                2,
                TG_CELL_CEILING_TPS,
            ),
        ] {
            let (group, reports) = rate_group_with_clamps(m);
            assert!(
                group.contains(want_printed),
                "{label}: the cell did not print its ceiling `{want_printed}`: {group:?}"
            );
            assert_eq!(
                group.width(),
                CELL_RATE,
                "{label}: a clamped rate group grew past {CELL_RATE} cells: {group:?}"
            );
            let report = reports
                .iter()
                .find(|r| r.cell == want_cell)
                .unwrap_or_else(|| panic!("{label}: no `{want_cell}` clamp was reported: {group:?}"));
            assert_eq!(
                report.tier, want_tier,
                "{label}: the report says tier {}, the line rendered tier {want_tier}: {report:?}",
                report.tier
            );
            assert_eq!(
                report.printed, want_ceiling,
                "{label}: the report's `printed` must be the cell's ceiling"
            );
            // The columns do NOT move. This is the assertion the clamp exists to make true,
            // and the mirror image of the one it deleted.
            match probe(&format_compact(m), "dur=") {
                Probe::At(at) => assert_eq!(
                    at,
                    CELL_RATE + SEP,
                    "{label}: `dur=` drifted to {at} although the group is {CELL_RATE} cells"
                ),
                other => panic!("{label}: `dur=` {other:?}"),
            }
        }

        // The no-false-clamp pair. 99,994.9 is past the 99,990 *range* but its rendering
        // still fits the cell, so it is formatting, not a bound value: identical bytes on the
        // line, one report and not the other.
        let mut under = fixture("qwen3", "stop");
        under.has_timing_split = true;
        under.prompt_tps = Some(99_994.9);
        under.generation_tps = Some(42.5);
        let (group_under, reports_under) = rate_group_with_clamps(&under);
        let (group_over, _) = rate_group_with_clamps(&prefill);
        assert_eq!(
            group_under.split_once(" pp=").expect("group shape").1,
            group_over.split_once(" pp=").expect("group shape").1,
            "the pair must render identically or this test proves nothing: {group_under:?} vs {group_over:?}"
        );
        assert!(
            reports_under.iter().all(|r| r.cell != "pp"),
            "99,994.9 tok/s renders inside the pp cell, so reporting it as a clamp would \
             inflate the counter with rounding: {group_under:?}"
        );

        // `json` and the pretty box print the absurd rate IN FULL. "Only the compact line
        // clamps; the record keeps the raw figure" is the sentence that makes the clamp
        // honest, and until now it was a claim about code nothing checked.
        let json = format_metrics(&absurd, StatsFormat::Json);
        assert!(
            json.contains("12345678.99"),
            "json lost the raw derived floor, so the clamp is no longer display-only: {json}"
        );
        let pretty = format_metrics(&absurd, StatsFormat::Pretty);
        assert!(
            pretty.contains("12345678.99"),
            "the pretty box clamped a rate, contradicting `tps_side`'s never-clamp rule: {pretty}"
        );
    }

    /// The tie between the two things `rate_group_with_clamps` decides: **what printed** and
    /// **what was reported**. A test that only checked the renderings could not see a report
    /// that fires on a line where nothing was bounded (the tier-2 `prompt_tps` case the split
    /// exists to prevent), and a test that only checked reports could not see a clamp that
    /// silently failed to render. So: for every fixture and every swept magnitude, the report
    /// vector is empty exactly when neither cell's rendering needed substituting, and a report
    /// naming a cell implies the group shows that cell's ceiling.
    ///
    /// The tightest in-band fixture (`s2`, decode 999.94) also asserts `is_empty()` here: the
    /// clamp threshold is only honest if the largest legal value is NOT counted, which is the
    /// half no past-ceiling case can prove.
    #[test]
    fn clamp_reports_and_the_rendered_group_always_agree() {
        // (label, metrics, tier, decode_value_rendering_fits, prefill_rendering_fits) is what
        // "fits" would mean; instead of restating the format specs here, measure the rendered
        // group: strip each label and compare against the shared value fields. A disagreement
        // between this arithmetic and the report vector is the bug.
        let mut cases: Vec<(String, RequestMetrics)> = contract_fixtures()
            .into_iter()
            .map(|(label, m)| (label.to_string(), m))
            .collect();
        for floor in [0.07, 999.9, 999.95, 1_000.0, 12_345_678.99] {
            let mut m = fixture("qwen3", "stop");
            m.completion_tokens = Some(10);
            m.decode_tps_min = floor;
            cases.push((format!("floor {floor}"), m));
        }
        for prefill in [1.0, 99_990.0, 99_994.9, 99_995.0, 880_447.55] {
            let mut m = fixture("qwen3", "stop");
            m.has_timing_split = true;
            m.prompt_tps = Some(prefill);
            m.generation_tps = Some(42.5);
            cases.push((format!("prefill {prefill}"), m));
        }

        for (label, m) in cases {
            let tier = tps_tier(&m);
            let (group, reports) = rate_group_with_clamps(&m);
            // Which rate each tier even PRINTS, and what its cell's format string produces for
            // it -- re-typed from the specs here on purpose, because that repetition IS the
            // cross-check. Measuring the rendered group cannot work: after a clamp the cell
            // shows the ceiling, which fits, so the line is silent about what was replaced.
            let decode = match tier {
                1 => m.generation_tps,
                2 => Some(m.decode_tps_min),
                _ => None,
            };
            let prefill = match tier {
                1 => Some(m.prompt_tps.unwrap_or(0.0)),
                _ => None,
            };
            let would_overflow = |v: Option<f64>, render: fn(f64) -> String, cells: usize| {
                v.filter(|v| v.is_finite())
                    .is_some_and(|v| render(v).as_str().width() > cells)
            };
            let over_decode = would_overflow(decode, render_decode, DECODE_VALUE_CELLS);
            let over_pp = would_overflow(prefill, render_prefill, PP_VALUE_CELLS);
            assert_eq!(
                reports.len(),
                usize::from(over_decode) + usize::from(over_pp),
                "{label} (tier {tier}): {reports:?} reported, but the specs say the decode \
                 rate {decode:?} does{} and the prefill rate {prefill:?} does{} fit its cell \
                 ({DECODE_VALUE_CELLS} and {PP_VALUE_CELLS} value cells) -- a report that \
                 disagrees with the line is the failure mode the shared decision site exists \
                 to prevent: {group:?}",
                if over_decode { "" } else { " not" },
                if over_pp { "" } else { " not" }
            );
            // And WHICH cell each report names, not merely how many: a report vector of the
            // right length pointing at the wrong cell would pass a count alone.
            let mut got: Vec<&str> = reports.iter().map(|r| r.cell).collect();
            got.sort_unstable();
            let mut want: Vec<&str> = Vec::new();
            if over_decode {
                want.push("tg");
            }
            if over_pp {
                want.push("pp");
            }
            want.sort_unstable();
            assert_eq!(got, want, "{label} (tier {tier}): wrong cells reported: {group:?}");
            for r in &reports {
                let ceiling = if r.cell == "tg" {
                    TG_CELL_CEILING_TPS
                } else {
                    PP_CELL_CEILING_TPS
                };
                assert!(
                    r.actual > ceiling,
                    "{label}: a `{}` clamp reported for {}, which is BELOW the {ceiling} \
                     ceiling: {group:?}",
                    r.cell,
                    r.actual
                );
                let shown = if r.cell == "tg" {
                    format!("tg{}999.9", if tier == 2 { ">=" } else { "=" })
                } else {
                    "pp=99.99k".to_string()
                };
                assert!(
                    group.contains(&shown),
                    "{label}: reported a `{}` clamp but the group does not show its ceiling \
                     {shown:?}: {group:?}",
                    r.cell
                );
            }
        }

        let s2 = contract_fixtures()
            .into_iter()
            .find(|(label, _)| *label == "s2-everything-max")
            .expect("the s2 fixture must exist for its clamp assertion to mean anything");
        assert_eq!(
            s2.1.generation_tps,
            Some(999.94),
            "s2 is pinned as the tightest in-band decode fixture; if its rate moved, the \
             assertion below is no longer testing the threshold"
        );
        let (s2_group, s2_reports) = rate_group_with_clamps(&s2.1);
        assert!(
            s2_reports.is_empty(),
            "s2 is the file's tightest IN-BAND fixture (decode 999.94, prefill 88,000) and it \
             reported {:?}, so the clamp threshold is set below the largest value the cells \
             can print: {s2_group:?}",
            s2_reports
        );
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
        // One step under the `{:>5.1}` rounding boundary on purpose: 999.95 rounds to
        // `1000.0`, six characters in a five-cell field, so 999.94 is the largest value that
        // still renders inside the cell. This is the file's tightest fixture -- if a future
        // trim narrows the field, THIS row goes red first, and the fix is to move the ceiling
        // into the sweep, never to soften this row. The clamp tie test also asserts this
        // fixture reports NO clamp, which keeps the threshold honest from the inside.
        s2.generation_tps = Some(999.94);
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
        // The head LEADS with the rate group, whose three tiers spell `tg=`, `tg>=` and
        // `tg=` -- no label in common, so `head_offsets()` skips it and cannot pin the
        // left edge. The table's own order does.
        assert_eq!(
            HEAD[0],
            ("", CELL_RATE),
            "the head starts with the rate cell, not with a labelled cell"
        );
        // The group's halves are DERIVED per tier from the labels plus the shared value
        // fields -- there is no longer one const per cell to add. All three tiers must
        // reach CELL_RATE from their own arithmetic: tier 1 and tier 3 share the narrow
        // `tg=` label, tier 2 pays the `>=` glyph and gives the cell back on `pp`.
        for (tier, tg_side, pp_side) in [
            (1usize, TG_SIDE_MEASURED, PP_SIDE_RATE),
            (2, TG_SIDE_FLOOR, PP_SIDE_FLOOR_PLACEHOLDER),
            (3, TG_SIDE_MEASURED, PP_SIDE_ABSENT_PLACEHOLDER),
        ] {
            assert_eq!(
                tg_side + SEP + pp_side,
                CELL_RATE,
                "tier {tier}: its documented halves ({tg_side} + {SEP} + {pp_side}) must add up \
                 to the group"
            );
        }
        assert_eq!(
            PP_SIDE_RATE, PP_SIDE_ABSENT_PLACEHOLDER,
            "tier 1's measured `pp` and tier 3's placeholder are the same width by design; if \
             they diverge, tier 3 is no longer indistinguishable from tier 1 at a glance"
        );
        assert_eq!(
            TG_SIDE_FLOOR - TG_SIDE_MEASURED,
            1,
            "tier 2's decode cell is exactly the `>=` glyph wider than tier 1's -- that single \
             cell is the whole reason the group balances on the `pp` placeholder. These consts \
             are load-bearing OUTSIDE this assert too: DECODE_VALUE_CELLS is the per-label bound \
             `readme_never_renders_a_retired_tg_field` enforces on README's renderings, so a typo \
             here loosens that docs bound as well. None of them can see the format strings -- \
             `head_offsets_survive_every_rate_magnitude` measures every rendered group"
        );
        assert_eq!(
            offsets.first(),
            Some(&("dur=", CELL_RATE + SEP)),
            "`dur=` is the head's first probeable cell: one separator past the group"
        );
        assert_eq!(
            offsets.last().map(|(_, o)| *o + CELL_FINISH),
            Some(HEAD_CELLS),
            "`finish=` is the last head cell, so its end is the head width"
        );
        assert_eq!(
            HEAD_CELLS, 124,
            "the head is 118 cells of content plus 6 separators. History of the total, all on \
             2026-11-21: 129 -> 128 when the rate group led and `pp` lost a cell (5da96be, \
             d05f96b), then 128 -> 124 when the group went 22 -> 18 by sharing the DECODE VALUE \
             FIELD across tiers and capping `pp` at two decimals. None of those was a release \
             shape. If the total moves again, update README's `cut -c` table AND the measured \
             table in the Plan 03 tracker together, never here alone -- and remember this assert \
             is only a checksum of HEAD: `the_readme_offset_table_matches_this_build` is what \
             ties the docs to it"
        );
    }

    /// Positive control for the table above, and the reason the contract's red means
    /// anything: with every single assertion red, "the formatter is ragged" and "the
    /// expected offsets are simply wrong" look identical, and a permanently-red
    /// contract test gets reconciled by editing its constants. These two literals are
    /// built from the cell widths (`fmt` + pad, never eyeballed spacing -- the first
    /// hand-typed pair here measured 127 cells, see the Plan 03 tracker) and asserted
    /// against `head_offsets()` -- if the table is wrong, THIS fails today.
    ///
    /// It is a control on the OFFSETS TABLE, not a sample of the output: only the rate
    /// group's TOTAL is asserted against `CELL_RATE`; the 10/11 split of its two fields
    /// is pinned where `rate_group` is measured per magnitude, in
    /// `head_offsets_survive_every_rate_magnitude`. So do not paste this line into a
    /// ticket as an example of what the proxy prints -- the README's example block is
    /// the pinned copy of the real rendering.
    ///
    /// `GOLDEN_CJK` is why the head must be padded in DISPLAY CELLS: its model field
    /// is 8 chars but 16 cells, so `{:<16}` char padding puts every column after it
    /// eight cells out.
    /// Hand-derived, not pasted from a run: the group is `tg=` + `{:>5.1}`(999.9) = `tg=999.9` (8
    /// cells, value flush against the label) + one separator + `pp=` + `{:>5.2}`(88.0) + `k` =
    /// `pp=88.00k` (9 cells) = 18 = CELL_RATE, then REST, which this build measures 106 cells
    /// (1+13+1+17+1+17+1+22+1+14+1+18, separators included). Verified with a cell-width count
    /// and a cell-index probe for all six labelled cells. Both rates used to sit at four integer
    /// digits (`9999.9`), which the trimmed cell now CLAMPS -- holding them at the ceiling keeps
    /// these fixtures what they always were, a max-magnitude line shape, and moves the clamp
    /// evidence to `rates_past_their_ceiling_clamp_and_report`.
    const GOLDEN_ASCII: &str = "tg=999.9 pp=88.00k dur=    184ms ctx=999999/262144 tok=186417/   498 model=qwen3-7b         stream=ok      finish=stop      ";

    /// The same head with a CJK model field: `本地大模型量化版` is 8 chars but 16 display cells, so it
    /// carries no padding and `stream=` abuts it. Cell widths, not chars, are what this golden
    /// exists to pin -- and it is why the derivation above counts cells.
    const GOLDEN_CJK: &str = "tg=999.9 pp=88.00k dur=    184ms ctx=999999/262144 tok=186417/   498 model=本地大模型量化版 stream=ok      finish=stop      ";

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
        }
    }

    /// A 1M-token vLLM window (`1048576`) and a prompt of similar size do not fit the
    /// `{:>6}` fields the plan pins, so the head cannot stay aligned for them. That
    /// widening (2 cells of head, against OQ7's width concern) is a human decision, not
    /// this test's -- and OQ7's premise that the head only ever grows is now outdated: on
    /// 2026-11-21 the repo voluntarily took a cell OUT of the rate group. What IS pinned
    /// here, and stays pinned however the width question is answered: an out-of-band number
    /// shifts the columns rather than being clamped, rounded or dropped.
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

            // The head's first cell is the rate group, which shares no label across its
            // three tiers, so it is measured off `rate_group` rather than anchored to a
            // string prefix: `context/pitfalls.md` bans encoding a column guarantee in
            // `contains`/`starts_with`/field counts, all of which are blind to interior
            // holes. The absolute offsets in the loop below pin the left edge on their
            // own -- a cell inserted ahead of the group moves every one of them.
            let occupied = rate_group_cells(m);
            if occupied != CELL_RATE {
                bad.push(format!(
                    "{name}: the rate group occupies {occupied} cells, expected {CELL_RATE}  ({line:?})"
                ));
            }
            // And that the head is actually BUILT from it. `HEAD[0]` pins the table and the
            // offsets pin the render, so without this the composition link between them is
            // unasserted. This is not the banned pattern: `context/pitfalls.md` forbids a
            // substring AS the column guarantee; the guarantee stays the offsets loop.
            if !line.starts_with(rate_group(m).as_str()) {
                bad.push(format!(
                    "{name}: the line does not begin with the rendered rate group  ({line:?})"
                ));
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

    /// The four lines `README.md` shows as its compact examples, built to be **read**
    /// rather than to be adversarial — that is `contract_fixtures`'s job. Four outcomes
    /// of one 9,074-token request, so the tiers are comparable line to line. Every
    /// figure comes from this deployment's own vLLM measurements (tracker Evidence
    /// 2026-10-16: engine compute 14,765 tok/s, warm corrected prefill 12,770-13,066 (mean 12,858), a
    /// 9,074-token prompt with a 3,200-token prefix-cache hit). Nothing is invented.
    ///
    /// Each fixture is a state the COLLECTOR can produce, not merely one the renderer
    /// accepts: a counted completion implies the floor and the cost rate derived from
    /// it, and a measured prefill implies `prompt_ms`, because the vLLM path sets both
    /// from ttft. A doc example the proxy could never have emitted is a fake screenshot
    /// of a log line — the class `contract_fixtures` already refuses to tolerate.
    fn readme_examples() -> Vec<(&'static str, RequestMetrics)> {
        // One request: 9,074 prompt tokens, 412 generated, 3.144 s of wall clock.
        const WALL_MS: f64 = 3_144.0;
        const DECODE_FLOOR: f64 = 412.0 / (WALL_MS / 1_000.0);
        const COST_RATE: f64 = (9_074.0 + 412.0) / (WALL_MS / 1_000.0);

        let base = |finish: &str| {
            let mut m = fixture("cosmo-6000", finish);
            m.context_total = Some(262_144);
            m.concurrent_requests = Some(1);
            m
        };

        // 1. Tier 1 — vLLM started with --enable-per-request-metrics: the backend
        // reported both halves, so both print as measured. `prompt_ms` is the ttft the
        // discounted rate came from: (9074 - 3200) / 14765 tok/s = 397.8 ms.
        let mut measured = base("stop");
        measured.has_timing_split = true;
        measured.prompt_tps = Some(14_765.0);
        measured.generation_tps = Some(146.32);
        measured.prompt_ms = 397.8;
        measured.generation_ms = 2_815.7;
        measured.decode_tps_min = DECODE_FLOOR;
        measured.total_tps = COST_RATE;
        measured.duration_ms = WALL_MS;
        measured.context_used = Some(9_074);
        measured.prompt_tokens = Some(9_074);
        measured.completion_tokens = Some(412);
        measured.cached_tokens = Some(3_200);
        measured.streaming = true;
        measured.stream_end = Some("ok");

        // 2. Tier 2 — the SAME request against a vLLM without that flag, which is the
        // normal case. No backend decode rate exists, so `pp` is suppressed and the
        // decode figure is a floor while `work` runs in the thousands beside it. That
        // contrast is the trap the README exists to teach.
        let mut floor = base("stop");
        floor.decode_tps_min = DECODE_FLOOR;
        floor.total_tps = COST_RATE;
        floor.duration_ms = WALL_MS;
        floor.context_used = Some(9_074);
        floor.prompt_tokens = Some(9_074);
        floor.completion_tokens = Some(412);
        floor.cached_tokens = Some(3_200);
        floor.streaming = true;
        floor.stream_end = Some("ok");

        // 3. Tier 3 — a stream the client abandoned before the backend's final chunk,
        // the only chunk carrying `usage`/`timings`: half the token counts are absent
        // and no rate exists.
        let mut gone = base("unknown");
        gone.duration_ms = WALL_MS;
        gone.context_used = Some(9_074);
        gone.prompt_tokens = Some(9_074);
        gone.completion_tokens = None;
        gone.streaming = true;
        gone.stream_end = Some("client_gone");

        // 4. Tier 3 from the other direction — a backend error body. Buffered, so the
        // badge reads `sync` (no other example in these docs shows it), no usage at all,
        // and the finish value arrives as "unknown", which the head renders `n/a`.
        let mut error_body = base("unknown");
        error_body.duration_ms = 12.0;
        error_body.streaming = false;

        // 5. The cells every dashboard keys on that no other line here exercises: a
        // model name past the 16-cell budget (truncated with an ASCII "...", so the head
        // still measures 128 and a full-name grep never matches) and `finish=tool_calls`,
        // the dominant agent value, which fills its 10 cells exactly. `group=` is the
        // multi-backend tail cell. Rates are the deployment's warm corrected cluster.
        const AT_PROMPT: u64 = 4_100;
        const AT_COMPLETION: u64 = 118;
        const AT_WALL_MS: f64 = 2_841.0;
        let mut agent_turn = base("tool_calls");
        agent_turn.model = "Qwen3-14B-128K-Q3_K_S.gguf".to_string();
        agent_turn.has_timing_split = true;
        agent_turn.prompt_tps = Some(12_858.0);
        agent_turn.generation_tps = Some(51.06);
        agent_turn.prompt_ms = AT_PROMPT as f64 / 12_858.0 * 1_000.0;
        agent_turn.generation_ms = 2_313.0;
        agent_turn.decode_tps_min = AT_COMPLETION as f64 / (AT_WALL_MS / 1_000.0);
        agent_turn.total_tps = (AT_PROMPT + AT_COMPLETION) as f64 / (AT_WALL_MS / 1_000.0);
        agent_turn.duration_ms = AT_WALL_MS;
        agent_turn.context_used = Some(AT_PROMPT);
        agent_turn.prompt_tokens = Some(AT_PROMPT);
        agent_turn.completion_tokens = Some(AT_COMPLETION);
        agent_turn.group_name = Some("local".to_string());
        agent_turn.concurrent_requests = Some(3);
        agent_turn.streaming = true;
        agent_turn.stream_end = Some("ok");

        // 6. A stream the backend cut short — the `trunc` badge — with NO tail at all.
        // This is the line where `cut -c130-` returns nothing and the head's trailing pad
        // is trimmed, which is the case an operator otherwise reads as a broken line.
        let mut cut_short = base("unknown");
        cut_short.duration_ms = 30_000.0;
        cut_short.context_used = Some(2_048);
        cut_short.prompt_tokens = Some(2_048);
        cut_short.completion_tokens = None;
        cut_short.concurrent_requests = None;
        cut_short.streaming = true;
        cut_short.stream_end = Some("truncated");

        vec![
            ("tier 1: measured split", measured),
            ("tier 2: wall-bounded floor", floor),
            ("tier 3: abandoned stream", gone),
            ("tier 3: backend error body", error_body),
            ("agent turn: truncation, tool_calls, group=", agent_turn),
            ("backend-cut stream, empty tail", cut_short),
        ]
    }

    /// The README's compact examples must BE `format_compact`'s output, not a
    /// description of it.
    ///
    /// This test exists because Plan 03 shipped a rewritten compact line while
    /// `README.md` still showed the retired rate key beside an unlabelled token pair,
    /// with a three-tier table teaching operators to grep for a cell that no longer
    /// existed — and the entire suite stayed green throughout. A doc example that nothing
    /// renders is not documentation, it is a false positive an operator pastes into a
    /// ticket.
    ///
    /// Matching is whole-line against a located fenced block, not `contains` over the
    /// file: a substring check would pass with the stale block still present, and a
    /// file-wide "any matching line" check would pass while a *second*, hand-typed stale
    /// block sat elsewhere in the doc. Scoping the retired-key sweep to that one block is
    /// what lets the migration banner name the keys it retires outright — a retired key
    /// in prose is a migration note an operator Ctrl-Fs for, while a retired key in the
    /// example block is a command that silently returns nothing. Banning the literal
    /// file-wide would have degraded the operator-facing artifact to satisfy a gate.
    ///
    /// Read at runtime rather than `include_str!` so editing the README does not
    /// recompile the crate. On a mismatch the failure prints the rendered line and the
    /// block's location, which makes regeneration a copy-paste: the formatter is the
    /// oracle, never a hand-typed string — this plan's own hand-typed T6 golden came out
    /// two cells short.
    #[test]
    fn readme_compact_example_is_the_rendered_line() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
            .expect("README.md must sit beside Cargo.toml for the docs pin");

        let examples = readme_examples();
        assert!(!examples.is_empty(), "the docs pin has no examples to pin");
        let rendered: Vec<String> = examples.iter().map(|(_, m)| format_compact(m)).collect();

        // The fenced block an operator actually pastes from, found by a needle no other
        // block in the file carries.
        let block = readme
            .split("```")
            .find(|b| b.contains("model=cosmo-6000"))
            .expect("README must carry the fenced compact example block");
        let block_lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        let anchor = readme
            .lines()
            .position(|l| l.contains("model=cosmo-6000"))
            .map_or(0, |i| i + 1);

        let mut stale = String::new();
        for ((label, _), line) in examples.iter().zip(&rendered) {
            if !block_lines.contains(&line.as_str()) {
                stale.push_str(&format!("{label}\n  rendered: {line}\n\n"));
            }
        }

        // Reported first on purpose: this is the failure that carries the lines needed to
        // repair the file, and the structural checks below cannot be acted on until it is
        // fixed.
        assert!(
            stale.is_empty(),
            "README's compact examples (block near line {anchor}) are stale — paste these \
             rendered lines over them:\n{stale}"
        );

        let mut distinct = rendered.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            rendered.len(),
            "two README examples render identically, so one of them proves nothing: {rendered:?}"
        );

        assert_eq!(
            block_lines.len(),
            rendered.len(),
            "the pinned block carries {} example lines but the formatter has {}; one side is \
             undocumented (near line {anchor})",
            block_lines.len(),
            rendered.len()
        );

        for retired in ["tps=", "tps>=", "reprompt="] {
            assert!(
                !block.contains(retired),
                "the pinned example block advertises `{retired}`, which format_compact does not \
                 emit (near line {anchor})"
            );
        }
    }

    /// A retired cell width must not survive in README's PROSE either.
    ///
    /// The pin above covers the fenced block. It cannot reach the tier table, the tier-3
    /// paragraph or the "the `tg` field is 10 cells on every tier" claim — all hand-typed —
    /// and when the rate group lost a cell on 2026-11-21 four rendered cells in prose were
    /// left showing 11-cell `tg` fields with the suite green. Same defect Plan 03 logged for
    /// retired KEYS, one level down: the tier table is where an operator copies a rendering
    /// from, so a stale padding there is a stale grep, not just stale prose.
    ///
    /// It is a WIDTH bound rather than a ban on the old paddings because "the old padding" is
    /// not a fixed string: `tg= 42.5` is correct today and `tg=  146.3` was correct yesterday.
    /// What cannot be correct is a rendering wider than its own label plus the shared value
    /// field, which is exactly what every pre-trim rendering is.
    ///
    /// Both rate cells are bounded now, per LABEL rather than per field: tier 2's `tg>=` label
    /// is one cell wider than tier 1's `tg=`, and a flat ceiling would either flag tier 2's
    /// correct rendering or excuse a tier 1 that quietly grew. `pp` joined the bound on
    /// 2026-11-21, when clamping made a past-ceiling `pp` rendering impossible; before that it
    /// could only be policed by blacklisting one retired literal at a time.
    #[test]
    fn readme_never_renders_a_retired_tg_field() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
            .expect("README.md must sit beside Cargo.toml for the docs pin");

        let mut stale: Vec<String> = Vec::new();
        let mut measured = 0usize;
        let mut seen_plain = false;
        let mut seen_bound = false;
        for (idx, line) in readme.lines().enumerate() {
            let mut from = 0usize;
            while let Some(pos) = line[from..].find("tg") {
                let start = from + pos;
                let rest = &line[start..];
                let label = match rest {
                    r if r.starts_with("tg>=") => 4,
                    r if r.starts_with("tg=") => 3,
                    // bare `tg` (a column name, a format string) — not a rendering
                    _ => {
                        from = start + 2;
                        continue;
                    }
                };
                let b = rest.as_bytes();
                let mut j = label;
                while j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                let mut k = j;
                // Terminators: whitespace, the inline-code fence, and the punctuation that
                // ends a PROSE mention of a rendering -- `its value can touch the label
                // (tg=376.7)`. No real cell value contains any of them, so stopping here
                // measures the cell rather than the sentence around it.
                while k < b.len() && !matches!(b[k], b' ' | b'`' | b',' | b';' | b':' | b')' | b']' | b'}' | b'"' | b'\'') {
                    k += 1;
                }
                // A `{`/`$`/`(`/`[` opener is a format spec or a shell pattern, not a
                // rendered cell, and it is not width that the contract cares about.
                let value = &rest[j..k];
                if value.starts_with(['{', '$', '(', '[', '*']) {
                    from = start + label;
                    continue;
                }
                let field = &rest[..k];
                let cells = UnicodeWidthStr::width(field);
                measured += 1;
                if label == 4 {
                    seen_bound = true;
                } else {
                    seen_plain = true;
                }
                // PER-LABEL bound: tier 2's label is one cell wider, so a flat ceiling
                // would either flag `tg>=131.0` (correct) or excuse a tier-1 cell that
                // quietly grew a cell. What both labels share is the value field.
                let bound = label + DECODE_VALUE_CELLS;
                if cells > bound {
                    stale.push(format!(
                        "line {}: `{field}` is {cells} cells; `{}` plus a {DECODE_VALUE_CELLS}-cell \
                         value field is {bound}. If this is prose rather than a rendering, put a \
                         space or a backtick between the label and the value so the scanner sees \
                         a cell, not a glued word.",
                        idx + 1,
                        &rest[..label]
                    ));
                }
                from = start + label;
            }
        }

        // `pp` is now width-BOUNDED like `tg`, symmetric, because the tell it used to be is
        // unprintable: past 99,990 tok/s the cell clamps to `99.99k`, so no README rendering
        // can legitimately exceed 9 cells any more. This replaces the exact-string ban on the
        // retired three-decimal padding, which could only ever blacklist one stale literal at
        // a time while a NEW stale width passed.
        let mut pp_measured = 0usize;
        let mut pp_seen_pad = false;
        for (idx, line) in readme.lines().enumerate() {
            let mut from = 0usize;
            while let Some(pos) = line[from..].find("pp=") {
                let start = from + pos;
                let rest = &line[start..];
                let b = rest.as_bytes();
                let mut j = 3;
                while j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                if j > 3 {
                    pp_seen_pad = true;
                }
                let mut k = j;
                while k < b.len() && !matches!(b[k], b' ' | b'`' | b',' | b';' | b':' | b')' | b']' | b'}' | b'"' | b'\'') {
                    k += 1;
                }
                let value = &rest[j..k];
                // A format spec, a shell pattern, or prose -- not a rendered cell.
                if value.is_empty()
                    || value.starts_with(['{', '$', '(', '[', '*', '<'])
                    || !(value.bytes().any(|c| c.is_ascii_digit()) || value.starts_with("n/a"))
                {
                    from = start + 3;
                    continue;
                }
                let field = &rest[..k];
                let cells = UnicodeWidthStr::width(field);
                pp_measured += 1;
                if cells > PP_SIDE_RATE {
                    stale.push(format!(
                        "line {}: `{field}` is {cells} cells; `pp=` plus a {PP_VALUE_CELLS}-cell \
                         value (the digits and their `k`) is {PP_SIDE_RATE}. Past its ceiling a \
                         rate CLAMPS instead of widening, so no `pp` rendering is ever wider \
                         than this now.",
                        idx + 1
                    ));
                }
                from = start + 3;
            }
        }

        assert!(
            stale.is_empty(),
            "README renders rate cells wider than the formatter's cells (a retired width the \
             docs still teach):\n{}",
            stale.join("\n")
        );
        // Vacuity guards. `stale.is_empty()` is trivially satisfied by a README the scanner
        // found nothing in, and by a rewrite whose renderings all begin with a skipped
        // opener -- so the scan itself is asserted. The lower bound is DERIVED from the
        // fixture list the docs pin already forces into the example block (one `tg` field
        // per pinned line), so it moves with the docs instead of rotting as a magic number,
        // which is the same trick as the pin's line-count bijection.
        let floor = readme_examples().len();
        assert!(
            measured >= floor,
            "the scanner measured {measured} `tg` renderings in README, fewer than the \
             {floor} lines the pinned example block must carry -- either the docs stopped \
             teaching renderings or the scanner stopped finding them, and this test is now \
             protecting nothing"
        );
        assert!(
            seen_plain && seen_bound,
            "the scanner saw only one tag class (`tg=`: {seen_plain}, `tg>=`: {seen_bound}); \
             a docs rewrite that dropped a tier would halve this test's coverage in silence"
        );
        // Both bounds must actually have run. A README that stopped showing a padded `pp`
        // value (tier 2's `pp=  n/a` is the only one in these docs) or that wrote every rate
        // as a format spec would satisfy `stale.is_empty()` by measuring nothing.
        assert!(
            pp_measured >= 4,
            "the `pp` scanner measured {pp_measured} renderings in README; the docs carry at \
             least four (one per example line plus the tier table), so either the docs stopped \
             teaching `pp` renderings or the scanner stopped finding them"
        );
        assert!(
            pp_seen_pad,
            "no `pp=` value in README had any internal pad, so tier 2's `pp=  n/a` placeholder \
             is gone from the docs and the pp bound is being fed one shape only"
        );
    }

    /// README's `cut -c` table is the only place an operator reads offsets from, and it has
    /// been wrong for two of the four head shapes that shipped today — the defence until now
    /// was a human remembering to run `cut` on a live line. This parses the table and compares
    /// it against the same arithmetic the renderer uses (`HEAD`, `head_offsets()`,
    /// `HEAD_CELLS`), so a geometry change that skips the docs fails a test instead of shipping.
    ///
    /// Every row is demanded BY NAME: nine rows parsed, and each expected label found exactly
    /// once. A row that stops parsing is a FAILURE rather than a filtered row, because a
    /// permissive parser rebuilds the vacuity problem inside the thing meant to fix vacuity.
    ///
    /// What this cannot see is the story in front of the head — the tracing prefix, the
    /// colour escapes, whether the example block really is what the binary prints. The live
    /// render in the Plan 03 tracker covers that; this covers the table.
    #[test]
    fn the_readme_offset_table_matches_this_build() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
            .expect("README.md must sit beside Cargo.toml for the docs pin");
        let header = readme
            .find("| Cell | Columns (this build")
            .expect("README must carry the offset table, headed `| Cell | Columns (this build`");
        // One row per `|`-delimited line after the header, stopping at the first line that is
        // not part of the table. The `|---|` rule line is dropped by shape, not by index.
        let rows: Vec<(String, String, String)> = readme[header..]
            .lines()
            .skip(1)
            .map(str::trim)
            .take_while(|l| l.starts_with('|'))
            .filter(|l| l.matches('|').count() >= 4)
            // The `|---|---|` rule row: every one of its cells is dashes.
            .filter(|l| {
                !l.split('|')
                    .filter(|c| !c.trim().is_empty())
                    .all(|c| !c.trim().is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
            })
            .map(|l| {
                // `|a|b|c|`.split('|') yields an empty first and last element.
                let mut cols = l.split('|').skip(1).map(|c| c.trim().to_string());
                let cell = cols.next().unwrap_or_default();
                let range = cols.next().unwrap_or_default();
                let slice = cols.next().unwrap_or_default();
                (cell, range, slice)
            })
            .collect();
        assert_eq!(
            rows.len(),
            9,
            "expected the table's nine rows (7 cells + whole head + tail), parsed {}: {rows:?}",
            rows.len()
        );

        // (label as README writes it, first cell, last cell as `None` when open-ended, and the
        // `cut -c` expression the Slice column must carry). Starts come from the SAME
        // derivation the renderer's offsets come from; head_offsets() is 0-based, README counts
        // from 1, so every start here is `+ 1`.
        let offsets = head_offsets();
        let mut expected: Vec<(String, usize, Option<usize>, String)> = vec![(
            "rate (`tg=`/`pp=`)".to_string(),
            1,
            Some(CELL_RATE),
            format!("`cut -c1-{CELL_RATE}`"),
        )];
        for (label, width) in HEAD.iter().skip(1) {
            let start = offsets
                .iter()
                .find(|(l, _)| l == label)
                .unwrap_or_else(|| panic!("head_offsets() lost `{label}`"))
                .1
                + 1;
            expected.push((format!("`{label}`"), start, Some(start + width - 1), String::new()));
        }
        expected.push(("whole head".to_string(), 1, Some(HEAD_CELLS), String::new()));
        expected.push(("tail".to_string(), HEAD_CELLS + SEP + 1, None, String::new()));

        for (label, want_start, want_end, _slice) in &expected {
            let (_, range, _slice) = rows.iter().find(|(cell, _, _)| cell == label).unwrap_or_else(|| {
                panic!(
                    "README's offset table has no row for {label}. Nine rows are required \
                         by name; found: {:?}",
                    rows.iter().map(|(c, _, _)| c).collect::<Vec<_>>()
                )
            });
            let (got_start, got_end) = match range.split_once('-') {
                Some((a, b)) if b.trim().is_empty() => (a.trim().parse::<usize>().ok(), None),
                Some((a, b)) => (a.trim().parse::<usize>().ok(), b.trim().parse::<usize>().ok()),
                None => (range.trim().parse::<usize>().ok(), None),
            };
            assert_eq!(
                got_start,
                Some(*want_start),
                "README's {label} row starts at column {range}; this build puts it at \
                 {want_start}. Fix the docs, not this test."
            );
            assert_eq!(
                got_end, *want_end,
                "README's {label} row ends at `{range}`; this build's cell ends at {:?} \
                 (the cell width is what moves when a value field changes)",
                want_end
            );
        }
        // And the Slice column must agree with the Columns column: README prints each offset
        // twice, and two typed numbers are two chances to be wrong.
        for (cell, range, slice) in &rows {
            let want = format!("`cut -c{range}`");
            assert_eq!(
                slice.trim(),
                want,
                "README's `{cell}` row says columns `{range}` but its Slice column says \
                 `{slice}`; the two must be the same fact"
            );
        }
    }

    /// Each tier's rate group must also appear in README **outside** the example block.
    ///
    /// The width bound above only says a hand-typed rendering is not too WIDE. It cannot see
    /// a correct-width-but-wrong-content string: `tg=  n/a` and `tg= 42.5` are both 8 cells,
    /// and `pp=   n/a` and `pp=14.77k` are both 9, so a tier table that printed absence where
    /// tier 1 prints a measurement would pass a width test. The three tier-table cells and the
    /// tier-3 paragraph are hand-typed, so they are pinned verbatim here.
    ///
    /// Scoped deliberately to the README MINUS the located block: the block already contains
    /// all three group strings, so a file-wide `contains` would pass with the tier table
    /// deleted. Same reasoning as the pin's refusal to `contains` over the whole file.
    #[test]
    fn readme_teaches_each_tiers_rendering_outside_the_example_block() {
        let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
            .expect("README.md must sit beside Cargo.toml for the docs pin");
        let block = readme
            .split("```")
            .find(|b| b.contains("model=cosmo-6000"))
            .expect("README must carry the fenced compact example block");
        let prose = readme.replace(block, "");

        // One group string per tier, taken from the fixtures the docs pin already uses.
        let mut wanted: Vec<(u8, String)> = Vec::new();
        for (label, m) in readme_examples() {
            let tier = tps_tier(&m);
            if !wanted.iter().any(|(t, _)| *t == tier) {
                wanted.push((tier, rate_group(&m)));
            }
            let _ = label;
        }
        assert_eq!(
            wanted.len(),
            3,
            "the docs fixtures must cover all three rate tiers, found {:?}",
            wanted.iter().map(|(t, _)| *t).collect::<Vec<_>>()
        );
        for (tier, group) in wanted {
            assert!(
                prose.contains(&group),
                "tier {tier} renders {group:?} and README no longer shows that rendering \
                 anywhere outside the example block -- the tier table and the tier-3 \
                 paragraph are where an operator copies a grep from"
            );
        }
    }

    /// README's `tail` row is the one offset in the table that is not a cell of `HEAD`, and
    /// it has been WRONG TWICE: it read `130-` through two shapes whose tail's first key
    /// measured 131, because the row was accepted on "the slice comes back with the payload"
    /// rather than "the key's first glyph is here" (cell 130 was the join space). The trim to
    /// a 128-cell head moved the tail to 130, where the docs had always claimed it, so the
    /// number is unchanged and only now true -- which is exactly why it needs a test rather
    /// than a measurement someone re-derives wrongly a third time.
    ///
    /// The rule, in 1-based cells: `HEAD_CELLS + 2` (head's last cell, join space, first tail
    /// key). Both halves are asserted on one real rendered line.
    #[test]
    fn the_tail_begins_one_space_past_the_head() {
        let mut m = fixture("qwen3", "stop");
        // One tail key only, so "first tail key" is unambiguous and the head keeps its pad.
        m.group_name = None;
        m.queue_ms = None;
        m.cached_tokens = None;
        m.total_tps = 0.0;
        m.concurrent_requests = Some(3);

        let line = format_compact(&m);
        let at = UnicodeWidthStr::width(&line[..line.find("concurrent=").expect("fixture must render a tail key")]);
        assert_eq!(
            at + 1,
            HEAD_CELLS + 2,
            "the first tail key starts at cell {}, and README's `tail` row must say \
             {}- (the head's last cell, then the join space)",
            at + 1,
            HEAD_CELLS + 2
        );
        // The head itself, joined-space excluded, is exactly HEAD_CELLS wide: the two
        // README rows (`whole head 1-{}`, `{}`-) are one fact, not two typed numbers.
        assert_eq!(
            UnicodeWidthStr::width(&line[..at - 1]),
            HEAD_CELLS,
            "the head is not {HEAD_CELLS} cells on a tailed line, so every offset right of \
             the change point is wrong in README: {line}"
        );
    }
}
