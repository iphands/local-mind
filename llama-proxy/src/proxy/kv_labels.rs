//! Pure parser for the vLLM `vllm:cache_config_info` gauge - the **pure half** of the
//! client-management-endpoint shim.
//!
//! `compat.rs` owns the I/O half (scrape the backend's `/metrics`, cache it, render the shim
//! body); THIS module owns only the text parse of that one metric family. It does no I/O, no
//! async, no caching, so it needs no mock backend and no port: `compat.rs` hands it a `&str`
//! and reads [`KvCacheInfo::labels`].
//!
//! This is deliberately NOT a general Prometheus parser. Exactly one family
//! (`vllm:cache_config_info`) is considered, its value column is ignored, and no cross-series
//! aggregation happens. The accepted grammar is pinned by the tests below.
//!
//! # Why every value stays a string
//! vLLM main @ `442bd00d9feff7d6a0cebf74248674bad74ec391`:
//! - `vllm/v1/metrics/loggers.py:1003-1025` registers the gauge from `CacheConfig::metrics_info()`
//! - `vllm/config/cache.py:311-313` defines `metrics_info()` as
//!   `{k: str(v) for k, v in self.__dict__.items()}`
//!
//! So every `CacheConfig` attribute is rendered through `str()`: `num_gpu_blocks` arrives as
//! `"5080"`, `enable_prefix_caching` as `"True"`. Nothing here coerces, parses or re-renders
//! those values; they pass through verbatim as `String`.

use std::collections::BTreeMap;

/// The single metric family this module considers.
const METRIC_NAME: &str = "vllm:cache_config_info";

/// Label that decides which matching series wins (see [`parse_cache_config_info`]).
const ENGINE_LABEL: &str = "engine";

/// One `vllm:cache_config_info` series: its label set, verbatim.
///
/// `BTreeMap` (not `HashMap`) so iteration and `Debug` are key-sorted and therefore
/// reproducible - the deterministic-ordering precedent this repo already follows is
/// `src/api/openai.rs:2239` (`serde_json::Map` is a `BTreeMap` because `preserve_order`
/// is off for that map).
//  `compat.rs` (plan todo 4) is the first production caller; until it lands, only the
//  tests below use these items. Same practice as the e2e mock-backend setters.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KvCacheInfo {
    labels: BTreeMap<String, String>,
}

impl KvCacheInfo {
    /// The winning series' labels. Values are exactly what vLLM printed, quotes removed.
    #[allow(dead_code)]
    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}

/// Extract the `vllm:cache_config_info` label set from a `/metrics` body.
///
/// * `Some` - the family is present with at least one label. A returned [`KvCacheInfo`]
///   ALWAYS carries at least one label.
/// * `None` - the family is absent (llama.cpp backend, comment-only body, malformed line).
///   Never an empty `Some`: the caller caches "metric absent on a clean 2xx" as a
///   once-per-process negative, and an empty `Some` would be indistinguishable from a hit.
///
/// # Determinism across series
/// A multi-engine deployment prints one line per engine. The line with the **lowest `engine`
/// label** wins, and the comparison is a plain string compare, so `"10" < "2"`: the pick is
/// stable and reproducible, NOT numerically ordered. A series with no `engine` label sorts
/// first (empty string), and equal `engine` values keep the FIRST occurrence in body order.
#[allow(dead_code)]
pub(crate) fn parse_cache_config_info(body: &str) -> Option<KvCacheInfo> {
    let mut best: Option<(String, KvCacheInfo)> = None;
    for line in body.lines() {
        let Some(labels) = cache_config_labels(line) else {
            continue;
        };
        let engine = labels.get(ENGINE_LABEL).cloned().unwrap_or_default();
        let wins = match &best {
            Some((lowest, _)) => engine < *lowest,
            None => true,
        };
        if wins {
            best = Some((engine, KvCacheInfo { labels }));
        }
    }
    best.map(|(_, info)| info)
}

/// Labels of ONE line, or `None` when the line is not a usable `vllm:cache_config_info`
/// series: a `#` HELP/TYPE comment, another metric family, a family whose name merely starts
/// with ours, or a series with no labels at all (nothing to surface).
fn cache_config_labels(line: &str) -> Option<BTreeMap<String, String>> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    // The name must be followed immediately by `{`. That boundary is also what rejects
    // `vllm:cache_config_info_extra` and a label-less `vllm:cache_config_info 1`.
    let rest = line.strip_prefix(METRIC_NAME)?.strip_prefix('{')?;
    let inner = label_set_inner(rest)?;
    let labels = parse_label_pairs(inner);
    (!labels.is_empty()).then_some(labels)
}

