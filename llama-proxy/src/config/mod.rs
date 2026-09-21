mod loader;
mod validate;

pub use validate::validate_http_url;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub use loader::load_config;

/// Main application configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub server: ServerConfig,
    /// Optional single backend. Absent + `backends:` present → group routing.
    /// Absent + absent → the proxy still starts; every completion request
    /// answers the task-4 503 envelope (NoMatchingBackend).
    #[serde(default)]
    pub backend: Option<BackendConfig>,
    #[serde(default)]
    pub backends: Option<BackendsConfig>,
    #[serde(default)]
    pub fixes: FixesConfig,
    #[serde(default)]
    pub stats: StatsConfig,
    #[serde(default)]
    pub exporters: ExportersConfig,
    #[serde(default)]
    pub streaming: StreamingMode,
    #[serde(default)]
    pub synthesis: SynthesisConfig,
    #[serde(default, rename = "augment-backend")]
    pub augment_backend: Option<AugmentBackendConfig>,
    #[serde(default)]
    pub reprompt: Option<RepromptConfig>,
    #[serde(default)]
    pub dump: DumpConfig,
}

/// Debug dump configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DumpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub path: String,
}

/// Proxy server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,

    /// Maximum in-flight completion requests before returning 429.
    ///
    /// `0` (the default) means unlimited, which preserves the behavior of configs
    /// written before this setting existed. Set a positive value to shed load
    /// instead of piling it onto the backend.
    ///
    /// Only completion routes are limited. Monitoring endpoints (`/health`,
    /// `/props`, `/slots`, `/v1/models`, `/metrics`, `/proxy/metrics`) are never
    /// rejected, so a saturated proxy stays observable.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Exact `Origin` values allowed to read cross-origin responses.
    ///
    /// `Some(list)` installs an exact-match allow-list: a request whose
    /// `Origin` equals a list entry gets that origin echoed in
    /// `Access-Control-Allow-Origin`; any other origin gets no CORS echo at
    /// all. Entries are exact strings (scheme://host[:port]) — no wildcards,
    /// no patterns. `None` (the default, key absent) keeps the permissive
    /// behavior of every build before this setting existed: every origin is
    /// allowed via the `*` wildcard.
    #[serde(default)]
    pub allowed_origins: Option<Vec<String>>,
}

/// Default concurrency limit: unlimited.
///
/// Deliberately `0` rather than a finite cap: silently capping existing
/// deployments on upgrade would turn a config-file no-op into a source of 429s.
pub fn default_max_concurrent() -> usize {
    0
}

/// Backend llama-server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    /// Full backend URL (e.g., "https://example.com:4234" or "http://localhost:8080")
    pub url: String,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// TLS configuration options
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    /// Model identifier in provider/model format (e.g., "anthropic/claude-sonnet-4-5")
    #[serde(default)]
    pub model: Option<String>,
    /// API key for backend authentication
    #[serde(default)]
    pub api_key: Option<String>,
    /// Strip this prefix from incoming request path before forwarding (e.g., "/v1" for Z.ai)
    #[serde(default)]
    pub strip_path_prefix: Option<String>,
}

/// TLS configuration for backend connections
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// Accept invalid certificates (self-signed, expired)
    #[serde(default)]
    pub accept_invalid_certs: bool,
    /// Path to custom CA certificate (PEM format)
    pub ca_cert_path: Option<String>,
    /// Path to client certificate for mTLS
    pub client_cert_path: Option<String>,
    /// Path to client private key for mTLS
    pub client_key_path: Option<String>,
}

fn default_timeout() -> u64 {
    300
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: default_timeout(),
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        }
    }
}

impl BackendConfig {
    /// Returns the base URL with trailing slash stripped
    pub fn base_url(&self) -> &str {
        self.url.trim_end_matches('/')
    }

    /// Returns true if the URL uses HTTPS
    pub fn is_tls(&self) -> bool {
        self.url.to_lowercase().starts_with("https://")
    }
}

/// Per-node configuration for multi-backend mode
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendNodeConfig {
    /// Full backend URL (e.g., "http://localhost:8080")
    pub url: String,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// TLS configuration options
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    /// Model identifier override (e.g., "anthropic/claude-sonnet-4-5")
    #[serde(default)]
    pub model: Option<String>,
    /// API key for backend authentication
    #[serde(default)]
    pub api_key: Option<String>,
    /// Strip this prefix from incoming request path before forwarding (e.g., "/v1" for Z.ai)
    #[serde(default)]
    pub strip_path_prefix: Option<String>,
    /// Override temperature for requests to this node (if absent, client value passes through)
    #[serde(default)]
    pub temperature: Option<f64>,
}

