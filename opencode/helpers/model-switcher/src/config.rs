//! File edits for opencode 2's config, relative to the `opencode/` dir:
//!
//! - `container_data/config/opencode/opencode.jsonc`: top-level `"model"`, and per-model
//!   `"limit"` / `"capabilities"` under `"providers"`
//! - `container_data/config/opencode/agents/*.md`: the `model:` key in each agent's frontmatter
//!
//! Edits are line-based so JSONC comments and the one-model-per-line layout survive. Each
//! opencode.jsonc write is re-parsed first and refused if the result would not parse.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub const OPENCODE_CONFIG_PATH: &str = "container_data/config/opencode/opencode.jsonc";
pub const AGENTS_DIR: &str = "container_data/config/opencode/agents";
/// Listed first in prompts and treated as "the agent model" in `all`; the rest are alphabetical.
pub const PRIMARY_AGENT: &str = "local-mind";

/// Inserted for `image on` when a model line has no capabilities object at all
/// (matches the existing opencode 2 model entries).
const DEFAULT_CAPABILITIES: &str =
    r#""capabilities": { "tools": true, "input": ["text", "image"], "output": ["text"] }"#;

// ---------------------------------------------------------------------------------------------
// JSONC
// ---------------------------------------------------------------------------------------------

/// Strip `//` and `/* */` comments (outside strings) and trailing commas, so serde_json can parse it.
pub fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while let Some(&n) = chars.peek() {
                    if n == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    remove_trailing_commas(&out)
}

fn remove_trailing_commas(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

pub fn parse_jsonc(src: &str) -> Result<Value> {
    serde_json::from_str(&strip_jsonc(src)).context("not valid JSONC")
}

fn read(path: &str) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))
}

fn read_config(path: &str) -> Result<Value> {
    parse_jsonc(&read(path)?).with_context(|| format!("Failed to parse {}", path))
}

/// Write `content` only if it still parses, so a bad line edit can't corrupt the config.
fn write_jsonc_checked(path: &str, content: &str) -> Result<()> {
    parse_jsonc(content).with_context(|| {
        format!("Refusing to write {}: the edit would leave it unparseable", path)
    })?;
    fs::write(path, content).with_context(|| format!("Failed to write {}", path))
}

/// Apply `f` to every line. Returns the new content (trailing newline kept) and the number of
/// lines that changed.
fn map_lines(content: &str, mut f: impl FnMut(&str) -> String) -> (String, usize) {
    let mut changes = 0;
    let mut out = content
        .lines()
        .map(|l| {
            let n = f(l);
            if n != l {
                changes += 1;
            }
            n
        })
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    (out, changes)
}

/// Brace/bracket depth at the start of each line, ignoring strings and comments.
fn line_start_depths(content: &str) -> Vec<usize> {
    let mut depths = Vec::new();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut in_block = false;
    for line in content.lines() {
        depths.push(depth);
        let b: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if in_block {
                if c == '*' && b.get(i + 1) == Some(&'/') {
                    in_block = false;
                    i += 1;
                }
            } else if in_str {
                if c == '\\' {
                    i += 1;
                } else if c == '"' {
                    in_str = false;
                }
            } else {
                match c {
                    '"' => in_str = true,
                    '/' if b.get(i + 1) == Some(&'/') => break,
                    '/' if b.get(i + 1) == Some(&'*') => {
                        in_block = true;
                        i += 1;
                    }
                    '{' | '[' => depth += 1,
                    '}' | ']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            i += 1;
        }
    }
    depths
}

/// Find `"key"` used as a key (followed by `:`) in `s`. Returns (start of the key's opening
/// quote, index just after the colon).
fn find_key(s: &str, key: &str) -> Option<(usize, usize)> {
    let pat = format!("\"{}\"", key);
    let mut from = 0;
    while let Some(off) = s[from..].find(&pat) {
        let start = from + off;
        let after = start + pat.len();
        let rest = &s[after..];
        let ws = rest.len() - rest.trim_start().len();
        if rest[ws..].starts_with(':') {
            return Some((start, after + ws + 1));
        }
        from = after;
    }
    None
}

/// The `open ... close` span starting at the first non-space char at/after `from`.
/// Returns (index of `open`, index of the matching `close`).
fn bracket_span(s: &str, from: usize, open: char, close: char) -> Option<(usize, usize)> {
    let rest = &s[from..];
    let start = from + (rest.len() - rest.trim_start().len());
    if !s[start..].starts_with(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in s[start..].char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some((start, start + i));
            }
        }
    }
    None
}

