//! Runtime handle for a single backend node

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::config::TlsConfig;

/// TCP connect timeout applied to every backend-node HTTP client (big-fix E-M7)
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// A single backend node with its own HTTP client
#[derive(Debug)]
pub struct BackendNode {
    pub url: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub timeout_seconds: u64,
    pub http_client: reqwest::Client,
    /// Arc so a BackendGuard can keep exactly this counter alive for the whole
    /// streamed-body lifetime, not just as long as its BackendNode borrow.
    pub active_requests: Arc<AtomicUsize>,
    pub strip_path_prefix: Option<String>,
    /// Optional temperature override for requests to this node
    pub temperature: Option<f64>,
    /// False while a forwarded failure (connect error, backend 429/5xx) keeps the
    /// node out of selection; see `mark_failed` / `mark_healthy` (big-fix E-M1).
    /// Crate-visible only so node-set test fixtures can build the struct; runtime
    /// code must go through the mark_*/in_cooldown/cooldown_expiry methods.
    pub(crate) healthy: AtomicBool,
    /// Instant from which a failed node may be selected again. A std Mutex is fine
    /// here: it is never held across an await and guards one Copy `Instant`.
    pub(crate) cooldown_until: Mutex<Instant>,
}

impl BackendNode {
    /// Construct a BackendNode from configuration parameters
    pub fn from_config(
        url: String,
        timeout_seconds: u64,
        tls: Option<&TlsConfig>,
        model: Option<String>,
        api_key: Option<String>,
        strip_path_prefix: Option<String>,
        temperature: Option<f64>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = build_node_client(timeout_seconds, tls)?;
        Ok(Self {
            url,
            model,
            api_key,
            timeout_seconds,
            http_client,
            active_requests: Arc::new(AtomicUsize::new(0)),
            strip_path_prefix,
            temperature,
            healthy: AtomicBool::new(true),
            cooldown_until: Mutex::new(Instant::now()),
        })
    }

    /// Take the node out of selection for `cooldown`. Repeated failures re-arm the
    /// window from the LATEST failure. A zero duration never excludes the node.
    pub fn mark_failed(&self, cooldown: Duration) {
        *self.cooldown_lock() = Instant::now() + cooldown;
        self.healthy.store(false, Ordering::Release);
    }

    /// A forwarded success proves the node alive: selection ignores any leftover window.
    pub fn mark_healthy(&self) {
        self.healthy.store(true, Ordering::Release);
    }

    /// Whether selection must exclude this node at `now`. The clock is a parameter:
    /// strategies call this with `Instant::now()`, tests pass synthetic times.
    pub fn in_cooldown(&self, now: Instant) -> bool {
        !self.healthy.load(Ordering::Acquire) && now < *self.cooldown_lock()
    }

    /// When a failed node becomes selectable again; the all-cooled fallback prefers
    /// the earliest expiry.
    pub fn cooldown_expiry(&self) -> Instant {
        *self.cooldown_lock()
    }

    /// Only a plain `Instant` copy happens while the lock is held, so a poisoned
    /// guard still yields a valid value; recovering it is never wrong here.
    fn cooldown_lock(&self) -> MutexGuard<'_, Instant> {
        self.cooldown_until.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Returns the base URL with trailing slash stripped
    pub fn base_url(&self) -> &str {
        self.url.trim_end_matches('/')
    }

    /// Returns the effective path after stripping any configured prefix
    pub fn effective_path<'a>(&self, path: &'a str) -> &'a str {
        if let Some(ref prefix) = self.strip_path_prefix {
            path.strip_prefix(prefix.as_str()).unwrap_or(path)
        } else {
            path
        }
    }
}

/// Attach the node's `Authorization: Bearer` header to a request builder.
///
/// The ONE auth builder (big-fix E-M3): every request that must carry a node's
/// api_key goes through here, so no call site can silently drop auth and get a
/// "successful connection, 401 body" miss.
pub(crate) fn with_auth(req: reqwest::RequestBuilder, api_key: Option<&str>) -> reqwest::RequestBuilder {
    match api_key {
        Some(key) => req.header(reqwest::header::AUTHORIZATION, format!("Bearer {}", key)),
        None => req,
    }
}

/// Build a request URL for `path` against a trailing-slash-stripped node `base`,
/// applying the node's `strip_path_prefix` exactly once (big-fix E-L6).
///
/// ONE implementation behind every out-bound URL site (preflight's /v1/models
/// probes AND the context monitor's native `/props` + `/v1/models` fetches —
/// the monitor passes `prefix: None` because monitoring is backend-native,
/// task 15). A prefix that is not a true prefix of `path` leaves `path`
/// untouched, matching the per-site `strip_prefix().unwrap_or(path)` behavior
/// this replaces.
pub(crate) fn node_url(base: &str, path: &str, prefix: Option<&str>) -> String {
    let native = prefix.and_then(|p| path.strip_prefix(p)).unwrap_or(path);
    format!("{base}{native}")
}

/// The node whose failure cooldown expires earliest; ties go to the lowest index
/// (`min_by_key` keeps the first minimum). Callers pass a node set where every node
/// is cooled; balancers reject empty node lists at construction, so the non-empty
/// invariant is asserted, not papered over (big-fix E-L1: no silent fallback).
pub(crate) fn soonest_recovering_node(nodes: &[Arc<BackendNode>]) -> Arc<BackendNode> {
    nodes
        .iter()
        .min_by_key(|node| node.cooldown_expiry())
        .cloned()
        .expect("at least one candidate")
}

