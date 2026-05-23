//! Reprompt engine — silently re-prompts the backend when it returns a premature stop.
//!
//! When finish_reason="stop" with no tool_calls, the engine injects a follow-up
//! user message (loaded from config) and re-sends the request to the backend.
//! - If the response contains any done_sentinel → return the original clean stop.
//! - If the response has tool_calls or a non-stop finish_reason → return it (hiding the stop).
//! - After max_retries exhaustion → return the last response seen.
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
}

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

    fn extract_assistant_text(response: &serde_json::Value) -> String {
        response
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|ch| ch.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string()
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
        let mut current = original_response;

        for attempt in 0..self.max_retries {
            tracing::debug!(attempt, "Sending reprompt follow-up");

            let follow_up_req = Self::build_follow_up(&prompt, original_request, &current, backend);

            let new_resp = match Self::send_follow_up(&follow_up_req, backend).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "Reprompt request failed, returning last response");
                    if self.log_stop_responses {
                        tracing::info!(
                            response = %serde_json::to_string_pretty(&current).unwrap_or_default(),
                            "REPROMPT: client response (request failed)"
                        );
                    }
                    return current;
                }
            };

            let new_text = Self::extract_assistant_text(&new_resp);

            if let Some(matched) = self.done_sentinels.iter().find(|s| new_text.contains(s.as_str())) {
                tracing::info!(attempt, sentinel = %matched, "Reprompt: done sentinel found, returning original stop");
                if self.log_stop_responses {
                    tracing::info!(
                        response = %serde_json::to_string_pretty(&clean_stop).unwrap_or_default(),
                        "REPROMPT: client response (done sentinel)"
                    );
                }
                return clean_stop;
            }

            if Self::has_continuation(&new_resp) {
                tracing::info!(attempt, "Reprompt: continuation found, returning new response");
                if self.log_stop_responses {
                    tracing::info!(
                        response = %serde_json::to_string_pretty(&new_resp).unwrap_or_default(),
                        "REPROMPT: client response (continuation)"
                    );
                }
                return new_resp;
            }

            tracing::debug!(
                attempt,
                "Reprompt: follow-up also stopped without tool_calls, continuing loop"
            );
            current = new_resp;
        }

        tracing::warn!(
            max_retries = self.max_retries,
            "Reprompt: exhausted retries, returning last response"
        );
        if self.log_stop_responses {
            tracing::info!(
                response = %serde_json::to_string_pretty(&current).unwrap_or_default(),
                "REPROMPT: client response (exhausted retries)"
            );
        }
        current
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
        };
        let result = e.resolve_prompt().await;
        assert_eq!(result, "New prompt.");
        // Verify internal state updated
        assert_eq!(*e.prompt.read().await, "New prompt.");
    }
}
