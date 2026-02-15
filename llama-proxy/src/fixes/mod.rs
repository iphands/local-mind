//! Response fix modules for correcting malformed LLM responses

mod registry;
mod toolcall_bad_filepath_fix;

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub use registry::{AsAny, FixRegistry};
pub use toolcall_bad_filepath_fix::ToolcallBadFilepathFix;

/// Trait for response fix modules
#[async_trait]
pub trait ResponseFix: Send + Sync {
    /// Unique identifier for the fix
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Check if this fix applies to the response
    fn applies(&self, response: &Value) -> bool;

    /// Apply the fix to the response (non-streaming)
    fn apply(&self, response: Value) -> Value;

    /// Apply fix to a streaming chunk (optional)
    fn apply_stream(&self, chunk: Value) -> Value {
        chunk // Default: pass through unchanged
    }
}

/// Create the default fix registry with all available fixes
pub fn create_default_registry() -> FixRegistry {
    let mut registry = FixRegistry::new();
    registry.register(Arc::new(ToolcallBadFilepathFix::new(true)));
    registry
}
