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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_info() {
        let json = r#"{
            "id": 0,
            "model": "llama-3",
            "n_ctx": 4096,
            "n_tokens": 100,
            "is_processing": true
        }"#;

        let slot: SlotInfo = serde_json::from_str(json).unwrap();
        assert_eq!(slot.id, 0);
        assert_eq!(slot.model, Some("llama-3".to_string()));
        assert_eq!(slot.n_ctx, 4096);
        assert!(slot.is_processing);
    }

    #[test]
    fn test_slot_info_with_params() {
        let json = r#"{
            "id": 0,
            "n_ctx": 4096,
            "n_tokens": 100,
            "is_processing": false,
            "params": {
                "temperature": 0.7,
                "top_k": 40,
                "top_p": 0.9
            }
        }"#;

        let slot: SlotInfo = serde_json::from_str(json).unwrap();
        assert!(slot.params.is_some());
        let params = slot.params.unwrap();
        assert_eq!(params.temperature, Some(0.7));
        assert_eq!(params.top_k, Some(40));
    }

    #[test]
    fn test_slot_params() {
        let params = SlotParams {
            prompt: Some("Hello".to_string()),
            temperature: Some(0.8),
            top_k: Some(50),
            top_p: Some(0.95),
            n_predict: Some(100),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: SlotParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.temperature, Some(0.8));
        assert_eq!(parsed.n_predict, Some(100));
    }

    #[test]
    fn test_server_props() {
        let json = r#"{
            "model_path": "/models/llama.gguf",
            "n_ctx": 4096,
            "n_batch": 512,
            "total_slots": 1,
            "chat_template": "chatml"
        }"#;

        let props: ServerProps = serde_json::from_str(json).unwrap();
        assert_eq!(props.model_path, Some("/models/llama.gguf".to_string()));
        assert_eq!(props.n_ctx, Some(4096));
        assert_eq!(props.total_slots, Some(1));
    }

    #[test]
    fn test_generation_settings() {
        let settings = GenerationSettings {
            temperature: Some(0.7),
            top_k: Some(40),
            top_p: Some(0.9),
            min_p: Some(0.1),
            n_predict: Some(-1),
            repeat_last_n: Some(64),
            repeat_penalty: Some(1.1),
            presence_penalty: Some(0.0),
            frequency_penalty: Some(0.0),
            mirostat: Some(0),
            mirostat_tau: Some(5.0),
            mirostat_eta: Some(0.1),
        };

        let json = serde_json::to_string(&settings).unwrap();
        let parsed: GenerationSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.temperature, Some(0.7));
        assert_eq!(parsed.mirostat_tau, Some(5.0));
    }

    #[test]
    fn test_build_info() {
        let json = r#"{
            "build_number": 1234,
            "commit": "abc123",
            "compiler": "gcc",
            "target": "x86_64-linux"
        }"#;

        let info: BuildInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.build_number, Some(1234));
        assert_eq!(info.commit, Some("abc123".to_string()));
    }

    #[test]
    fn test_health_status() {
        let json = r#"{"status": "ok"}"#;
        let health: HealthStatus = serde_json::from_str(json).unwrap();
        assert_eq!(health.status, "ok");
    }

    #[test]
    fn test_health_status_error() {
        let json = r#"{"status": "error"}"#;
        let health: HealthStatus = serde_json::from_str(json).unwrap();
        assert_eq!(health.status, "error");
    }

    #[test]
    fn test_models_response() {
        let json = r#"{
            "data": [
                {
                    "id": "llama-3-8b",
                    "object": "model",
                    "created": 1234567890,
                    "owned_by": "meta"
                }
            ]
        }"#;

        let models: ModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(models.data.len(), 1);
        assert_eq!(models.data[0].id, "llama-3-8b");
        assert_eq!(models.data[0].object, "model");
    }

    #[test]
    fn test_model_info() {
        let model = ModelInfo {
            id: "test-model".to_string(),
            object: "model".to_string(),
            created: Some(1234567890),
            owned_by: Some("test".to_string()),
        };

        let json = serde_json::to_string(&model).unwrap();
        let parsed: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-model");
        assert_eq!(parsed.owned_by, Some("test".to_string()));
    }

    #[test]
    fn test_empty_models_response() {
        let json = r#"{"data": []}"#;
        let models: ModelsResponse = serde_json::from_str(json).unwrap();
        assert!(models.data.is_empty());
    }

    #[test]
    fn test_server_props_partial() {
        let json = r#"{"n_ctx": 2048}"#;
        let props: ServerProps = serde_json::from_str(json).unwrap();
        assert_eq!(props.n_ctx, Some(2048));
        assert!(props.model_path.is_none());
    }

    #[test]
    fn test_slot_info_minimal() {
        let json = r#"{
            "id": 1,
            "n_ctx": 0,
            "n_tokens": 0,
            "is_processing": false
        }"#;

        let slot: SlotInfo = serde_json::from_str(json).unwrap();
        assert_eq!(slot.id, 1);
        assert!(slot.model.is_none());
        assert!(!slot.is_processing);
    }
}
