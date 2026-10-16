//! Metrics collection from LLM responses

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// Collected metrics from a request/response cycle
#[derive(Debug, Clone, Serialize)]
pub struct RequestMetrics {
    /// Unique request ID
    pub request_id: String,
    /// Timestamp of the request
    pub timestamp: DateTime<Utc>,
    /// Model name
    pub model: String,
    /// Backend group name (only set in multi-backend mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    /// Client identifier (from header or generated)
    pub client_id: Option<String>,
    /// Conversation/session ID
    pub conversation_id: Option<String>,
    /// Number of prompt tokens, or `None` when the backend reported no usage
    /// for it. `None` is the absence of a measurement; `Some(0)` is a measured
    /// zero (a prompt-free request). Formatters print `n/a` for `None` and
    /// exporters omit the field, so an absent count never reads as zero.
    pub prompt_tokens: Option<u64>,
    /// Number of completion tokens. Same absent-vs-zero contract as
    /// `prompt_tokens`.
    pub completion_tokens: Option<u64>,
    /// Total tokens
    pub total_tokens: u64,
    /// Prefill (prompt processing) tokens/sec, or `None` when the backend reported no
    /// prefill rate at all. `None` is the ABSENCE of a measurement; the old `0.0`
    /// sentinel made "the engine never said" indistinguishable from "prefill was
    /// instant", which is how a vLLM body carrying only a ttft could print a decode
    /// rate of `0.00` under a label the README defines as *measured*.
    ///
    /// Exporters OMIT this field when `None` and the plausibility canary never sees it;
    /// `json` serializes it as `null` rather than dropping the key, because it appears
    /// in the documented core example, so every line keeps one schema shape and
    /// `jq '.prompt_tps'` never means "missing" on one backend and "null" on another.
    pub prompt_tps: Option<f64>,
    /// Decode (generation) tokens/sec. Same absent-vs-zero contract as `prompt_tps`.
    /// This is the number an operator actually scans the line for, so the compact
    /// renderer refuses to print a split without it -- see the tier gate there.
    pub generation_tps: Option<f64>,
    /// Prompt processing time in ms. 0.0 when the backend did not report it.
    pub prompt_ms: f64,
    /// Generation time in ms. 0.0 when the backend did not report it.
    pub generation_ms: f64,
    /// (prompt + completion) / wall clock. A COST rate: one duration covering
    /// prefill and decode together. It is not a speed and must never be printed
    /// under a bare `tps=` key -- the compact line shows it as `work=`. Kept
    /// because it is the only throughput figure that spans every token the
    /// request moved, and it reconciles with `sum(total_tokens)` in InfluxDB.
    pub total_tps: f64,
    /// Lower bound on decode speed: completion_tokens / wall clock. Always
    /// computable once the backend counted output tokens, and the best number
    /// available when it reports no split at all -- but the denominator still
    /// contains prefill, network and (in fake mode) synthesis, so it
    /// UNDER-states the engine's decode rate and must never be printed as a
    /// measurement. The compact line renders it as `tps>=`. Populated on
    /// backend-split samples too, so the exporter field is never sparse.
    pub decode_tps_min: f64,
    /// Prompt tokens served from the engine's prefix cache. Subsumed by
    /// `prompt_tokens` on the OpenAI/vLLM shape and by `timings.cache_n` on
    /// llama.cpp; DISJOINT from `input_tokens` on the Anthropic shape, which is
    /// why `context_used` is corrected in `extract_usage` and not here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    /// Prompt tokens written to the prefix cache this request. Only the
    /// Anthropic shape reports it (`cache_creation_input_tokens`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// True when prompt_tps/generation_tps came from the backend rather than
    /// being absent. Lets consumers tell "no split available" from "zero". A
    /// wall-bounded `decode_tps_min` is NOT a split and never sets this.
    pub has_timing_split: bool,
    /// Time the request spent queued before the engine scheduled it, in ms
    /// (vLLM only). This is the direct answer to "is concurrency making
    /// requests wait?" -- a rising queue time means saturation, which a falling
    /// per-stream decode rate on its own does not establish.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_ms: Option<f64>,
    /// Mean inter-token latency in ms (vLLM only). Its reciprocal is
    /// steady-state decode speed with prefill excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_itl_ms: Option<f64>,
    /// Total context size (n_ctx)
    pub context_total: Option<u64>,
    /// Context tokens used
    pub context_used: Option<u64>,
    /// Context usage percentage
    pub context_percent: Option<f64>,
    /// Input message count
    pub input_messages: usize,
    /// Input length (approximate characters)
    pub input_len: usize,
    /// Output length (characters)
    pub output_len: usize,
    /// Whether this was a streaming request
    pub streaming: bool,
    /// Stream ended without the client receiving a whole answer (truncated,
    /// stalled, or backend error). Client disconnects are excluded.
    #[serde(default)]
    pub stream_incomplete: bool,
    /// Why the stream ended: "ok" | "truncated" | "stalled" | "client_gone"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_end: Option<&'static str>,
    /// Finish reason
    pub finish_reason: String,
    /// Request duration in ms
    pub duration_ms: f64,

    // Extended token details (Opencode/Copilot extensions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<u64>,

    /// Number of in-flight requests at the time this request completed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrent_requests: Option<usize>,
}

impl RequestMetrics {
    /// Create a new metrics instance with defaults
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            model: "unknown".to_string(),
            group_name: None,
            client_id: None,
            conversation_id: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: 0,
            prompt_tps: None,
            generation_tps: None,
            prompt_ms: 0.0,
            generation_ms: 0.0,
            total_tps: 0.0,
            decode_tps_min: 0.0,
            cached_tokens: None,
            cache_creation_tokens: None,
            has_timing_split: false,
            queue_ms: None,
            mean_itl_ms: None,
            context_total: None,
            context_used: None,
            context_percent: None,
            input_messages: 0,
            input_len: 0,
            output_len: 0,
            streaming: false,
            stream_incomplete: false,
            stream_end: None,
            finish_reason: "unknown".to_string(),
            duration_ms: 0.0,
            reasoning_tokens: None,
            accepted_prediction_tokens: None,
            rejected_prediction_tokens: None,
            concurrent_requests: None,
        }
    }

    /// Extract metrics from response and request
    pub fn from_response(response: &Value, request: &Value, streaming: bool, duration_ms: f64) -> Self {
        let mut metrics = Self::new();
        metrics.streaming = streaming;
        metrics.duration_ms = duration_ms;

        // Debug: Log the response structure
        tracing::debug!(
            "Extracting metrics from response: {}",
            serde_json::to_string(response).unwrap_or_else(|_| "invalid".to_string())
        );

        // Extract model (check both top-level and nested in message)
        if let Some(model) = response.get("model").and_then(|m| m.as_str()) {
            tracing::debug!("Found model at top level: {}", model);
            metrics.model = model.to_string();
        } else if let Some(model) = response.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
            tracing::debug!("Found model in message object: {}", model);
            metrics.model = model.to_string();
        } else {
            tracing::debug!("No model field found in response");
        }

        extract_usage(&mut metrics, response);

        extract_timings(&mut metrics, response, duration_ms);

        // The ONE writer of `has_timing_split`, derived from the RATES. Both extraction
        // branches used to set this themselves -- the llama.cpp branch unconditionally on
        // seeing a `timings` object, the vLLM branch from `prompt_ms`/`generation_ms` --
        // which is how `tps= 0.00/ 0.00` reached a line whose label means *measured*.
        //
        // This is PROVENANCE only: every INPUT to the rates came from the backend. It
        // does NOT mean the backend computed a rate. On the vLLM path the engine
        // supplies a DURATION (`time_to_first_token_ms`) plus token counts and the
        // proxy does the division -- and after the prefix-cache discount it divides a
        // numerator the engine never reported either. So `prompt_tps` there is
        // backend-sourced arithmetic, not an engine-measured figure; the only value on
        // this line that a backend actually produced is llama.cpp's
        // `predicted_per_second`. Label it accordingly in any dashboard.
        // It deliberately does
        // not claim the durations exist: `prompt_ms`/`generation_ms` keep their own
        // `0.0 == not reported` convention and can be > 0 while this is false, which is
        // a new combination on the wire -- `prompt_ms > 0` used to imply this flag was
        // true -- so any consumer using the flag as a proxy for "durations present" is
        // wrong from here on. Pinned by `split_flag_is_rate_provenance_not_duration_presence`.
        metrics.has_timing_split = metrics.prompt_tps.is_some() || metrics.generation_tps.is_some();

        // Always available, whether or not the backend reported a split.
        // finite_rate is not decoration here: `u64::MAX tokens / MIN_POSITIVE ms`
        // overflows to +inf, serde_json then emits a literal `inf`, and InfluxDB
        // rejects the whole body (see `finite_rate`).
        if duration_ms > 0.0 && metrics.total_tokens > 0 {
            metrics.total_tps = finite_rate(
                (metrics.total_tokens as f64 / duration_ms) * 1000.0,
                "total_tps",
                &metrics.model,
            );
        }

        // The honest floor on decode speed. The wall clock still contains
        // prefill, network and (in fake mode) synthesis, so this UNDER-states
        // the engine's decode rate -- but it can never over-state it, which is
        // exactly the property `total_tps` lacked. Runs after `extract_timings`
        // so the timings token-count fallback has already had its turn, and
        // through `finite_rate` because a derived division is where +inf
        // actually enters this struct.
        let counted_output = metrics.completion_tokens.unwrap_or(0) as f64;
        if duration_ms > 0.0 && counted_output > 0.0 {
            metrics.decode_tps_min = finite_rate(counted_output / duration_ms * 1000.0, "decode_tps_min", &metrics.model);
        }

        extract_length_fallbacks(&mut metrics, response, request);

        metrics
    }

    /// Calculate context percentage
    pub fn calculate_context_percent(&mut self) {
        if let (Some(used), Some(total)) = (self.context_used, self.context_total) {
            if total > 0 {
                self.context_percent = Some((used as f64 / total as f64) * 100.0);
            }
        }
    }

    /// True when the backend told us anything at all about throughput: a token
    /// count or a rate.
    ///
    /// The negative case is a stream the client abandoned before the backend's
    /// final chunk (llama.cpp carries `usage` and `timings` only there) or a
    /// backend error body. Such a sample still has a real `duration_ms`, but its
    /// token counts are `None` and every tokens/sec field in it is zero: that is
    /// the *absence of a measurement*, not a slow request. Handing it to an
    /// exporter writes a wall of zeros that drags every aggregate toward the
    /// floor, so callers drop it and log it instead - see
    /// [`crate::exporters::log_sample_and_should_export`]. `None` token counts
    /// ARE that documented skip condition; they are never exported as zeros.
    ///
    /// Deliberately not a check on `stream_end`: a client-gone stream that *did*
    /// receive the final chunk carries a genuine measurement and is exported.
    /// The cost of the filter is that aborted load the proxy never observed stops
    /// appearing in InfluxDB, so `sum(total_tokens)` there under-counts real
    /// backend work. [`crate::exporters::EXPORTS_SKIPPED_TOTAL`] is the number to
    /// reconcile that gap with.
    pub fn has_throughput_signal(&self) -> bool {
        // `total_tokens` is not redundant with `total_tps`: the latter is only
        // computed when duration_ms > 0 (see above), so a request finishing
        // inside the timer's resolution would otherwise read as "no signal".
        // `decode_tps_min` is likewise not worth a term of its own: it is
        // derived from `completion_tokens`, so a nonzero floor always implies
        // `total_tokens > 0` and the disjunction below already admits it.
        //
        // The rate terms stay `> 0.0` and are NOT `.is_some()`. This is a filter that
        // decides what reaches every exporter, not a renderer: its documented purpose
        // is to stop a run of literal zeros reaching every throughput aggregate, and
        // the aborted-stream traffic it targets is exactly the class that reports
        // `predicted_per_second: 0.0`. `.is_some()` would promote those samples to
        // "signal" and post the zeros this exists to block. `finite_rate_opt` already
        // maps 0.0 to `None`; this guard is the second lock, so a caller that
        // constructs a sample directly cannot defeat the filter.
        self.total_tokens > 0
            || self.total_tps > 0.0
            || self.prompt_tps.is_some_and(|v| v > 0.0)
            || self.generation_tps.is_some_and(|v| v > 0.0)
    }

    /// How the request ended, as a label. Shared by the log line and the InfluxDB
    /// `stream_end` tag so the two cannot drift apart.
    pub fn stream_end_label(&self) -> &'static str {
        match self.stream_end {
            Some(end) => end,
            None if !self.streaming => "sync",
            None => "unknown",
        }
    }
}

