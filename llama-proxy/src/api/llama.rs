//! llama.cpp-specific API types

use serde::{Deserialize, Serialize};

/// Slot information from /slots endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlotInfo {
    pub id: u32,
    pub model: Option<String>,
    pub n_ctx: u64,
    pub n_tokens: u64,
    pub is_processing: bool,
    #[serde(default)]
    pub params: Option<SlotParams>,
}

/// Slot parameters
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlotParams {
    pub prompt: Option<String>,
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub n_predict: Option<i32>,
}

/// Server properties from /props endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerProps {
    pub model_path: Option<String>,
    pub n_ctx: Option<u64>,
    pub n_batch: Option<u32>,
    pub n_ubatch: Option<u32>,
    pub flash_attn: Option<bool>,
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    pub n_gpu_layers: Option<i32>,
    pub main_gpu: Option<u32>,
    pub total_slots: Option<u32>,
    pub chat_template: Option<String>,
    pub default_generation_settings: Option<GenerationSettings>,
    pub build_info: Option<BuildInfo>,
}

/// Generation settings
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GenerationSettings {
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    pub n_predict: Option<i32>,
    pub repeat_last_n: Option<i32>,
    pub repeat_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub mirostat: Option<u32>,
    pub mirostat_tau: Option<f32>,
    pub mirostat_eta: Option<f32>,
}

/// Build information
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildInfo {
    pub build_number: Option<u32>,
    pub commit: Option<String>,
    pub compiler: Option<String>,
    pub target: Option<String>,
}

/// Health status
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HealthStatus {
    pub status: String,
}

/// Models list response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelsResponse {
    pub data: Vec<ModelInfo>,
}

/// Model information
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}