/// Build an HTTP client for a single backend node.
///
/// The ONE client factory for backend traffic: forwarding (via `from_config`) and
/// startup preflight probes (big-fix E-M5) must not drift on TLS handling or
/// timeouts, so preflight calls this instead of hand-rolling a builder.
pub(crate) fn build_node_client(
    timeout_seconds: u64,
    tls: Option<&TlsConfig>,
) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_max_idle_per_host(10);

    if let Some(tls) = tls {
        if tls.accept_invalid_certs {
            client_builder = client_builder.danger_accept_invalid_certs(true);
            tracing::warn!("TLS: Accepting invalid certificates (use only for development/testing)");
        }

        if let Some(ref ca_path) = tls.ca_cert_path {
            let ca_cert = std::fs::read(ca_path)?;
            let ca_cert = reqwest::Certificate::from_pem(&ca_cert)?;
            client_builder = client_builder.add_root_certificate(ca_cert);
            tracing::info!("TLS: Loaded custom CA certificate from {}", ca_path);
        }

        if let (Some(cert_path), Some(key_path)) = (&tls.client_cert_path, &tls.client_key_path) {
            let cert_pem = std::fs::read(cert_path)?;
            let key_pem = std::fs::read(key_path)?;
            let identity = reqwest::Identity::from_pem(&[cert_pem, key_pem].concat())?;
            client_builder = client_builder.identity(identity);
            tracing::info!("TLS: Loaded client certificate from {} for mTLS", cert_path);
        }
    }

    Ok(client_builder.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn connect_timeout_is_five_seconds() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn node_url_applies_the_prefix_exactly_once() {
        assert_eq!(node_url("http://h:1", "/v1/models", Some("/v1")), "http://h:1/models");
        assert_eq!(node_url("http://h:1", "/v1/models", None), "http://h:1/v1/models");
        assert_eq!(
            node_url("http://h:1", "/v1/models", Some("/日本")),
            "http://h:1/v1/models",
            "non-prefix leaves the path untouched"
        );
        assert_eq!(
            node_url("http://h:1", "/v1/models", Some("/v1/models")),
            "http://h:1",
            "degenerate prefix==path strips to root"
        );
        assert_eq!(
            node_url("http://h:1", "/props", Some("/v1")),
            "http://h:1/props",
            "true prefix of a native path is NOT stripped away by accident"
        );
        assert_eq!(
            node_url("http://h:1", "/v1/日本語", Some("/v1")),
            "http://h:1/日本語",
            "multibyte tail survives"
        );
    }

    fn cooldown_node() -> BackendNode {
        BackendNode::from_config("http://127.0.0.1:1".to_string(), 300, None, None, None, None, None)
            .expect("config node for cooldown tests")
    }

    #[test]
    fn fresh_node_is_selectable() {
        let node = cooldown_node();
        assert!(!node.in_cooldown(Instant::now()), "a fresh node must be selectable");
        assert!(
            node.cooldown_expiry() <= Instant::now(),
            "an unfailed node must report an already-past expiry"
        );
    }

    #[test]
    fn mark_failed_cools_the_node_until_expiry() {
        let node = cooldown_node();
        let t0 = Instant::now();
        node.mark_failed(Duration::from_secs(30));
        assert!(
            node.in_cooldown(t0 + Duration::from_secs(1)),
            "1s into a 30s window the node must be cooled"
        );
        assert!(
            !node.in_cooldown(t0 + Duration::from_secs(31)),
            "31s past marking, the window must have expired and the node selectable again"
        );
        let until = node.cooldown_expiry();
        assert!(
            until >= t0 + Duration::from_secs(30) && until <= Instant::now() + Duration::from_secs(30),
            "expiry must be now + the requested duration, got {until:?}"
        );
    }

    #[test]
    fn mark_healthy_clears_the_cooldown_immediately() {
        let node = cooldown_node();
        node.mark_failed(Duration::from_secs(30));
        assert!(node.in_cooldown(Instant::now()));
        node.mark_healthy();
        assert!(
            !node.in_cooldown(Instant::now()),
            "a success must end the cooldown without waiting for the window"
        );
    }

    #[test]
    fn zero_cooldown_never_excludes() {
        // failure_cooldown_secs: 0 in config must mean "never cooldown".
        let node = cooldown_node();
        node.mark_failed(Duration::ZERO);
        assert!(
            !node.in_cooldown(Instant::now()),
            "a zero-duration failure must never take the node out of selection"
        );
    }

    /// Wiring proof: a client produced by `build_node_client` must abort a blackholed
    /// connect at the connect timeout (~5s), NOT at its 1h request timeout.
    /// 192.0.2.1 is RFC 5737 TEST-NET-1: unroutable, SYNs are silently dropped
    /// (verified on this host). Requires no HTTP proxy env vars to be set.
    #[tokio::test]
    async fn build_node_client_applies_connect_timeout() {
        let client = build_node_client(3600, None).expect("builder must produce a client");
        let start = Instant::now();
        let outcome = tokio::time::timeout(CONNECT_TIMEOUT * 6, client.get("http://192.0.2.1/").send()).await;
        let elapsed = start.elapsed();
        let result = outcome.expect("reqwest must surface the 5s connect timeout, not outlive the 30s cap");
        assert!(result.is_err(), "blackholed connect to TEST-NET-1 must fail, got Ok");
        assert!(
            elapsed >= CONNECT_TIMEOUT - Duration::from_secs(1),
            "connect failed after {elapsed:?}, too fast for the ~{CONNECT_TIMEOUT:?} connect timeout to have applied"
        );
    }
}