impl Default for RequestMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract usage counts (support both OpenAI and Anthropic formats) and the
/// Opencode/Copilot `completion_tokens_details` extensions.
fn extract_usage(metrics: &mut RequestMetrics, response: &Value) {
    // Extract usage (support both OpenAI and Anthropic formats)
    if let Some(usage) = response.get("usage") {
        tracing::debug!("Found usage: {:?}", usage);

        // Try OpenAI format first
        if let Some(prompt) = usage.get("prompt_tokens").and_then(|t| t.as_u64()) {
            metrics.prompt_tokens = Some(prompt);
            metrics.completion_tokens = usage.get("completion_tokens").and_then(|t| t.as_u64());
            metrics.total_tokens = usage
                .get("total_tokens")
                .and_then(|t| t.as_u64())
                .unwrap_or_else(|| prompt.saturating_add(metrics.completion_tokens.unwrap_or(0)));
            // Prefix-cache accounting. On this shape `prompt_tokens` ALREADY
            // contains the cached prefix, so this is reporting only: it lets a
            // reader discount a wall-clock rate, and it must not be added to
            // `context_used` (see the Anthropic branch for the opposite rule).
            metrics.cached_tokens = usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|t| t.as_u64());
        }
        // Try Anthropic format
        else if let Some(input) = usage.get("input_tokens").and_then(|t| t.as_u64()) {
            metrics.prompt_tokens = Some(input);
            metrics.completion_tokens = usage.get("output_tokens").and_then(|t| t.as_u64());
            metrics.total_tokens = input.saturating_add(metrics.completion_tokens.unwrap_or(0));
            metrics.cached_tokens = usage.get("cache_read_input_tokens").and_then(|t| t.as_u64());
            metrics.cache_creation_tokens = usage.get("cache_creation_input_tokens").and_then(|t| t.as_u64());
            // Anthropic's `input_tokens` counts only the tokens it did NOT read
            // from or write to the cache, so it is disjoint from the two fields
            // above -- unlike OpenAI's `prompt_tokens`, which subsumes them.
            // The engine's KV occupancy is therefore the SUM, and this is the
            // one place allowed to add them. `extract_timings` defers to a
            // `context_used` that is already set for exactly this reason.
            metrics.context_used = Some(
                input
                    .saturating_add(metrics.cached_tokens.unwrap_or(0))
                    .saturating_add(metrics.cache_creation_tokens.unwrap_or(0)),
            );
        }

        // Extract extended usage details (Opencode/Copilot extensions)
        if let Some(details) = usage.get("completion_tokens_details") {
            metrics.reasoning_tokens = details.get("reasoning_tokens").and_then(|t| t.as_u64());
            metrics.accepted_prediction_tokens = details.get("accepted_prediction_tokens").and_then(|t| t.as_u64());
            metrics.rejected_prediction_tokens = details.get("rejected_prediction_tokens").and_then(|t| t.as_u64());
        }
    } else {
        tracing::debug!("No usage field found in response");
    }
}

/// Gate a backend-sourced f64 metric (a rate or a duration) for the wire.
///
/// serde_json's f64 parse maps any too-large literal (1e400) to +inf, and a
/// derived `tokens / ms * 1000` overflows to +inf on subnormal inputs. An inf
/// anywhere in the exporter's JSON body is rejected wholesale (InfluxDB 400),
/// the write retry ladder burns four attempts per sample, and the sample dies
/// counted-but-silent. The metrics contract already renders 0.0 as "not
/// reported", so a non-finite value maps to 0.0 and is DEBUG-named.
fn finite_rate(value: f64, field: &str, model: &str) -> f64 {
    if value.is_finite() {
        return value;
    }
    tracing::debug!(field, model, raw = %value, "non-finite backend metric - reporting 0.0 (not reported), never inf");
    0.0
}

/// `finite_rate` for the RATE fields, whose absent marker is `None`.
///
/// A deliberate SIBLING, not a signature change: `finite_rate`'s `0.0 == absent`
/// contract is what the millisecond fields still rely on, and routing a rate through
/// the `f64` version would yield `Some(0.0)` -- the fabricated zero smuggled *inside*
/// an `Option`, which is the exact defect this shape exists to remove.
///
/// Non-finite maps to `None`, not to `Some(0.0)`: an overflowed `tokens/ms*1000` is
/// not a measurement of anything. So is an exact `0.0` -- a rate of zero tokens per
/// second is not a slow speed, it is the absence of a speed. The reachable source is
/// the llama.cpp corrected-prefill path when `prompt_n == cache_n` (a full prefix
/// cache hit: zero tokens were computed), which would otherwise read as "prefill ran
/// at 0 tok/s". `has_throughput_signal` ALSO gates on `> 0.0`; both guards stay, so
/// neither the export filter nor `has_timing_split` can be poisoned by a caller that
/// bypasses this function.
fn finite_rate_opt(value: f64, field: &str, model: &str) -> Option<f64> {
    if value.is_finite() && value > 0.0 {
        return Some(value);
    }
    tracing::debug!(field, model, raw = %value, "backend rate not usable - reporting absent (not reported), never 0.0 or inf");
    None
}

/// Extract llama.cpp `timings`, with the token-count fallback and, when timings
/// are absent, the context_used fallback and the vLLM `metrics` path.
fn extract_timings(metrics: &mut RequestMetrics, response: &Value, duration_ms: f64) {
    // Extract timings (llama.cpp specific)
    if let Some(timings) = response.get("timings") {
        tracing::debug!("Found timings: {:?}", timings);
        metrics.prompt_ms = timings
            .get("prompt_ms")
            .and_then(|t| t.as_f64())
            .map(|v| finite_rate(v, "prompt_ms", &metrics.model))
            .unwrap_or(0.0);
        metrics.generation_ms = timings
            .get("predicted_ms")
            .and_then(|t| t.as_f64())
            .map(|v| finite_rate(v, "generation_ms", &metrics.model))
            .unwrap_or(0.0);
        // `None` when the key is absent. The old `.unwrap_or(0.0)` here was a second
        // fabrication site: it reported "0 tok/s" for a field the engine never sent.
        metrics.generation_tps = timings
            .get("predicted_per_second")
            .and_then(|t| t.as_f64())
            .and_then(|v| finite_rate_opt(v, "generation_tps", &metrics.model));

        // Read prompt_n / cache_n from timings before assigning them, so the
        // corrected prefill rate can use them.
        let prompt_n = timings.get("prompt_n").and_then(|t| t.as_u64());
        let cache_n = timings.get("cache_n").and_then(|t| t.as_u64());

        // llama.cpp computes `prompt_per_second = prompt_n / prompt_ms * 1000`.
        // With a prefix cache, `prompt_n` includes the cached tokens but
        // `prompt_ms` only covers the non-cached compute, so the backend's
        // figure is inflated (e.g. 164,663 tokens reported at 880k tok/s when
        // only ~567 were actually computed). Rate the tokens the engine really
        // processed: (prompt_n - cache_n) / prompt_ms * 1000.
        // Plan 1's Open Unknown 1 is still open and CANNOT be settled here: it asks
        // whether llama.cpp already excludes cached tokens from `prompt_n`, in which
        // case this double-subtracts and the backend value should stand. The live
        // backend is vLLM 0.33.0 (T1: `/props` 404, and no `timings` object appears in
        // any of the six captured payloads), so this entire branch is UNEXERCISED on
        // this box and no capture can speak to it. Settling it requires a llama.cpp
        // backend -- do not "verify" it against the running server, which never
        // reaches this code. Note the two branches deliberately differ: this one may
        // saturate because `prompt_n - cache_n` is llama.cpp's own accounting, whereas
        // the vLLM path below treats `cached > prompt` as a data error to report.
        let corrected_prompt_tps = prompt_n
            .zip(cache_n)
            .filter(|(_, cached)| *cached > 0)
            .filter(|_| metrics.prompt_ms > 0.0)
            .map(|(used, cached)| {
                let computed = used.saturating_sub(cached) as f64;
                computed / metrics.prompt_ms * 1000.0
            });
        metrics.prompt_tps = corrected_prompt_tps
            .or_else(|| timings.get("prompt_per_second").and_then(|t| t.as_f64()))
            .and_then(|v| finite_rate_opt(v, "prompt_tps", &metrics.model));
        // `has_timing_split` is NOT set here. It is derived once, from the rates, at
        // the end of `from_response` -- see the note there on why a branch that only
        // saw a `timings` object must not claim a split it produced no rate for.

        // Context info - use prompt_n for actual context consumption
        if let Some(prompt_n) = prompt_n {
            metrics.context_used = Some(prompt_n);
        }

        // llama.cpp spells the prefix-cache count `cache_n`, and like OpenAI's
        // `cached_tokens` it is SUBSUMED by prompt_n, so context_used above
        // stays correct untouched. extract_timings runs after extract_usage, so
        // where a body somehow carries both spellings this one wins: llama.cpp
        // is authoritative for its own extension, and vLLM never sends timings.
        metrics.cached_tokens = cache_n;

        // Fallback to timings for token counts when usage is missing (e.g., timeout
        // scenarios). Absent and zero are one trigger class here, matching the
        // pre-Option baseline where absence was stored as 0.
        if metrics.prompt_tokens.unwrap_or(0) == 0 && metrics.completion_tokens.unwrap_or(0) == 0 {
            metrics.prompt_tokens = timings.get("prompt_n").and_then(|t| t.as_u64());
            metrics.completion_tokens = timings.get("predicted_n").and_then(|t| t.as_u64());
            metrics.total_tokens = metrics
                .prompt_tokens
                .unwrap_or(0)
                .saturating_add(metrics.completion_tokens.unwrap_or(0));

            tracing::debug!(
                "Using timings fallback for token counts: prompt={:?}, completion={:?}",
                metrics.prompt_tokens,
                metrics.completion_tokens
            );
        }
    } else {
        tracing::debug!("No timings field found in response");

        // If no timings, use prompt_tokens as context_used fallback
        // (Anthropic format responses have usage.input_tokens but no timings).
        // Skipped when the shape-specific code already set it: on the Anthropic
        // shape prompt_tokens alone omits the cached prefix entirely, and
        // extract_usage wrote the correct sum.
        if metrics.context_used.is_none() {
            if let Some(prompt_tokens) = metrics.prompt_tokens.filter(|p| *p > 0) {
                metrics.context_used = Some(prompt_tokens);
                tracing::debug!("Using prompt_tokens as context_used: {}", prompt_tokens);
            }
        }

        extract_vllm_usage(metrics, response, duration_ms);
    }
}

