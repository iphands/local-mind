//! Pure config predicates for the loader's validation sweep (task 71).
//!
//! This is the SINGLE validation home for config shape: the CLI never
//! re-checks what passed here (task 72 validates only what CLI overrides
//! can change), and consumers downstream trust these values.

use super::ConfigError;

/// Validate a bind host: an IP literal (IPv4 or IPv6, bracket-tolerant for
/// the IPv6 form operators copy from URLs, e.g. `[::1]`) or an RFC-1123
/// hostname. Rejects empty and whitespace-containing junk with a message
/// that names the bad value.
pub(crate) fn validate_bind_host(host: &str) -> Result<(), ConfigError> {
    let stripped = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    if stripped.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    if is_rfc1123_hostname(stripped) {
        return Ok(());
    }
    Err(ConfigError::Validation(format!(
        "server host '{host}' is neither an IP address nor a valid RFC-1123 hostname"
    )))
}

fn is_rfc1123_hostname(host: &str) -> bool {
    let trimmed = host.strip_suffix('.').unwrap_or(host);
    if trimmed.is_empty() || trimmed.len() > 255 {
        return false;
    }
    trimmed.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.starts_with(|c: char| c.is_ascii_alphanumeric())
            && label.ends_with(|c: char| c.is_ascii_alphanumeric())
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// Validate an http(s) URL with a host, for `label`-named config fields.
pub fn validate_http_url(url: &str, label: &str) -> Result<(), ConfigError> {
    let parsed = url::Url::parse(url).map_err(|e| ConfigError::Validation(format!("Invalid {label} URL '{url}': {e}")))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(ConfigError::Validation(format!(
            "{label} URL must use http:// or https://, got '{scheme}'"
        )));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(ConfigError::Validation(format!("{label} URL must include a host: '{url}'")));
    }
    Ok(())
}

/// Per-node temperature override: finite and within `0..=2`.
/// Rejects NaN/inf (YAML `.nan`/`.inf` parse into real f64 specials).
pub(crate) fn validate_temperature(t: f64, where_: &str) -> Result<(), ConfigError> {
    if !t.is_finite() {
        return Err(ConfigError::Validation(format!(
            "{where_} temperature {t} is not a finite number"
        )));
    }
    if !(0.0..=2.0).contains(&t) {
        return Err(ConfigError::Validation(format!(
            "{where_} temperature {t} is outside the valid range 0..=2"
        )));
    }
    Ok(())
}

/// Reject empty-string sentinels where a non-empty value is required.
pub(crate) fn require_nonempty(value: &str, what: &str) -> Result<(), ConfigError> {
    if value.is_empty() {
        return Err(ConfigError::Validation(format!("{what} must not be empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_host_pin_table() {
        let accepted = [
            "0.0.0.0",
            "127.0.0.1",
            "::1",
            "[::1]",
            "[::ffff:127.0.0.1]",
            "[127.0.0.1]",
            "localhost",
            "example.com",
            "example.com.",
            "cosmo.lan",
            "xn--bcher-kva.example",
            "a.com",
        ];
        for h in accepted {
            assert!(validate_bind_host(h).is_ok(), "{h} must be accepted");
        }
        let rejected = [
            "",
            "not a host!!",
            "-bad.com",
            "bad-.com",
            "under_score.com",
            "dots..inside",
            "[::1",
            "::1]",
        ];
        for h in rejected {
            assert!(validate_bind_host(h).is_err(), "{h} must be rejected");
        }
    }

    #[test]
    fn bind_host_rfc1123_edge_lengths() {
        let label63 = "a".repeat(63);
        assert!(validate_bind_host(&label63).is_ok(), "63-char label must pass");
        assert!(validate_bind_host(&"a".repeat(64)).is_err(), "64-char label must fail");
        let total255 = format!("{label63}.{label63}.{label63}.{}", "a".repeat(63));
        assert_eq!(total255.len(), 255);
        assert!(validate_bind_host(&total255).is_ok(), "255-char total must pass");
        let total256 = format!("{label63}.{label63}.{label63}.{}", "a".repeat(64));
        assert_eq!(total256.len(), 256);
        assert!(validate_bind_host(&total256).is_err(), "256-char total must fail");
    }

    #[test]
    fn http_url_accepts_and_rejects() {
        assert!(validate_http_url("http://localhost:8080", "backend").is_ok());
        assert!(validate_http_url("https://example.com:4234", "backend").is_ok());
        assert!(validate_http_url("http://192.168.1.100:9000", "backend").is_ok());
        assert!(validate_http_url("not-a-url", "backend").is_err());
        assert!(validate_http_url("ftp://example.com", "backend").is_err());
        assert!(validate_http_url("http://", "backend").is_err());
        assert!(validate_http_url("", "backend").is_err());
    }

    #[test]
    fn temperature_pin_table() {
        for ok in [0.0f64, -0.0, 1.0, 2.0] {
            assert!(validate_temperature(ok, "node").is_ok(), "{ok} must be accepted");
        }
        for bad in [-0.001f64, 2.0001, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(validate_temperature(bad, "node").is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn require_nonempty_rejects_only_empty() {
        assert!(require_nonempty("ok", "field").is_ok());
        assert!(require_nonempty("", "field").is_err());
    }
}