/// Configuration for a single backend group
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendGroupConfig {
    /// Model names this group handles (empty = catch-all)
    pub mappings: Vec<String>,
    /// Load balancing strategy (round_robin or priority_free)
    #[serde(default = "default_strategy")]
    pub strategy: String,
    /// How long a node that just failed (connect error / 429 / 5xx) stays out of
    /// selection. 0 disables cooldown entirely.
    #[serde(default = "default_failure_cooldown_secs")]
    pub failure_cooldown_secs: u64,
    /// Opt this group out of the no-catch-all positional fallback (E-M6):
    /// an unmatched model is never routed here unless the group maps it
    /// explicitly. A group with empty `mappings` is the catch-all and owns
    /// unmatched traffic at a rung that never consults this flag, so
    /// `exclusive: true` on a catch-all group is a no-op.
    #[serde(default)]
    pub exclusive: bool,
    /// List of backend nodes in this group
    pub nodes: Vec<BackendNodeConfig>,
}

fn default_strategy() -> String {
    "round_robin".to_string()
}

fn default_failure_cooldown_secs() -> u64 {
    30
}

/// Multi-backend configuration - named groups with per-group strategy
pub type BackendsConfig = HashMap<String, BackendGroupConfig>;

/// Error when no backend matches the requested model
#[derive(Debug, Clone)]
pub struct NoMatchingBackend {
    pub requested_model: Option<String>,
}

impl std::fmt::Display for NoMatchingBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.requested_model {
            Some(model) => write!(f, "No backend configured for model: {}", model),
            None => write!(f, "No backend configured (no model specified and no catch-all)"),
        }
    }
}

impl std::error::Error for NoMatchingBackend {}

/// Response fix modules configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FixesConfig {
    #[serde(default = "default_fixes_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub modules: HashMap<String, FixModuleConfig>,
}

fn default_fixes_enabled() -> bool {
    true
}

fn always_true() -> bool {
    true
}

impl Default for FixesConfig {
    fn default() -> Self {
        Self {
            enabled: default_fixes_enabled(),
            modules: HashMap::new(),
        }
    }
}

/// Individual fix module configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FixModuleConfig {
    #[serde(default = "always_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub options: HashMap<String, serde_yaml::Value>,
}

/// Stats logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatsConfig {
    #[serde(default = "default_stats_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub format: StatsFormat,
}

fn default_stats_enabled() -> bool {
    true
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            enabled: default_stats_enabled(),
            format: StatsFormat::default(),
        }
    }
}

/// Stats output format
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StatsFormat {
    #[default]
    Pretty,
    Json,
    Compact,
}

/// Streaming mode configuration
///
/// Controls how the proxy handles streaming responses on the OpenAI
/// `/v1/chat/completions` route:
/// - `Disabled`: accepted and treated exactly like `Fake`; not yet a distinct
///   behavior (reserved for a future "refuse streaming entirely" mode).
/// - `Fake`: forces `stream:false` to the backend, gets one JSON, synthesizes SSE
///   to the client. Fixes run; reprompt runs.
/// - `Passthrough`: leaves `stream:true` intact, forwards the backend's SSE verbatim
///   (byte-faithful framing), stats computed from the accumulated bytes. Fixes DETECT
///   but do NOT repair (patching partial deltas corrupts them); reprompt cannot run
///   (it needs the complete body). `/v1/messages` stays buffered even here — there is
///   no OpenAI→Anthropic SSE translator.
///
/// The request path reads this setting (handler.rs). Enforcement is decided AFTER the
/// CLI/config precedence in `resolve()`, so the CLI switch can override the file.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StreamingMode {
    Disabled,
    #[default]
    Fake,
    /// Backend SSE copied verbatim, stats computed from accumulated raw bytes.
    Passthrough,
}

impl std::fmt::Display for StreamingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl StreamingMode {
    /// Returns true if streaming is completely disabled
    pub fn is_disabled(&self) -> bool {
        matches!(self, StreamingMode::Disabled)
    }

    /// Returns true if using fake streaming mode
    pub fn is_fake(&self) -> bool {
        matches!(self, StreamingMode::Fake)
    }

    pub fn is_passthrough(&self) -> bool {
        matches!(self, StreamingMode::Passthrough)
    }

    /// What the mode actually becomes. `disabled` says "synthesized" because that is
    /// what it does today, even though its name promises something else.
    pub fn effective_label(&self) -> &'static str {
        match self {
            StreamingMode::Fake => "backend JSON, synthesized SSE",
            StreamingMode::Passthrough => "backend SSE, verbatim (OpenAI path only)",
            StreamingMode::Disabled => "backend JSON, synthesized SSE (reserved: disabled)",
        }
    }

    /// Lowercase spelling, as it appears in config files and the CLI.
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamingMode::Disabled => "disabled",
            StreamingMode::Fake => "fake",
            StreamingMode::Passthrough => "passthrough",
        }
    }

    /// Parse a mode from a CLI string (case-insensitive).
    /// Returns None for unknown values.
    pub fn from_str_mode(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "disabled" => Some(StreamingMode::Disabled),
            "fake" => Some(StreamingMode::Fake),
            "passthrough" => Some(StreamingMode::Passthrough),
            _ => None,
        }
    }

    /// Resolve the effective streaming mode: CLI switch wins, then the loaded
    /// config value, then the default (`fake`). `cli_override` is the raw
    /// `--streaming-mode` value if present.
    ///
    /// Returns Err(unknown value) rather than exiting so callers (and tests)
    /// control failure.
    pub fn resolve(cli_override: Option<&str>, config_value: StreamingMode) -> Result<Self, String> {
        match cli_override {
            Some(raw) => Self::from_str_mode(raw)
                .ok_or_else(|| format!("Invalid streaming mode: {}. Use 'disabled', 'fake', or 'passthrough'.", raw)),
            None => Ok(config_value),
        }
    }
}

