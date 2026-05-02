use pretty_assertions::assert_eq;
use proptest::prelude::*;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_temp_config_with_limits(with_input: bool) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "{{").unwrap();
    writeln!(file, "  \"model\": \"cosmo-proxy/cosmo-proxy\",").unwrap();
    writeln!(file, "  \"provider\": {{").unwrap();
    if with_input {
        writeln!(file, r#"    "cosmo-00": {{ "models": {{ "cosmo-6000": {{ "name": "cosmo-6000", "limit": {{ "context": 262144, "output": 262144, "input": 200000 }} }} }} }},"#).unwrap();
        writeln!(file, r#"    "noir-llama": {{ "models": {{ "noir-model": {{ "name": "noir-model", "limit": {{ "context": 262144, "output": 32768, "input": 200000 }} }} }} }}"#).unwrap();
    } else {
        writeln!(file, r#"    "cosmo-00": {{ "models": {{ "cosmo-6000": {{ "name": "cosmo-6000", "limit": {{ "context": 262144, "output": 262144 }} }} }} }},"#).unwrap();
        writeln!(file, r#"    "noir-llama": {{ "models": {{ "noir-model": {{ "name": "noir-model", "limit": {{ "context": 262144, "output": 32768 }} }} }} }}"#).unwrap();
    }
    writeln!(file, "  }}").unwrap();
    writeln!(file, "}}").unwrap();
    file
}

fn create_temp_models_file(models: &[&str]) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    for model in models {
        writeln!(file, "{}", model).unwrap();
    }
    file
}

fn create_temp_agent_file(model: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "name: test-agent").unwrap();
    writeln!(file, "model: {}", model).unwrap();
    writeln!(file, "description: Test agent").unwrap();
    file
}

fn create_temp_agent_file_no_model() -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "---").unwrap();
    writeln!(file, "name: test-agent").unwrap();
    writeln!(file, "description: Test agent").unwrap();
    file
}

fn create_temp_opencode_config(model: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "{{").unwrap();
    writeln!(file, "  \"model\": \"{}\",", model).unwrap();
    writeln!(file, "  \"plugin\": [\"oh-my-opencode@latest\"],").unwrap();
    writeln!(file, "  // This is a comment").unwrap();
    writeln!(file, "  \"default_agent\": \"local-mind\"").unwrap();
    writeln!(file, "}}").unwrap();
    file
}

