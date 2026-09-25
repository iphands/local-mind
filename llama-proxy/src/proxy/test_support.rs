//! Scripted mock HTTP backend shared by the proxy's `#[cfg(test)]` suites.
//!
//! `scripted_backend` binds `127.0.0.1:0`, answers each connection with the next queued
//! response for the **exact request-target** the client asked for, and records what it was
//! asked for. Dispatch is path-keyed, never arrival-ordered: a caller that probes `/props`
//! first and `/v1/models` second must be able to script both without predicting which one
//! the code under test tries first, and a request for an unscripted path must explode
//! loudly instead of quietly draining another path's queue (the desync class that makes a
//! shared mock backend un-debuggable).
//!
//! **Route values are complete HTTP/1.1 response frames, not bodies** — that is how status
//! codes (404s, 500s) are scripted. Build them with `json_response` / `scripted_response`;
//! `with_connection_close` still force-adds the header to a hand-rolled frame.
//!
//! Teardown: the accept loop is a task that owns the `TcpListener`, so the ephemeral port
//! is released when the test's `#[tokio::test]` runtime shuts down — or the instant a
//! panic (unmapped path / exhausted queue) kills the task.
//!
//! This module is `#[cfg(test)]`: it is compiled into the test binary only and never
//! reaches the shipped proxy.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A client that connects and never sends must not wedge the loop.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-path script: each path owns its own FIFO of response frames.
type PathQueues = HashMap<String, VecDeque<Vec<u8>>>;

/// Observable state of a running [`scripted_backend`].
pub(crate) struct ScriptedState {
    accepts: AtomicUsize,
    requests: Mutex<Vec<(String, String)>>,
    queues: Mutex<PathQueues>,
}

/// Handle to a running backend, shared with the accept-loop task.
pub(crate) type SharedScriptedState = Arc<ScriptedState>;

impl ScriptedState {
    /// Connections accepted so far. One per request, because every response closes
    /// its socket (reqwest would otherwise pool the connection and reuse it).
    pub(crate) fn accepts(&self) -> usize {
        self.accepts.load(Ordering::SeqCst)
    }

    /// `(method, request-target)` of every request served, in arrival order —
    /// the `(method, path)` pair `e2e/src/types.rs` `ReceivedRequest` records.
    pub(crate) fn requests(&self) -> Vec<(String, String)> {
        self.requests.lock().expect("scripted_backend requests lock").clone()
    }

    /// Response frames still queued across every path.
    pub(crate) fn remaining(&self) -> usize {
        self.queues.lock().expect("scripted_backend queues lock").values().map(VecDeque::len).sum()
    }

    fn record(&self, method: &str, path: &str) {
        self.requests.lock().expect("scripted_backend requests lock").push((method.to_string(), path.to_string()));
    }

    /// Pop the next frame scripted for `path`.
    ///
    /// Panics naming the path (never hangs) when the path was never scripted or its
    /// queue is drained. The queue lock is released before the panic so the mutex is
    /// not poisoned for the assertions the failing test still wants to run.
    fn take(&self, method: &str, path: &str) -> Vec<u8> {
        let mut queues = self.queues.lock().expect("scripted_backend queues lock");
        if let Some(queue) = queues.get_mut(path) {
            if let Some(frame) = queue.pop_front() {
                return frame;
            }
            let message = format!("scripted_backend: exhausted response queue for path `{path}` (method {method})");
            drop(queues);
            panic!("{message}");
        }
        let mut mapped: Vec<&str> = queues.keys().map(String::as_str).collect();
        mapped.sort_unstable();
        let message = format!("scripted_backend: unmapped path `{path}` (method {method}); scripted paths: {mapped:?}");
        drop(queues);
        panic!("{message}");
    }
}

/// Spawn a mock backend answering `routes[path]`'s next frame, per exact path.
///
/// Returns the ephemeral port it bound. Every test must call this separately (its own
/// port): caches keyed by backend URL (e.g. `CONTEXT_CACHE`) are process-global and the
/// suite runs tests in parallel inside one process, so a shared port cross-contaminates.
pub(crate) async fn scripted_backend(routes: HashMap<String, Vec<Vec<u8>>>) -> (u16, SharedScriptedState) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("scripted backend must bind an ephemeral port");
    let port = listener.local_addr().expect("scripted backend must report its addr").port();
    let state = Arc::new(ScriptedState {
        accepts: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
        queues: Mutex::new(routes.into_iter().map(|(path, queue)| (path, queue.into_iter().collect())).collect()),
    });
    let task_state = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { break };
            task_state.accepts.fetch_add(1, Ordering::SeqCst);
            serve_one(&mut sock, &task_state).await;
        }
    });
    (port, state)
}