/// Streaming-synthesis chunk timing configuration (`synthesis:` section).
///
/// Controls how fake-mode synthesis paces and splits its synthesized SSE
/// stream. The `streaming:` key remains a plain mode string; this section is
/// purely additive to [`AppConfig`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SynthesisConfig {
    /// Delay inserted between synthesized SSE chunks, in milliseconds.
    /// 0 (default) emits every chunk without an artificial sleep.
    #[serde(default)]
    pub chunk_delay_ms: u64,
    /// Maximum characters per synthesized text chunk. CHAR-based, matching
    /// the emoji-aware splitter (`split_chars`), never bytes.
    #[serde(default = "default_synthesis_chunk_size_chars")]
    pub chunk_size_chars: usize,
}

fn default_synthesis_chunk_size_chars() -> usize {
    2000
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self {
            chunk_delay_ms: 0,
            chunk_size_chars: default_synthesis_chunk_size_chars(),
        }
    }
}

/// Augment backend configuration - experimental feature for enriching requests
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AugmentBackendConfig {
    /// Enable augment-backend feature (opt-in: absent key defaults to false)
    #[serde(default = "default_augment_enabled")]
    pub enabled: bool,
    /// Full URL to augment backend (e.g., "http://cosmo.lan:8701")
    pub url: String,
    /// Model identifier for augment backend
    pub model: String,
    /// Path to backend prompt file (defaults to "./augmenter/backend_prompt.md")
    #[serde(default = "default_prompt_file")]
    pub prompt_file: String,
    /// Path to request prompt file injected into enriched user message (defaults to "./augmenter/request_prompt.md")
    #[serde(default = "default_request_prompt_file")]
    pub request_prompt_file: String,
    /// HTTP timeout in seconds for calls to the augment backend (default: 15)
    #[serde(default = "default_augment_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_augment_enabled() -> bool {
    false
}

fn default_augment_timeout_secs() -> u64 {
    15
}

fn default_prompt_file() -> String {
    "./augmenter/backend_prompt.md".to_string()
}

fn default_request_prompt_file() -> String {
    "./augmenter/request_prompt.md".to_string()
}

/// Error when augment-backend is configured but not properly set up
#[derive(Debug, Clone)]
pub struct AugmentBackendError {
    pub message: String,
}

impl std::fmt::Display for AugmentBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AugmentBackendError: {}", self.message)
    }
}

impl std::error::Error for AugmentBackendError {}

// NOTE: the Default impl stays until augment.rs's test fixture (1272) drops
// its `..Default::default()` struct-update — that file is off-limits to this
// task; see big-fix 73 DONECLAIM (blocked items).
impl Default for AugmentBackendConfig {
    fn default() -> Self {
        Self {
            enabled: default_augment_enabled(),
            url: String::new(),
            model: String::new(),
            prompt_file: default_prompt_file(),
            request_prompt_file: default_request_prompt_file(),
            timeout_secs: default_augment_timeout_secs(),
        }
    }
}

/// Reprompt engine configuration — silently re-prompts on premature stop
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepromptConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Path to a markdown file containing the continue-prompt text
    #[serde(default)]
    pub prompt_file: Option<String>,
    /// Inline continue-prompt text (used when prompt_file is not set)
    #[serde(default)]
    pub prompt: Option<String>,
    /// Maximum reprompt retries before giving up (default: 3)
    #[serde(default = "default_reprompt_max_retries")]
    pub max_retries: u32,
    /// If the follow-up response text contains any of these strings, treat as clean finish.
    /// Accepts a single string (done_sentinel) or a list (done_sentinels) for backward compat.
    #[serde(
        alias = "done_sentinel",
        default = "default_reprompt_done_sentinels",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub done_sentinels: Vec<String>,
    /// Re-read prompt_file from disk on each trigger if the file has changed (default: true).
    /// Set to false to load the prompt once at startup and never re-read.
    #[serde(default = "default_reprompt_dynamic_prompt")]
    pub dynamic_prompt: bool,
    /// Log the full stop response body when a reprompt is triggered (default: false).
    /// Useful for debugging why the client-side state isn't updating correctly.
    #[serde(default)]
    pub log_stop_responses: bool,
    /// Skip the engine for requests that expose no file-mutating tools (default: true).
    /// Those are read-only subagents — code reviewers, explorers — which have no task list to
    /// resume, so the continue-prompt only pushes them into more searching and delays their answer.
    #[serde(default = "default_reprompt_skip_read_only")]
    pub skip_read_only_requests: bool,
    /// Wall-clock budget for the whole reprompt loop (default: 30_000 ms). Checked before
    /// each round and wrapped around every follow-up POST; on expiry the engine returns the
    /// collected turn instead of waiting. 0 exhausts the budget before round one.
    #[serde(default = "default_reprompt_max_total_ms")]
    pub max_total_ms: u64,
}

