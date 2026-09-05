//! Reprompt engine — silently re-prompts the backend when it returns a premature stop.
//!
//! When finish_reason="stop" with no tool_calls, the engine injects a follow-up
//! user message (loaded from config) and re-sends the request to the backend.
//! - If the response contains any done_sentinel → return the original clean stop.
//! - If the response has tool_calls or a non-stop finish_reason → return it, with every
//!   assistant text seen so far merged into its content.
//! - After max_retries exhaustion → return the original stop, with every assistant text
//!   seen so far merged into its content.
//!
//! The engine never drops assistant text it has already received. A follow-up turn is an
//! *addition* to the stopped turn, never a replacement for it — otherwise a subagent whose
//! whole job is to answer once (a code reviewer, say) has its answer silently swallowed and
//! the client sees a session made entirely of tool calls with no text.
//!
//! Requests that expose no file-mutating tools are skipped entirely (see
//! `request_is_read_only`). Those are read-only subagents: they have no task list to resume,
//! so the continue-prompt only pushes them into more pointless searching.
//!
//! When dynamic_prompt is enabled (default), the prompt file is re-read from disk on
//! each trigger if its mtime has changed since the last read. This allows live edits
//! to the prompt without restarting the proxy.

use crate::backends::BackendNode;
use crate::config::RepromptConfig;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

pub struct RepromptEngine {
    /// Current prompt text — guarded for dynamic reload
    prompt: RwLock<String>,
    /// Path to reload from (None when using inline prompt)
    prompt_file: Option<PathBuf>,
    /// mtime of the prompt file at last successful read
    last_mtime: RwLock<Option<SystemTime>>,
    /// Re-read the file on each trigger if mtime changed (default: true)
    dynamic_prompt: bool,
    pub max_retries: u32,
    pub done_sentinels: Vec<String>,
    log_stop_responses: bool,
    /// Skip the whole engine for requests that expose no file-mutating tools
    skip_read_only_requests: bool,
}

/// Tool names that mean the caller can change something. A request offering none of these is a
/// read-only agent. Covers both OpenCode ids (`bash`, `write`, `edit`, `apply_patch`, `todowrite`,
/// `task`) and the Claude Code spellings, matched case-insensitively.
const MUTATING_TOOL_NAMES: &[&str] = &[
    "apply_patch",
    "applypatch",
    "bash",
    "edit",
    "multiedit",
    "notebookedit",
    "patch",
    "run_command",
    "shell",
    "str_replace_editor",
    "task",
    "todowrite",
    "write",
];

impl RepromptEngine {
    pub fn from_config(config: &RepromptConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (prompt, prompt_file, initial_mtime) = if let Some(ref path) = config.prompt_file {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("reprompt: failed to read prompt_file '{}': {}", path, e))?;
            let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
            (text, Some(PathBuf::from(path)), mtime)
        } else if let Some(ref inline) = config.prompt {
            (inline.clone(), None, None)
        } else {
            return Err("reprompt: neither prompt_file nor prompt is configured".into());
        };

        if prompt.trim().is_empty() {
            return Err("reprompt: prompt text is empty".into());
        }

