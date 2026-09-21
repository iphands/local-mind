//! Reprompt engine — silently re-prompts the backend when it returns a premature stop.
//!
//! When finish_reason="stop" with no tool_calls, the engine injects a follow-up
//! user message (loaded from config) and re-sends the request to the backend.
//! - If the response has tool_calls or a non-stop finish_reason → return it, with every
//!   assistant text seen so far merged into its content — even when its text also mentions
//!   a done sentinel: a continuation outranks the sentinel (big-fix 65).
//! - Otherwise, if the response contains any done_sentinel → close the loop on the stopped
//!   turn, keeping the sentinel turn's text in the answer, sentinel intact.
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
//! to the prompt without restarting the proxy. The mtime-check/async-read mechanics
//! live in the shared [`crate::prompt_cache::PromptFileCache`], not in this module.

use crate::backends::BackendNode;
use crate::config::RepromptConfig;
use crate::prompt_cache::{self, Refresh};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RepromptEngine {
    /// Current prompt text — guarded for dynamic reload. The file's mtime is
    /// deliberately NOT a second cell here: it is owned by the shared
    /// [`prompt_cache::PromptFileCache`] keyed by `prompt_file`, so no reader can
    /// ever pair text from one reload with the mtime of another (big-fix 63).
    prompt: RwLock<String>,
    /// Path to reload from (None when using inline prompt)
    prompt_file: Option<PathBuf>,
    /// Re-read the file on each trigger if mtime changed (default: true)
    dynamic_prompt: bool,
    pub max_retries: u32,
    /// Wall-clock budget for the whole loop; see RepromptConfig::max_total_ms
    max_total_ms: u64,
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
        let (prompt, prompt_file) = if let Some(ref path) = config.prompt_file {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("reprompt: failed to read prompt_file '{}': {}", path, e))?;
            (text, Some(PathBuf::from(path)))
        } else if let Some(ref inline) = config.prompt {
            (inline.clone(), None)
        } else {
            return Err("reprompt: neither prompt_file nor prompt is configured".into());
        };

        if prompt.trim().is_empty() {
            return Err("reprompt: prompt text is empty".into());
        }

        Ok(Self {
            prompt: RwLock::new(prompt),
            prompt_file,
            dynamic_prompt: config.dynamic_prompt,
            max_retries: config.max_retries,
            max_total_ms: config.max_total_ms,
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

        let cache = prompt_cache::shared(path).await;
        match cache.refresh(path).await {
            Refresh::Reloaded { text, mtime } => {
                tracing::info!(
                    path = %path.display(),
                    mtime = ?mtime,
                    "Reprompt: prompt file changed, reloading"
                );
                *self.prompt.write().await = text.clone();
                text
            }
            Refresh::Unchanged => self.prompt.read().await.clone(),
            Refresh::Empty => {
                tracing::warn!(
                    path = %path.display(),
                    "Reprompt: prompt file is empty after reload, keeping previous prompt"
                );
                self.prompt.read().await.clone()
            }
            Refresh::Failed(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Reprompt: failed to reload prompt file, keeping previous prompt"
                );
                self.prompt.read().await.clone()
            }
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
    /// finish_reason, usage) untouched. Returns false when there is no object-or-null message
    /// to write into — serde_json's IndexMut panics on scalar/array messages, so a refusal
    /// must precede the assignment. The caller falls back to `force_assistant_text`.
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

        if !message.is_object() && !message.is_null() {
            return false;
        }
        message["content"] = serde_json::Value::String(text);
        true
    }

    /// Install `{"role":"assistant","content":text}` as choices[0].message no matter what the
    /// body looked like: replaces a missing or non-object message, grows an empty choices
    /// array, or gives the body a choices array. Last-resort write path behind
    /// `set_assistant_text`; only finish() and the merge fallback reach it, and a non-object
    /// message at this point holds no fields worth preserving.
    fn force_assistant_text(response: &mut serde_json::Value, text: String) {
        if !response.is_object() {
            *response = serde_json::Value::Object(serde_json::Map::new());
        }
        let has_choice = response
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|choices| !choices.is_empty());
        if has_choice {
            response["choices"][0]["message"] = serde_json::json!({"role": "assistant", "content": text});
        } else {
            response["choices"] =
                serde_json::json!([{"finish_reason": "stop", "message": {"role": "assistant", "content": text}}]);
        }
    }

    fn has_assistant_message(response: &serde_json::Value) -> bool {
        response
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .is_some_and(serde_json::Value::is_object)
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
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(self.max_total_ms);

        for attempt in 0..self.max_retries {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::debug!(
                    attempt,
                    max_total_ms = self.max_total_ms,
                    "Reprompt time budget exhausted before round, returning collected text"
                );
                return self.finish(clean_stop, collected, "time budget");
            }

            tracing::debug!(attempt, "Sending reprompt follow-up");

            let follow_up_req = Self::build_follow_up(&prompt, original_request, &current, backend);

            let new_resp = match tokio::time::timeout(remaining, Self::send_follow_up(&follow_up_req, backend)).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    tracing::warn!(attempt, error = %e, "Reprompt request failed, returning collected text");
                    return self.finish(clean_stop, collected, "request failed");
                }
                Err(_elapsed) => {
                    tracing::debug!(
                        attempt,
                        max_total_ms = self.max_total_ms,
                        "Reprompt follow-up exceeded the time budget, returning collected text"
                    );
                    return self.finish(clean_stop, collected, "time budget");
                }
            };

            let new_text = Self::extract_assistant_text(&new_resp);

            // Continuation outranks the sentinel (big-fix 65): a follow-up carrying
            // tool_calls or a non-stop finish must survive its text mentioning a sentinel.
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

            if let Some(matched) = self.done_sentinels.iter().find(|s| new_text.contains(s.as_str())) {
                tracing::info!(attempt, sentinel = %matched, "Reprompt: done sentinel without continuation, closing the loop");
                // The sentinel turn stays in the returned text, sentinel intact: it can carry
                // the model's closing summary, and dropping it is the consumed-then-lost bug.
                Self::push_text(&mut collected, new_text);
                return self.finish(clean_stop, collected, "done sentinel");
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
    /// The returned body always has an object message: text is never dropped for want of a
    /// writable shape, and a message-less choice is never handed to a client.
    fn finish(&self, mut clean_stop: serde_json::Value, collected: Vec<String>, reason: &str) -> serde_json::Value {
        let text = collected.join("\n\n");
        if !text.is_empty() {
            if !Self::set_assistant_text(&mut clean_stop, text.clone()) {
                tracing::debug!(
                    reason,
                    "Reprompt: stop body had no writable message, forcing an assistant message"
                );
                Self::force_assistant_text(&mut clean_stop, text);
            }
        } else if !Self::has_assistant_message(&clean_stop) {
            Self::force_assistant_text(&mut clean_stop, String::new());
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
            dynamic_prompt: false,
            max_retries: 3,
            max_total_ms: 30_000,
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
            active_requests: Arc::new(AtomicUsize::new(0)),
            strip_path_prefix: None,
            temperature: None,
            healthy: std::sync::atomic::AtomicBool::new(true),
            cooldown_until: std::sync::Mutex::new(std::time::Instant::now()),
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
            max_total_ms: 30_000,
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
            max_total_ms: 30_000,
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
            max_total_ms: 30_000,
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
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
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

        let e = RepromptEngine {
            prompt: RwLock::new("File prompt.".into()),
            prompt_file: Some(path),
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        // First resolve cold-loads the shared cache; the prompt text is unchanged.
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

        let e = RepromptEngine {
            prompt: RwLock::new("Old prompt.".into()),
            prompt_file: Some(path),
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
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
            dynamic_prompt: false,
            max_retries,
            max_total_ms: 30_000,
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
    async fn test_done_sentinel_keeps_turn_with_sentinel() {
        // Retargeted (big-fix 65): the old pin (test_done_sentinel_returns_original_without_
        // sentinel) asserted the sentinel turn was dropped from the answer - exactly the
        // consumed-then-lost behavior plan 65 condemns. New contract: the non-continuation
        // sentinel response keeps its text, sentinel intact.
        let url = spawn_backend(vec![stop_resp("DONE_NO_MORE_PROXY_REPROMPT")]).await;
        let e = write_capable_engine(2);
        let original = stop_resp("the complete review");
        let result = e
            .maybe_reprompt(original, &req_with_tools(&["read", "write"]), &node_at(url).await)
            .await;

        let content = result["choices"][0]["message"]["content"].as_str().unwrap();
        assert_eq!(content, "the complete review\n\nDONE_NO_MORE_PROXY_REPROMPT");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn test_continuation_with_sentinel_still_merges() {
        let body = serde_json::json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":"DONE_NO_MORE_PROXY_REPROMPT then I edit","tool_calls":[{"id":"c9","type":"function","function":{"name":"write","arguments":"{}"}}]}}]});
        let url = spawn_backend(vec![body]).await;
        let e = write_capable_engine(2);
        let result = e
            .maybe_reprompt(stop_resp("half"), &req_with_tools(&["read", "write"]), &node_at(url).await)
            .await;
        assert_eq!(
            result["choices"][0]["finish_reason"], "tool_calls",
            "continuation outranks the sentinel"
        );
        assert_eq!(result["choices"][0]["message"]["tool_calls"][0]["id"], "c9");
        let content = result["choices"][0]["message"]["content"].as_str().unwrap();
        assert!(
            content.contains("half") && content.contains("DONE_NO_MORE_PROXY_REPROMPT"),
            "got: {content}"
        );
    }

    #[tokio::test]
    async fn test_sentinel_mid_text_keeps_whole_turn() {
        let url = spawn_backend(vec![stop_resp(
            "closing remarks: DONE_NO_MORE_PROXY_REPROMPT — nothing further.",
        )])
        .await;
        let e = write_capable_engine(2);
        let result = e
            .maybe_reprompt(
                stop_resp("first half"),
                &req_with_tools(&["read", "write"]),
                &node_at(url).await,
            )
            .await;
        let content = result["choices"][0]["message"]["content"].as_str().unwrap();
        assert_eq!(
            content,
            "first half\n\nclosing remarks: DONE_NO_MORE_PROXY_REPROMPT — nothing further."
        );
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

    // --- task 47 pins: reload arms that had no coverage before the shared
    //     PromptFileCache extraction (green on baseline too - they guard the
    //     rewiring, not new behavior) ---

    #[tokio::test]
    async fn test_resolve_prompt_dynamic_empty_file_keeps_previous_prompt() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        write!(f, "   \n  ").unwrap();
        let path = f.path().to_path_buf();

        let e = RepromptEngine {
            prompt: RwLock::new("Old prompt.".into()),
            prompt_file: Some(path),
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };

        assert_eq!(e.resolve_prompt().await, "Old prompt.");
        assert_eq!(
            *e.prompt.read().await,
            "Old prompt.",
            "whitespace-only reload must not replace the prompt"
        );
    }

    #[tokio::test]
    async fn test_resolve_prompt_dynamic_missing_file_keeps_previous_prompt() {
        let e = RepromptEngine {
            prompt: RwLock::new("Old prompt.".into()),
            prompt_file: Some(std::env::temp_dir().join("task47-no-such-reprompt-prompt-4c1d.md")),
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };

        assert_eq!(e.resolve_prompt().await, "Old prompt.");
        assert_eq!(
            *e.prompt.read().await,
            "Old prompt.",
            "unreadable file must not replace the prompt"
        );
    }

    // --- task 63 pins: the engine reads through the process-wide registry ---

    #[tokio::test]
    async fn test_resolve_prompt_uses_shared_registry() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        write!(f, "ON-DISK").unwrap();
        let path = f.path().to_path_buf();

        crate::prompt_cache::shared(&path).await.refresh(&path).await;

        let e = RepromptEngine {
            prompt: RwLock::new("ENGINE-BOOT-TEXT".into()),
            prompt_file: Some(path),
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        };
        assert_eq!(
            e.resolve_prompt().await,
            "ENGINE-BOOT-TEXT",
            "a file the shared cache already validated as Unchanged must not be re-read per engine"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_concurrent_resolves_never_tear_and_converge() {
        // Eventual-consistency contract: concurrent triggers each write ONE atomic
        // prompt cell, so every observable value is a whole file text (never a mix),
        // and a trigger after the last file write converges on it. Mid-flight, the
        // loser of a last-writer race can linger until the next refresh re-reads -
        // the shared cache's mtime check heals that, so the final resolve is the
        // convergence point, not any single concurrent return.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.md");
        std::fs::write(&path, "V1").unwrap();

        let e = std::sync::Arc::new(RepromptEngine {
            prompt: RwLock::new("V1".into()),
            prompt_file: Some(path.clone()),
            dynamic_prompt: true,
            max_retries: 3,
            max_total_ms: 30_000,
            done_sentinels: vec!["DONE".into()],
            log_stop_responses: false,
            skip_read_only_requests: true,
        });

        let mut tasks = Vec::new();
        for _ in 0..4 {
            let e = std::sync::Arc::clone(&e);
            tasks.push(tokio::spawn(async move { e.resolve_prompt().await }));
        }
        std::fs::write(&path, "V2").unwrap();
        let mut seen = Vec::new();
        for t in tasks {
            seen.push(t.await.unwrap());
        }
        assert!(
            seen.iter().all(|s| s == "V1" || s == "V2"),
            "every concurrent resolve must return a whole file text, got {seen:?}"
        );

        std::fs::write(&path, "V3").unwrap();
        assert_eq!(e.resolve_prompt().await, "V3", "the trigger after the last write converges");
    }

    // --- task 64: wall-clock budget on the reprompt loop [C-M3] ---

    #[tokio::test]
    async fn test_budget_zero_skips_every_round() {
        // Deterministic: the round check fires before any POST, so the serving
        // backend's queued continuation is never requested, never merged.
        let url = spawn_backend(vec![tool_call_resp()]).await;
        let mut e = write_capable_engine(2);
        e.max_total_ms = 0;
        let result = e
            .maybe_reprompt(
                stop_resp("answer before the budget"),
                &req_with_tools(&["read", "write"]),
                &node_at(url).await,
            )
            .await;
        assert_eq!(
            result["choices"][0]["finish_reason"], "stop",
            "the queued tool_call must not be fetched"
        );
        assert_eq!(result["choices"][0]["message"]["content"], "answer before the budget");
    }

    #[tokio::test]
    async fn test_stalled_backend_returns_within_budget() {
        // Backend sleeps 5000ms; budget 200ms. The timeout fires at ~200ms; the
        // wall assertion (3000ms) keeps 15x headroom over the budget and stays
        // 1.67x under the stall, separating budgeted from unbounded by 66x/1.67x.
        use axum::{routing::post, Json, Router};
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
                Json(stop_resp("too late"))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut e = write_capable_engine(3);
        e.max_total_ms = 200;
        let mut node = test_node();
        node.url = format!("http://{addr}");

        let start = std::time::Instant::now();
        let result = e
            .maybe_reprompt(
                stop_resp("the answer that must arrive fast"),
                &req_with_tools(&["read", "write"]),
                &std::sync::Arc::new(node),
            )
            .await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(3000),
            "budget ignored: {elapsed:?}"
        );
        assert_eq!(result["choices"][0]["message"]["content"], "the answer that must arrive fast");
    }

    // --- task 66: finish() guarantees a message [C-M2] ---

    #[tokio::test]
    async fn test_finish_grows_message_when_absent() {
        let url = spawn_backend(vec![stop_resp("follow-up text"); 5]).await;
        let e = write_capable_engine(2);
        let original = serde_json::json!({"choices":[{"finish_reason":"stop"}]});
        let r = e
            .maybe_reprompt(original, &req_with_tools(&["read", "write"]), &node_at(url).await)
            .await;
        assert_eq!(r["choices"][0]["message"]["role"], "assistant");
        assert_eq!(
            r["choices"][0]["message"]["content"], "follow-up text\n\nfollow-up text",
            "both retry rounds survive into the forced message"
        );
        assert_eq!(r["choices"][0]["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn test_finish_forces_assistant_message_on_string_message() {
        let url = spawn_backend(vec![stop_resp("follow-up text"); 5]).await;
        let e = write_capable_engine(2);
        let original = serde_json::json!({"choices":[{"finish_reason":"stop","message":"raw string not object"}]});
        let r = e
            .maybe_reprompt(original, &req_with_tools(&["read", "write"]), &node_at(url).await)
            .await;
        assert_eq!(
            r["choices"][0]["message"]["role"], "assistant",
            "scalar message forced to a proper message, no panic"
        );
        assert_eq!(
            r["choices"][0]["message"]["content"], "follow-up text\n\nfollow-up text",
            "both retry rounds survive into the forced message"
        );
    }

    #[test]
    fn test_finish_grows_choice_when_choices_empty() {
        // finish() is called only behind should_trigger, so an empty-choices body must be
        // proven total at the finish() seam itself: it can never hand back a text-less husk.
        let e = engine();
        let r = e.finish(serde_json::json!({"choices": []}), vec!["orphan text".into()], "unit");
        assert_eq!(r["choices"][0]["message"]["content"], "orphan text");
        assert_eq!(r["choices"][0]["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn test_body_without_stop_choice_passes_through_untouched() {
        let e = write_capable_engine(2);
        let original = serde_json::json!({"choices": []});
        let r = e
            .maybe_reprompt(
                original.clone(),
                &req_with_tools(&["read", "write"]),
                &std::sync::Arc::new(test_node()),
            )
            .await;
        assert_eq!(r, original, "no stop choice means no reprompt and no mutation");
    }
}