fn default_reprompt_max_retries() -> u32 {
    3
}

fn default_reprompt_skip_read_only() -> bool {
    true
}

fn default_reprompt_done_sentinels() -> Vec<String> {
    vec!["DONE".to_string()]
}

fn default_reprompt_dynamic_prompt() -> bool {
    true
}

fn default_reprompt_max_total_ms() -> u64 {
    30_000
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    struct StringOrVec;
    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "a string or list of strings")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<String>, E> {
            Ok(vec![v.to_owned()])
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut v = Vec::new();
            while let Some(s) = seq.next_element()? {
                v.push(s);
            }
            Ok(v)
        }
    }
    deserializer.deserialize_any(StringOrVec)
}

impl Default for RepromptConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt_file: None,
            prompt: None,
            max_retries: default_reprompt_max_retries(),
            done_sentinels: default_reprompt_done_sentinels(),
            dynamic_prompt: default_reprompt_dynamic_prompt(),
            log_stop_responses: false,
            skip_read_only_requests: default_reprompt_skip_read_only(),
            max_total_ms: default_reprompt_max_total_ms(),
        }
    }
}

/// Exporters configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ExportersConfig {
    #[serde(default)]
    pub influxdb: InfluxDbConfig,
}

/// InfluxDB exporter configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InfluxDbConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_influxdb_url")]
    pub url: String,
    #[serde(default = "default_influxdb_org")]
    pub org: String,
    #[serde(default = "default_influxdb_bucket")]
    pub bucket: String,
    #[serde(default)]
    pub token: String,
}

fn default_influxdb_url() -> String {
    "http://localhost:8086".to_string()
}

fn default_influxdb_org() -> String {
    "my-org".to_string()
}

fn default_influxdb_bucket() -> String {
    "llama-metrics".to_string()
}

impl Default for InfluxDbConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_influxdb_url(),
            org: default_influxdb_org(),
            bucket: default_influxdb_bucket(),
            token: String::new(),
        }
    }
}

impl AppConfig {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        load_config(path)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    NotFound(String),

