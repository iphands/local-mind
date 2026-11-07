use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Minimal self-cleaning temp dir (no tempfile crate: keeps the build offline-friendly).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "model-switcher-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&p)?;
        Ok(TempDir(p))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The model-line shape opencode 2's opencode.jsonc uses.
const V2_LINE: &str = r#"        "cosmo-proxy": { "name": "cosmo-proxy", "capabilities": { "tools": true, "input": ["text", "image"], "output": ["text"] }, "limit": { "context": 262144, "input": 222822, "output": 262144 } },"#;

const V2_CONFIG: &str = r#"{
  // OpenCode 2 schema: providers / package / settings
  "$schema": "https://opencode.ai/config.json",
  "model": "cosmo-proxy/cosmo-proxy",
  "default_agent": "local-mind", // trailing comment
  /* block
     comment with "model": "nope" */
  "permissions": [
    { "action": "shell", "resource": "*", "effect": "allow" }
  ],
  "providers": {
    "cosmo-proxy": {
      "name": "cosmo-proxy",
      "settings": { "baseURL": "http://cosmo.lan:8799/v1" },
      "models": {
        "cosmo-proxy": { "name": "cosmo-proxy", "capabilities": { "tools": true, "input": ["text", "image"], "output": ["text"] }, "limit": { "context": 262144, "input": 222822, "output": 262144 } },
        "cosmo-light": { "name": "cosmo-light", "capabilities": { "tools": true, "input": ["text"], "output": ["text"] }, "limit": { "context": 131072, "output": 32768 } }
      }
    },
    "cosmo-00": {
      "name": "cosmo-00",
      "models": {
        "cosmo-6000": { "name": "cosmo-6000", "limit": { "context": 262144, "output": 262144 } },
      },
    },
  }
}
"#;

const AGENT_MD: &str = "---\ndescription: Senior Engineer\nmode: subagent\nmodel: cosmo-proxy/cosmo-proxy\ncolor: \"#FF6B35\"\npermissions:\n  - action: read\n    model: not-this-one\n---\n\nmodel: also not this one (body text)\n";

fn write_config(dir: &TempDir, content: &str) -> String {
    let p = dir.path().join("opencode.jsonc");
    fs::write(&p, content).unwrap();
    p.to_string_lossy().into_owned()
}

// ---------- JSONC parsing ----------

#[test]
fn parses_v2_config_with_comments_and_trailing_commas() {
    let v = parse_jsonc(V2_CONFIG).unwrap();
    assert_eq!(v["model"], "cosmo-proxy/cosmo-proxy");
    assert_eq!(v["$schema"], "https://opencode.ai/config.json"); // `//` inside a string survives
}

#[test]
fn read_models_lists_provider_model_ids_sorted() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, V2_CONFIG);
    assert_eq!(
        read_models(&path).unwrap(),
        vec![
            "cosmo-00/cosmo-6000",
            "cosmo-proxy/cosmo-light",
            "cosmo-proxy/cosmo-proxy"
        ]
    );
}

#[test]
fn read_models_accepts_v1_provider_key() {
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        r#"{ "provider": { "p": { "models": { "m": { "name": "m" } } } } }"#,
    );
    assert_eq!(read_models(&path).unwrap(), vec!["p/m"]);
}

#[test]
fn read_model_info_reports_limits_and_image() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, V2_CONFIG);
    let info = read_model_info(&path).unwrap();
    let light = info.iter().find(|m| m.id == "cosmo-proxy/cosmo-light").unwrap();
    assert_eq!(light.context, Some(131072));
    assert_eq!(light.input, None);
    assert!(!light.image);
    let proxy = info.iter().find(|m| m.id == "cosmo-proxy/cosmo-proxy").unwrap();
    assert_eq!(proxy.input, Some(222822));
    assert!(proxy.image);
}

// ---------- default model ----------

#[test]
fn default_model_round_trip_keeps_comments_and_layout() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, V2_CONFIG);
    assert_eq!(get_default_model(&path).unwrap(), "cosmo-proxy/cosmo-proxy");
    update_default_model(&path, "cosmo-00/cosmo-6000").unwrap();
    assert_eq!(get_default_model(&path).unwrap(), "cosmo-00/cosmo-6000");
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(
        after,
        V2_CONFIG.replacen(
            r#""model": "cosmo-proxy/cosmo-proxy""#,
            r#""model": "cosmo-00/cosmo-6000""#,
            1
        )
    );
}

#[test]
fn default_model_ignores_nested_model_keys() {
    let dir = TempDir::new().unwrap();
    let cfg = "{\n  \"agent\": {\n    \"x\": {\n      \"model\": \"a/a\"\n    }\n  },\n  \"model\": \"b/b\"\n}\n";
    let path = write_config(&dir, cfg);
    update_default_model(&path, "c/c").unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("\"model\": \"a/a\""));
    assert!(after.contains("\"model\": \"c/c\"\n"), "no comma added: {after}");
}

// ---------- limits ----------

