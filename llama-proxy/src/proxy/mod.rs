//! HTTP proxy server

mod compat;
mod context;
mod handler;
mod kv_labels;
pub mod reprompt;
pub mod server;
mod streaming;
mod synthesis;
#[cfg(test)]
pub(crate) mod test_support;

pub use context::{cache_context_from_preflight, fetch_context_total, warn_context_fetch_failed_once};
pub use handler::ProxyHandler;
pub use server::{run_server, ProxyState};
pub use synthesis::{
    synthesize_anthropic_openai_format_response, synthesize_anthropic_streaming_response, synthesize_streaming_response,
};