    #[error("Failed to read configuration file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse configuration: {0}")]
    Parse(#[from] serde_yaml::Error),

    #[error("Configuration validation error: {0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_base_url() {
        let config = BackendConfig {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert_eq!(config.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_backend_config_https() {
        let config = BackendConfig {
            url: "https://example.com:4234".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert_eq!(config.base_url(), "https://example.com:4234");
        assert!(config.is_tls());
    }

    #[test]
    fn test_backend_config_is_tls() {
        let http_config = BackendConfig {
            url: "http://localhost:8080".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert!(!http_config.is_tls());

        let https_config = BackendConfig {
            url: "https://secure.example.com".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert!(https_config.is_tls());
    }

    #[test]
    fn test_backend_config_trailing_slash() {
        let config = BackendConfig {
            url: "http://localhost:8080/".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert_eq!(config.base_url(), "http://localhost:8080");
    }

    #[test]
    fn test_backend_config_default() {
        let config = BackendConfig::default();
        assert_eq!(config.url, "http://localhost:8080");
        assert_eq!(config.timeout_seconds, 300);
        assert!(config.tls.is_none());
        assert!(config.model.is_none());
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_backend_config_tls_options() {
        let config = BackendConfig {
            url: "https://secure.example.com".to_string(),
            timeout_seconds: 300,
            tls: Some(TlsConfig {
                accept_invalid_certs: true,
                ca_cert_path: Some("/path/to/ca.pem".to_string()),
                client_cert_path: None,
                client_key_path: None,
            }),
            model: None,
            api_key: None,
            strip_path_prefix: None,
        };
        assert!(config.tls.is_some());
        let tls = config.tls.unwrap();
        assert!(tls.accept_invalid_certs);
        assert_eq!(tls.ca_cert_path, Some("/path/to/ca.pem".to_string()));
    }

    #[test]
    fn test_backend_config_model_and_api_key() {
        let config = BackendConfig {
            url: "https://api.example.com".to_string(),
            timeout_seconds: 300,
            tls: None,
            model: Some("anthropic/claude-sonnet-4-5".to_string()),
            api_key: Some("sk-test-key".to_string()),
            strip_path_prefix: None,
        };
        assert_eq!(config.model, Some("anthropic/claude-sonnet-4-5".to_string()));
        assert_eq!(config.api_key, Some("sk-test-key".to_string()));
    }

    #[test]
    fn test_stats_format_default() {
        let format = StatsFormat::default();
        assert!(matches!(format, StatsFormat::Pretty));
    }

    #[test]
    fn test_stats_format_serde() {
        // Test serialization
        let pretty = StatsFormat::Pretty;
        let json = StatsFormat::Json;
        let compact = StatsFormat::Compact;

        assert_eq!(serde_json::to_string(&pretty).unwrap(), "\"pretty\"");
        assert_eq!(serde_json::to_string(&json).unwrap(), "\"json\"");
        assert_eq!(serde_json::to_string(&compact).unwrap(), "\"compact\"");
    }

    #[test]
    fn test_stats_format_deserialize() {
        let pretty: StatsFormat = serde_json::from_str("\"pretty\"").unwrap();
        let json: StatsFormat = serde_json::from_str("\"json\"").unwrap();
        let compact: StatsFormat = serde_json::from_str("\"compact\"").unwrap();

        assert!(matches!(pretty, StatsFormat::Pretty));
        assert!(matches!(json, StatsFormat::Json));
        assert!(matches!(compact, StatsFormat::Compact));
    }

    #[test]
    fn test_fix_module_config() {
        let config = FixModuleConfig {
            enabled: true,
            options: HashMap::new(),
        };
        assert!(config.enabled);
        assert!(config.options.is_empty());
    }

    #[test]
    fn test_fix_module_config_with_options() {
        // options is a FREE-FORM map: every documented option key that no
        // fixer consumed was purged (task 98), the map itself accepts anything.
        let mut options = HashMap::new();
        options.insert("arbitrary_key".to_string(), serde_yaml::Value::Bool(true));

        let config = FixModuleConfig { enabled: false, options };
        assert!(!config.enabled);
        assert!(config.options.contains_key("arbitrary_key"));
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::NotFound("test.yaml".to_string());
        assert!(err.to_string().contains("test.yaml"));

        let err = ConfigError::Parse(serde_yaml::from_str::<AppConfig>("invalid").unwrap_err());
        assert!(err.to_string().contains("parse"));

        let err = ConfigError::Validation("invalid URL".to_string());
        assert!(err.to_string().contains("invalid URL"));
    }

    #[test]
    fn test_server_config() {
        let config = ServerConfig {
            port: 8066,
            host: "0.0.0.0".to_string(),
            max_concurrent_requests: default_max_concurrent(),
            allowed_origins: None,
        };
        assert_eq!(config.port, 8066);
        assert_eq!(config.host, "0.0.0.0");
        // Unlimited by default - an absent config key must not start rejecting traffic.
        assert_eq!(config.max_concurrent_requests, 0);
    }

    #[test]
    fn test_stats_config() {
        let config = StatsConfig {
            enabled: true,
            format: StatsFormat::Json,
        };
        assert!(config.enabled);
        assert!(matches!(config.format, StatsFormat::Json));
    }

    #[test]
    fn test_influxdb_config() {
        let config = InfluxDbConfig {
            enabled: true,
            url: "http://localhost:8086".to_string(),
            org: "my-org".to_string(),
            bucket: "metrics".to_string(),
            token: "secret".to_string(),
        };
        assert!(config.enabled);
        assert_eq!(config.url, "http://localhost:8086");
    }

    #[test]
    fn test_streaming_mode_default() {
        let mode = StreamingMode::default();
        assert_eq!(mode, StreamingMode::Fake);
        assert!(mode.is_fake());
        assert!(!mode.is_disabled());
        assert!(!mode.is_passthrough());
    }

    #[test]
    fn test_streaming_mode_disabled() {
        let mode = StreamingMode::Disabled;
        assert!(mode.is_disabled());
        assert!(!mode.is_fake());
        assert!(!mode.is_passthrough());
        // `disabled` has no distinct behavior yet; it must not claim one.
        assert!(mode.effective_label().starts_with("backend JSON"));
    }

    #[test]
    fn test_streaming_mode_fake() {
        let mode = StreamingMode::Fake;
        assert!(mode.is_fake());
        assert!(!mode.is_disabled());
        assert!(!mode.is_passthrough());
    }

    #[test]
    fn test_streaming_mode_passthrough() {
        let mode = StreamingMode::Passthrough;
        assert!(mode.is_passthrough());
        assert!(!mode.is_disabled());
        assert!(!mode.is_fake());
    }

    #[test]
    fn test_effective_label_names_real_behavior() {
        assert!(StreamingMode::Passthrough.effective_label().contains("verbatim"));
        assert!(StreamingMode::Fake.effective_label().contains("synthesized"));
        // The OpenAI-path-only caveat must be visible in the banner text.
        assert!(StreamingMode::Passthrough.effective_label().contains("OpenAI"));
    }

    #[test]
    fn test_streaming_mode_serde() {
        // Test serialization
        let disabled = StreamingMode::Disabled;
        let fake = StreamingMode::Fake;
        let passthrough = StreamingMode::Passthrough;

        assert_eq!(serde_json::to_string(&disabled).unwrap(), "\"disabled\"");
        assert_eq!(serde_json::to_string(&fake).unwrap(), "\"fake\"");
        assert_eq!(serde_json::to_string(&passthrough).unwrap(), "\"passthrough\"");

        // Test deserialization
        let disabled: StreamingMode = serde_json::from_str("\"disabled\"").unwrap();
        let fake: StreamingMode = serde_json::from_str("\"fake\"").unwrap();
        let passthrough: StreamingMode = serde_json::from_str("\"passthrough\"").unwrap();

        assert_eq!(disabled, StreamingMode::Disabled);
        assert_eq!(fake, StreamingMode::Fake);
        assert_eq!(passthrough, StreamingMode::Passthrough);
    }

    #[test]
    fn test_streaming_mode_from_str_mode() {
        assert_eq!(StreamingMode::from_str_mode("disabled"), Some(StreamingMode::Disabled));
        assert_eq!(StreamingMode::from_str_mode("FAKE"), Some(StreamingMode::Fake));
        assert_eq!(StreamingMode::from_str_mode("Passthrough"), Some(StreamingMode::Passthrough));
        assert_eq!(
            StreamingMode::from_str_mode("accumulator"),
            None,
            "the legacy alias is deleted (D2)"
        );
        assert_eq!(StreamingMode::from_str_mode("bogus"), None);
    }

    #[test]
    fn test_streaming_mode_resolve_precedence() {
        // CLI switch wins over config
        assert_eq!(
            StreamingMode::resolve(Some("disabled"), StreamingMode::Fake).unwrap(),
            StreamingMode::Disabled
        );
        assert_eq!(
            StreamingMode::resolve(Some("passthrough"), StreamingMode::Disabled).unwrap(),
            StreamingMode::Passthrough
        );
        // Config wins over default when no switch
        assert_eq!(
            StreamingMode::resolve(None, StreamingMode::Disabled).unwrap(),
            StreamingMode::Disabled
        );
        // Default when neither set (config already carries the serde default)
        assert_eq!(
            StreamingMode::resolve(None, StreamingMode::default()).unwrap(),
            StreamingMode::Fake
        );
        // Unknown switch value is an error, never silent
        assert!(StreamingMode::resolve(Some("bogus"), StreamingMode::Fake).is_err());
    }

    #[test]
    fn test_backend_group_config_parsing() {
        let yaml = r#"
mappings:
  - "haiku"
  - "claude-haiku-4-5-20251001"
strategy: "priority_free"
nodes:
  - url: "http://localhost:8080"
    timeout_seconds: 300
  - url: "http://localhost:8081"
"#;
        let group: BackendGroupConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(group.mappings, vec!["haiku", "claude-haiku-4-5-20251001"]);
        assert_eq!(group.strategy, "priority_free");
        assert_eq!(group.nodes.len(), 2);
        assert_eq!(group.nodes[0].url, "http://localhost:8080");
        assert_eq!(group.nodes[1].timeout_seconds, 300); // default
    }

    #[test]
    fn test_backend_group_config_catch_all() {
        let yaml = r#"
mappings: []
strategy: "round_robin"
nodes:
  - url: "http://localhost:8080"
"#;
        let group: BackendGroupConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(group.mappings.is_empty());
        assert_eq!(group.strategy, "round_robin");
    }

    #[test]
    fn test_backend_group_config_default_strategy() {
        let yaml = r#"
mappings:
  - "opus"
nodes:
  - url: "http://localhost:8080"
"#;
        let group: BackendGroupConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(group.strategy, "round_robin"); // default
    }

    #[test]
    fn test_backend_group_config_exclusive_defaults_false() {
        let yaml = r#"
mappings:
  - "opus"
nodes:
  - url: "http://localhost:8080"
"#;
        let group: BackendGroupConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            !group.exclusive,
            "absent exclusive key defaults false — every group keeps f414d16 pre-plumbing fallback eligibility"
        );
    }

    #[test]
    fn test_backend_group_config_exclusive_true_loads() {
        // The task-70 deny_unknown_fields gate rejected this key outright
        // (unknown field `exclusive`) until the 87-follow landed the field.
        let yaml = r#"
mappings:
  - "opus"
exclusive: true
nodes:
  - url: "http://localhost:8080"
"#;
        let group: BackendGroupConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(group.exclusive, "exclusive: true must reach the group struct");
    }

    #[test]
    fn test_backend_group_config_exclusive_string_rejected_by_typed_bool() {
        // new_input_parsing gate: the field is a typed bool, not a lenient
        // stringly flag — `exclusive: "true"` must fail with the serde typed
        // error, never coerce.
        let yaml = r#"
mappings: []
exclusive: "true"
nodes: []
"#;
        let err = serde_yaml::from_str::<BackendGroupConfig>(yaml).expect_err("string 'true' must not coerce into bool");
        let msg = err.to_string();
        assert!(
            msg.contains("invalid type: string \"true\"") && msg.contains("boolean"),
            "error must be the honest typed-bool rejection, got: {msg}"
        );
    }

    #[test]
    fn test_no_matching_backend_display() {
        let err = NoMatchingBackend {
            requested_model: Some("gpt-4".to_string()),
        };
        assert_eq!(err.to_string(), "No backend configured for model: gpt-4");

        let err = NoMatchingBackend { requested_model: None };
        assert_eq!(err.to_string(), "No backend configured (no model specified and no catch-all)");
    }

    #[test]
    fn test_backends_config_hashmap() {
        let yaml = r#"
opus:
  mappings: ["opus", "opus4.5"]
  strategy: priority_free
  nodes:
    - url: "http://cosmo.lan:8700"
haiku:
  mappings: ["haiku"]
  strategy: round_robin
  nodes:
    - url: "http://cosmo.lan:8701"
    - url: "http://foobar.lan:1234"
catch_all:
  mappings: []
  strategy: priority_free
  nodes:
    - url: "http://example.com:8222"
"#;
        let backends: BackendsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(backends.len(), 3);
        assert!(backends.contains_key("opus"));
        assert!(backends.contains_key("haiku"));
        assert!(backends.contains_key("catch_all"));

        // Verify catch-all has empty mappings
        assert!(backends.get("catch_all").unwrap().mappings.is_empty());

        // Verify opus has 1 node
        assert_eq!(backends.get("opus").unwrap().nodes.len(), 1);

        // Verify haiku has 2 nodes
        assert_eq!(backends.get("haiku").unwrap().nodes.len(), 2);
    }

    #[test]
    fn test_backend_node_config_no_mapping_field() {
        // Test that BackendNodeConfig no longer has mapping field
        let yaml = r#"
url: "http://localhost:8080"
timeout_seconds: 300
"#;
        let node: BackendNodeConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(node.url, "http://localhost:8080");
        assert_eq!(node.timeout_seconds, 300);
        // mapping field no longer exists
    }

    #[test]
    fn test_failure_cooldown_secs_defaults_to_30_when_absent() {
        let yaml = r#"
main:
  mappings: []
  nodes:
    - url: "http://localhost:8080"
"#;
        let backends: BackendsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            backends.get("main").unwrap().failure_cooldown_secs,
            30,
            "an absent failure_cooldown_secs must default to a 30s cooldown"
        );
    }

    #[test]
    fn test_failure_cooldown_secs_zero_is_kept_as_zero() {
        let yaml = r#"
main:
  mappings: []
  failure_cooldown_secs: 0
  nodes:
    - url: "http://localhost:8080"
"#;
        let backends: BackendsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            backends.get("main").unwrap().failure_cooldown_secs,
            0,
            "0 disables cooldown and must not be replaced by the default"
        );
    }

    #[test]
    fn test_failure_cooldown_secs_explicit_value_is_kept() {
        let yaml = r#"
main:
  mappings: []
  failure_cooldown_secs: 90
  nodes:
    - url: "http://localhost:8080"
"#;
        let backends: BackendsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(backends.get("main").unwrap().failure_cooldown_secs, 90);
    }

    #[test]
    fn test_augment_backend_absent_enabled_deserializes_disabled() {
        let yaml = r#"
url: "http://localhost:8701"
model: "fast-model"
"#;
        let cfg: AugmentBackendConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            !cfg.enabled,
            "F-M13: augment-backend is opt-in - absent `enabled` must deserialize to false"
        );
    }

    #[test]
    fn test_augment_backend_default_impl_matches_opt_in() {
        assert!(
            !AugmentBackendConfig::default().enabled,
            "the Default impl must not re-introduce the old default-true"
        );
    }

    #[test]
    fn test_augment_backend_default_impl_timeout_is_15() {
        assert_eq!(AugmentBackendConfig::default().timeout_secs, 15);
    }

    #[test]
    fn test_augment_backend_timeout_secs_absent_defaults_15() {
        let yaml = r#"
url: "http://localhost:8701"
model: "fast-model"
"#;
        let cfg: AugmentBackendConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            cfg.timeout_secs, 15,
            "F-L8: absent timeout_secs must deserialize to 15, not the old hard-coded 60"
        );
    }

    #[test]
    fn test_augment_backend_timeout_secs_explicit_is_honored() {
        let yaml = r#"
url: "http://localhost:8701"
model: "fast-model"
timeout_secs: 42
"#;
        let cfg: AugmentBackendConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.timeout_secs, 42);
    }
}

#[cfg(test)]
mod synthesis_section_tests {
    use super::*;

    #[test]
    fn synthesis_section_absent_yields_defaults() {
        let yaml = "server:\n  host: 0.0.0.0\n  port: 8066\nbackend:\n  url: http://localhost:8080\n";
        let cfg: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.synthesis, SynthesisConfig::default());
        assert_eq!(cfg.synthesis.chunk_delay_ms, 0);
        assert_eq!(cfg.synthesis.chunk_size_chars, 2000);
    }

    #[test]
    fn synthesis_section_parses_both_keys() {
        let yaml = "server:\n  host: 0.0.0.0\n  port: 8066\nbackend:\n  url: http://localhost:8080\nstreaming: fake\nsynthesis:\n  chunk_delay_ms: 25\n  chunk_size_chars: 512\n";
        let cfg: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.synthesis.chunk_delay_ms, 25);
        assert_eq!(cfg.synthesis.chunk_size_chars, 512);
        assert_eq!(cfg.streaming, StreamingMode::Fake);
    }