/// Extract vLLM per-request `metrics` (needs --enable-per-request-metrics);
/// with neither timings nor metrics, total throughput is all a duration supports.
fn extract_vllm_usage(metrics: &mut RequestMetrics, response: &Value, duration_ms: f64) {
    // No `timings` means the backend did not tell us where the time went.
    // `timings` is llama.cpp-specific (prompt_per_second/predicted_per_second),
    // so every vLLM response lands here.
    //
    // This used to split the wall clock 20% prompt / 80% generation and
    // divide the token counts by those. That is not a measurement, and it
    // reads as one: on a vLLM server whose real prefill was measured at
    // ~14.5k tok/s it reported 113,344 tok/s, purely because a 39k-token
    // prompt was divided by 20% of a 1.7s request. The two figures were
    // also locked to each other -- prompt_tps/generation_tps was always
    // exactly 4 * prompt_tokens/completion_tokens -- so they carried no
    // information the token counts did not already carry, while making
    // short-completion requests look like a throughput collapse.
    //
    // Report only what a single duration can support: total throughput.
    // A real split needs either backend timings or a measured TTFT, which
    // is only observable on streaming responses.
    // vLLM reports the same information under `metrics`, but only when
    // the server was started with --enable-per-request-metrics. Without
    // that flag the key is present and null, which is exactly how this
    // code ended up estimating instead of measuring.
    //
    // Field semantics are vLLM's own: generation_time_ms is the decode
    // interval alone (first output token -> last), excluding queue wait
    // and prefill; time_to_first_token_ms is measured from scheduling,
    // so it excludes queue wait too.
    if let Some(vm) = response.get("metrics").filter(|v| !v.is_null()) {
        // A usable backend measurement is FINITE and at least f64's smallest
        // NORMAL number: JSON's f64 parse turns 1e-400 into 0.0 and clamps
        // 1e-45..1e-308 to SUBNORMALS, and `*v > 0.0` admits every one of
        // them. A subnormal ttft/gen_ms then overflows tokens/ms*1000 to
        // +inf, the exporter's JSON body carries a literal `inf`, InfluxDB
        // answers 400, the retry ladder burns 4 attempts per sample, and the
        // loss is silent. Below normal range there is no timing signal.
        let f = |k: &str| {
            vm.get(k)
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite() && *v >= f64::MIN_POSITIVE)
        };

        metrics.queue_ms = f("queue_time_ms");
        metrics.mean_itl_ms = f("mean_itl_ms");

        if let Some(ttft) = f("time_to_first_token_ms") {
            metrics.prompt_ms = ttft;
            if let Some(prompt_tokens) = metrics.prompt_tokens.filter(|p| *p > 0) {
                // vLLM's `prompt_tokens` counts tokens served from the prefix cache,
                // while `time_to_first_token_ms` covers only the compute the engine
                // actually did. Dividing one by the other therefore rates tokens that
                // were never processed: the captured 186,417-token request at a
                // 321.6 ms ttft reads 580k tok/s, when 182,400 of those tokens were
                // cached and the honest figure is 12,491. Same correction the
                // llama.cpp branch above applies, with this path's inputs.
                //
                // The denominator stays `ttft` alone. T1 measured `queue_time_ms` at
                // 0.0077-0.0118 ms across all six captures, so subtracting it would
                // move the rate by well under 0.01%; and vLLM measures ttft from
                // scheduling, which already excludes queue wait. Cite the captures,
                // not a guess.
                //
                // `cached_tokens` is a subset of `prompt_tokens` ONLY on the OpenAI
                // usage shape (extract_usage: "prompt_tokens ALREADY contains the
                // cached prefix"). The Anthropic branch fills it from
                // `cache_read_input_tokens`, which is DISJOINT from `input_tokens` --
                // which is exactly why `context_used` there is the SUM of the three.
                // Subtracting it on that shape deletes a prefix that was never in the
                // numerator and under-states prefill by the whole cached amount, and
                // nothing downstream can see it: the rate merely looks slow. Stats are
                // built from the backend's raw body and this path runs for any body
                // without `timings`, so an Anthropic-format backend that also passes
                // vLLM per-request metrics through reaches it.
                let cache_is_a_subset = response.get("usage").is_some_and(|u| u.get("prompt_tokens").is_some());
                let raw_rate = prompt_tokens as f64 / ttft * 1000.0;
                let computed_tokens = match metrics.cached_tokens.filter(|_| cache_is_a_subset).filter(|c| *c > 0) {
                    Some(c) if c < prompt_tokens => Some(prompt_tokens - c),
                    Some(c) if c == prompt_tokens => {
                        // Every token cached is a legitimate state, not a data error --
                        // `finite_rate_opt`'s own doc names the full prefix cache hit as
                        // reachable. So this is DEBUG, not WARN: warning per request on
                        // healthy traffic is a flood. It is also the worst case for the
                        // number itself -- ttft is at its minimum here, so the
                        // uncorrected rate is the most inflated it can be.
                        tracing::debug!(
                            model = %metrics.model,
                            prompt_tokens,
                            cached_tokens = c,
                            time_to_first_token_ms = ttft,
                            reported_prompt_tps = raw_rate,
                            "cached_tokens == prompt_tokens: full prefix cache hit (or a backend data error); reporting the uncorrected prefill rate"
                        );
                        None
                    }
                    Some(c) => {
                        // More cached than sent is unambiguously a backend DATA ERROR.
                        // Saturating to zero would report a full-cache prefill that never
                        // happened, and blanking to `None` is indistinguishable on the
                        // wire from "this backend reports no split" -- either would erase
                        // a rate that really was measured. (Saturating is not even a third
                        // option: `finite_rate_opt` maps 0.0 to None, so saturate and blank
                        // are the same wire outcome.) Keep the uncorrected figure, and say
                        // so loudly -- this number ships to InfluxDB as `measured`.
                        tracing::warn!(
                            model = %metrics.model,
                            prompt_tokens,
                            cached_tokens = c,
                            time_to_first_token_ms = ttft,
                            queue_ms = ?metrics.queue_ms,
                            reported_prompt_tps = raw_rate,
                            "cached_tokens > prompt_tokens: backend data error, reporting the uncorrected prefill rate"
                        );
                        None
                    }
                    None => None,
                };
                if let Some(computed) = computed_tokens {
                    tracing::debug!(
                        model = %metrics.model,
                        prompt_tokens,
                        cached_tokens = prompt_tokens - computed,
                        computed_tokens = computed,
                        time_to_first_token_ms = ttft,
                        "prefill rated on non-cached tokens only"
                    );
                }
                let numerator = computed_tokens.unwrap_or(prompt_tokens) as f64;
                metrics.prompt_tps = finite_rate_opt(numerator / ttft * 1000.0, "prompt_tps", &metrics.model);
            }
        }
        if let Some(gen_ms) = f("generation_time_ms") {
            metrics.generation_ms = gen_ms;
            if let Some(completion_tokens) = metrics.completion_tokens.filter(|c| *c > 0) {
                metrics.generation_tps =
                    finite_rate_opt(completion_tokens as f64 / gen_ms * 1000.0, "generation_tps", &metrics.model);
            }
        }
        // `has_timing_split` is deliberately NOT assigned here. It used to read
        // `prompt_ms > 0.0 || generation_ms > 0.0` -- derived from the DURATIONS, while
        // the two rates above are only set when the matching TOKEN COUNT is present.
        // That mismatch is what produced the fabricated split: a body carrying
        // `time_to_first_token_ms` with no `prompt_tokens` got a true "measured split"
        // flag with both rates still zero, and the compact line printed
        // `tps=  0.00/  0.00` under a label meaning *measured*. It is derived once from
        // the rates at the end of `from_response`.

        tracing::debug!(
            "vLLM per-request metrics: ttft={:.1}ms gen={:.1}ms queue={:?}ms itl={:?}ms",
            metrics.prompt_ms,
            metrics.generation_ms,
            metrics.queue_ms,
            metrics.mean_itl_ms
        );
    } else if duration_ms > 0.0 && metrics.total_tokens > 0 {
        tracing::debug!(
            "No backend timings and no vLLM metrics (is the server missing \
                     --enable-per-request-metrics?); reporting total throughput only: {:.2} tok/s",
            (metrics.total_tokens as f64 / duration_ms) * 1000.0
        );
    }
}

/// Extract finish reason and output length (OpenAI and Anthropic shapes) plus
/// the request-side message count and input length.
fn extract_length_fallbacks(metrics: &mut RequestMetrics, response: &Value, request: &Value) {
    // Extract finish reason and output length (support both OpenAI and Anthropic formats)
    // Try OpenAI format first
    if let Some(choices) = response.get("choices").and_then(|c| c.as_array()) {
        if let Some(first_choice) = choices.first() {
            metrics.finish_reason = first_choice
                .get("finish_reason")
                .and_then(|f| f.as_str())
                .unwrap_or("unknown")
                .to_string();

            // Extract output length
            if let Some(content) = first_choice
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                metrics.output_len = content.len();
            }
        }
    }
    // Try Anthropic format
    else if let Some(stop_reason) = response.get("stop_reason").and_then(|f| f.as_str()) {
        metrics.finish_reason = stop_reason.to_string();

        // Extract output length from content array
        if let Some(content_array) = response.get("content").and_then(|c| c.as_array()) {
            metrics.output_len = content_array
                .iter()
                .filter_map(|item| {
                    // Sum up text content lengths
                    item.get("text").and_then(|t| t.as_str()).map(|s| s.len())
                })
                .sum();
        }
    }

    // Extract request info
    if let Some(messages) = request.get("messages").and_then(|m| m.as_array()) {
        metrics.input_messages = messages.len();
        metrics.input_len = messages
            .iter()
            .map(|m| {
                m.get("content")
                    .and_then(|c| match c {
                        Value::String(s) => Some(s.len()),
                        Value::Array(arr) => Some(
                            arr.iter()
                                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                .map(|s| s.len())
                                .sum(),
                        ),
                        _ => None,
                    })
                    .unwrap_or(0)
            })
            .sum();
    }
}

/// Context information from slots endpoint
#[derive(Debug, Clone)]
pub struct ContextInfo {
    pub total_context: u64,
    pub used_context: u64,
    pub slots: Vec<SlotMetrics>,
}

/// Per-slot metrics
#[derive(Debug, Clone)]
pub struct SlotMetrics {
    pub slot_id: u64,
    pub n_tokens: u64,
    pub n_ctx: u64,
    pub is_processing: bool,
}