/// The object value of `"key":` on this line, as (open brace index, close brace index).
fn object_after_key(s: &str, key: &str) -> Option<(usize, usize)> {
    let (_, after) = find_key(s, key)?;
    bracket_span(s, after, '{', '}')
}

/// Append a raw `"k": v` entry to a single-line `{ ... }` object.
fn insert_entry(obj: &str, entry: &str) -> String {
    let close = obj.len() - 1;
    if obj[1..close].trim().is_empty() {
        return format!("{{ {} }}", entry);
    }
    let body_end = obj[..close].trim_end().len();
    format!("{}, {}{}", &obj[..body_end], entry, &obj[body_end..])
}

/// Remove the `"key": <value>` entry starting at `key_start` and ending at `value_end`
/// (exclusive), along with one adjoining comma.
fn remove_entry(obj: &str, key_start: usize, value_end: usize) -> String {
    if let Some(comma) = obj[..key_start].rfind(',') {
        if obj[comma + 1..key_start].trim().is_empty() {
            return format!("{}{}", &obj[..comma], &obj[value_end..]);
        }
    }
    let after = &obj[value_end..];
    let ws = after.len() - after.trim_start().len();
    if after[ws..].starts_with(',') {
        let rest = &after[ws + 1..];
        let ws2 = rest.len() - rest.trim_start().len();
        return format!("{}{}", &obj[..key_start], &rest[ws2..]);
    }
    format!("{}{}", &obj[..key_start], &obj[value_end..])
}

/// Set (`Some`) or remove (`None`) a numeric entry in a single-line `{ ... }` object.
fn set_number_in_object(obj: &str, key: &str, val: Option<u64>) -> String {
    match (find_key(obj, key), val) {
        (Some((_, after)), Some(v)) => {
            let rest = &obj[after..];
            let num_start = after + (rest.len() - rest.trim_start().len());
            let num_len = obj[num_start..]
                .bytes()
                .take_while(|b| b.is_ascii_digit())
                .count();
            format!("{}{}{}", &obj[..num_start], v, &obj[num_start + num_len..])
        }
        // `input` goes right after `context` (the layout the config uses), else at the end.
        (None, Some(v)) => match find_key(obj, "context").filter(|_| key == "input") {
            Some((_, after)) => {
                let rest = &obj[after..];
                let num_start = after + (rest.len() - rest.trim_start().len());
                let num_end = num_start
                    + obj[num_start..]
                        .bytes()
                        .take_while(|b| b.is_ascii_digit())
                        .count();
                format!(
                    "{}, \"{}\": {}{}",
                    &obj[..num_end],
                    key,
                    v,
                    &obj[num_end..]
                )
            }
            None => insert_entry(obj, &format!("\"{}\": {}", key, v)),
        },
        (Some((key_start, after)), None) => {
            let rest = &obj[after..];
            let num_start = after + (rest.len() - rest.trim_start().len());
            let num_end = num_start
                + obj[num_start..]
                    .bytes()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
            remove_entry(obj, key_start, num_end)
        }
        (None, None) => obj.to_string(),
    }
}

fn splice(line: &str, (s, e): (usize, usize), replacement: &str) -> String {
    format!("{}{}{}", &line[..s], replacement, &line[e + 1..])
}

// ---------------------------------------------------------------------------------------------
// Models (opencode.jsonc)
// ---------------------------------------------------------------------------------------------

/// opencode 2 calls it `providers`; v1 used `provider`. Accept either for reading.
fn providers(cfg: &Value) -> Option<&serde_json::Map<String, Value>> {
    cfg.get("providers")
        .or_else(|| cfg.get("provider"))
        .and_then(Value::as_object)
}

/// Every `provider/model` id declared in opencode.jsonc, sorted.
pub fn read_models(path: &str) -> Result<Vec<String>> {
    Ok(read_model_info(path)?.into_iter().map(|m| m.id).collect())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub context: Option<u64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub image: bool,
}

