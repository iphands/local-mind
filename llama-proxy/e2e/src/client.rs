//! HTTP client that simulates how Claude Code / Opencode talks to the proxy

use bytes::Bytes;
use futures::StreamExt;
use reqwest::Client;

use crate::types::{ProxyResponse, SseEvent, StreamingResponse};

/// Build an HTTP client (no connection pooling for test isolation)
pub fn build_client() -> Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build reqwest client")
}

/// Send a non-streaming chat completion request to the proxy
pub async fn send_non_streaming(
    client: &Client,
    proxy_addr: &str,
    request_body: serde_json::Value,
) -> anyhow::Result<ProxyResponse> {
    let url = format!("http://{proxy_addr}/v1/chat/completions");

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send request to proxy: {}", e))?;

    let status = resp.status().as_u16();
    let body_text = resp
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read proxy response: {}", e))?;

    let body: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
        anyhow::anyhow!(
            "Proxy response is not valid JSON: {}: {}",
            e,
            &body_text[..body_text.len().min(500)]
        )
    })?;

    Ok(ProxyResponse { status, body })
}

/// Send a streaming chat completion request to the proxy, collect all SSE events
pub async fn send_streaming(
    client: &Client,
    proxy_addr: &str,
    mut request_body: serde_json::Value,
) -> anyhow::Result<StreamingResponse> {
    // Set stream: true
    request_body["stream"] = serde_json::Value::Bool(true);

    let url = format!("http://{proxy_addr}/v1/chat/completions");

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send streaming request to proxy: {}", e))?;

    let status = resp.status().as_u16();
    if status != 200 {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Proxy returned error {}: {}", status, body));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("text/event-stream") {
        return Err(anyhow::anyhow!("Expected text/event-stream but got: {}", content_type));
    }

    // Collect all bytes from the stream
    let mut stream = resp.bytes_stream();
    let mut all_bytes: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk: Bytes = chunk.map_err(|e| anyhow::anyhow!("Stream read error: {}", e))?;
        all_bytes.extend_from_slice(&chunk);
    }

    let body_text = String::from_utf8_lossy(&all_bytes);
    let events = parse_sse(&body_text)?;

    Ok(StreamingResponse { events })
}

/// Parse SSE body text into events
///
/// SSE format: Each event is separated by \n\n
/// Each event line: "data: <content>" or "event: <type>" etc.
/// We only care about "data: " lines.
fn parse_sse(text: &str) -> anyhow::Result<Vec<SseEvent>> {
    let mut events = Vec::new();

    // Split on double newlines to get individual events
    for raw_event in text.split("\n\n") {
        let raw_event = raw_event.trim();
        if raw_event.is_empty() {
            continue;
        }

        // Find the "data: " line in this event (may have multiple lines like "event: foo\ndata: bar")
        let mut data_line: Option<&str> = None;
        for line in raw_event.lines() {
            if let Some(stripped) = line.strip_prefix("data: ") {
                data_line = Some(stripped);
            }
        }

        let data = match data_line {
            Some(d) => d,
            None => continue, // No data line in this event block (e.g. comment-only)
        };

        if data == "[DONE]" {
            events.push(SseEvent {
                data: "[DONE]".to_string(),
                is_done: true,
            });
        } else {
            events.push(SseEvent {
                data: data.to_string(),
                is_done: false,
            });
        }
    }

    Ok(events)
}

/// Send a GET request to the proxy (for passthrough endpoints like /health, /props, /slots)
pub async fn send_get(client: &Client, proxy_addr: &str, path: &str) -> anyhow::Result<ProxyResponse> {
    let url = format!("http://{proxy_addr}{path}");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to GET {}: {}", url, e))?;

    let status = resp.status().as_u16();
    let body_text = resp.text().await.unwrap_or_default();

    let body: serde_json::Value = serde_json::from_str(&body_text).unwrap_or(serde_json::Value::String(body_text));

    Ok(ProxyResponse { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_basic() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: [DONE]\n\n";
        let events = parse_sse(sse).unwrap();
        assert_eq!(events.len(), 2);
        assert!(!events[0].is_done);
        assert!(events[1].is_done);
    }

    #[test]
    fn test_parse_sse_multiple_events() {
        let sse = "data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: [DONE]\n\n";
        let events = parse_sse(sse).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data, r#"{"a":1}"#);
        assert_eq!(events[1].data, r#"{"b":2}"#);
        assert!(events[2].is_done);
    }

    #[test]
    fn test_parse_sse_with_event_type() {
        // axum Sse can emit "event: <type>" before "data: ..."
        let sse = "event: message\ndata: {\"x\":1}\n\ndata: [DONE]\n\n";
        let events = parse_sse(sse).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, r#"{"x":1}"#);
    }
}