#[test]
fn limit_edit_does_not_touch_capabilities_input() {
    // Regression: v1 code edited the first "input": on the line, which in v2 is the
    // capabilities.input array.
    let out = edit_limit_on_line(V2_LINE, "input", Some(200000));
    assert!(out.contains(r#""input": ["text", "image"]"#));
    assert!(out.contains(r#""limit": { "context": 262144, "input": 200000, "output": 262144 }"#));
}

#[test]
fn limit_input_add_and_remove() {
    let line = r#"  "m": { "name": "m", "limit": { "context": 1000, "output": 10 } },"#;
    let added = edit_limit_on_line(line, "input", Some(850));
    assert_eq!(
        added,
        r#"  "m": { "name": "m", "limit": { "context": 1000, "input": 850, "output": 10 } },"#
    );
    assert_eq!(edit_limit_on_line(&added, "input", None), line);
    // removing from the middle of the object
    let removed = edit_limit_on_line(V2_LINE, "input", None);
    assert!(removed.contains(r#""limit": { "context": 262144, "output": 262144 }"#));
    assert!(removed.contains(r#""input": ["text", "image"]"#));
}

#[test]
fn limit_edit_leaves_non_model_lines_alone() {
    let line = r#"  "model": "cosmo-proxy/cosmo-proxy","#;
    assert_eq!(edit_limit_on_line(line, "context", Some(5)), line);
}

#[test]
fn update_limits_sets_every_model_and_ctx_off_only_drops_input() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, V2_CONFIG);
    assert_eq!(update_limits(&path, Some(131072), Some(111411)).unwrap(), 3);
    for m in read_model_info(&path).unwrap() {
        assert_eq!((m.context, m.input), (Some(131072), Some(111411)), "{}", m.id);
    }
    assert_eq!(update_limits(&path, None, None).unwrap(), 3);
    for m in read_model_info(&path).unwrap() {
        assert_eq!((m.context, m.input), (Some(131072), None), "{}", m.id);
    }
    // comments survive
    assert!(fs::read_to_string(&path).unwrap().contains("// trailing comment"));
}

// ---------- image ----------

#[test]
fn image_toggle_edits_capabilities_input() {
    let off = set_image_on_line(V2_LINE, false);
    assert!(off.contains(r#""capabilities": { "tools": true, "input": ["text"], "output": ["text"] }"#));
    assert!(off.contains(r#""input": 222822"#), "limit.input untouched");
    assert_eq!(set_image_on_line(&off, false), off, "idempotent");
    assert_eq!(set_image_on_line(&off, true), V2_LINE);
    assert_eq!(set_image_on_line(V2_LINE, true), V2_LINE, "idempotent");
}

#[test]
fn image_on_adds_capabilities_when_missing() {
    let line = r#"  "m": { "name": "m", "limit": { "context": 1000 } }"#;
    let on = set_image_on_line(line, true);
    assert_eq!(
        on,
        r#"  "m": { "name": "m", "capabilities": { "tools": true, "input": ["text", "image"], "output": ["text"] }, "limit": { "context": 1000 } }"#
    );
    assert_eq!(set_image_on_line(line, false), line);
}

#[test]
fn update_image_on_and_off_across_config() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, V2_CONFIG);
    assert_eq!(update_image(&path, true).unwrap(), 2); // cosmo-light + cosmo-6000
    assert!(read_model_info(&path).unwrap().iter().all(|m| m.image));
    assert_eq!(update_image(&path, false).unwrap(), 3);
    assert!(read_model_info(&path).unwrap().iter().all(|m| !m.image));
}

#[test]
fn checked_write_refuses_broken_jsonc() {
    let dir = TempDir::new().unwrap();
    let path = write_config(&dir, V2_CONFIG);
    assert!(write_jsonc_checked(&path, "{ \"model\": ").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), V2_CONFIG);
}

// ---------- agents ----------

#[test]
fn agent_model_only_reads_and_writes_top_level_frontmatter() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("neckbeard.md");
    fs::write(&p, AGENT_MD).unwrap();
    let path = p.to_string_lossy().into_owned();
    assert_eq!(get_current_model(&path).unwrap(), "cosmo-proxy/cosmo-proxy");
    update_agent_model(&path, "cosmo-01/cosmo-4060").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        AGENT_MD.replacen("model: cosmo-proxy/cosmo-proxy", "model: cosmo-01/cosmo-4060", 1)
    );
}

#[test]
fn agent_without_frontmatter_model_is_an_error() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("x.md");
    fs::write(&p, "---\nmode: primary\n---\nmodel: body/only\n").unwrap();
    let path = p.to_string_lossy().into_owned();
    assert!(get_current_model(&path).is_err());
    assert!(update_agent_model(&path, "a/b").is_err());
}

#[test]
fn discover_agents_puts_primary_first_and_skips_unpinned() {
    let dir = TempDir::new().unwrap();
    for name in ["neckbeard", "chat", "local-mind", "hoodie"] {
        fs::write(dir.path().join(format!("{name}.md")), AGENT_MD).unwrap();
    }
    fs::write(dir.path().join("plain.md"), "---\nmode: subagent\n---\n").unwrap();
    fs::write(dir.path().join("notes.txt"), AGENT_MD).unwrap();
    let d = dir.path().to_string_lossy().into_owned();
    let names: Vec<String> = discover_agents(&d).unwrap().into_iter().map(|a| a.name).collect();
    assert_eq!(names, vec!["local-mind", "chat", "hoodie", "neckbeard"]);
    assert_eq!(unpinned_agents(&d).unwrap(), vec!["plain"]);
}