pub fn read_model_info(path: &str) -> Result<Vec<ModelInfo>> {
    let cfg = read_config(path)?;
    let providers =
        providers(&cfg).with_context(|| format!("No \"providers\" block in {}", path))?;
    let mut out = Vec::new();
    for (pid, p) in providers {
        let Some(models) = p.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (mid, m) in models {
            let limit = |k: &str| m.get("limit").and_then(|l| l.get(k)).and_then(Value::as_u64);
            // opencode 2: capabilities.input; v1: modalities.input
            let inputs = m
                .get("capabilities")
                .or_else(|| m.get("modalities"))
                .and_then(|c| c.get("input"))
                .and_then(Value::as_array);
            out.push(ModelInfo {
                id: format!("{}/{}", pid, mid),
                context: limit("context"),
                input: limit("input"),
                output: limit("output"),
                image: inputs.is_some_and(|a| a.iter().any(|v| v.as_str() == Some("image"))),
            });
        }
    }
    Ok(out)
}

/// The top-level `"model"` in opencode.jsonc.
pub fn get_default_model(path: &str) -> Result<String> {
    read_config(path)?
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("No top-level \"model\" in {}", path))
}

/// Replace the string value of `"key": "..."` on this line, keeping everything around it.
fn replace_string_value(line: &str, key: &str, value: &str) -> Option<String> {
    let (_, after) = find_key(line, key)?;
    let open = after + line[after..].find('"')?;
    let mut close = None;
    let mut esc = false;
    for (i, c) in line[open + 1..].char_indices() {
        if esc {
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '"' {
            close = Some(open + 1 + i);
            break;
        }
    }
    let close = close?;
    Some(format!("{}{}{}", &line[..open + 1], value, &line[close..]))
}

/// Set the top-level `"model"` in opencode.jsonc (nested `"model"` keys are left alone).
pub fn update_default_model(path: &str, model: &str) -> Result<()> {
    let content = read(path)?;
    let depths = line_start_depths(&content);
    let idx = content
        .lines()
        .enumerate()
        .position(|(i, l)| depths[i] == 1 && l.trim_start().starts_with("\"model\""))
        .with_context(|| format!("No top-level \"model\" line in {}", path))?;
    let mut failed = false;
    let mut i = 0;
    let (new_content, _) = map_lines(&content, |l| {
        let out = if i == idx {
            replace_string_value(l, "model", model).unwrap_or_else(|| {
                failed = true;
                l.to_string()
            })
        } else {
            l.to_string()
        };
        i += 1;
        out
    });
    if failed {
        bail!("Could not parse the \"model\" line in {}", path);
    }
    write_jsonc_checked(path, &new_content)
}

/// Set or remove one numeric key inside the `"limit": { ... }` object on a model line.
/// Lines without a single-line limit object are returned unchanged.
pub fn edit_limit_on_line(line: &str, key: &str, val: Option<u64>) -> String {
    let Some(span) = object_after_key(line, "limit") else {
        return line.to_string();
    };
    let obj = set_number_in_object(&line[span.0..=span.1], key, val);
    splice(line, span, &obj)
}

/// Update every model's `limit`: `context` is set when `Some` (left alone when `None`),
/// `input` is set when `Some` and removed when `None`. Returns the number of model lines changed.
pub fn update_limits(path: &str, context: Option<u64>, input: Option<u64>) -> Result<usize> {
    let content = read(path)?;
    let (new_content, changes) = map_lines(&content, |l| {
        let l = match context {
            Some(c) => edit_limit_on_line(l, "context", Some(c)),
            None => l.to_string(),
        };
        edit_limit_on_line(&l, "input", input)
    });
    write_jsonc_checked(path, &new_content)?;
    Ok(changes)
}

fn parse_string_array(arr: &str) -> Vec<String> {
    arr[1..arr.len() - 1]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn render_string_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|i| format!("\"{}\"", i)).collect();
    format!("[{}]", quoted.join(", "))
}