    #[test]
    fn synthesis_section_partial_keys_default_the_missing_one() {
        let yaml = "server:\n  host: 0.0.0.0\n  port: 8066\nbackend:\n  url: http://localhost:8080\nsynthesis:\n  chunk_delay_ms: 10\n";
        let cfg: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.synthesis.chunk_delay_ms, 10);
        assert_eq!(cfg.synthesis.chunk_size_chars, 2000);
    }
}

#[cfg(test)]
mod deny_unknown_fields_tests {
    use super::*;

    fn rejected<T: std::fmt::Debug + serde::de::DeserializeOwned>(yaml: &str, bogus_key: &str) {
        let err = serde_yaml::from_str::<T>(yaml).expect_err("typo key must be rejected");
        assert!(
            err.to_string().contains(bogus_key),
            "error must name the bogus key {bogus_key}, got: {err}"
        );
    }

    #[test]
    fn appconfig_rejects_bogus_app_key() {
        rejected::<AppConfig>(
            "server:\n  port: 8066\n  host: \"0.0.0.0\"\nbogus_app_key: 1\n",
            "bogus_app_key",
        );
    }

    #[test]
    fn serverconfig_rejects_bogus_server_key() {
        rejected::<ServerConfig>("port: 8066\nhost: \"0.0.0.0\"\nbogus_server_key: 1\n", "bogus_server_key");
    }