/// Read one request head, dispatch on its path, write one response, close.
async fn serve_one(sock: &mut TcpStream, state: &ScriptedState) {
    let Some((method, path)) = read_request_head(sock).await else {
        return; // peer went quiet or reset mid-head; nothing to serve or record
    };
    state.record(&method, &path);
    let frame = with_connection_close(&state.take(&method, &path));
    let _ = sock.write_all(&frame).await;
    let _ = sock.flush().await;
    let _ = sock.shutdown().await;
}

/// Read bytes until `\r\n\r\n` and pull `(method, request-target)` off the request line.
///
/// The request-target is used verbatim (origin-form, query string included), so route
/// keys must match exactly what the client sends.
async fn read_request_head(sock: &mut TcpStream) -> Option<(String, String)> {
    let mut head: Vec<u8> = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        // Timeout, EOF and socket error all mean "nothing to serve"; the accept counter
        // already recorded that the client connected.
        let Ok(Ok(read)) = tokio::time::timeout(READ_TIMEOUT, sock.read(&mut buf)).await else {
            return None;
        };
        if read == 0 {
            return None;
        }
        head.extend_from_slice(&buf[..read]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let mut request_line = text.lines().next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    Some((method, path))
}

/// Frame a complete HTTP/1.1 response for a `scripted_backend` route table.
///
/// `status` is the full status tail (`"200 OK"`, `"404 Not Found"`). Mirrors the
/// `raw_http_response` fixture in `handler.rs` and always emits `connection: close`.
pub(crate) fn scripted_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n").into_bytes();
    for (name, value) in headers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("content-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out
}

/// `scripted_response` with `content-type: application/json`.
pub(crate) fn json_response(status: &str, body: &str) -> Vec<u8> {
    scripted_response(status, &[("content-type", "application/json")], body.as_bytes())
}

/// Guarantee `connection: close` on a response frame, adding it when the caller forgot.
///
/// Not cosmetic: reqwest pools connections, so one scripted response served over a
/// socket the client keeps would hand the client's *next* request to a socket that is
/// already gone, desyncing the per-path queues. Both repo precedents force it for the
/// same reason (`context.rs`'s listeners, `handler.rs`'s `raw_http_response`).
fn with_connection_close(frame: &[u8]) -> Vec<u8> {
    let Some(head_end) = frame.windows(4).position(|w| w == b"\r\n\r\n") else {
        return frame.to_vec(); // not a framed response at all - leave it untouched
    };
    let declares_connection = String::from_utf8_lossy(&frame[..head_end])
        .lines()
        .any(|line| line.trim_start().to_ascii_lowercase().starts_with("connection:"));
    if declares_connection {
        return frame.to_vec();
    }
    // Splice the header in right after the status line's CRLF.
    let Some(status_line_end) = frame.windows(2).position(|w| w == b"\r\n") else {
        return frame.to_vec();
    };
    let mut out = Vec::with_capacity(frame.len() + b"connection: close\r\n".len());
    out.extend_from_slice(&frame[..status_line_end + 2]);
    out.extend_from_slice(b"connection: close\r\n");
    out.extend_from_slice(&frame[status_line_end + 2..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the per-path route table `scripted_backend` consumes.
    fn routes(pairs: Vec<(&str, Vec<Vec<u8>>)>) -> HashMap<String, Vec<Vec<u8>>> {
        pairs
            .into_iter()
            .map(|(path, queue)| (path.to_string(), queue))
            .collect()
    }

    async fn get_body(port: u16, path: &str) -> String {
        let url = format!("http://127.0.0.1:{port}{path}");
        reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {path} must reach the scripted backend: {e}"))
            .text()
            .await
            .unwrap_or_else(|e| panic!("GET {path} body must be readable: {e}"))
    }

    async fn post_body(port: u16, path: &str, payload: &str) -> String {
        let url = format!("http://127.0.0.1:{port}{path}");
        reqwest::Client::new()
            .post(&url)
            .header("content-type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .unwrap_or_else(|e| panic!("POST {path} must reach the scripted backend: {e}"))
            .text()
            .await
            .unwrap_or_else(|e| panic!("POST {path} body must be readable: {e}"))
    }

    async fn get_status_and_body(port: u16, path: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{port}{path}");
        let res = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {path} must reach the scripted backend: {e}"));
        let status = res.status().as_u16();
        (status, res.text().await.expect("body readable"))
    }

    #[tokio::test]
    async fn dispatches_by_path_not_by_order() {
        let (port, state) = scripted_backend(routes(vec![
            ("/a", vec![json_response("200 OK", r#"{"slot":"a1"}"#)]),
            ("/b", vec![json_response("200 OK", r#"{"slot":"b1"}"#)]),
        ]))
        .await;

        // A connection-order queue would hand out `/a`'s body on this first call.
        assert_eq!(get_body(port, "/b").await, r#"{"slot":"b1"}"#);
        assert_eq!(get_body(port, "/a").await, r#"{"slot":"a1"}"#);
        println!("recorded paths: {:?}", state.requests());
        assert_eq!(
            state.requests(),
            vec![
                ("GET".to_string(), "/b".to_string()),
                ("GET".to_string(), "/a".to_string()),
            ],
            "recording order is arrival order; only the response lookup is path-keyed"
        );
        assert_eq!(state.remaining(), 0, "each path drained its own queue");
    }

    #[tokio::test]
    async fn counts_every_accept() {
        let (port, state) = scripted_backend(routes(vec![
            (
                "/one",
                vec![json_response("200 OK", "1a"), json_response("200 OK", "1b")],
            ),
            ("/two", vec![json_response("200 OK", "2a")]),
        ]))
        .await;
        assert_eq!(state.accepts(), 0, "no client has connected yet");

        assert_eq!(get_body(port, "/one").await, "1a");
        assert_eq!(get_body(port, "/two").await, "2a");
        assert_eq!(get_body(port, "/one").await, "1b");

        // reqwest pools connections, so this only holds because every response carries
        // `connection: close`: one accept per request, never a reused socket.
        assert_eq!(state.accepts(), 3);
        assert_eq!(
            state.accepts(),
            state.requests().len(),
            "accepts and recorded requests can never diverge"
        );
    }

    #[tokio::test]
    async fn records_method_and_path() {
        let (port, state) = scripted_backend(routes(vec![
            ("/v1/chat/completions", vec![json_response("200 OK", "{}")]),
            ("/props", vec![json_response("200 OK", "{}")]),
            (
                "/metrics",
                vec![scripted_response("200 OK", &[("content-type", "text/plain")], b"# nothing\n")],
            ),
        ]))
        .await;

        post_body(port, "/v1/chat/completions", r#"{"model":"x"}"#).await;
        get_body(port, "/props").await;
        let (status, metrics) = get_status_and_body(port, "/metrics").await;
        assert_eq!((status, metrics.as_str()), (200, "# nothing\n"));
        println!("recorded requests: {:?}", state.requests());

        assert_eq!(
            state.requests(),
            vec![
                ("POST".to_string(), "/v1/chat/completions".to_string()),
                ("GET".to_string(), "/props".to_string()),
                ("GET".to_string(), "/metrics".to_string()),
            ],
            "method and request-target are recorded exactly as the client sent them"
        );
    }

    #[tokio::test]
    async fn serves_the_same_path_twice() {
        // Hand-rolled frame with NO `connection:` header: the seam must add it, because a
        // reused socket would make the second request never reach the accept loop.
        let hand_rolled = b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n\r\nsecond";
        let framed = with_connection_close(hand_rolled);
        let framed_text = String::from_utf8_lossy(&framed).to_ascii_lowercase();
        assert!(
            framed_text.contains("connection: close"),
            "the writer must force `connection: close` onto a frame that omits it, got {framed_text:?}"
        );
        assert!(framed.ends_with(b"second"), "the body must survive the splice");

        let (port, state) = scripted_backend(routes(vec![(
            "/kv",
            vec![json_response("200 OK", "first"), framed],
        )]))
        .await;
        assert_eq!(state.remaining(), 2);

        assert_eq!(get_body(port, "/kv").await, "first");
        assert_eq!(state.remaining(), 1, "one queue entry was consumed");
        assert_eq!(get_body(port, "/kv").await, "second");
        assert_eq!(state.remaining(), 0);
        assert_eq!(state.accepts(), 2, "the same path twice is two connections");
    }
}