/// Add or remove `"image"` in `capabilities.input` on a model line (one with a `"limit"`).
pub fn set_image_on_line(line: &str, enable: bool) -> String {
    let Some((limit_start, _)) = find_key(line, "limit") else {
        return line.to_string();
    };
    let Some(caps_span) = object_after_key(line, "capabilities") else {
        // No capabilities object yet: add the standard one in front of "limit".
        if !enable {
            return line.to_string();
        }
        return format!(
            "{}{}, {}",
            &line[..limit_start],
            DEFAULT_CAPABILITIES,
            &line[limit_start..]
        );
    };
    let caps = &line[caps_span.0..=caps_span.1];
    let new_caps = match find_key(caps, "input") {
        Some((_, after)) => {
            let Some(arr_span) = bracket_span(caps, after, '[', ']') else {
                return line.to_string();
            };
            let mut items = parse_string_array(&caps[arr_span.0..=arr_span.1]);
            let has = items.iter().any(|i| i == "image");
            if enable == has {
                return line.to_string();
            }
            if enable {
                items.push("image".to_string());
            } else {
                items.retain(|i| i != "image");
            }
            splice(caps, arr_span, &render_string_array(&items))
        }
        None if enable => insert_entry(caps, r#""input": ["text", "image"]"#),
        None => return line.to_string(),
    };
    splice(line, caps_span, &new_caps)
}

/// Add/remove image input on every model. Returns the number of model lines changed.
pub fn update_image(path: &str, enable: bool) -> Result<usize> {
    let content = read(path)?;
    let (new_content, changes) = map_lines(&content, |l| set_image_on_line(l, enable));
    write_jsonc_checked(path, &new_content)?;
    Ok(changes)
}

// ---------------------------------------------------------------------------------------------
// Agents (agents/*.md frontmatter)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub name: String,
    pub path: String,
}

/// Index of the closing `---` of a leading YAML frontmatter block.
fn frontmatter_end(lines: &[&str]) -> Option<usize> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return None;
    }
    lines
        .iter()
        .skip(1)
        .position(|l| l.trim_end() == "---")
        .map(|p| p + 1)
}

/// Index of the top-level `model:` line inside the frontmatter (indented keys don't count).
fn model_line_index(lines: &[&str]) -> Option<usize> {
    let end = frontmatter_end(lines)?;
    (1..end).find(|&i| lines[i].starts_with("model:"))
}

/// The `model:` an agent pins in its frontmatter.
pub fn get_current_model(path: &str) -> Result<String> {
    let content = read(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let idx = model_line_index(&lines)
        .with_context(|| format!("No model: in the frontmatter of {}", path))?;
    let value = lines[idx]["model:".len()..].trim();
    Ok(value.trim_matches('"').trim_matches('\'').to_string())
}

pub fn update_agent_model(path: &str, model: &str) -> Result<()> {
    let content = read(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let idx = model_line_index(&lines)
        .with_context(|| format!("No model: in the frontmatter of {}", path))?;
    let mut i = 0;
    let (new_content, _) = map_lines(&content, |l| {
        let out = if i == idx {
            format!("model: {}", model)
        } else {
            l.to_string()
        };
        i += 1;
        out
    });
    fs::write(path, new_content).with_context(|| format!("Failed to write {}", path))
}

/// Every `agents/*.md` whose frontmatter pins a `model:` (agents without one inherit the
/// default). `PRIMARY_AGENT` first, then alphabetical.
pub fn discover_agents(dir: &str) -> Result<Vec<Agent>> {
    let mut agents = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let path_str = path.to_string_lossy().into_owned();
        if get_current_model(&path_str).is_ok() {
            agents.push(Agent {
                name: name.to_string(),
                path: path_str,
            });
        }
    }
    agents.sort_by(|a, b| {
        (a.name != PRIMARY_AGENT, &a.name).cmp(&(b.name != PRIMARY_AGENT, &b.name))
    });
    Ok(agents)
}

/// Agents in `dir` that don't pin a model (they inherit the default).
pub fn unpinned_agents(dir: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if get_current_model(&path.to_string_lossy()).is_err() {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Fail early with a useful message when not run from the `opencode/` dir.
pub fn check_layout() -> Result<()> {
    for p in [OPENCODE_CONFIG_PATH, AGENTS_DIR] {
        if !Path::new(p).exists() {
            bail!(
                "{} not found: run from the opencode/ dir (bin/model-switcher does this for you)",
                p
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
