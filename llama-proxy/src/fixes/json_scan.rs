//! Structural JSON-key scanner for tool-call arguments fixing.
//!
//! Two entry points, both depth-1 only (direct children of the root object):
//! - [`top_level_key_count`]: serde-stream walk (spec-mandated mechanism).
//! - [`top_level_key_spans`]: ordered byte-offset capture via a raw structural
//!   scanner, so a later fix can rebuild the raw string keeping the FIRST value.
//!
//! Why two mechanisms: serde_json's stream `MapAccess` never surfaces byte
//! offsets mid-walk, so offset capture must be a brace/bracket/quote-aware raw
//! scan; counting stays on the parser. The two agree on every valid input and
//! on structurally malformed input (both yield 0 / empty), pinned by
//! `count_matches_spans_on_every_fixture`.
//!
//! Premise (pinned by `duplicate_keys_counted_twice_by_stream_premise`):
//! duplicate-key collapse happens ONLY inside serde_json's `Value`
//! MapAccumulator (value/de.rs), NOT in the parser stream — an `IgnoredAny`
//! walk sees every entry. If a serde_json upgrade ever breaks that, switch
//! `top_level_key_count` to the raw scanner (same signature).
//!
//! The naive baseline this replaces (`str::matches("\"filePath\"").count()`,
//! the current practice in `ToolCallAccumulator::accumulate_and_check`)
//! miscounts: on `{"msg":"he said \"filePath"}` it returns 1 — the value's own
//! closing quote completes a `"filePath"` substring — and on
//! `{"meta":{"filePath":"/x"}}` it counts a nested-only key. The scanner
//! returns 0 for both; see `key_appearance_inside_string_value_is_zero`.

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::Deserializer as _;
use std::fmt;
use std::ops::Range;

/// Byte spans of one depth-1 key/value entry into the input string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeySpan {
    /// Quoted key token including quotes, e.g. `"filePath"`.
    pub(crate) key: Range<usize>,
    /// Full value token: quoted string incl. quotes, container incl. braces,
    /// or literal (number / `true` / `false` / `null`).
    pub(crate) value: Range<usize>,
}

struct KeyCounter<'a> {
    key: &'a str,
}

impl<'de, 'a> Visitor<'de> for KeyCounter<'a> {
    type Value = usize;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut count = 0usize;
        // String (not &str) keys: an escaped key like "file\u0050ath" cannot
        // borrow from the input and would error the walk.
        while let Some(k) = map.next_key::<String>()? {
            count += usize::from(k == self.key);
            // Every value must be consumed, matched or not, or the walk
            // desynchronizes at the next key.
            map.next_value::<IgnoredAny>()?;
        }
        Ok(count)
    }
}

/// Count occurrences of `key` among the direct children of the root JSON
/// object. Duplicate keys count separately. Malformed JSON yields 0; callers
/// needing strict validity gate on `serde_json::from_str::<Value>` first.
/// Data after the closing brace is not inspected (the stream stops there).
pub(crate) fn top_level_key_count(json: &str, key: &str) -> usize {
    let mut de = serde_json::de::Deserializer::from_str(json);
    de.deserialize_map(KeyCounter { key }).unwrap_or(0)
}

/// Ordered spans for every occurrence of `key` at depth 1.
/// Empty when the input is not structurally a JSON object (callers detect
/// invalid JSON separately). Keeping only the first entry means deleting
/// `preceding-comma..value.end` for every later span; see KeySpan.
pub(crate) fn top_level_key_spans(json: &str, key: &str) -> Vec<KeySpan> {
    scan_spans(json, key).unwrap_or_default()
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while matches!(b.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// `b[i]` must be `b'"'`; returns the index just past the closing quote.
/// Raw control chars (<0x20) are rejected, as serde_json does. `\\` skips
/// exactly one char, which covers `\"`, `\\\\` and `\uXXXX` (hex digits are
/// never `"` or `\\`).
fn scan_string(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            c if c < 0x20 => return None,
            _ => j += 1,
        }
    }
    None
}

