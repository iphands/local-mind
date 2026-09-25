//! A dedicated proxy process plus its own mock backend, on ports nothing else in the
//! run touches.
//!
//! The harness boots ONE proxy for the whole run (`main.rs::do_spawn_and_run`) and every
//! test shares its `http://127.0.0.1:18080`. That is not a neutral container: the proxy
//! memoises what a backend advertised in process-global maps keyed by backend base_url
//! (`CONTEXT_CACHE` in `src/proxy/context.rs`, `KV_INFO` in `src/proxy/compat.rs`), so
//! whichever test first resolves a window for that key decides what every later test on
//! the same key is told. A test that needs a cold key, or a backend advertising something
//! else, cannot ask that process for one — it needs its own proxy and its own base_url.
//!
//! [`spawn_configured`] enforces the one ordering that matters: the mock is bound and
//! configured BEFORE the proxy starts, so the proxy's startup preflight probes the
//! fixture under test rather than a default.

/// A running dedicated proxy, its mock's shared state, and the addresses to reach them.
pub struct IsolatedProxy {
    /// `host:port` the dedicated proxy listens on.
    pub proxy_addr: String,
    /// The base_url the dedicated proxy forwards to — and therefore the cache key it uses.
    pub backend_url: String,
    /// The dedicated mock's state, for installing fixtures and reading recorded requests.
    pub mock: crate::types::SharedBackendState,
    child: tokio::process::Child,
}

/// Start a mock on `backend_port`, hand it to `configure`, then start a proxy pointed at
/// it on `proxy_port` and wait for readiness.
///
/// The proxy is killed when the returned guard is dropped (`kill_on_drop`), so the ports
/// are released without every early-return path having to remember it.
pub async fn spawn_configured(
    proxy_port: u16,
    backend_port: u16,
    configure: impl FnOnce(&crate::types::SharedBackendState),
) -> anyhow::Result<IsolatedProxy> {
    let mock = crate::backend::start(backend_port).await?;
    configure(&mock);

    let backend_url = format!("http://127.0.0.1:{backend_port}");
    let proxy_addr = format!("127.0.0.1:{proxy_port}");
    let proxy_bin = crate::find_proxy_bin()?;
    println!("    isolation   : dedicated proxy {proxy_bin} --port {proxy_port} --backend-url {backend_url}");
    println!("    isolation   : CONTEXT_CACHE/KV_INFO are keyed by base_url, so {backend_url} is a cold key");

    let child = tokio::process::Command::new(&proxy_bin)
        .arg("run")
        .arg("--config")
        .arg(crate::DEFAULT_PROXY_CONFIG)
        .arg("--port")
        .arg(proxy_port.to_string())
        .arg("--backend-url")
        .arg(&backend_url)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn dedicated proxy {proxy_bin}: {e}"))?;

    let proxy = IsolatedProxy {
        proxy_addr,
        backend_url,
        mock,
        child,
    };
    wait_until_ready(&proxy.proxy_addr).await?;
    Ok(proxy)
}

/// Poll the proxy's own `/health` (not the backend's), so readiness does not depend on
/// the fixture answering at all.
async fn wait_until_ready(addr: &str) -> anyhow::Result<()> {
    let client = crate::client::build_client();
    for attempt in 0..30u64 {
        tokio::time::sleep(std::time::Duration::from_millis(200 + attempt * 100)).await;
        if client.get(format!("http://{addr}/health")).send().await.is_ok() {
            return Ok(());
        }
    }
    Err(anyhow::anyhow!(
        "dedicated proxy at {addr} never started accepting connections"
    ))
}

impl Drop for IsolatedProxy {
    /// Requests the kill as soon as the test's guard goes out of scope, so a failing
    /// `?` releases the port just as reliably as a passing run.
    fn drop(&mut self) {
        self.child.start_kill().ok();
    }
}