impl ContextInfo {
    /// Parse from /slots response
    pub fn from_slots_response(response: &Value) -> Option<Self> {
        let slots = response.as_array()?;
        let mut total_context: u64 = 0;
        let mut used_context: u64 = 0;
        let mut slot_metrics = Vec::new();

        for slot in slots {
            let slot_id = slot.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            let n_ctx = slot.get("n_ctx").and_then(|n| n.as_u64()).unwrap_or(0);
            let n_tokens = slot.get("n_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
            let is_processing = slot.get("is_processing").and_then(|p| p.as_bool()).unwrap_or(false);

            total_context = total_context.saturating_add(n_ctx);
            used_context = used_context.saturating_add(n_tokens);

            slot_metrics.push(SlotMetrics {
                slot_id,
                n_tokens,
                n_ctx,
                is_processing,
            });
        }

        Some(Self {
            total_context,
            used_context,
            slots: slot_metrics,
        })
    }
}

/// Merge policy for the per-chunk stats slice of a streaming response:
/// which envelope fields of one SSE chunk reach the accumulator, and under
/// what condition. The stream framer owns delta assembly; this owns the
/// measurement fields, so a new stats-carrying backend extension lands in
/// one place.
///
/// - `usage`: last present chunk wins verbatim (never deep-merged).
/// - `model`: llama.cpp sends it once in the first chunk; a null resend
///   must not clear the recorded model.
/// - `timings`: llama.cpp extension carried on the final chunk.
/// - `metrics`: vLLM's equivalent of `timings`, on the usage chunk.
///   Without it the collector sees neither key and every tokens/sec column
///   is empty on a vLLM backend, because the wall-clock estimate it used to
///   fall back to was removed as not-a-measurement. A null `metrics` is not
///   a measurement and never replaces a measured block (vLLM sends
///   `metrics: null` unless started with --enable-per-request-metrics).
pub fn merge_stream_stats(acc: &mut Value, chunk: &Value) {
    if let Some(usage) = chunk.get("usage") {
        acc["usage"] = usage.clone();
    }
    if let Some(model) = chunk.get("model") {
        if !model.is_null() {
            acc["model"] = model.clone();
        }
    }
    if let Some(timings) = chunk.get("timings") {
        acc["timings"] = timings.clone();
    }
    if let Some(metrics) = chunk.get("metrics").filter(|v| !v.is_null()) {
        acc["metrics"] = metrics.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::merge_stream_stats;
    use serde_json::json;

    #[test]
    fn merge_stream_stats_usage_last_present_chunk_wins() {
        let mut acc = json!({"usage": {"prompt_tokens": 4}});
        merge_stream_stats(&mut acc, &json!({"usage": {"prompt_tokens": 7, "completion_tokens": 9}}));
        assert_eq!(acc["usage"]["completion_tokens"].as_u64(), Some(9));
        merge_stream_stats(&mut acc, &json!({"choices": []}));
        assert_eq!(acc["usage"]["prompt_tokens"].as_u64(), Some(7));
    }

    #[test]
    fn merge_stream_stats_null_model_resend_does_not_clear_it() {
        let mut acc = json!({"model": "qwen3"});
        merge_stream_stats(&mut acc, &json!({"model": null}));
        assert_eq!(acc["model"].as_str(), Some("qwen3"));
        merge_stream_stats(&mut acc, &json!({"model": "qwen3-coder"}));
        assert_eq!(acc["model"].as_str(), Some("qwen3-coder"));
    }

    #[test]
    fn merge_stream_stats_timings_land_and_persist() {
        let mut acc = json!({});
        merge_stream_stats(&mut acc, &json!({"timings": {"prompt_ms": 50.5}}));
        merge_stream_stats(&mut acc, &json!({"choices": []}));
        assert_eq!(acc["timings"]["prompt_ms"].as_f64(), Some(50.5));
    }

    #[test]
    fn merge_stream_stats_null_metrics_never_replaces_measured_block() {
        // vLLM sends `metrics: null` unless started with --enable-per-request-metrics.
        let mut acc = json!({"metrics": {"generation_time_ms": 562.0}});
        merge_stream_stats(&mut acc, &json!({"usage": {"completion_tokens": 1}, "metrics": null}));
        assert_eq!(acc["metrics"]["generation_time_ms"].as_f64(), Some(562.0));
        merge_stream_stats(&mut acc, &json!({"metrics": {"tokens_per_second": 84.1}}));
        assert_eq!(acc["metrics"]["tokens_per_second"].as_f64(), Some(84.1));
    }
    use super::*;

    #[test]
    fn test_request_metrics_new() {
        let metrics = RequestMetrics::new();

        assert!(!metrics.request_id.is_empty());
        assert_eq!(metrics.model, "unknown");
        assert!(metrics.client_id.is_none());
        assert!(metrics.conversation_id.is_none());
        assert_eq!(metrics.prompt_tokens, None);
        assert_eq!(metrics.completion_tokens, None);
        assert_eq!(metrics.total_tokens, 0);
        assert_eq!(metrics.prompt_tps, None, "no split reported is absent, not zero");
        assert_eq!(metrics.generation_tps, None);
        assert!(!metrics.streaming);
        assert_eq!(metrics.finish_reason, "unknown");
    }

    #[test]
    fn test_request_metrics_default() {
        let metrics = RequestMetrics::default();
        assert_eq!(metrics.model, "unknown");
    }

    #[test]
    fn test_request_metrics_from_response_basic() {
        let response = serde_json::json!({
            "model": "test-model",
            "choices": [{
                "finish_reason": "stop",
                "message": {"content": "Hello world"}
            }]
        });

        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 100.0);

        assert_eq!(metrics.model, "test-model");
        assert_eq!(metrics.finish_reason, "stop");
        assert_eq!(metrics.output_len, 11); // "Hello world"
        assert!(!metrics.streaming);
        assert_eq!(metrics.duration_ms, 100.0);
    }

    #[test]
    fn test_request_metrics_from_response_with_usage() {
        let response = serde_json::json!({
            "model": "test-model",
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            },
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({});

        let metrics = RequestMetrics::from_response(&response, &request, true, 200.0);

        assert_eq!(metrics.prompt_tokens, Some(100));
        assert_eq!(metrics.completion_tokens, Some(50));
        assert_eq!(metrics.total_tokens, 150);
        assert!(metrics.streaming);
    }

    #[test]
    fn test_request_metrics_from_response_with_timings() {
        let response = serde_json::json!({
            "model": "test-model",
            "timings": {
                "prompt_ms": 50.5,
                "predicted_ms": 100.25,
                "prompt_per_second": 198.0,
                "predicted_per_second": 99.75,
                "prompt_n": 42,
                "cache_n": 10
            },
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({});

        let metrics = RequestMetrics::from_response(&response, &request, false, 150.0);

        assert_eq!(metrics.prompt_ms, 50.5);
        assert_eq!(metrics.generation_ms, 100.25);
        // Prefix cache present: corrected prefill rate = (42 - 10) / 50.5 * 1000
        assert!(
            metrics.prompt_tps.is_some_and(|v| (v - 633.66).abs() < 0.01),
            "prompt_tps = {:?}",
            metrics.prompt_tps
        );
        assert_eq!(metrics.generation_tps, Some(99.75));
        assert_eq!(metrics.cached_tokens, Some(10));
        assert_eq!(metrics.context_used, Some(42)); // Uses prompt_n, not cache_n
    }

    #[test]
    fn test_request_metrics_prefill_rate_no_cache_uses_backend_verbatim() {
        let response = serde_json::json!({
            "model": "test-model",
            "timings": {
                "prompt_ms": 50.5,
                "prompt_per_second": 198.0,
                "predicted_per_second": 99.75,
                "prompt_n": 42,
            },
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({});
        let metrics = RequestMetrics::from_response(&response, &request, false, 150.0);

        // No cache_n -> backend prompt_per_second is reported verbatim.
        assert_eq!(metrics.prompt_tps, Some(198.0));
        assert_eq!(metrics.cached_tokens, None);
    }

    #[test]
    fn test_request_metrics_from_response_with_messages() {
        let response = serde_json::json!({
            "model": "test-model",
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hello there"}
            ]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 50.0);

        assert_eq!(metrics.input_messages, 2);
        // "You are helpful" (15) + "Hello there" (11) = 26
        assert_eq!(metrics.input_len, 26);
    }

    #[test]
    fn test_request_metrics_from_response_multimodal_content() {
        let response = serde_json::json!({
            "model": "test-model",
            "choices": [{"finish_reason": "stop"}]
        });

        let request = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's in this image?"},
                    {"type": "image_url", "image_url": {"url": "http://example.com/image.png"}}
                ]
            }]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 50.0);

        assert_eq!(metrics.input_messages, 1);
        assert_eq!(metrics.input_len, 21); // "What's in this image?"
    }

    #[test]
    fn test_request_metrics_from_response_no_choices() {
        let response = serde_json::json!({
            "model": "test-model"
        });

        let request = serde_json::json!({});

        let metrics = RequestMetrics::from_response(&response, &request, false, 50.0);

        assert_eq!(metrics.finish_reason, "unknown");
        assert_eq!(metrics.output_len, 0);
    }

    #[test]
    fn test_request_metrics_calculate_context_percent() {
        let mut metrics = RequestMetrics::new();
        metrics.context_used = Some(50);
        metrics.context_total = Some(100);

        metrics.calculate_context_percent();

        assert_eq!(metrics.context_percent, Some(50.0));
    }

    #[test]
    fn test_request_metrics_calculate_context_percent_zero_total() {
        let mut metrics = RequestMetrics::new();
        metrics.context_used = Some(50);
        metrics.context_total = Some(0);

        metrics.calculate_context_percent();

        assert_eq!(metrics.context_percent, None);
    }

    #[test]
    fn test_request_metrics_calculate_context_percent_missing_values() {
        let mut metrics = RequestMetrics::new();

        metrics.calculate_context_percent();

        assert_eq!(metrics.context_percent, None);
    }

    #[test]
    fn test_has_throughput_signal_needs_a_token_count_or_a_rate() {
        let mut m = RequestMetrics::new();
        assert!(!m.has_throughput_signal(), "an all-zero sample is not a measurement");

        m.total_tokens = 1;
        assert!(m.has_throughput_signal());

        // total_tps is only computed when duration_ms > 0, so tokens alone must
        // count - otherwise a request faster than the timer's resolution reads as
        // having no signal.
        let mut m = RequestMetrics::new();
        m.total_tokens = 100;
        assert!(m.has_throughput_signal(), "total_tokens must not be redundant");

        for rate in ["total_tps", "prompt_tps", "generation_tps"] {
            let mut m = RequestMetrics::new();
            match rate {
                "total_tps" => m.total_tps = 12.5,
                "prompt_tps" => m.prompt_tps = Some(12.5),
                _ => m.generation_tps = Some(12.5),
            }
            assert!(m.has_throughput_signal(), "{rate} alone should be a signal");
        }
    }

    /// The second lock on the export filter. `has_timing_split` is derived with
    /// `is_some()`, so a `Some(0.0)` that reached the struct by any route other than
    /// the extractor would otherwise certify itself as a measurement and ship a run
    /// of literal zeros into every throughput aggregate -- the exact failure the
    /// filter exists to stop.
    #[test]
    fn a_constructed_zero_rate_is_not_a_throughput_signal() {
        let mut m = RequestMetrics::new();
        m.prompt_tps = Some(0.0);
        m.generation_tps = Some(0.0);
        assert!(
            !m.has_throughput_signal(),
            "zero is absence, not a slow measurement -- and not a signal"
        );
    }

    /// The collector's side of the same rule: a rate the backend never usablely
    /// reported is `None`, never `0.0` and never `inf`.
    #[test]
    fn finite_rate_opt_reports_absent_not_zero() {
        let model = "test-model".to_string();
        assert_eq!(finite_rate_opt(42.5, "x", &model), Some(42.5));
        assert_eq!(finite_rate_opt(0.0, "x", &model), None, "0.0 is not reported");
        assert_eq!(finite_rate_opt(-1.0, "x", &model), None);
        assert_eq!(finite_rate_opt(f64::NAN, "x", &model), None);
        assert_eq!(finite_rate_opt(f64::INFINITY, "x", &model), None);
    }

    /// `has_timing_split` is a statement about RATES, not about durations. A
    /// backend that sends `timings` carrying only millisecond fields proves the
    /// request took time -- it does not prove anything measured a prefill/decode
    /// split. This is the assignment that used to live in the llama branch as
    /// `= true` and in the vLLM branch as `prompt_ms > 0.0 || ...`, and it is what
    /// produced the fabricated `tps= 0.00/ 0.00`.
    #[test]
    fn split_flag_is_rate_provenance_not_duration_presence() {
        let response = serde_json::json!({
            "model": "test-model",
            "timings": { "prompt_ms": 500.0, "predicted_ms": 100.0 },
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 },
            "choices": [{ "finish_reason": "stop" }]
        });
        let request = serde_json::json!({ "messages": [] });

        let m = RequestMetrics::from_response(&response, &request, false, 1000.0);

        assert!(m.prompt_ms > 0.0, "the duration is a real measurement and survives");
        assert_eq!(m.prompt_tps, None, "no rate reported");
        assert_eq!(m.generation_tps, None, "no rate reported");
        assert!(!m.has_timing_split, "milliseconds are not a split");
        assert!(m.total_tps > 0.0, "the cost rate is still computable, and still a cost");
    }

    /// The operator's line: 24.8s of wall clock, `stream=gone finish=unknown`,
    /// because llama.cpp carries `usage`/`timings` only in the chunk that never
    /// arrived. Duration is real; throughput is absent.
    #[test]
    fn test_aborted_stream_without_usage_has_no_signal() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "choices": [{"index": 0, "delta": {"content": "partial"}, "finish_reason": null}]
        });
        let request = serde_json::json!({"messages": [{"role": "user", "content": "hi"}]});

        let m = RequestMetrics::from_response(&response, &request, true, 24811.0);

        assert_eq!(m.total_tokens, 0);
        assert_eq!(m.total_tps, 0.0);
        assert!(!m.has_throughput_signal());
        assert_eq!(m.duration_ms, 24811.0, "the wall clock is still measured");
    }

    /// A refusal / zero-completion answer is a genuine measurement: the prompt
    /// tokens were really processed. It must survive the filter.
    #[test]
    fn test_zero_completion_with_usage_keeps_signal() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 1200, "completion_tokens": 0, "total_tokens": 1200},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});

        let m = RequestMetrics::from_response(&response, &request, false, 340.0);

        assert!(m.has_throughput_signal());
    }

    #[test]
    fn test_stream_end_label() {
        let mut m = RequestMetrics::new();
        assert_eq!(m.stream_end_label(), "sync", "buffered request, no stream observed");

        m.streaming = true;
        assert_eq!(m.stream_end_label(), "unknown", "streaming but the end was never seen");

        m.stream_end = Some("client_gone");
        assert_eq!(m.stream_end_label(), "client_gone", "an observed end always wins");
    }

    #[test]
    fn test_request_metrics_serialize() {
        let metrics = RequestMetrics::new();
        let json = serde_json::to_string(&metrics);
        assert!(json.is_ok());
        assert!(json.unwrap().contains("request_id"));
    }

    /// The wire contract for the new fields: the floor is always present (it is
    /// not optional), the cache counts are not (they are Option).
    #[test]
    fn serialized_sample_always_carries_the_floor_and_only_reports_cache_counts() {
        let json = serde_json::to_value(RequestMetrics::new()).unwrap();
        assert!(json.get("decode_tps_min").is_some(), "got: {json}");
        assert!(json.get("cached_tokens").is_none(), "got: {json}");
        assert!(json.get("cache_creation_tokens").is_none(), "got: {json}");
    }

    #[test]
    fn test_context_info_from_slots_response() {
        let response = serde_json::json!([
            {
                "id": 0,
                "n_ctx": 4096,
                "n_tokens": 100,
                "is_processing": true
            },
            {
                "id": 1,
                "n_ctx": 4096,
                "n_tokens": 50,
                "is_processing": false
            }
        ]);

        let info = ContextInfo::from_slots_response(&response);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.total_context, 8192);
        assert_eq!(info.used_context, 150);
        assert_eq!(info.slots.len(), 2);
        assert_eq!(info.slots[0].slot_id, 0);
        assert!(info.slots[0].is_processing);
        assert!(!info.slots[1].is_processing);
    }

    #[test]
    fn test_context_info_from_slots_response_empty() {
        let response = serde_json::json!([]);
        let info = ContextInfo::from_slots_response(&response);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.total_context, 0);
        assert_eq!(info.used_context, 0);
        assert!(info.slots.is_empty());
    }

    #[test]
    fn test_context_info_from_slots_response_not_array() {
        let response = serde_json::json!({"not": "array"});
        let info = ContextInfo::from_slots_response(&response);
        assert!(info.is_none());
    }

    #[test]
    fn test_context_info_from_slots_response_partial_data() {
        let response = serde_json::json!([
            {"id": 0}, // Missing other fields
            {"n_ctx": 2048, "n_tokens": 25}
        ]);

        let info = ContextInfo::from_slots_response(&response);

        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.slots.len(), 2);
        assert_eq!(info.slots[0].n_ctx, 0); // Default
        assert_eq!(info.slots[1].slot_id, 0); // Default
    }

    /// A llama.cpp slot id is a u64 on the wire. A narrower field silently
    /// truncated u64::MAX to u32::MAX (baseline: `as u32` cast).
    #[test]
    fn context_info_keeps_full_u64_slot_id() {
        let response = serde_json::json!([
            {"id": u64::MAX, "n_ctx": 10, "n_tokens": 1}
        ]);

        let info = ContextInfo::from_slots_response(&response).unwrap();
        assert_eq!(info.slots[0].slot_id, u64::MAX);
    }

    /// Summing slot contexts must saturate, never wrap or panic: a malformed or
    /// hostile /slots body carrying u64::MAX must not take the proxy down
    /// (baseline: `attempt to add with overflow` panic under debug, two's-
    /// complement wrap under release).
    #[test]
    fn context_info_saturates_at_u64_max() {
        let response = serde_json::json!([
            {"id": 0, "n_ctx": u64::MAX, "n_tokens": u64::MAX},
            {"id": 1, "n_ctx": 1, "n_tokens": 1}
        ]);

        let info = ContextInfo::from_slots_response(&response).unwrap();
        assert_eq!(info.total_context, u64::MAX, "saturating_add, not wrap");
        assert_eq!(info.used_context, u64::MAX, "saturating_add, not wrap");
    }

    #[test]
    fn test_extended_usage_extraction() {
        let response = serde_json::json!({
            "choices": [{
                "message": {"role": "assistant", "content": "test"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "completion_tokens_details": {
                    "reasoning_tokens": 20,
                    "accepted_prediction_tokens": 5
                }
            }
        });

        let metrics = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 100.0);

        assert_eq!(metrics.reasoning_tokens, Some(20));
        assert_eq!(metrics.accepted_prediction_tokens, Some(5));
        assert_eq!(metrics.rejected_prediction_tokens, None);
    }

    /// A response with neither `usage` nor `timings` carries NO token
    /// measurement. Baseline stored that absence as 0 (formatter then printed
    /// "0" and the skip-filtered path never distinguishes); it must be None.
    #[test]
    fn usage_absent_yields_none_not_zero() {
        let response = serde_json::json!({
            "model": "m",
            "choices": [{"finish_reason": "stop", "message": {"content": "hi"}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({}), false, 50.0);

        assert_eq!(m.prompt_tokens, None);
        assert_eq!(m.completion_tokens, None);
        assert_eq!(m.total_tokens, 0);
        assert!(!m.has_throughput_signal());
    }

    /// usage carrying only prompt_tokens: the prompt is measured, the
    /// completion is NOT — None, not 0. total_tokens falls back to the
    /// saturating sum with the absent side counted as 0 (baseline arithmetic,
    /// unchanged).
    #[test]
    fn prompt_only_usage_keeps_completion_none() {
        let response = serde_json::json!({
            "usage": {"prompt_tokens": 5},
            "choices": [{"finish_reason": "stop"}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({}), false, 50.0);

        assert_eq!(m.prompt_tokens, Some(5));
        assert_eq!(m.completion_tokens, None);
        assert_eq!(m.total_tokens, 5);
    }

    /// usage carrying only completion_tokens is neither shape the collector
    /// claims (OpenAI branch requires prompt_tokens, Anthropic requires
    /// input_tokens), so nothing is recorded. Pinned pre-Option behavior: the
    /// absent prompt must not pull the lone count in with a fabricated 0 pair.
    #[test]
    fn completion_only_usage_is_recorded_neither() {
        let response = serde_json::json!({
            "usage": {"completion_tokens": 7},
            "choices": [{"finish_reason": "stop"}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({}), false, 50.0);

        assert_eq!(m.prompt_tokens, None);
        assert_eq!(m.completion_tokens, None);
        assert_eq!(m.total_tokens, 0);
    }

    /// A measured zero is Some(0), not None — the distinction the whole change
    /// exists for. Zero-completion still reports zero in the fields.
    #[test]
    fn measured_zero_stays_some_zero_not_none() {
        let response = serde_json::json!({
            "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({}), false, 50.0);

        assert_eq!(m.prompt_tokens, Some(0));
        assert_eq!(m.completion_tokens, Some(0));
        assert_eq!(m.total_tokens, 0);
    }

    /// llama.cpp timeout shape: a rate is reported but the counts are absent.
    /// The rate is a real measurement (the sample keeps its export signal),
    /// while the counts stay None instead of being fabricated as 0.
    #[test]
    fn timings_rate_without_counts_keeps_none_counts_and_real_signal() {
        let response = serde_json::json!({
            "model": "m",
            "timings": {"prompt_per_second": 100.0},
            "choices": [{"finish_reason": "stop"}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({}), false, 50.0);

        assert_eq!(m.prompt_tokens, None);
        assert_eq!(m.completion_tokens, None);
        assert_eq!(m.prompt_tps, Some(100.0));
        assert!(m.has_throughput_signal(), "the rate alone is a measurement");
    }

    /// Anthropic usage without output_tokens: prompt measured, completion None,
    /// total is the saturating sum against 0.
    #[test]
    fn anthropic_usage_without_output_tokens_yields_none_completion() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"input_tokens": 10},
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn"
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({}), false, 50.0);

        assert_eq!(m.prompt_tokens, Some(10));
        assert_eq!(m.completion_tokens, None);
        assert_eq!(m.total_tokens, 10);
        assert_eq!(m.context_used, Some(10), "Anthropic KV occupancy with no cache tokens");
        assert_eq!(m.cached_tokens, None, "the shape has no cache fields here");
    }

    #[test]
    fn test_request_metrics_from_response_timings_only() {
        // Scenario: streaming timeout where usage is missing but timings has token counts
        let response = serde_json::json!({
            "model": "test-model",
            "timings": {
                "prompt_n": 538,
                "predicted_n": 983,
                "prompt_ms": 316.829,
                "predicted_ms": 29669.411,
                "prompt_per_second": 1698.07,
                "predicted_per_second": 33.13
            },
            "prompt_progress": {"total": 562},
            "choices": [{"finish_reason": null, "delta": {"content": null}}]
        });

        let metrics = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), true, 30181.0);

        assert_eq!(metrics.prompt_tokens, Some(538));
        assert_eq!(metrics.completion_tokens, Some(983));
        assert_eq!(metrics.total_tokens, 1521);
        assert_eq!(metrics.context_used, Some(538)); // Uses prompt_n, not cache_n
    }

    #[test]
    fn test_request_metrics_from_anthropic_merged() {
        // This simulates the merged response from Anthropic SSE events
        let response = serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "Qwen3-14B-128K-Q3_K_S.gguf",
            "content": [
                {"type": "text", "text": "Hello world"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 124,
                "output_tokens": 273
            }
        });

        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "Hi"}]
        });

        let metrics = RequestMetrics::from_response(&response, &request, true, 8000.0);

        assert_eq!(metrics.model, "Qwen3-14B-128K-Q3_K_S.gguf");
        assert_eq!(metrics.prompt_tokens, Some(124));
        assert_eq!(metrics.completion_tokens, Some(273));
        assert_eq!(metrics.total_tokens, 397);
        assert_eq!(metrics.finish_reason, "end_turn");
        assert_eq!(metrics.output_len, 11); // "Hello world"
        assert!(metrics.streaming);
        // Verify context_used fallback (Anthropic format has no timings)
        assert_eq!(metrics.context_used, Some(124));
    }

    #[test]
    fn test_request_metrics_from_anthropic_with_thinking() {
        // Anthropic response with thinking content block
        let response = serde_json::json!({
            "id": "msg_2",
            "type": "message",
            "role": "assistant",
            "model": "claude-3",
            "content": [
                {"type": "thinking", "thinking": "Let me reason..."},
                {"type": "text", "text": "The answer is 42"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 200
            }
        });

        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "What is the answer?"}]
        });

        let metrics = RequestMetrics::from_response(&response, &request, false, 5000.0);

        assert_eq!(metrics.model, "claude-3");
        assert_eq!(metrics.prompt_tokens, Some(100));
        assert_eq!(metrics.completion_tokens, Some(200));
        // Output len should only count text content, not thinking
        assert_eq!(metrics.output_len, 16); // "The answer is 42"
        assert_eq!(metrics.finish_reason, "end_turn");
        // Verify context_used fallback (Anthropic format has no timings)
        assert_eq!(metrics.context_used, Some(100));
    }

    /// vLLM responses carry `usage` but no llama.cpp `timings`. That path used to
    /// fabricate a prefill/decode split from a hardcoded 20%/80% division of the
    /// wall clock. Assert it leaves the split empty and reports only what one
    /// duration can support: a cost rate and a floor on decode.
    #[test]
    fn test_no_timings_reports_total_only() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 39240, "completion_tokens": 146, "total_tokens": 39386},
            "choices": [{"finish_reason": "tool_calls", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1731.0);

        assert!(!m.has_timing_split);
        assert_eq!(m.prompt_tps, None, "must not invent a prefill rate");
        assert_eq!(m.generation_tps, None, "must not invent a decode rate");
        // the old code produced 113344.89 here, on a server measured at ~14.5k tok/s
        assert!((m.total_tps - 22753.32).abs() < 0.1, "total_tps was {}", m.total_tps);
        // ... and this is what the same request ACTUALLY did: 146 tokens over a
        // 1731ms wall, i.e. at least 84 tok/s of decode. Two orders of magnitude
        // under the cost rate, which is the whole reason both are printed.
        assert!(
            (m.decode_tps_min - 84.34).abs() < 0.01,
            "decode_tps_min was {}",
            m.decode_tps_min
        );
        assert!(
            m.decode_tps_min < m.total_tps,
            "the floor can never exceed the cost rate: {} vs {}",
            m.decode_tps_min,
            m.total_tps
        );
    }

    /// A backend that does report timings keeps the real split.
    #[test]
    fn test_timings_present_keeps_split() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150},
            "timings": {"prompt_per_second": 200.5, "predicted_per_second": 42.5,
                        "prompt_ms": 500.0, "predicted_ms": 1176.0},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1676.0);

        assert!(m.has_timing_split);
        assert_eq!(m.prompt_tps, Some(200.5));
        assert_eq!(m.generation_tps, Some(42.5));
        assert!(m.total_tps > 0.0, "total_tps should still be populated");
        // The floor is populated on tier-1 samples too, so the InfluxDB field is
        // never a sparse hole. It is simply not what the line prints when a real
        // measurement exists.
        assert!(m.decode_tps_min > 0.0, "decode_tps_min must still be filled in");
    }

    /// The number that replaces the old `tps=<n>tot` on a backend with no split:
    /// completion tokens over the wall clock. Measured, always available, and a
    /// FLOOR -- the denominator still contains prefill, so it understates.
    #[test]
    fn decode_tps_min_is_a_wall_bounded_floor() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"input_tokens": 6360, "output_tokens": 157},
            "stop_reason": "tool_use"
        });
        // The first line of the operator log that prompted this: the old code
        // printed tps=6073.63tot for a request that decoded 157 tokens.
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 1073.0);

        assert!((m.decode_tps_min - 146.32).abs() < 0.01, "got {}", m.decode_tps_min);
        assert!((m.total_tps - 6073.63).abs() < 0.1, "got {}", m.total_tps);
        assert!(!m.has_timing_split, "a derived floor is not a backend split");
        assert_eq!(m.prompt_tps, None);
    }

    /// No counted output means no floor. A 0 here would read as "instantly slow"
    /// on an aborted or prompt-only request.
    #[test]
    fn decode_tps_min_absent_without_completion_tokens() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": 500, "completion_tokens": null, "total_tokens": 500}
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), true, 500.0);

        assert_eq!(m.completion_tokens, None);
        assert_eq!(m.decode_tps_min, 0.0, "absent output is not a zero rate");
    }

    /// The floor is computed after `extract_timings`, so a backend that reports
    /// counts only under `timings` (the timeout path) still gets one.
    #[test]
    fn decode_tps_min_survives_the_timings_token_fallback() {
        let response = serde_json::json!({
            "model": "m",
            "timings": {"prompt_n": 1000, "predicted_n": 400, "predicted_ms": 2000.0},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 2000.0);

        assert_eq!(m.completion_tokens, Some(400));
        assert!(
            (m.decode_tps_min - 200.0).abs() < 0.01,
            "decode_tps_min was {}",
            m.decode_tps_min
        );
    }

    /// A request that finishes inside the timer's resolution has no denominator.
    #[test]
    fn decode_tps_min_zero_duration_stays_zero() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": 10, "completion_tokens": 100, "total_tokens": 110}
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 0.0);

        assert_eq!(m.decode_tps_min, 0.0);
        assert_eq!(m.total_tps, 0.0);
    }

    /// Hostile-body class, same vector as every other derived rate in this file:
    /// a huge count over `f64::MIN_POSITIVE` ms overflows the division to +inf,
    /// which serde_json emits as a literal `inf` and InfluxDB 400s wholesale.
    /// Both derived rates map to the contract's 0.0 = "not reported".
    #[test]
    fn decode_tps_min_clamps_hostile_division() {
        let huge = u64::MAX;
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": huge, "completion_tokens": huge, "total_tokens": huge},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, f64::MIN_POSITIVE);

        assert!(m.total_tokens > 0, "the counts are real; only the rate is junk");
        assert!(
            !m.decode_tps_min.is_infinite() && m.decode_tps_min == 0.0,
            "decode_tps_min = {}",
            m.decode_tps_min
        );
        assert!(
            !m.total_tps.is_infinite() && m.total_tps == 0.0,
            "total_tps = {}",
            m.total_tps
        );
    }

    /// Anthropic's `input_tokens` EXCLUDES cached and cache-written tokens, so
    /// KV occupancy is the sum. This is the only shape that needs the addition.
    #[test]
    fn anthropic_cache_fields_are_extracted() {
        let response = serde_json::json!({
            "model": "m",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "cache_read_input_tokens": 30000,
                "cache_creation_input_tokens": 500
            }
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 100.0);

        assert_eq!(m.prompt_tokens, Some(100), "input_tokens keeps its own meaning");
        assert_eq!(m.total_tokens, 120, "the cost total stays input+output");
        assert_eq!(m.cached_tokens, Some(30000));
        assert_eq!(m.cache_creation_tokens, Some(500));
        // KV occupancy = input_tokens + cache_read + cache_creation
        // (Anthropic's input_tokens excludes both cache fields; output_tokens
        // are not part of KV occupancy). 100 + 30000 + 500 = 30600.
        assert_eq!(m.context_used, Some(30600), "the engine held all of it in KV");
    }

    /// The opposite rule for the opposite shape: OpenAI/vLLM `prompt_tokens`
    /// ALREADY contains the cached prefix. Reading the count must not add it to
    /// context_used a second time.
    #[test]
    fn openai_cached_tokens_do_not_inflate_context_used() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {
                "prompt_tokens": 5000,
                "completion_tokens": 10,
                "total_tokens": 5010,
                "prompt_tokens_details": {"cached_tokens": 4096}
            },
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 100.0);

        assert_eq!(m.cached_tokens, Some(4096));
        assert_eq!(m.cache_creation_tokens, None, "this shape never reports it");
        assert_eq!(m.context_used, Some(5000), "subsumed, so NOT 9096");
    }

    /// llama.cpp spells the same idea `timings.cache_n`, subsumed by `prompt_n`.
    #[test]
    fn llama_cache_n_populates_cached_tokens() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": 2000, "completion_tokens": 5, "total_tokens": 2005},
            "timings": {"prompt_n": 2000, "predicted_n": 5, "cache_n": 1500,
                        "prompt_ms": 100.0, "predicted_ms": 50.0},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 200.0);

        assert_eq!(m.cached_tokens, Some(1500));
        assert_eq!(m.context_used, Some(2000), "prompt_n, cache included already");
    }

    /// A backend that never mentions its cache must not be reported as having
    /// missed one: absent, not zero.
    #[test]
    fn cache_fields_stay_absent_when_the_backend_stays_silent() {
        let response = serde_json::json!({
            "model": "m",
            "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12},
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 50.0);

        assert_eq!(m.cached_tokens, None);
        assert_eq!(m.cache_creation_tokens, None);
    }

    /// vLLM with --enable-per-request-metrics returns a `metrics` object. It must
    /// be used instead of the total-throughput fallback, and must produce a real
    /// split rather than the old 20/80 estimate.
    #[test]
    fn test_vllm_per_request_metrics_are_used() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 16, "completion_tokens": 120, "total_tokens": 136},
            "metrics": {
                "time_to_first_token_ms": 291.44680208992213,
                "generation_time_ms": 760.3732609422877,
                "queue_time_ms": 12.5,
                "mean_itl_ms": 6.3896912684225855,
                "tokens_per_second": 114.0879549816357
            },
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1100.0);

        assert!(m.has_timing_split, "vLLM metrics must count as a real split");
        assert!((m.prompt_ms - 291.4468).abs() < 0.01);
        assert!((m.generation_ms - 760.3733).abs() < 0.01);
        // 120 tokens over the decode interval -- matches the 157.6 tok/s measured
        // independently by scripts/qwen-conc-sweep for this config.
        assert!(
            m.generation_tps.is_some_and(|v| (v - 157.82).abs() < 0.1),
            "got {:?}",
            m.generation_tps
        );
        assert_eq!(m.queue_ms, Some(12.5));
        assert!((m.mean_itl_ms.unwrap() - 6.3897).abs() < 0.01);
    }

    /// A vLLM body on the `metrics` path: OpenAI usage shape, optional
    /// `prompt_tokens_details.cached_tokens`, optional ttft.
    fn vllm_response(prompt: u64, cached: Option<u64>, completion: u64, ttft: f64) -> Value {
        let mut usage = serde_json::json!({
            "prompt_tokens": prompt,
            "completion_tokens": completion,
            "total_tokens": prompt + completion,
        });
        if let Some(c) = cached {
            usage["prompt_tokens_details"] = serde_json::json!({ "cached_tokens": c });
        }
        serde_json::json!({
            "model": "cosmo-6000",
            "usage": usage,
            "metrics": {
                "time_to_first_token_ms": ttft,
                // Arbitrary decode duration, never asserted -- every test on this
                // builder targets prompt_tps. Deliberately non-zero so no fixture can
                // reach the wall-clock fallback. It is NOT from a log line: the compact
                // line prints decode tps, never decode ms.
                "generation_time_ms": 2546.0,
                "queue_time_ms": 0.0095,
            },
            "choices": [{"finish_reason": "stop", "message": {"content": ""}}]
        })
    }

    /// Wall clock for the `metrics`-path fixtures. Inert here -- `extract_vllm_usage`
    /// only consults `duration_ms` in its `else if` fallback, which `metrics` being
    /// present makes unreachable -- but deliberately non-zero so no fixture can slide
    /// onto a wall-derived rate without the test noticing. It is the `dur=3163.0ms`
    /// of the 186417-token logged line, plan 03 Context (2).
    const WALL_MS: f64 = 3163.0;

    /// "Over 15k prefill is probably a bug" -- plan 03 Context (2)
    /// (context/plans/03_columnar_stats_honest_prefill.md). T5 promotes this to the
    /// `stats.max_plausible_prefill_tps` config key; here it is the test's oracle. Only
    /// the ceiling is checked here; the expected floor comes from the exact
    /// `corrected_pps` assertions.
    const PLAUSIBLE_MAX_PPS: f64 = 15_000.0;

    fn metrics_of(response: &Value) -> RequestMetrics {
        RequestMetrics::from_response(response, &serde_json::json!({"messages": []}), false, WALL_MS)
    }

    fn prefill_of(response: &Value) -> Option<f64> {
        metrics_of(response).prompt_tps
    }

    /// One message shape for the verbatim cases: the arithmetic, the expectation and
    /// what actually came back, so a failure needs no scrollback to diagnose.
    fn assert_pps(got: Option<f64>, want: f64, arithmetic: &str) {
        assert!(
            got.is_some_and(|v| (v - want).abs() < 1.0),
            "{arithmetic} => {want:.2} tok/s, got {got:?}"
        );
    }

    /// One row of plan 03's Context (2) table
    /// (context/plans/03_columnar_stats_honest_prefill.md:78-84), captured live
    /// 2026-10-16 from `stats.format: compact` against cosmo-6000 (vLLM).
    /// `logged_pps` is what the buggy build printed; `corrected_pps` is the plan's
    /// hand-computed answer for these inputs.
    #[derive(Debug, Clone, Copy)]
    struct Row {
        prompt: u64,
        cached: u64,
        ttft_ms: f64,
        corrected_pps: f64,
        logged_pps: f64,
    }

    /// The five live log lines that opened plan 03, replayed as unit-test data.
    ///
    /// Every `ttft_ms` below is RECONSTRUCTED as `prompt / logged_pps * 1000` -- it is
    /// not an independently captured figure. So the `raw` assertion is a transcription
    /// guard on that literal, NOT a second measurement of the backend: the backend's
    /// own numbers enter this test only through `logged_pps` and `cached`. What it does
    /// establish is that the implementation reproduces the plan's independently
    /// hand-computed `corrected_pps` from those inputs.
    ///
    /// The shape of a correct discount is the *spread*, not the shift: the heavily
    /// cached rows fall 46x-96x while the barely-cached control row moves
    /// 15098 -> 12798. A formula subtracting a constant would land in plausible range
    /// and still fail here.
    #[test]
    fn vllm_prefill_is_rated_on_non_cached_tokens_across_the_five_logged_rows() {
        const ROWS: [Row; 5] = [
            Row {
                prompt: 59_043,
                cached: 51_200,
                ttft_ms: 1221.5584,
                corrected_pps: 6_421.0,
                logged_pps: 48_334.16,
            },
            // control row: barely cached, so plan 03 requires it to move LEAST.
            Row {
                prompt: 21_001,
                cached: 3_200,
                ttft_ms: 1390.9384,
                corrected_pps: 12_797.0,
                logged_pps: 15_098.44,
            },
            Row {
                prompt: 105_798,
                cached: 99_200,
                ttft_ms: 506.7069,
                corrected_pps: 13_021.0,
                logged_pps: 208_795.26,
            },
            Row {
                prompt: 162_906,
                cached: 156_800,
                ttft_ms: 519.7498,
                corrected_pps: 11_748.0,
                logged_pps: 313_431.57,
            },
            Row {
                prompt: 186_417,
                cached: 182_400,
                ttft_ms: 321.5830,
                corrected_pps: 12_491.0,
                logged_pps: 579_685.47,
            },
        ];
        let mut measured: Vec<(Row, f64)> = Vec::new();
        for row in ROWS {
            // completion is inert for prompt_tps (any non-zero keeps the fixture
            // realistic); 498 is the 186417-row completion from plan-03 line 71, reused
            // for every row rather than each row's own 201/622/401/101/498.
            let m = metrics_of(&vllm_response(row.prompt, Some(row.cached), 498, row.ttft_ms));
            let actual = m.prompt_tps.unwrap_or_else(|| panic!("{row:?}: prefill rate went absent"));
            assert!(
                (actual - row.corrected_pps).abs() / row.corrected_pps < 0.01,
                "{row:?}: got {actual:.2}, expected {:.0} +-1%",
                row.corrected_pps
            );
            let raw = row.prompt as f64 / row.ttft_ms * 1000.0;
            // Removing cached tokens from the numerator can only LOWER the rate; this
            // path must never exceed the figure it exists to correct.
            assert!(
                actual <= raw + 1e-9,
                "{row:?}: discount RAISED the rate ({actual:.2} > raw {raw:.2})"
            );
            assert!(
                actual < PLAUSIBLE_MAX_PPS,
                "{row:?}: corrected rate still implausible: {actual:.2}"
            );
            // Guards against a mistyped `ttft_ms` literal, nothing more -- see the
            // reconstruction note on this test.
            assert!(
                (raw - row.logged_pps).abs() / row.logged_pps < 0.01,
                "{row:?}: ttft literal drifted from the logged line (raw {raw:.2} vs logged {:.2})",
                row.logged_pps
            );
            measured.push((row, actual));
        }
        // Plan 03's T4 acceptance line: "the control row (cache 3200) moves least".
        // Off the MEASURED rates, not the expected literals: a discount applied as a
        // constant factor rather than a proportion flattens this ordering, and that is
        // precisely the failure this catches.
        let shrink = |r: &Row, rate: f64| (r.prompt as f64 / r.ttft_ms * 1000.0) / rate;
        let (control, control_rate) = *measured.iter().find(|(r, _)| r.cached == 3_200).expect("control row present");
        let control_shrink = shrink(&control, control_rate);
        for (row, rate) in &measured {
            if row.cached != 3_200 {
                let s = shrink(row, *rate);
                assert!(
                    s > control_shrink,
                    "{row:?}: shrank {s:.2}x, no more than the control row's {control_shrink:.2}x"
                );
            }
        }
    }

    /// No cache reported is not "nothing cached" -- the numerator stays the full
    /// prompt, because there is no evidence any of it was served from cache.
    #[test]
    fn vllm_prefill_is_verbatim_when_the_backend_reports_no_cache() {
        assert_pps(
            prefill_of(&vllm_response(21_001, None, 622, 1390.9384)),
            15_098.44,
            "21001 tok / 1390.9384 ms, no cache reported",
        );
    }

    /// A measured zero cache hit leaves the rate verbatim. This pins the OUTCOME only:
    /// routing `Some(0)` through the discount arm computes `p - 0` and prints the same
    /// number, so no value assertion can separate it from the `.filter(|c| *c > 0)`
    /// guard at the match head. That guard exists to stop the discount path logging
    /// that it discounted zero tokens, and it is pinned by
    /// `vllm_prefill_logs_at_the_level_its_case_deserves`, not here.
    #[test]
    fn vllm_prefill_is_verbatim_when_cache_is_measured_zero() {
        assert_pps(
            prefill_of(&vllm_response(21_001, Some(0), 622, 1390.9384)),
            15_098.44,
            "21001 tok / 1390.9384 ms, cache reported as 0",
        );
    }

    /// `cached > prompt` is a backend data error. Saturating to zero would report a
    /// prefill that ran at 0 tok/s (and `finite_rate_opt` maps that to `None`
    /// anyway), and blanking is indistinguishable from "no split" on the wire -- both
    /// delete a measurement. The uncorrected rate is the only honest survivor.
    #[test]
    fn vllm_prefill_is_verbatim_when_cache_exceeds_the_prompt() {
        assert_pps(
            prefill_of(&vllm_response(1_000, Some(5_000), 100, 500.0)),
            2_000.0,
            "1000 tok / 500 ms, cache 5000 > prompt (data error)",
        );
    }

    /// `cached == prompt` is a legitimate FULL prefix cache hit, not a data error --
    /// the same case `finite_rate_opt`'s doc names as reachable. Same verbatim
    /// outcome, different reason, so it is pinned separately: this is the state where
    /// the uncorrected number is most inflated, and the one that must not be treated
    /// as backend misbehaviour.
    #[test]
    fn vllm_prefill_is_verbatim_on_a_full_prefix_cache_hit() {
        assert_pps(
            prefill_of(&vllm_response(50_000, Some(50_000), 100, 250.0)),
            200_000.0,
            "50000 tok / 250 ms, fully cached",
        );
    }

    /// A cache figure with no prefill window is not a rate: no duration, no
    /// measurement. Absent, never zero and never a guess off the wall clock.
    #[test]
    fn vllm_prefill_is_absent_when_ttft_is_missing_even_with_a_cache() {
        let mut response = vllm_response(186_417, Some(182_400), 498, 321.583);
        response["metrics"] = serde_json::json!({ "generation_time_ms": 2546.0 });
        let m = prefill_of(&response);
        assert_eq!(m, None, "no prefill window means no prefill rate");
    }

    /// The fabricated-split guard. A body carrying `metrics` but no usable
    /// `prompt_tokens` has no prefill numerator, so there is no prefill rate to report
    /// -- and the split flag must not be set from durations either, which is exactly
    /// how `tps= 0.00/ 0.00` once reached a line whose label means *measured*. The
    /// wall clock is present and non-zero here, so this also proves the `else if`
    /// duration fallback does not leak a wall-derived prefill onto the `metrics` path.
    #[test]
    fn vllm_metrics_without_a_prompt_count_yield_no_prefill_rate() {
        // The two rows differ in more than the numerator. `prompt_tokens: 0` still
        // parses, so the OpenAI usage branch runs and `completion_tokens` survives;
        // `prompt_tokens: null` fails `as_u64()`, which skips BOTH usage branches --
        // so this body loses its completion count too and has no decode rate to
        // report. That is `extract_usage`'s shape dispatch, not the discount, but it
        // is recorded here because it is otherwise invisible.
        for (prompt_json, decode_counted) in [("0", true), ("null", false)] {
            let body = format!(
                r#"{{"model":"cosmo-6000","usage":{{"prompt_tokens":{prompt_json},"completion_tokens":10}},
                    "metrics":{{"time_to_first_token_ms":250.0,"generation_time_ms":100.0}}}}"#
            );
            let response: Value = serde_json::from_str(&body).unwrap();
            let m = metrics_of(&response);
            assert_eq!(m.prompt_tps, None, "prompt_tokens={prompt_json} produced a rate");
            // ttft WAS parsed into prompt_ms, so a duration is present with no rate
            // beside it -- the combination that used to force the flag true and put
            // `tps= 0.00/...` on a line labelled *measured*. The flag tracks rates only.
            assert!(m.prompt_ms > 0.0, "ttft should still be recorded as a duration");
            assert_eq!(
                m.has_timing_split,
                m.prompt_tps.is_some() || m.generation_tps.is_some(),
                "prompt_tokens={prompt_json}: split flag drifted from its rates"
            );
            assert_eq!(
                m.generation_tps.is_some(),
                decode_counted,
                "prompt_tokens={prompt_json}: decode countedness changed unexpectedly"
            );
        }
    }

    /// In-file tracing capture (the `backends/node.rs` / `augment.rs` pattern):
    /// thread-local `set_default` keeps this parallel-safe.
    #[derive(Clone)]
    struct CaptureWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = Self;
        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture_logs<R>(f: impl FnOnce() -> R) -> (R, String) {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("off,llama_proxy::stats::collector=debug"))
            .with_ansi(false)
            .with_target(false)
            .with_writer(CaptureWriter(buf.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let r = f();
        drop(_guard);
        let captured = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        (r, captured)
    }

    /// The four prefill cases collapse to two observable rates, so the ONLY thing
    /// separating a legitimate full cache hit from a backend data error is the log
    /// level -- and getting that backwards is a production incident either way: `warn!`
    /// on a healthy full hit floods one line per request on the busiest traffic there
    /// is, while `debug!` on `cached > prompt` hides a backend that is lying about its
    /// token counts. Untestable from `RequestMetrics`, so it is asserted through a real
    /// subscriber. Also pins the `Some(0)` path, which no value assertion can reach: a
    /// zero cache must never claim in the log that it discounted anything.
    #[test]
    fn vllm_prefill_logs_at_the_level_its_case_deserves() {
        let (_, logged) = capture_logs(|| {
            metrics_of(&vllm_response(50_000, Some(50_000), 100, 250.0));
        });
        assert!(
            logged.contains("full prefix cache hit"),
            "full cache hit logged nothing, got: {logged:?}"
        );
        assert!(
            !logged.contains("WARN"),
            "a healthy full cache hit warned per request: {logged}"
        );

        let (_, logged) = capture_logs(|| {
            metrics_of(&vllm_response(1_000, Some(5_000), 100, 500.0));
        });
        assert!(
            logged.contains("WARN") && logged.contains("backend data error"),
            "cached > prompt did not warn: {logged}"
        );

        let (_, logged) = capture_logs(|| {
            metrics_of(&vllm_response(59_043, Some(51_200), 201, 1221.5584));
        });
        assert!(
            logged.contains("prefill rated on non-cached tokens only"),
            "the discount ran silently: {logged}"
        );
        assert!(!logged.contains("WARN"), "a normal cached request warned: {logged}");

        let (_, logged) = capture_logs(|| {
            metrics_of(&vllm_response(21_001, Some(0), 622, 1390.9384));
        });
        assert!(
            !logged.contains("prefill rated on non-cached tokens only"),
            "a zero cache was logged as a discount: {logged}"
        );
    }

    /// THE shape guard. On the Anthropic usage shape `cached_tokens` comes from
    /// `cache_read_input_tokens`, which is DISJOINT from `input_tokens` -- that is why
    /// `context_used` there sums the three. Discounting it would subtract a prefix
    /// that was never in the numerator and silently under-report prefill by the whole
    /// cached amount, which the plausibility canary cannot see (it only catches rates
    /// that are too HIGH). Stats are built from the backend's raw body, so an
    /// Anthropic-format backend passing vLLM metrics through reaches this path.
    #[test]
    fn anthropic_cache_read_tokens_are_never_discounted_from_the_prompt() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {
                "input_tokens": 50_000,
                "completion_tokens": 100,
                "cache_read_input_tokens": 10_000,
                "cache_creation_input_tokens": 500,
            },
            "metrics": { "time_to_first_token_ms": 500.0, "generation_time_ms": 2546.0 },
            "content": [{"type": "text", "text": ""}]
        });
        let m = RequestMetrics::from_response(&response, &serde_json::json!({"messages": []}), false, 3163.0);
        assert_eq!(m.prompt_tokens, Some(50_000));
        assert_eq!(m.cached_tokens, Some(10_000), "disjoint from the prompt");
        // Verbatim 50000/500ms = 100k. The discount would have said 80k.
        assert!(
            m.prompt_tps.is_some_and(|v| (v - 100_000.0).abs() < 1.0),
            "Anthropic shape was discounted: got {:?}",
            m.prompt_tps
        );
        assert_ne!(m.prompt_tps, Some(80_000.0), "the disjoint cache was subtracted");
    }

    /// A null `metrics` (server without --enable-per-request-metrics) must fall
    /// back to total throughput, NOT to an invented split.
    #[test]
    fn test_null_vllm_metrics_falls_back() {
        let response = serde_json::json!({
            "model": "cosmo-6000",
            "usage": {"prompt_tokens": 39240, "completion_tokens": 146, "total_tokens": 39386},
            "metrics": serde_json::Value::Null,
            "choices": [{"finish_reason": "tool_calls", "message": {"content": ""}}]
        });
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1731.0);

        assert!(!m.has_timing_split);
        assert_eq!(m.prompt_tps, None);
        assert_eq!(m.generation_tps, None);
        assert_eq!(m.queue_ms, None);
        assert!((m.total_tps - 22753.32).abs() < 0.1);
    }

    /// big-fix F12-R4 [MAJOR 10]: the baseline filter `*v > 0.0` admits
    /// SUBNORMALS (1e-320 parses as-is, 1e-400 lands 0.0), and
    /// `prompt_tokens / ttft * 1000` on a subnormal ttft overflows to +inf.
    /// An inf field makes serde_json emit a literal `inf`, InfluxDB rejects
    /// the whole body with 400, the retry ladder burns 4 attempts, and the
    /// sample dies silently. A subnormal duration is the absence of a
    /// timing signal, not a measurement.
    #[test]
    fn subnormal_vllm_durations_yield_no_infinite_rate() {
        let response: serde_json::Value = serde_json::from_str(
            r#"{"model":"m",
               "usage":{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150},
               "metrics":{"time_to_first_token_ms":1e-320,"generation_time_ms":1e-320},
               "choices":[{"finish_reason":"stop","message":{"content":""}}]}"#,
        )
        .unwrap();
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1000.0);

        assert_eq!(m.prompt_tps, None, "an overflowing rate is absent, never inf and never 0.0");
        assert_eq!(
            m.generation_tps, None,
            "an overflowing rate is absent, never inf and never 0.0"
        );
        assert_eq!(m.prompt_ms, 0.0, "a subnormal duration is not a measurement");
        assert_eq!(m.generation_ms, 0.0);
        assert!(!m.has_timing_split);
        assert!(m.total_tps.is_finite(), "total_tps = {}", m.total_tps);
    }

    /// The gate's other input class: a duration that PASSES the filter
    /// (f64::MIN_POSITIVE is finite and normal-range) with a huge token count
    /// still overflows `tokens / ms * 1000` past f64 -> +inf. The derived-rate
    /// gate maps that to the contract's 0.0 = "not reported". A huge-but-
    /// FINITE rate is deliberately NOT rejected (implausibility is not what
    /// this gate owns; +inf is).
    #[test]
    fn derived_rate_overflow_is_clamped_to_not_reported() {
        let response: serde_json::Value = serde_json::from_str(&format!(
            r#"{{"model":"m",
               "usage":{{"prompt_tokens":{},"completion_tokens":{},"total_tokens":{}}},
               "metrics":{{"time_to_first_token_ms":{},"generation_time_ms":{}}},
               "choices":[{{"finish_reason":"stop","message":{{"content":""}}}}]}}"#,
            u64::MAX,
            u64::MAX,
            u64::MAX,
            f64::MIN_POSITIVE,
            f64::MIN_POSITIVE,
        ))
        .unwrap();
        let request = serde_json::json!({"messages": []});
        let m = RequestMetrics::from_response(&response, &request, false, 1000.0);

        assert_eq!(
            m.prompt_tps, None,
            "u64::MAX/MIN_POSITIVE*1000 overflows f64 -> absent, never 0.0"
        );
        assert_eq!(
            m.generation_tps, None,
            "an overflowing rate is absent, never inf and never 0.0"
        );
        // The durations themselves are real (if absurd) measurements and survive.
        assert_eq!(m.prompt_ms, f64::MIN_POSITIVE);
        assert_eq!(m.generation_ms, f64::MIN_POSITIVE);
    }

    /// Honest limit of the hostile-body class: serde_json REJECTS an
    /// out-of-range number literal outright ("number out of range" - pinned
    /// raw below), so +inf never arrives through `as_f64` and the real inf
    /// vector is the derived division above. The `timings` gate is therefore
    /// belt-and-braces; this pins both the parse-level wall and the gate
    /// itself against a hand-built +inf that no parser produces but no wire
    /// may ever carry.
    #[test]
    fn out_of_range_literals_reject_at_parse_and_the_gate_covers_handbuilt_inf() {
        let hostile = serde_json::from_str::<serde_json::Value>(r#"{"timings":{"prompt_per_second":1e400}}"#);
        assert!(
            matches!(&hostile, Err(e) if e.to_string().contains("number out of range")),
            "serde_json's own parse-level wall, got {hostile:?}"
        );

        assert_eq!(finite_rate(f64::INFINITY, "prompt_tps", "m"), 0.0);
        assert_eq!(finite_rate(f64::NEG_INFINITY, "generation_ms", "m"), 0.0);
        assert_eq!(finite_rate(12.5, "prompt_ms", "m"), 12.5, "finite values ride through");
    }
}