/// Given `a="b",c="d"} 1` (the opening `{` already consumed), return the text up to the
/// closing `}`. The terminator is the first `}` OUTSIDE quotes, so a `}` inside a quoted
/// value cannot truncate the set. `None` on an unterminated set - not a line to trust.
fn label_set_inner(rest: &str) -> Option<&str> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (idx, ch) in rest.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && in_quotes {
            escaped = true;
        } else if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == '}' && !in_quotes {
            return Some(&rest[..idx]);
        }
    }
    None
}

/// Split the label set on `,` OUTSIDE quotes, then each pair on the FIRST `=` (so a value
/// may contain one), then unquote the value. Label names are unquoted in the exposition
/// format, so they are only trimmed.
fn parse_label_pairs(inner: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    let mut start = 0usize;
    let mut in_quotes = false;
    let mut escaped = false;
    for (idx, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && in_quotes {
            escaped = true;
        } else if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ',' && !in_quotes {
            insert_pair(&mut labels, &inner[start..idx]);
            start = idx + 1; // `,` is a single byte
        }
    }
    insert_pair(&mut labels, &inner[start..]);
    labels
}

/// Insert one `name="value"` segment. A segment without `=`, with an empty name, or with an
/// unquoted value is not the promised grammar and is skipped rather than guessed at.
fn insert_pair(labels: &mut BTreeMap<String, String>, pair: &str) {
    let Some((name, value)) = pair.split_once('=') else {
        return;
    };
    let name = name.trim();
    let Some(value) = unquote(value.trim()) else {
        return;
    };
    if !name.is_empty() {
        labels.insert(name.to_string(), value);
    }
}