        Ok(Self {
            prompt: RwLock::new(prompt),
            prompt_file,
            last_mtime: RwLock::new(initial_mtime),
            dynamic_prompt: config.dynamic_prompt,
            max_retries: config.max_retries,
            done_sentinels: config.done_sentinels.clone(),
            log_stop_responses: config.log_stop_responses,
            skip_read_only_requests: config.skip_read_only_requests,
        })
    }

    /// If dynamic_prompt is enabled and the file has changed, reload it.
    /// Called once per trigger (before the retry loop). Returns the prompt text to use.
    async fn resolve_prompt(&self) -> String {
        if !self.dynamic_prompt {
            return self.prompt.read().await.clone();
        }

        let Some(ref path) = self.prompt_file else {
            // Inline prompt — no file to reload
            return self.prompt.read().await.clone();
        };

        let current_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());

        let needs_reload = {
            let last = self.last_mtime.read().await;
            match (*last, current_mtime) {
                (Some(last_t), Some(cur_t)) => cur_t != last_t,
                (None, Some(_)) => true, // first stat after startup without mtime
                _ => false,
            }
        };

        if needs_reload {
            match std::fs::read_to_string(path) {
                Ok(new_text) if !new_text.trim().is_empty() => {
                    tracing::info!(
                        path = %path.display(),
                        "Reprompt: prompt file changed, reloading"
                    );
                    *self.prompt.write().await = new_text.clone();
                    *self.last_mtime.write().await = current_mtime;
                    new_text
                }
                Ok(_) => {
                    tracing::warn!(
                        path = %path.display(),
                        "Reprompt: prompt file is empty after reload, keeping previous prompt"
                    );
                    self.prompt.read().await.clone()
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Reprompt: failed to reload prompt file, keeping previous prompt"
                    );
                    self.prompt.read().await.clone()
                }
            }
        } else {
            self.prompt.read().await.clone()
        }
    }

    /// Returns true when the response should trigger a reprompt:
    /// finish_reason == "stop" AND no tool_calls in choices[0].message.
    pub fn should_trigger(&self, response: &serde_json::Value) -> bool {
        let choices = match response.get("choices").and_then(|c| c.as_array()) {
            Some(c) if !c.is_empty() => c,
            _ => return false,
        };

        let first = &choices[0];

        let finish = first.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("");
        if finish != "stop" {
            return false;
        }

        let has_tool_calls = first
            .get("message")
            .and_then(|m| m.get("tool_calls"))
            .map(|tc| tc.is_array() && !tc.as_array().map(|a| a.is_empty()).unwrap_or(true))
            .unwrap_or(false);

        !has_tool_calls
    }

    /// Returns true when the request offers no tool that can change anything — the signature of a
    /// read-only subagent. Missing or malformed `tools` returns false so plain chat keeps the
    /// engine's original behaviour.
    pub fn request_is_read_only(request: &serde_json::Value) -> bool {
        let tools = match request.get("tools").and_then(|t| t.as_array()) {
            Some(t) if !t.is_empty() => t,
            _ => return false,
        };

        !tools.iter().any(|tool| {
            let name = tool
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| tool.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let name = name.to_ascii_lowercase();
            MUTATING_TOOL_NAMES.contains(&name.as_str())
        })
    }

    /// Pull the assistant text out of a response. Handles `content` as a plain string and as an
    /// array of parts, falling back to `reasoning_content` for backends that put everything there.
    fn extract_assistant_text(response: &serde_json::Value) -> String {
        let message = response
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|ch| ch.get("message"));

        let Some(message) = message else {
            return String::new();
        };

        let from_content = match message.get("content") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        };

        if !from_content.trim().is_empty() {
            return from_content;
        }

        message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning_text"))
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Overwrite `choices[0].message.content` with `text`, leaving everything else (tool_calls,
    /// finish_reason, usage) untouched. Returns false when the response has no message to write
    /// into — the caller must then fall back to a shape it can write, or the text is lost.
    #[must_use]
    fn set_assistant_text(response: &mut serde_json::Value, text: String) -> bool {
        let Some(message) = response
            .get_mut("choices")
            .and_then(|c| c.as_array_mut())
            .and_then(|c| c.first_mut())
            .and_then(|ch| ch.get_mut("message"))
        else {
            return false;
        };

        message["content"] = serde_json::Value::String(text);
        true
    }

    /// Returns true if the response has tool_calls or a non-stop finish_reason.
    fn has_continuation(response: &serde_json::Value) -> bool {
        let choices = match response.get("choices").and_then(|c| c.as_array()) {
            Some(c) if !c.is_empty() => c,
            _ => return false,
        };

        let first = &choices[0];

        let has_tools = first
            .get("message")
            .and_then(|m| m.get("tool_calls"))
            .map(|tc| tc.is_array() && !tc.as_array().map(|a| a.is_empty()).unwrap_or(true))
            .unwrap_or(false);

        if has_tools {
            return true;
        }

        first.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("stop") != "stop"
    }

    /// Build the follow-up request: original + assistant stop turn + continue-prompt user message.
    fn build_follow_up(
        prompt: &str,
        original_req: &serde_json::Value,
        stopped_resp: &serde_json::Value,
        backend: &BackendNode,
    ) -> serde_json::Value {
        let mut req = original_req.clone();

        req["stream"] = serde_json::Value::Bool(false);
        if let Some(obj) = req.as_object_mut() {
            obj.remove("stream_options");
        }

        if let Some(ref model) = backend.model {
            req["model"] = serde_json::Value::String(model.clone());
        }
        if let Some(temp) = backend.temperature {
            req["temperature"] = serde_json::Value::from(temp);
        }

        let assistant_content = Self::extract_assistant_text(stopped_resp);
        let assistant_msg = serde_json::json!({"role": "assistant", "content": assistant_content});
        let user_msg = serde_json::json!({"role": "user", "content": prompt});

        if let Some(msgs) = req.get_mut("messages").and_then(|m| m.as_array_mut()) {
            msgs.push(assistant_msg);
            msgs.push(user_msg);
        }

        req
    }

    async fn send_follow_up(
        req: &serde_json::Value,
        backend: &BackendNode,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/v1/chat/completions", backend.base_url());
        let mut builder = backend.http_client.post(&url).json(req);
        if let Some(ref key) = backend.api_key {
            builder = builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {}", key));
        }

        let resp = builder.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("reprompt backend returned {}: {}", status, body).into());
        }

        Ok(resp.json().await?)
    }

    pub async fn maybe_reprompt(
        &self,
        original_response: serde_json::Value,
        original_request: &serde_json::Value,
        backend: &Arc<BackendNode>,
    ) -> serde_json::Value {
        if !self.should_trigger(&original_response) {
            return original_response;
        }

        if self.skip_read_only_requests && Self::request_is_read_only(original_request) {
            tracing::debug!("Reprompt skipped: request exposes no mutating tools (read-only agent)");
            return original_response;
        }

        tracing::info!(
            max_retries = self.max_retries,
            "Reprompt triggered: finish_reason=stop with no tool_calls"
        );

        if self.log_stop_responses {
            tracing::info!(
                response = %serde_json::to_string_pretty(&original_response).unwrap_or_default(),
                "REPROMPT: triggering stop response"
            );
        }

        // Resolve prompt once per trigger (may reload from disk if changed)
        let prompt = self.resolve_prompt().await;

        let clean_stop = original_response.clone();
        // Every assistant text seen so far, oldest first. Whatever we hand back to the client
        // carries all of it — a follow-up turn adds to the stopped turn, it never replaces it.
        let mut collected: Vec<String> = Vec::new();
        Self::push_text(&mut collected, Self::extract_assistant_text(&original_response));
        let mut current = original_response;

        for attempt in 0..self.max_retries {
            tracing::debug!(attempt, "Sending reprompt follow-up");

            let follow_up_req = Self::build_follow_up(&prompt, original_request, &current, backend);

            let new_resp = match Self::send_follow_up(&follow_up_req, backend).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "Reprompt request failed, returning collected text");
                    return self.finish(clean_stop, collected, "request failed");
                }
            };

            let new_text = Self::extract_assistant_text(&new_resp);

            if let Some(matched) = self.done_sentinels.iter().find(|s| new_text.contains(s.as_str())) {
                tracing::info!(attempt, sentinel = %matched, "Reprompt: done sentinel found, returning original stop");
                // The sentinel turn is bookkeeping, not content — drop it and keep what we had.
                return self.finish(clean_stop, collected, "done sentinel");
            }

            if Self::has_continuation(&new_resp) {
                tracing::info!(attempt, "Reprompt: continuation found, returning new response");
                Self::push_text(&mut collected, new_text);
                let mut merged = new_resp;
                if !Self::set_assistant_text(&mut merged, collected.join("\n\n")) {
                    // Nowhere to put the text in the continuation — keep the stop turn instead
                    // of trading a real answer for a malformed one.
                    tracing::warn!("Reprompt: continuation has no message object, returning collected text");
                    return self.finish(clean_stop, collected, "continuation unwritable");
                }
                if self.log_stop_responses {
                    tracing::info!(
                        response = %serde_json::to_string_pretty(&merged).unwrap_or_default(),
                        "REPROMPT: client response (continuation)"
                    );
                }
                return merged;
            }

            tracing::debug!(
                attempt,
                "Reprompt: follow-up also stopped without tool_calls, continuing loop"
            );
            Self::push_text(&mut collected, new_text);
            current = new_resp;
        }

        tracing::warn!(
            max_retries = self.max_retries,
            "Reprompt: exhausted retries, returning collected text"
        );
        self.finish(clean_stop, collected, "exhausted retries")
    }

    fn push_text(collected: &mut Vec<String>, text: String) {
        if !text.trim().is_empty() {
            collected.push(text);
        }
    }

    /// Return the original stop turn carrying every assistant text collected along the way.
    fn finish(&self, mut clean_stop: serde_json::Value, collected: Vec<String>, reason: &str) -> serde_json::Value {
        if !collected.is_empty() {
            let _ = Self::set_assistant_text(&mut clean_stop, collected.join("\n\n"));
        }
        if self.log_stop_responses {
            tracing::info!(
                reason,
                response = %serde_json::to_string_pretty(&clean_stop).unwrap_or_default(),
                "REPROMPT: client response"
            );
        }
        clean_stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn engine() -> RepromptEngine {
        RepromptEngine {
            prompt: RwLock::new("Continue or say DONE.".into()),
            prompt_file: None,
            last_mtime: RwLock::new(None),
            dynamic_prompt: false,
            max_retries: 3,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        }
    }

    fn stop_resp(content: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": content, "tool_calls": null}
            }]
        })
    }

    fn tool_call_resp() -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]
                }
            }]
        })
    }

    fn test_node() -> BackendNode {
        BackendNode {
            url: "http://localhost:8080".into(),
            model: None,
            api_key: None,
            timeout_seconds: 300,
            http_client: reqwest::Client::new(),
            active_requests: AtomicUsize::new(0),
            strip_path_prefix: None,
            temperature: None,
        }
    }

    #[test]
    fn test_should_trigger_stop_no_tools() {
        assert!(engine().should_trigger(&stop_resp("partial work done")));
    }

    #[test]
    fn test_should_trigger_false_tool_calls() {
        assert!(!engine().should_trigger(&tool_call_resp()));
    }

    #[test]
    fn test_should_trigger_false_length() {
        let r = serde_json::json!({"choices": [{"finish_reason": "length", "message": {"content": "x"}}]});
        assert!(!engine().should_trigger(&r));
    }

    #[test]
    fn test_should_trigger_false_empty_choices() {
        let r = serde_json::json!({"choices": []});
        assert!(!engine().should_trigger(&r));
    }

    #[test]
    fn test_should_trigger_false_empty_tool_calls_array() {
        let r = serde_json::json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "x", "tool_calls": []}
            }]
        });
        assert!(engine().should_trigger(&r));
    }

    #[test]
    fn test_has_continuation_tool_calls() {
        assert!(RepromptEngine::has_continuation(&tool_call_resp()));
    }

    #[test]
    fn test_has_continuation_non_stop() {
        let r = serde_json::json!({"choices": [{"finish_reason": "length", "message": {"content": "x"}}]});
        assert!(RepromptEngine::has_continuation(&r));
    }

    #[test]
    fn test_has_continuation_stop_no_tools() {
        assert!(!RepromptEngine::has_continuation(&stop_resp("text")));
    }

    #[test]
    fn test_build_follow_up_appends_messages() {
        let node = test_node();
        let req = serde_json::json!({
            "model": "test",
            "messages": [{"role": "user", "content": "Do X"}]
        });
        let result = RepromptEngine::build_follow_up("Continue or say DONE.", &req, &stop_resp("I did half."), &node);
        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "I did half.");
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "Continue or say DONE.");
        assert_eq!(result["stream"], false);
        assert!(result.get("stream_options").is_none());
    }

    #[test]
    fn test_build_follow_up_strips_stream_options() {
        let node = test_node();
        let req = serde_json::json!({
            "model": "test",
            "stream_options": {"include_usage": true},
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let result = RepromptEngine::build_follow_up("Continue.", &req, &stop_resp("ok"), &node);
        assert!(result.get("stream_options").is_none());
    }

    #[test]
    fn test_build_follow_up_applies_model_override() {
        let mut node = test_node();
        node.model = Some("override-model".into());
        let req = serde_json::json!({
            "model": "original",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let result = RepromptEngine::build_follow_up("Continue.", &req, &stop_resp("ok"), &node);
        assert_eq!(result["model"], "override-model");
    }

    #[test]
    fn test_from_config_inline() {
        let cfg = RepromptConfig {
            enabled: true,
            prompt_file: None,
            prompt: Some("Continue or DONE.".into()),
            max_retries: 2,
            done_sentinels: vec!["DONE".into()],
            dynamic_prompt: false,
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        let e = RepromptEngine::from_config(&cfg).unwrap();
        assert_eq!(e.max_retries, 2);
    }

    #[test]
    fn test_from_config_no_prompt_errors() {
        let cfg = RepromptConfig {
            enabled: true,
            prompt_file: None,
            prompt: None,
            max_retries: 3,
            done_sentinels: vec!["DONE".into()],
            dynamic_prompt: false,
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        assert!(RepromptEngine::from_config(&cfg).is_err());
    }

    #[test]
    fn test_from_config_empty_prompt_errors() {
        let cfg = RepromptConfig {
            enabled: true,
            prompt_file: None,
            prompt: Some("   ".into()),
            max_retries: 3,
            done_sentinels: vec!["DONE".into()],
            dynamic_prompt: false,
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        assert!(RepromptEngine::from_config(&cfg).is_err());
    }

    #[test]
    fn test_extract_assistant_text() {
        let r = stop_resp("hello world");
        assert_eq!(RepromptEngine::extract_assistant_text(&r), "hello world");
    }

    #[test]
    fn test_extract_assistant_text_missing_content() {
        let r = serde_json::json!({"choices": [{"message": {"role": "assistant"}}]});
        assert_eq!(RepromptEngine::extract_assistant_text(&r), "");
    }

    #[tokio::test]
    async fn test_resolve_prompt_static() {
        let e = engine(); // dynamic_prompt: false, no file
        assert_eq!(e.resolve_prompt().await, "Continue or say DONE.");
    }

    #[tokio::test]
    async fn test_resolve_prompt_dynamic_no_file() {
        // dynamic_prompt: true but no prompt_file → just reads from RwLock
        let e = RepromptEngine {
            prompt: RwLock::new("Static text.".into()),
            prompt_file: None,
            last_mtime: RwLock::new(None),
            dynamic_prompt: true,
            max_retries: 3,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        assert_eq!(e.resolve_prompt().await, "Static text.");
    }

    #[tokio::test]
    async fn test_resolve_prompt_dynamic_file_unchanged() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "File prompt.").unwrap();
        let path = f.path().to_path_buf();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

        let e = RepromptEngine {
            prompt: RwLock::new("File prompt.".into()),
            prompt_file: Some(path),
            last_mtime: RwLock::new(Some(mtime)),
            dynamic_prompt: true,
            max_retries: 3,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        // mtime hasn't changed, should return cached prompt without re-reading
        assert_eq!(e.resolve_prompt().await, "File prompt.");
    }

    #[tokio::test]
    async fn test_resolve_prompt_dynamic_file_changed() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "Old prompt.").unwrap();
        let path = f.path().to_path_buf();

        // Write new content and advance mtime
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut f2 = std::fs::OpenOptions::new().write(true).truncate(true).open(&path).unwrap();
        write!(f2, "New prompt.").unwrap();
        drop(f2);

        let old_mtime = std::time::UNIX_EPOCH; // clearly older than real file

        let e = RepromptEngine {
            prompt: RwLock::new("Old prompt.".into()),
            prompt_file: Some(path),
            last_mtime: RwLock::new(Some(old_mtime)),
            dynamic_prompt: true,
            max_retries: 3,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        let result = e.resolve_prompt().await;
        assert_eq!(result, "New prompt.");
        // Verify internal state updated
        assert_eq!(*e.prompt.read().await, "New prompt.");
    }

    // --- read-only skip gate ---

    fn req_with_tools(names: &[&str]) -> serde_json::Value {
        let tools: Vec<serde_json::Value> = names
            .iter()
            .map(|n| serde_json::json!({"type": "function", "function": {"name": n}}))
            .collect();
        serde_json::json!({
            "model": "test",
            "messages": [{"role": "user", "content": "Review this"}],
            "tools": tools
        })
    }

    #[test]
    fn test_request_is_read_only_reviewer_tools() {
        // neckbeard / hoodie expose exactly these
        assert!(RepromptEngine::request_is_read_only(&req_with_tools(&[
            "read", "grep", "glob"
        ])));
    }

    #[test]
    fn test_request_is_read_only_false_when_can_write() {
        assert!(!RepromptEngine::request_is_read_only(&req_with_tools(&[
            "read", "grep", "glob", "write", "edit", "bash"
        ])));
    }

    #[test]
    fn test_request_is_read_only_false_for_single_mutating_tool() {
        assert!(!RepromptEngine::request_is_read_only(&req_with_tools(&["read", "todowrite"])));
    }

    #[test]
    fn test_request_is_read_only_matches_case_insensitively() {
        // Claude Code spells them capitalised
        assert!(!RepromptEngine::request_is_read_only(&req_with_tools(&["Read", "Bash"])));
    }

    #[test]
    fn test_request_is_read_only_false_without_tools() {
        // Plain chat keeps the engine's original behaviour
        let req = serde_json::json!({"model": "test", "messages": []});
        assert!(!RepromptEngine::request_is_read_only(&req));
        let req = serde_json::json!({"model": "test", "tools": []});
        assert!(!RepromptEngine::request_is_read_only(&req));
    }

    #[tokio::test]
    async fn test_maybe_reprompt_skips_read_only_request() {
        // No backend is running — if the gate failed to short-circuit, send_follow_up would
        // error out and we'd still get a response back, so assert on identity instead.
        let e = engine();
        let node = Arc::new(test_node());
        let original = stop_resp("**Issues**: none. Looks good.");
        let result = e
            .maybe_reprompt(original.clone(), &req_with_tools(&["read", "grep", "glob"]), &node)
            .await;
        assert_eq!(result, original);
    }

    // --- text extraction ---

    #[test]
    fn test_extract_assistant_text_array_parts() {
        let r = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": [
                {"type": "text", "text": "part one "},
                {"type": "text", "text": "part two"}
            ]}}]
        });
        assert_eq!(RepromptEngine::extract_assistant_text(&r), "part one part two");
    }

    #[test]
    fn test_extract_assistant_text_falls_back_to_reasoning() {
        let r = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "", "reasoning_content": "thinking out loud"}}]
        });
        assert_eq!(RepromptEngine::extract_assistant_text(&r), "thinking out loud");
    }

    #[test]
    fn test_extract_assistant_text_prefers_content_over_reasoning() {
        let r = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "the answer", "reasoning_content": "thinking"}}]
        });
        assert_eq!(RepromptEngine::extract_assistant_text(&r), "the answer");
    }

    #[test]
    fn test_set_assistant_text_preserves_tool_calls() {
        let mut r = tool_call_resp();
        assert!(RepromptEngine::set_assistant_text(&mut r, "carried forward".into()));
        assert_eq!(r["choices"][0]["message"]["content"], "carried forward");
        assert_eq!(r["choices"][0]["message"]["tool_calls"][0]["id"], "c1");
        assert_eq!(r["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn test_set_assistant_text_reports_failure_without_message() {
        let mut r = serde_json::json!({"choices": [{"finish_reason": "stop"}]});
        assert!(!RepromptEngine::set_assistant_text(&mut r, "text".into()));
    }

    #[tokio::test]
    async fn test_unwritable_continuation_keeps_original_text() {
        // finish_reason=length with no message object → a continuation we cannot write into
        let odd = serde_json::json!({"choices": [{"finish_reason": "length"}]});
        let url = spawn_backend(vec![odd]).await;
        let e = write_capable_engine(2);
        let result = e
            .maybe_reprompt(
                stop_resp("the review that must survive"),
                &req_with_tools(&["read", "write"]),
                &node_at(url).await,
            )
            .await;
        assert_eq!(result["choices"][0]["message"]["content"], "the review that must survive");
    }

    // --- never lose text ---

    /// Serve a fixed queue of JSON bodies on 127.0.0.1, one per POST.
    async fn spawn_backend(responses: Vec<serde_json::Value>) -> String {
        use axum::{routing::post, Json, Router};
        use std::sync::Mutex;

        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(responses)));
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let queue = queue.clone();
                async move {
                    let next = queue.lock().unwrap().pop_front();
                    Json(next.unwrap_or_else(|| serde_json::json!({"error": "queue exhausted"})))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{}", addr)
    }

    fn write_capable_engine(max_retries: u32) -> RepromptEngine {
        RepromptEngine {
            prompt: RwLock::new("Continue or say DONE.".into()),
            prompt_file: None,
            last_mtime: RwLock::new(None),
            dynamic_prompt: false,
            max_retries,
            done_sentinels: vec!["DONE_NO_MORE_PROXY_REPROMPT".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        }
    }

    async fn node_at(url: String) -> Arc<BackendNode> {
        let mut node = test_node();
        node.url = url;
        Arc::new(node)
    }

    #[tokio::test]
    async fn test_continuation_carries_original_text_forward() {
        // The regression this whole change exists for: a finished answer followed by a tool call
        // used to be replaced by the tool call, leaving the client with no text at all.
        let url = spawn_backend(vec![tool_call_resp()]).await;
        let e = write_capable_engine(2);
        let result = e
            .maybe_reprompt(
                stop_resp("**Issues**: [HIGH] null deref at foo.rs:12"),
                &req_with_tools(&["read", "write"]),
                &node_at(url).await,
            )
            .await;

        let content = result["choices"][0]["message"]["content"].as_str().unwrap();
        assert!(content.contains("[HIGH] null deref at foo.rs:12"), "got: {content}");
        assert_eq!(result["choices"][0]["message"]["tool_calls"][0]["id"], "c1");
    }

    #[tokio::test]
    async fn test_exhausted_retries_keeps_every_turn() {
        let url = spawn_backend(vec![stop_resp("and one more thing"), stop_resp("plus this")]).await;
        let e = write_capable_engine(2);
        let result = e
            .maybe_reprompt(
                stop_resp("first half of the review"),
                &req_with_tools(&["read", "write"]),
                &node_at(url).await,
            )
            .await;

        let content = result["choices"][0]["message"]["content"].as_str().unwrap();
        assert!(content.contains("first half of the review"), "got: {content}");
        assert!(content.contains("and one more thing"), "got: {content}");
        assert!(content.contains("plus this"), "got: {content}");
    }

    #[tokio::test]
    async fn test_done_sentinel_returns_original_without_sentinel() {
        let url = spawn_backend(vec![stop_resp("DONE_NO_MORE_PROXY_REPROMPT")]).await;
        let e = write_capable_engine(2);
        let original = stop_resp("the complete review");
        let result = e
            .maybe_reprompt(original, &req_with_tools(&["read", "write"]), &node_at(url).await)
            .await;

        let content = result["choices"][0]["message"]["content"].as_str().unwrap();
        assert_eq!(content, "the complete review");
        assert!(!content.contains("DONE_NO_MORE_PROXY_REPROMPT"));
    }

    #[tokio::test]
    async fn test_backend_failure_keeps_original_text() {
        // Nothing listening on this port → send_follow_up errors on the first attempt.
        let e = write_capable_engine(2);
        let node = node_at("http://127.0.0.1:1".into()).await;
        let result = e
            .maybe_reprompt(stop_resp("the only answer"), &req_with_tools(&["read", "write"]), &node)
            .await;
        assert_eq!(result["choices"][0]["message"]["content"], "the only answer");
    }
}