/// Index past the value token at `b[i]`: strings via `scan_string`, containers
/// via quote-aware depth tracking, literals run to the next delimiter.
fn scan_value(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => scan_string(b, i),
        b'{' | b'[' => {
            let mut depth = 0i32;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    // `continue` (no j += 1 below): scan_string already left
                    // j on the first byte after the closing quote.
                    b'"' => {
                        j = scan_string(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    return Some(j + 1);
                } else if depth < 0 {
                    return None;
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while !matches!(b.get(j), None | Some(b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')) {
                j += 1;
            }
            Some(j)
        }
    }
}

fn scan_spans(json: &str, key: &str) -> Option<Vec<KeySpan>> {
    let b = json.as_bytes();
    let mut i = skip_ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return None;
    }
    let mut out = Vec::new();
    i = skip_ws(b, i + 1);
    if *b.get(i)? == b'}' {
        return Some(out); // {}
    }
    loop {
        if *b.get(i)? != b'"' {
            return None; // structural error -> empty, agreeing with count's 0
        }
        let key_start = i;
        let after_key = scan_string(b, i)?;
        let mut j = skip_ws(b, after_key);
        if b.get(j) != Some(&b':') {
            return None;
        }
        j = skip_ws(b, j + 1);
        let val_end = scan_value(b, j)?;
        // Escaped keys decode via serde so `file\u0050ath` matches `filePath`,
        // mirroring the String-keyed stream walk; unescaped tokens compare raw.
        let token = &json[key_start..after_key];
        let inner = &token[1..token.len() - 1];
        let hit = if inner.contains('\\') {
            matches!(serde_json::from_str::<String>(token), Ok(d) if d == key)
        } else {
            inner == key
        };
        if hit {
            out.push(KeySpan {
                key: key_start..after_key,
                value: j..val_end,
            });
        }
        i = skip_ws(b, val_end);
        match *b.get(i)? {
            b',' => i = skip_ws(b, i + 1),
            b'}' => return Some(out), // trailing data after the object is ignored
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- oracle premise + duplicate-key discipline ----

    #[test]
    fn duplicate_keys_counted_twice_by_stream_premise() {
        // ORACLE PREMISE PIN: serde_json's stream MapAccess yields BOTH
        // entries; collapse lives only in Value's MapAccumulator. If this
        // ever counts 1 on a serde_json upgrade, move count onto the raw
        // scanner (spec escape hatch).
        let json = r#"{"filePath":1,"filePath":2}"#;
        assert_eq!(top_level_key_count(json, "filePath"), 2);
        let spans = top_level_key_spans(json, "filePath");
        assert_eq!(spans.len(), 2);
        // value slices point at the literals, so a rebuild can keep the first
        assert_eq!(&json[spans[0].value.clone()], "1");
        assert_eq!(&json[spans[1].value.clone()], "2");
        assert_eq!(&json[spans[0].key.clone()], "\"filePath\"");
    }

    // ---- depth discipline ----

    #[test]
    fn nested_key_not_counted_at_depth_one() {
        // One depth-1 hit; the nested object/array copies must not count.
        // Naive substring counting returns 3 here.
        let json = r#"{"a":{"filePath":1},"b":[{"filePath":2}],"filePath":3}"#;
        assert_eq!(top_level_key_count(json, "filePath"), 1);
        let spans = top_level_key_spans(json, "filePath");
        assert_eq!(spans.len(), 1);
        assert_eq!(&json[spans[0].value.clone()], "3");
        // Purely-nested key: naive counting returns 1, truth is 0.
        let nested_only = r#"{"meta":{"filePath":"/x"},"content":"hi"}"#;
        assert_eq!(top_level_key_count(nested_only, "filePath"), 0);
        assert!(top_level_key_spans(nested_only, "filePath").is_empty());
    }

    #[test]
    fn key_appearance_inside_string_value_is_zero() {
        // The value's closing quote completes a raw `"filePath"` substring,
        // so the old matches("\"filePath\"") probe counts 1. Structurally the
        // only depth-1 key is "msg".
        let json = r#"{"msg":"he said \"filePath"}"#;
        assert_eq!(top_level_key_count(json, "filePath"), 0);
        assert!(top_level_key_spans(json, "filePath").is_empty());
        // json stays valid: proves the fixture is well-formed, not a parse
        // failure doing the work.
        serde_json::from_str::<serde_json::Value>(json).expect("valid fixture");
    }

    // ---- escaping discipline in the raw scanner ----

    #[test]
    fn escaped_quote_payload_counts_two() {
        // \" escapes inside the content string must not shift the value end.
        let json = r#"{"content":"x = \"filePath\"; y","filePath":"/a","filePath":"/b"}"#;
        serde_json::from_str::<serde_json::Value>(json).expect("valid fixture");
        assert_eq!(top_level_key_count(json, "filePath"), 2);
        assert_eq!(top_level_key_spans(json, "filePath").len(), 2);
    }

    #[test]
    fn unicode_escape_key_matches_target() {
        // \u0050 == 'P': the decoded key equals "filePath", and the
        // String-keyed stream walk compares decoded keys too -> both count 2.
        let json = r#"{"file\u0050ath":1,"filePath":2}"#;
        assert_eq!(top_level_key_count(json, "filePath"), 2);
        assert_eq!(top_level_key_spans(json, "filePath").len(), 2);
        // Conversely a key that merely CONTAINS the target between escaped
        // quotes decodes to something else -> only the real key counts.
        let decoy = r#"{"say\"filePath\"x":1,"filePath":2}"#;
        assert_eq!(top_level_key_count(decoy, "filePath"), 1);
        assert_eq!(top_level_key_spans(decoy, "filePath").len(), 1);
    }

    #[test]
    fn cjk_payload_offsets_are_char_boundary_safe() {
        // Multibyte payloads shift every later offset; spans must stay
        // byte-exact so slices land on token boundaries.
        let json = r#"{"content":"你好世界","filePath":"/路径/文件.rs","note":"中","filePath":"/第二"}"#;
        serde_json::from_str::<serde_json::Value>(json).expect("valid fixture");
        let spans = top_level_key_spans(json, "filePath");
        assert_eq!(spans.len(), 2);
        assert_eq!(&json[spans[0].value.clone()], "\"/路径/文件.rs\"");
        assert_eq!(&json[spans[1].value.clone()], "\"/第二\"");
        assert_eq!(top_level_key_count(json, "filePath"), 2);
    }

    // ---- value shapes ----

    #[test]
    fn empty_containers_as_values() {
        let json = r#"{"filePath":{},"filePath":[]}"#;
        assert_eq!(top_level_key_count(json, "filePath"), 2);
        let spans = top_level_key_spans(json, "filePath");
        assert_eq!(&json[spans[0].value.clone()], "{}");
        assert_eq!(&json[spans[1].value.clone()], "[]");
    }

    #[test]
    fn pretty_whitespace_newlines_counted() {
        let json = "{\n  \"filePath\" : 1,\n  \"other\": 2,\n  \"filePath\"\t:\t{\"nested\":true}\n}\n";
        assert_eq!(top_level_key_count(json, "filePath"), 2);
        let spans = top_level_key_spans(json, "filePath");
        assert_eq!(spans.len(), 2);
        assert_eq!(&json[spans[1].value.clone()], "{\"nested\":true}");
    }

    #[test]
    fn deeply_nested_one_and_two_levels() {
        let one = r#"{"a":{"filePath":1},"filePath":2}"#;
        assert_eq!(top_level_key_count(one, "filePath"), 1);
        assert_eq!(top_level_key_spans(one, "filePath").len(), 1);
        let two = r#"{"a":{"b":{"filePath":1}},"c":[[{"filePath":2}]],"filePath":3}"#;
        assert_eq!(top_level_key_count(two, "filePath"), 1);
        assert_eq!(top_level_key_spans(two, "filePath").len(), 1);
    }

    // ---- malformed input ----

    #[test]
    fn malformed_yields_zero_everywhere() {
        let cases = [
            "",
            "   ",
            "-",
            "123",
            "\"filePath\"",
            "[{\"filePath\":1}]",
            "{\"filePath\":1,",
            "{\"filePath\"",
            "{\"filePath\":1,}",
            "{-}",
            "{\"a\":1\"filePath\":2}",
        ];
        for json in cases {
            assert_eq!(top_level_key_count(json, "filePath"), 0, "count {json:?}");
            assert!(top_level_key_spans(json, "filePath").is_empty(), "spans {json:?}");
        }
    }

    #[test]
    fn trailing_garbage_after_object_ignored_by_both() {
        // Deliberate shared leniency: the stream walk stops at the closing
        // brace (no EOF check) and the scanner never looks past it, so the
        // two agree. Strict validity remains the caller's from_str gate.
        let json = r#"{"filePath":1}trailing junk"#;
        assert_eq!(top_level_key_count(json, "filePath"), 1);
        assert_eq!(top_level_key_spans(json, "filePath").len(), 1);
    }

    // ---- cross validation: the two mechanisms must agree ----

    #[test]
    fn count_matches_spans_on_every_fixture() {
        let fixtures: &[(&str, usize)] = &[
            (r#"{"filePath":1,"filePath":2}"#, 2),
            (r#"{"a":{"filePath":1},"b":[{"filePath":2}]}"#, 0),
            (r#"{"msg":"he said \"filePath"}"#, 0),
            (r#"{"content":"x = \"filePath\"; y","filePath":"/a","filePath":"/b"}"#, 2),
            (r#"{"file\u0050ath":1,"filePath":2}"#, 2),
            (r#"{"say\"filePath\"x":1,"filePath":2}"#, 1),
            (
                r#"{"content":"你好世界","filePath":"/路径/文件.rs","note":"中","filePath":"/第二"}"#,
                2,
            ),
            (r#"{"filePath":{},"filePath":[]}"#, 2),
            (
                "{\n  \"filePath\" : 1,\n  \"other\": 2,\n  \"filePath\"\t:\t{\"nested\":true}\n}\n",
                2,
            ),
            (r#"{"a":{"b":{"filePath":1}},"c":[[{"filePath":2}]],"filePath":3}"#, 1),
            (r#"{"filePath":1}trailing junk"#, 1),
            (r#"{"filePath":{"deep":[{"x":"\"}"}]},"filePath":2,"z":"}{["}"#, 2),
            (r#"{"filePath":{"a":"x"},"filePath":["e","f"],"g":{"h":{"i":[1,"}}]"]}}}"#, 2),
            (r#"{"filePath":-1.5e3,"filePath":true,"filePath":null,"other":""}"#, 3),
            (r#"{"":""}"#, 0),
        ];
        for (json, expected) in fixtures {
            assert_eq!(top_level_key_count(json, "filePath"), *expected, "count {json:?}");
            assert_eq!(top_level_key_spans(json, "filePath").len(), *expected, "spans {json:?}");
        }
    }

    // ---- rebuild sufficiency (consumer contract for tasks 24/25) ----

    #[test]
    fn first_wins_rebuild_from_spans() {
        let json = r#"{"content":"x","filePath":"/a","role":"user","filePath":"/b","filePath":"/c"}"#;
        let rebuilt = rebuild_first_wins(json, "filePath");
        assert_eq!(&rebuilt, r#"{"content":"x","filePath":"/a","role":"user"}"#);
        let v: serde_json::Value = serde_json::from_str(&rebuilt).expect("rebuilt is valid");
        assert_eq!(v["filePath"], "/a");
        assert_eq!(v["role"], "user");
        assert_eq!(top_level_key_count(&rebuilt, "filePath"), 1);

        // Pretty-printed: the cut must walk back over newline/tab whitespace
        // to reach the comma preceding the key.
        let pretty = "{\n  \"content\":\"x\",\n  \"filePath\":\"/a\",\n  \"filePath\":\"/b\"\n}";
        let rebuilt_p = rebuild_first_wins(pretty, "filePath");
        assert_eq!(&rebuilt_p, "{\n  \"content\":\"x\",\n  \"filePath\":\"/a\"\n}");
        serde_json::from_str::<serde_json::Value>(&rebuilt_p).expect("rebuilt pretty is valid");
    }

    /// Drop every depth-1 `key` entry after the first using ONLY the spans:
    /// for each later span, delete `preceding-comma..value.end`. This is the
    /// proof that the exposed offsets are sufficient for task 24/25.
    fn rebuild_first_wins(json: &str, key: &str) -> String {
        let spans = top_level_key_spans(json, key);
        assert!(spans.len() > 1);
        let b = json.as_bytes();
        let mut cuts: Vec<Range<usize>> = Vec::new();
        for sp in &spans[1..] {
            let mut s = sp.key.start;
            while matches!(b[s - 1], b' ' | b'\t' | b'\n' | b'\r') {
                s -= 1;
            }
            assert_eq!(b[s - 1], b',');
            cuts.push(s - 1..sp.value.end);
        }
        let mut out = String::new();
        let mut prev = 0usize;
        for c in &cuts {
            out.push_str(&json[prev..c.start]);
            prev = c.end;
        }
        out.push_str(&json[prev..]);
        out
    }
}