/// Strip the surrounding double quotes and resolve the escapes Prometheus allows inside a
/// label value (`\"`, `\\`, `\n`, tolerating `\r`/`\t`). An unquoted value is not this
/// grammar, hence `Option`.
fn unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    if !inner.contains('\\') {
        return Some(inner.to_string());
    }
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            escaped = false;
            out.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                literal => literal, // `\"` and `\\` land here
            });
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gauge exactly as vLLM main (`442bd00`) renders it: `CacheConfig.metrics_info()`
    /// is `{k: str(v) for k, v in self.__dict__.items()}`, so EVERY attribute - including
    /// the numeric-looking `num_gpu_blocks` - arrives as a quoted string.
    const VLLM_LINE: &str = r#"vllm:cache_config_info{block_size="16",cache_dtype="auto",engine="0",gpu_memory_utilization="0.9",kv_cache_size_tokens="81280",num_gpu_blocks="5080"} 1"#;

    #[test]
    fn parses_cache_config_info_labels() {
        // Given: a vLLM /metrics body carrying HELP/TYPE comments and the gauge.
        let body = format!(
            "# HELP vllm:cache_config_info Information of the LLMEngine CacheConfig\n\
             # TYPE vllm:cache_config_info gauge\n\
             {VLLM_LINE}\n"
        );

        // When: the pure parser sees the body.
        let info = parse_cache_config_info(&body).expect("the gauge is present, so Some");
        let labels = info.labels();

        // Then: values stay STRINGS - no coercion anywhere in the contract.
        assert_eq!(
            labels.get("num_gpu_blocks").map(String::as_str),
            Some("5080"),
            "vLLM emits every CacheConfig attribute as a string; \"5080\" must not become 5080"
        );
        assert_eq!(labels.get("block_size").map(String::as_str), Some("16"));
        assert_eq!(labels.get("kv_cache_size_tokens").map(String::as_str), Some("81280"));
        assert_eq!(labels.get("gpu_memory_utilization").map(String::as_str), Some("0.9"));
        assert_eq!(labels.get("cache_dtype").map(String::as_str), Some("auto"));
        assert_eq!(labels.get("engine").map(String::as_str), Some("0"));
        assert_eq!(labels.len(), 6, "exactly the gauge's own labels: {labels:?}");
        println!("parsed vllm:cache_config_info label map = {labels:?}");
    }

    #[test]
    fn ignores_help_lines_and_other_families() {
        // Given: comment lines, two foreign metric families, a family whose NAME merely
        // starts with ours, and the real gauge (carrying all seven memory-relevant labels).
        let body = concat!(
            "# HELP vllm:num_requests_running Number of requests currently running on an instance.\n",
            "# TYPE vllm:num_requests_running gauge\n",
            "vllm:num_requests_running{engine=\"0\",model_name=\"qwen3\"} 3.0\n",
            "# HELP vllm:cache_config_info Information of the LLMEngine CacheConfig\n",
            "# TYPE vllm:cache_config_info gauge\n",
            "vllm:cache_config_info{block_size=\"16\",cache_dtype=\"auto\",enable_prefix_caching=\"True\",\
             engine=\"0\",gpu_memory_utilization=\"0.9\",kv_cache_max_concurrency=\"5\",\
             kv_cache_size_tokens=\"81280\",num_gpu_blocks=\"5080\"} 1\n",
            "# HELP vllm:prompt_tokens_total Number of prefill tokens processed.\n",
            "# TYPE vllm:prompt_tokens_total counter\n",
            "vllm:prompt_tokens_total{engine=\"0\",model_name=\"qwen3\"} 1.0e+04\n",
            "vllm:cache_config_info_extra{num_gpu_blocks=\"9999\"} 1\n",
        );

        // When:
        let info = parse_cache_config_info(body).expect("our gauge is present");
        let labels = info.labels();

        // Then: all seven memory-relevant labels pass through verbatim...
        for name in [
            "num_gpu_blocks",
            "block_size",
            "kv_cache_size_tokens",
            "kv_cache_max_concurrency",
            "gpu_memory_utilization",
            "cache_dtype",
            "enable_prefix_caching",
        ] {
            assert!(labels.contains_key(name), "memory-relevant label {name} missing: {labels:?}");
        }
        // ...and nothing from HELP lines, other families, or the name-prefix family does.
        assert_eq!(
            labels.get("num_gpu_blocks").map(String::as_str),
            Some("5080"),
            "the 9999 belonging to vllm:cache_config_info_extra must not leak"
        );
        assert!(!labels.contains_key("model_name"), "a foreign family's label leaked: {labels:?}");
        assert_eq!(labels.len(), 8, "no aggregation across families (7 memory labels + engine): {labels:?}");
    }

    #[test]
    fn returns_none_when_metric_absent() {
        // Given: a llama.cpp-shaped /metrics body - a clean 2xx with no vLLM gauge at all.
        let body = concat!(
            "# HELP llama_cpp_prompt_tokens_total Total number of prompt tokens processed\n",
            "# TYPE llama_cpp_prompt_tokens_total counter\n",
            "llama_cpp_prompt_tokens_total{model=\"qwen3\"} 1.234567e+06\n",
            "# HELP llama_cpp_predicted_tokens_total Total number of tokens generated\n",
            "# TYPE llama_cpp_predicted_tokens_total counter\n",
            "llama_cpp_predicted_tokens_total{model=\"qwen3\"} 9.876543e+05\n",
            "# HELP process_cpu_seconds_total Total user and system CPU time spent in seconds.\n",
            "# TYPE process_cpu_seconds_total counter\n",
            "process_cpu_seconds_total 12.5\n",
        );

        // When:
        let parsed = parse_cache_config_info(body);

        // Then: `None`, NOT an empty `Some`. This distinction is load-bearing: todo 4 caches
        // "metric absent on a clean 2xx" as a once-per-process negative, and an empty `Some`
        // would be indistinguishable from "present but label-less" and poison that decision.
        assert!(parsed.is_none(), "an absent gauge must be None, not an empty Some");
    }

    #[test]
    fn unescapes_quoted_label_values() {
        // Given: values holding an escaped quote, an embedded comma + newline, and an
        // escaped backslash. Raw string: `\"` and `\n` are the two-character TEXT escapes.
        let body = r#"vllm:cache_config_info{cache_dtype="fp8\"kv",engine="0",note="a,b\nc",quant_param="\\"} 1"#;

        // When:
        let info = parse_cache_config_info(body).expect("the gauge is present");
        let labels = info.labels();

        // Then: quotes stripped, escapes resolved, and a quoted comma never split the pair.
        assert_eq!(
            labels.get("cache_dtype").map(String::as_str),
            Some("fp8\"kv"),
            "an escaped quote must unescape, not terminate the value: {labels:?}"
        );
        assert_eq!(
            labels.get("note").map(String::as_str),
            Some("a,b\nc"),
            "a comma inside quotes belongs to the value and must not split the pair: {labels:?}"
        );
        assert_eq!(labels.get("quant_param").map(String::as_str), Some("\\"));
        assert_eq!(labels.len(), 4, "the quoted comma must not invent a label: {labels:?}");
    }

    #[test]
    fn picks_lowest_engine_across_series() {
        // Given: three matching series (multi-engine / data-parallel deployment), each with
        // its own num_gpu_blocks, deliberately NOT in ascending order.
        let body = concat!(
            "vllm:cache_config_info{engine=\"2\",num_gpu_blocks=\"2222\"} 1\n",
            "vllm:cache_config_info{engine=\"10\",num_gpu_blocks=\"1111\"} 1\n",
            "vllm:cache_config_info{engine=\"1\",num_gpu_blocks=\"5080\"} 1\n",
        );

        // When:
        let info = parse_cache_config_info(body).expect("three series");
        let labels = info.labels();

        // Then: the LOWEST `engine` wins, by string compare (documented on the parser:
        // "1" < "10" < "2"), and the chosen series is returned whole - never merged.
        assert_eq!(labels.get("engine").map(String::as_str), Some("1"));
        assert_eq!(
            labels.get("num_gpu_blocks").map(String::as_str),
            Some("5080"),
            "the whole winning series is returned, not a blend"
        );
        assert_eq!(labels.len(), 2, "series are never merged: {labels:?}");
    }
}