    #[test]
    fn backendconfig_rejects_bogus_backend_key() {
        rejected::<BackendConfig>("url: \"http://localhost:8080\"\nbogus_backend_key: 1\n", "bogus_backend_key");
    }

    #[test]
    fn backendnodeconfig_rejects_bogus_node_key() {
        rejected::<BackendNodeConfig>("url: \"http://localhost:8080\"\nbogus_node_key: 1\n", "bogus_node_key");
    }

    #[test]
    fn backendgroupconfig_rejects_bogus_group_key() {
        rejected::<BackendGroupConfig>("mappings: []\nnodes: []\nbogus_group_key: 1\n", "bogus_group_key");
    }

    #[test]
    fn statsconfig_rejects_bogus_stats_key() {
        rejected::<StatsConfig>("enabled: true\nbogus_stats_key: 1\n", "bogus_stats_key");
    }

    #[test]
    fn dumpconfig_rejects_bogus_dump_key() {
        rejected::<DumpConfig>("enabled: false\nbogus_dump_key: 1\n", "bogus_dump_key");
    }

    #[test]
    fn synthesisconfig_rejects_bogus_synthesis_key() {
        rejected::<SynthesisConfig>("chunk_delay_ms: 0\nbogus_synthesis_key: 1\n", "bogus_synthesis_key");
    }