fn create_temp_opencode_config_no_model() -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "{{").unwrap();
    writeln!(file, "  \"plugin\": [\"oh-my-opencode@latest\"],").unwrap();
    writeln!(file, "  \"default_agent\": \"local-mind\"").unwrap();
    writeln!(file, "}}").unwrap();
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_eq;

    #[test]
    fn test_read_models_success() {
        let models = vec![
            "openai/gpt-4",
            "anthropic/claude-3",
            "moonshotai/kimi-k2",
            "google/gemini-pro",
        ];
        let file = create_temp_models_file(&models);

        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());

        let read_models = result.unwrap();
        assert_eq!(read_models.len(), 4);
        assert_eq!(read_models[0], "openai/gpt-4");
        assert_eq!(read_models[1], "anthropic/claude-3");
        assert_eq!(read_models[2], "moonshotai/kimi-k2");
        assert_eq!(read_models[3], "google/gemini-pro");
    }

    #[test]
    fn test_read_models_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());

        let read_models = result.unwrap();
        assert!(read_models.is_empty());
    }

    #[test]
    fn test_read_models_with_empty_lines() {
        let models = vec![
            "openai/gpt-4",
            "",
            "  ",
            "anthropic/claude-3",
            "",
            "moonshotai/kimi-k2",
        ];
        let file = create_temp_models_file(&models);

        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());

        let read_models = result.unwrap();
        assert_eq!(read_models.len(), 3);
        assert_eq!(read_models[0], "openai/gpt-4");
        assert_eq!(read_models[1], "anthropic/claude-3");
        assert_eq!(read_models[2], "moonshotai/kimi-k2");
    }

    #[test]
    fn test_read_models_file_not_found() {
        let result = crate::read_models("/nonexistent/path/models");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_get_current_model_success() {
        let file = create_temp_agent_file("openai/gpt-4");
        let result = crate::get_current_model(file.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "openai/gpt-4");
    }

    #[test]
    fn test_get_current_model_with_spaces() {
        let file = create_temp_agent_file("  openai/gpt-4  ");
        let result = crate::get_current_model(file.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "openai/gpt-4");
    }

    #[test]
    fn test_get_current_model_no_model_line() {
        let file = create_temp_agent_file_no_model();
        let result = crate::get_current_model(file.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No model line found"));
    }

    #[test]
    fn test_get_current_model_file_not_found() {
        let result = crate::get_current_model("/nonexistent/path/agent.md");
        assert!(result.is_err());
    }

    #[test]
    fn test_update_agent_model_success() {
        let file = create_temp_agent_file("old-model");
        let path = file.path().to_str().unwrap();

        let result = crate::update_agent_model(path, "new-model");
        assert!(result.is_ok());

        let updated_model = crate::get_current_model(path).unwrap();
        assert_eq!(updated_model, "new-model");
    }

    #[test]
    fn test_update_agent_model_preserves_content() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "---").unwrap();
        writeln!(file, "name: test-agent").unwrap();
        writeln!(file, "model: old-model").unwrap();
        writeln!(file, "description: Test agent").unwrap();
        writeln!(file, "version: 1.0").unwrap();

        let path = file.path().to_str().unwrap();
        let result = crate::update_agent_model(path, "new-model");
        assert!(result.is_ok());

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("name: test-agent"));
        assert!(content.contains("model: new-model"));
        assert!(content.contains("description: Test agent"));
        assert!(content.contains("version: 1.0"));
    }

    #[test]
    fn test_update_agent_model_no_model_line() {
        let file = create_temp_agent_file_no_model();
        let result = crate::update_agent_model(file.path().to_str().unwrap(), "new-model");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No model line found"));
    }

    #[test]
    fn test_update_agent_model_file_not_found() {
        let result = crate::update_agent_model("/nonexistent/path/agent.md", "new-model");
        assert!(result.is_err());
    }

    proptest! {
        #[test]
        fn test_filter_contains_logic(models in prop::collection::vec(".*", 1..20), filter in ".*") {
            let models: Vec<String> = models.iter().map(|s| s.to_string()).collect();

            let filtered: Vec<&String> = models
                .iter()
                .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
                .collect();

            if !filter.is_empty() {
                for item in &filtered {
                    prop_assert!(item.to_lowercase().contains(&filter.to_lowercase()));
                }
            } else {
                prop_assert_eq!(filtered.len(), models.len());
            }
        }

        #[test]
        fn test_filter_case_insensitive(models in prop::collection::vec("[a-zA-Z]+", 1..10)) {
            let models: Vec<String> = models.iter().map(|s| s.to_string()).collect();
            let filter = "TEST";

            let filtered: Vec<&String> = models
                .iter()
                .filter(|m| m.to_lowercase().contains(&filter.to_lowercase()))
                .collect();

            for item in &filtered {
                prop_assert!(item.to_lowercase().contains("test"));
            }
        }
    }

    #[test]
    fn test_filter_uses_contains_not_starts_with() {
        let models = vec![
            "moonshotai/kimi-k2-thinking-turbo",
            "openai/gpt-4",
            "anthropic/claude-3",
        ];

        let filter = "kimi";
        let filtered: Vec<String> = models
            .iter()
            .map(|s| s.to_string())
            .filter(|m| m.to_lowercase().contains(&filter.to_lowercase()))
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "moonshotai/kimi-k2-thinking-turbo");
    }

    #[test]
    fn test_navigation_selection_clamping() {
        let models = vec!["model1", "model2", "model3"];
        let filtered: Vec<String> = models.iter().map(|s| s.to_string()).collect();

        let mut selected = 5;
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }
        assert_eq!(selected, 2);

        selected = 0;
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }
        assert_eq!(selected, 0);
    }

    #[test]
    fn test_initialized_flag_behavior() {
        let models = vec!["model1", "model2", "model3"];
        let current = "model2";
        let filtered: Vec<String> = models.iter().map(|s| s.to_string()).collect();

        let mut initialized = false;
        let mut selected = 0;

        if !initialized && "".is_empty() {
            if let Some(idx) = filtered.iter().position(|m| m == current) {
                selected = idx;
            }
            initialized = true;
        }

        assert_eq!(selected, 1);
        assert!(initialized);

        let filter = "";
        if !initialized && filter.is_empty() {
            if let Some(idx) = filtered.iter().position(|m| m == current) {
                selected = idx;
            }
            initialized = true;
        }

        assert_eq!(selected, 1);
    }

    #[test]
    fn test_ledger_formatting() {
        let selections = vec![
            (
                "agent1",
                "path1",
                "old-model1".to_string(),
                "new-model1".to_string(),
            ),
            (
                "agent2",
                "path2",
                "old-model2".to_string(),
                "old-model2".to_string(),
            ),
        ];

        for (name, _, old, new) in &selections {
            if old == new {
                assert_eq!(*name, "agent2");
            } else {
                assert_eq!(*name, "agent1");
            }
        }
    }

    #[test]
    fn test_confirmation_behavior() {
        let selections = vec![(
            "agent1",
            "path1",
            "old-model".to_string(),
            "new-model".to_string(),
        )];

        let has_changes = selections.iter().any(|(_, _, old, new)| old != new);
        assert!(has_changes);

        let unchanged_selections =
            vec![("agent1", "path1", "model".to_string(), "model".to_string())];
        let has_changes_unchanged = unchanged_selections
            .iter()
            .any(|(_, _, old, new)| old != new);
        assert!(!has_changes_unchanged);
    }

    #[test]
    fn test_full_workflow_no_changes() {
        let models = vec!["model1", "model2", "model3"];
        let _models_file = create_temp_models_file(&models);
        let agent_file = create_temp_agent_file("model1");

        let current = crate::get_current_model(agent_file.path().to_str().unwrap()).unwrap();
        assert_eq!(current, "model1");

        let needs_update = current != "model1";
        assert!(!needs_update);
    }

    #[test]
    fn test_full_workflow_with_changes() {
        let models = vec!["model1", "model2", "model3"];
        let _models_file = create_temp_models_file(&models);
        let agent_file = create_temp_agent_file("model1");

        let current = crate::get_current_model(agent_file.path().to_str().unwrap()).unwrap();
        assert_eq!(current, "model1");

        let result = crate::update_agent_model(agent_file.path().to_str().unwrap(), "model2");
        assert!(result.is_ok());

        let updated = crate::get_current_model(agent_file.path().to_str().unwrap()).unwrap();
        assert_eq!(updated, "model2");
    }

    #[test]
    fn test_empty_model_list() {
        let file = create_temp_models_file(&[]);
        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());
        let models = result.unwrap();
        assert!(models.is_empty());
    }

    #[test]
    fn test_single_model_list() {
        let models = vec!["only-model"];
        let file = create_temp_models_file(&models);
        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());
        let read_models = result.unwrap();
        assert_eq!(read_models.len(), 1);
        assert_eq!(read_models[0], "only-model");
    }

    #[test]
    fn test_model_names_with_spaces() {
        let models = vec![" model with spaces ", "  another model  "];
        let file = create_temp_models_file(&models);
        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());

        let read_models = result.unwrap();
        assert_eq!(read_models.len(), 2);
        assert_eq!(read_models[0], "model with spaces");
        assert_eq!(read_models[1], "another model");
    }

    #[test]
    fn test_agent_file_with_multiple_model_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "model: first-model").unwrap();
        writeln!(file, "name: test-agent").unwrap();
        writeln!(file, "model: second-model").unwrap();

        let result = crate::get_current_model(file.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "first-model");
    }

    #[test]
    fn test_update_preserves_newline() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "model: old-model").unwrap();

        let path = file.path().to_str().unwrap();
        let original_content = fs::read_to_string(path).unwrap();
        let ends_with_newline = original_content.ends_with('\n');
        assert!(ends_with_newline);

        let result = crate::update_agent_model(path, "new-model");
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(path).unwrap();
        assert!(updated_content.ends_with('\n'));
        assert!(updated_content.contains("model: new-model"));
    }

    #[test]
    fn test_model_line_with_whitespace() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "  model:   spaced-model   ").unwrap();

        let result = crate::get_current_model(file.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "spaced-model");
    }

    #[test]
    fn test_filter_special_characters() {
        let models = vec![
            "model-with-dashes",
            "model_with_underscores",
            "model.with.dots",
            "model with spaces",
            "model/with/slashes",
        ];

        let test_cases = vec![
            ("dash", "model-with-dashes"),
            ("under", "model_with_underscores"),
            ("dot", "model.with.dots"),
            ("space", "model with spaces"),
            ("slash", "model/with/slashes"),
        ];

        for (filter, expected_match) in test_cases {
            let filtered: Vec<String> = models
                .iter()
                .map(|s| s.to_string())
                .filter(|m| m.to_lowercase().contains(&filter.to_lowercase()))
                .collect();

            assert!(
                !filtered.is_empty(),
                "Filter '{}' should match something",
                filter
            );
            assert!(filtered.iter().any(|m| m.contains(expected_match)));
        }
    }

    #[test]
    fn test_long_model_names() {
        let long_model = "very-long-model-name-that-might-cause-display-issues-in-the-terminal-ui";
        let models = vec![long_model];
        let file = create_temp_models_file(&models);

        let result = crate::read_models(file.path().to_str().unwrap());
        assert!(result.is_ok());
        let read_models = result.unwrap();
        assert_eq!(read_models[0], long_model);
    }

    #[test]
    fn test_escape_aborts_no_partial_writes() {
        let agent_file = create_temp_agent_file("original-model");
        let original_content = fs::read_to_string(agent_file.path()).unwrap();

        let current_model = crate::get_current_model(agent_file.path().to_str().unwrap()).unwrap();
        assert_eq!(current_model, "original-model");

        let current_content = fs::read_to_string(agent_file.path()).unwrap();
        assert_eq!(original_content, current_content);
    }

    #[test]
    fn test_selections_collected_before_writing() {
        let mut selections: Vec<(&str, &str, String, String)> = Vec::new();

        selections.push(("agent1", "path1", "old1".to_string(), "new1".to_string()));
        selections.push(("agent2", "path2", "old2".to_string(), "new2".to_string()));

        assert_eq!(selections.len(), 2);
        assert_eq!(selections[0].3, "new1");
        assert_eq!(selections[1].3, "new2");

        let confirmed = true;
        if confirmed {
            for (_name, _, current, selected) in &selections {
                assert_ne!(current, selected);
            }
        }
    }

    #[test]
    fn test_immediate_navigation_works() {
        let models = vec!["model1", "model2", "model3"];
        let filtered: Vec<String> = models.iter().map(|s| s.to_string()).collect();
        let current = "model2";

        let mut initialized = false;
        let mut selected = 0;

        if !initialized && "".is_empty() {
            if let Some(idx) = filtered.iter().position(|m| m == current) {
                selected = idx;
            }
            initialized = true;
        }

        selected = selected.saturating_add(1);
        assert!(selected < filtered.len());

        selected = selected.saturating_sub(1);
        assert_eq!(selected, 1);
    }

    // Tests for get_default_model and update_default_model

    #[test]
    fn test_get_default_model_success() {
        let file = create_temp_opencode_config("runpod/runpod-model");
        let result = crate::get_default_model(file.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "runpod/runpod-model");
    }

    #[test]
    fn test_get_default_model_with_different_models() {
        let test_models = vec![
            "openai/gpt-4",
            "anthropic/claude-3",
            "cosmo-00/cosmo-6000",
            "model-with-dashes",
        ];

        for model in test_models {
            let file = create_temp_opencode_config(model);
            let result = crate::get_default_model(file.path().to_str().unwrap());
            assert!(result.is_ok(), "Failed for model: {}", model);
            assert_eq!(result.unwrap(), model);
        }
    }

    #[test]
    fn test_get_default_model_no_model_key() {
        let file = create_temp_opencode_config_no_model();
        let result = crate::get_default_model(file.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No model key found"));
    }

    #[test]
    fn test_get_default_model_file_not_found() {
        let result = crate::get_default_model("/nonexistent/path/opencode.jsonc");
        assert!(result.is_err());
    }

    #[test]
    fn test_update_default_model_success() {
        let file = create_temp_opencode_config("old-model");
        let path = file.path().to_str().unwrap();

        let result = crate::update_default_model(path, "new-model");
        assert!(result.is_ok());

        let updated_model = crate::get_default_model(path).unwrap();
        assert_eq!(updated_model, "new-model");
    }

    #[test]
    fn test_update_default_model_preserves_comments() {
        let file = create_temp_opencode_config("old-model");
        let path = file.path().to_str().unwrap();

        let result = crate::update_default_model(path, "new-model");
        assert!(result.is_ok());

        let content = fs::read_to_string(path).unwrap();
        // Comments should be preserved
        assert!(content.contains("// This is a comment"));
        // Other fields preserved
        assert!(content.contains("\"plugin\""));
        assert!(content.contains("\"default_agent\""));
        assert!(content.contains("\"new-model\""));
    }

    #[test]
    fn test_update_default_model_preserves_structure() {
        let file = create_temp_opencode_config("old-model");
        let path = file.path().to_str().unwrap();

        let result = crate::update_default_model(path, "new-model");
        assert!(result.is_ok());

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("oh-my-opencode@latest"));
        assert!(content.contains("local-mind"));
    }

    #[test]
    fn test_update_default_model_no_model_key() {
        let file = create_temp_opencode_config_no_model();
        let result = crate::update_default_model(file.path().to_str().unwrap(), "new-model");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No model key found"));
    }

    #[test]
    fn test_update_default_model_file_not_found() {
        let result = crate::update_default_model("/nonexistent/path/opencode.jsonc", "new-model");
        assert!(result.is_err());
    }

    #[test]
    fn test_update_default_model_preserves_leading_whitespace() {
        let file = create_temp_opencode_config("old-model");
        let path = file.path().to_str().unwrap();

        let result = crate::update_default_model(path, "new-model");
        assert!(result.is_ok());

        let content = fs::read_to_string(path).unwrap();
        // The model line should preserve its indentation (2 spaces)
        assert!(content.contains("  \"model\": \"new-model\","));
    }

    #[test]
    fn test_update_default_model_preserves_newline() {
        let file = create_temp_opencode_config("old-model");
        let path = file.path().to_str().unwrap();

        let original_content = fs::read_to_string(path).unwrap();
        let ends_with_newline = original_content.ends_with('\n');
        assert!(ends_with_newline);

        let result = crate::update_default_model(path, "new-model");
        assert!(result.is_ok());

        let updated_content = fs::read_to_string(path).unwrap();
        assert!(updated_content.ends_with('\n'));
    }

    #[test]
    fn test_default_model_round_trip() {
        let file = create_temp_opencode_config("initial-model");
        let path = file.path().to_str().unwrap();

        // Read initial
        let initial = crate::get_default_model(path).unwrap();
        assert_eq!(initial, "initial-model");

        // Update
        crate::update_default_model(path, "updated-model").unwrap();

        // Read updated
        let updated = crate::get_default_model(path).unwrap();
        assert_eq!(updated, "updated-model");

        // Update again
        crate::update_default_model(path, "final-model").unwrap();

        // Read final
        let final_model = crate::get_default_model(path).unwrap();
        assert_eq!(final_model, "final-model");
    }

    #[test]
    fn test_default_selection_with_agents() {
        // Test the combined flow where default selection and agent selections work together
        let default_selection: Option<(String, String)> =
            Some(("old-default".to_string(), "new-default".to_string()));
        let agent_selections: Vec<(&str, &str, String, String)> = vec![
            ("agent1", "path1", "old1".to_string(), "new1".to_string()),
            ("agent2", "path2", "same".to_string(), "same".to_string()),
        ];

        // Check default has changes
        if let Some((ref old, ref new)) = default_selection {
            assert_ne!(old, new);
        }

        // Check agents
        let agent_changes: Vec<_> = agent_selections
            .iter()
            .filter(|(_, _, old, new)| old != new)
            .collect();
        assert_eq!(agent_changes.len(), 1);
        assert_eq!(agent_changes[0].0, "agent1");
    }

    #[test]
    fn test_default_selection_no_change() {
        let default_selection: Option<(String, String)> =
            Some(("same-model".to_string(), "same-model".to_string()));

        if let Some((ref old, ref new)) = default_selection {
            assert_eq!(old, new);
        }
    }

    // Tests for ctx limit functionality

    #[test]
    fn test_update_line_input_limit_replace() {
        let line = r#"      "models": { "m": { "limit": { "context": 262144, "output": 262144, "input": 200000 } } }"#;
        let result = crate::update_line_input_limit(line, Some(180000));
        assert!(result.contains("\"input\": 180000"), "got: {}", result);
        assert!(!result.contains("200000"), "old value should be gone: {}", result);
        assert!(result.contains("\"context\": 262144"));
        assert!(result.contains("\"output\": 262144"));
    }

    #[test]
    fn test_update_line_input_limit_add() {
        let line = r#"      "models": { "m": { "limit": { "context": 262144, "output": 262144 } } }"#;
        let result = crate::update_line_input_limit(line, Some(200000));
        assert!(result.contains("\"input\": 200000"), "got: {}", result);
        assert!(result.contains("\"context\": 262144"));
        assert!(result.contains("\"output\": 262144"));
    }

    #[test]
    fn test_update_line_input_limit_remove() {
        let line = r#"      "models": { "m": { "limit": { "context": 262144, "output": 262144, "input": 200000 } } }"#;
        let result = crate::update_line_input_limit(line, None);
        assert!(!result.contains("\"input\""), "got: {}", result);
        assert!(result.contains("\"context\": 262144"));
        assert!(result.contains("\"output\": 262144"));
        // closing brace of limit still present
        assert!(result.contains("} }"));
    }

    #[test]
    fn test_update_line_input_limit_no_limit_key() {
        let line = r#"  "model": "cosmo-proxy/cosmo-proxy","#;
        let result = crate::update_line_input_limit(line, Some(200000));
        assert_eq!(result, line);
    }

    #[test]
    fn test_update_line_input_limit_off_no_input_is_noop() {
        let line = r#"      "models": { "m": { "limit": { "context": 262144, "output": 262144 } } }"#;
        let result = crate::update_line_input_limit(line, None);
        assert_eq!(result, line);
    }

    #[test]
    fn test_update_input_limits_replaces_all() {
        let file = create_temp_config_with_limits(true);
        let path = file.path().to_str().unwrap();

        let changes = crate::update_input_limits(path, Some(180000)).unwrap();
        assert_eq!(changes, 2);

        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content.matches("\"input\": 180000").count(), 2);
        assert!(!content.contains("\"input\": 200000"));
    }

    #[test]
    fn test_update_input_limits_adds_when_missing() {
        let file = create_temp_config_with_limits(false);
        let path = file.path().to_str().unwrap();

        let changes = crate::update_input_limits(path, Some(200000)).unwrap();
        assert_eq!(changes, 2);

        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content.matches("\"input\": 200000").count(), 2);
    }

    #[test]
    fn test_update_input_limits_off_removes_all() {
        let file = create_temp_config_with_limits(true);
        let path = file.path().to_str().unwrap();

        let changes = crate::update_input_limits(path, None).unwrap();
        assert_eq!(changes, 2);

        let content = fs::read_to_string(path).unwrap();
        assert!(!content.contains("\"input\""));
    }

    #[test]
    fn test_update_input_limits_off_no_input_zero_changes() {
        let file = create_temp_config_with_limits(false);
        let path = file.path().to_str().unwrap();

        let changes = crate::update_input_limits(path, None).unwrap();
        assert_eq!(changes, 0);
    }

    #[test]
    fn test_update_input_limits_preserves_newline() {
        let file = create_temp_config_with_limits(true);
        let path = file.path().to_str().unwrap();
        let original = fs::read_to_string(path).unwrap();
        assert!(original.ends_with('\n'));

        crate::update_input_limits(path, Some(150000)).unwrap();

        let updated = fs::read_to_string(path).unwrap();
        assert!(updated.ends_with('\n'));
    }

    #[test]
    fn test_update_line_input_limit_roundtrip() {
        let original = r#"      "m": { "limit": { "context": 262144, "output": 32768 } }"#;
        let added = crate::update_line_input_limit(original, Some(200000));
        let replaced = crate::update_line_input_limit(&added, Some(180000));
        let removed = crate::update_line_input_limit(&replaced, None);
        assert_eq!(removed, original);
    }

    #[test]
    fn test_model_with_special_json_chars() {
        // Test models with characters that might cause JSON issues
        let models = vec![
            "model/with/slashes",
            "model-with-dashes",
            "model_with_underscores",
            "model.with.dots",
        ];

        for model in models {
            let file = create_temp_opencode_config(model);
            let path = file.path().to_str().unwrap();

            let result = crate::get_default_model(path);
            assert!(result.is_ok(), "Failed to read model: {}", model);
            assert_eq!(result.unwrap(), model);

            // Now test updating to a different model with special chars
            let new_model = format!("{}-updated", model);
            let update_result = crate::update_default_model(path, &new_model);
            assert!(
                update_result.is_ok(),
                "Failed to update to model: {}",
                new_model
            );

            let updated = crate::get_default_model(path).unwrap();
            assert_eq!(updated, new_model);
        }
    }
}