    #[test]
    fn influxdbconfig_rejects_bogus_influx_key() {
        rejected::<InfluxDbConfig>("enabled: false\nbogus_influx_key: 1\n", "bogus_influx_key");
    }

    #[test]
    fn repromptconfig_rejects_bogus_reprompt_key() {
        rejected::<RepromptConfig>("enabled: false\nbogus_reprompt_key: 1\n", "bogus_reprompt_key");
    }

    #[test]
    fn augmentbackendconfig_rejects_bogus_augment_key() {
        rejected::<AugmentBackendConfig>(
            "url: \"http://localhost:8701\"\nmodel: \"fast\"\nbogus_augment_key: 1\n",
            "bogus_augment_key",
        );
    }

    #[test]
    fn tlsconfig_rejects_bogus_tls_key() {
        rejected::<TlsConfig>("accept_invalid_certs: false\nbogus_tls_key: 1\n", "bogus_tls_key");
    }

    #[test]
    fn shipped_config_yaml_default_still_parses_after_deny_sweep() {
        let _: AppConfig = serde_yaml::from_str(include_str!("../../config.yaml.default"))
            .expect("config.yaml.default must parse under deny_unknown_fields");
    }

    #[test]
    fn alias_key_still_accepted_under_deny() {
        // deny_unknown_fields must not kill serde aliases: reprompt's legacy
        // done_sentinel key is an alias, not an unknown field.
        let cfg: RepromptConfig =
            serde_yaml::from_str("enabled: true\nprompt: \"x\"\ndone_sentinel: \"FINISH\"\n").expect("alias key must load");
        assert_eq!(cfg.done_sentinels, vec!["FINISH".to_string()]);
    }
}

#[cfg(test)]
mod dead_surface_tests {
    use super::*;

    #[test]
    fn fix_module_without_enabled_key_defaults_enabled() {
        let cfg: FixModuleConfig = serde_yaml::from_str("{}").expect("bare module must parse");
        assert!(cfg.enabled, "a bare module entry must mean enabled");
    }

    #[test]
    fn deleted_accumulator_alias_is_now_unknown() {
        let err = serde_yaml::from_str::<StreamingMode>("accumulator")
            .expect_err("the legacy accumulator spelling must be gone (D2)");
        assert!(err.to_string().contains("accumulator"), "got {err}");
    }
}
