//! Selection policy shared by migration, settings, and execution. Catalog IDs
//! are UI identifiers; only the `model` slug is sent to Codex.
use crate::types::{Provider, Review, Settings, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn validate_catalog(catalog: &[Value]) -> Result<(), String> {
    if catalog.is_empty() {
        return Err("Codex returned no available models.".into());
    }
    let mut slugs = std::collections::HashSet::new();
    for model in catalog {
        let slug = model["model"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("Codex returned a model without a model identifier.")?;
        if !slugs.insert(slug) {
            return Err("Codex returned duplicate model identifiers.".into());
        }
        if !model["displayName"].is_string()
            || !model["isDefault"].is_boolean()
            || !model["defaultReasoningEffort"].is_string()
            || !model["supportedReasoningEfforts"]
                .as_array()
                .is_some_and(|efforts| {
                    efforts
                        .iter()
                        .all(|e| e["reasoningEffort"].as_str().is_some_and(|v| !v.is_empty()))
                })
        {
            return Err(format!(
                "Codex returned incomplete catalog details for '{slug}'."
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelection {
    pub model: String,
    pub effort: String,
}
impl ModelSelection {
    pub fn requested(review: &Review) -> Self {
        Self {
            model: review.model.clone(),
            effort: review.effort.clone(),
        }
    }
    pub fn workspace(settings: &Settings) -> Self {
        Self {
            model: settings.model.clone(),
            effort: settings.effort.clone(),
        }
    }
    pub fn complete(&self) -> bool {
        !self.model.is_empty() && !self.effort.is_empty()
    }
    pub fn chat(review: &Review) -> Result<Self, String> {
        let requested = Self::requested(review);
        if requested.complete() {
            return Ok(requested);
        }
        review
            .legacy_chat_selection
            .clone()
            .ok_or_else(|| "Choose a chat model and effort before sending a follow-up.".into())
    }
    pub fn validate(&self, catalog: &[Value]) -> Result<(), String> {
        if !self.complete() {
            return Err("Choose an explicit model and reasoning effort in Settings.".into());
        }
        let mut matches = catalog.iter().filter(|m| m["model"] == self.model);
        let model = matches.next().ok_or_else(|| format!(
            "Model '{}' is unavailable. Reconnect Codex or choose an available model in Settings.", self.model))?;
        if matches.next().is_some() {
            return Err(
                "Codex returned duplicate model identifiers. Reconnect to refresh the catalog."
                    .into(),
            );
        }
        if !model["supportedReasoningEfforts"]
            .as_array()
            .is_some_and(|efforts| efforts.iter().any(|e| e["reasoningEffort"] == self.effort))
        {
            return Err(format!(
                "Model '{}' does not support effort '{}'. Choose a supported effort in Settings.",
                self.model, self.effort
            ));
        }
        Ok(())
    }
    pub fn validate_execution(&self, state: &Snapshot) -> Result<(), String> {
        if !state.codex_ready {
            return Err("Codex is disconnected. Reconnect in Settings.".into());
        }
        if !state.model_catalog_ready {
            return Err(state.model_configuration_error.clone().unwrap_or_else(|| {
                "Model discovery is not ready. Reconnect Codex in Settings.".into()
            }));
        }
        self.validate(&state.models)
    }
    pub fn verify_response(&self, response: &Value) -> Result<(), String> {
        if response["model"].as_str() != Some(self.model.as_str())
            || response["reasoningEffort"].as_str() != Some(self.effort.as_str())
        {
            return Err(format!("Codex did not confirm the requested model '{}' and effort '{}' (reported model: {}, effort: {}). No inference was started. Reconnect Codex and retry.",
                self.model, self.effort,
                response["model"].as_str().unwrap_or("not reported"),
                response["reasoningEffort"].as_str().unwrap_or("not reported")));
        }
        Ok(())
    }
}

impl ModelSelection {
    /// Checks a run by `provider` can start with this selection now.
    pub fn validate_for(&self, provider: Provider, state: &Snapshot) -> Result<(), String> {
        match provider {
            Provider::Codex => self.validate_execution(state),
            Provider::Claude => {
                if !state.claude.ready {
                    return Err(state
                        .claude
                        .error
                        .clone()
                        .unwrap_or_else(|| "Claude isn't ready. Check it in Settings.".into()));
                }
                self.validate_claude()
            }
        }
    }
    /// Checks the selection against the Claude models Orbit offers.
    pub fn validate_claude(&self) -> Result<(), String> {
        let catalog = crate::claude::catalog();
        let model = catalog
            .iter()
            .find(|m| m["model"] == self.model)
            .ok_or_else(|| {
                format!(
                    "Claude model '{}' is not offered. Choose a Claude model in Settings.",
                    self.model
                )
            })?;
        if !model["supportedReasoningEfforts"]
            .as_array()
            .is_some_and(|efforts| efforts.iter().any(|e| e["reasoningEffort"] == self.effort))
        {
            return Err(format!(
                "Claude model '{}' does not support effort '{}'. Choose a supported effort in Settings.",
                self.model, self.effort
            ));
        }
        Ok(())
    }
}

/// The Claude model and effort new Claude reviews use: the saved choice, or
/// the default model and its default effort.
pub fn claude_selection(settings: &Settings) -> ModelSelection {
    let catalog = crate::claude::catalog();
    let model = if settings.claude_model.is_empty() {
        catalog
            .iter()
            .find(|m| m["isDefault"] == true)
            .and_then(|m| m["model"].as_str())
            .unwrap_or_default()
            .to_string()
    } else {
        settings.claude_model.clone()
    };
    let effort = if settings.claude_effort.is_empty() {
        catalog
            .iter()
            .find(|m| m["model"] == model)
            .and_then(|m| m["defaultReasoningEffort"].as_str())
            .unwrap_or("high")
            .to_string()
    } else {
        settings.claude_effort.clone()
    };
    ModelSelection { model, effort }
}

#[derive(Debug)]
pub struct Resolution {
    pub selection: ModelSelection,
    pub notice: Option<String>,
}

/// Only empty values are resolved. Call with the current state under its lock,
/// after discovery, so intervening explicit Settings edits always win.
pub fn resolve(
    settings: &Settings,
    catalog: &[Value],
    config: &Result<Value, String>,
) -> Result<Resolution, String> {
    let mut selection = ModelSelection::workspace(settings);
    let mut model_source = "your saved Orbit selection".to_string();
    let mut effort_source = "your saved Orbit selection".to_string();
    if selection.model.is_empty() {
        let config = config.as_ref().map_err(|e| format!("Could not read effective Codex settings: {e}. Reconnect or select a model in Settings."))?;
        let effective = config["config"]["model"].as_str().filter(|m| !m.is_empty());
        if let Some(model) = effective.filter(|m| catalog.iter().any(|entry| entry["model"] == *m))
        {
            selection.model = model.into();
            model_source = "your Codex configuration".into();
        } else {
            let defaults: Vec<_> = catalog.iter().filter(|m| m["isDefault"] == true).collect();
            if defaults.len() != 1 {
                return Err(
                    "Codex has no unique default model. Choose a model and effort in Settings."
                        .into(),
                );
            }
            selection.model = defaults[0]["model"]
                .as_str()
                .filter(|m| !m.is_empty())
                .ok_or("Codex returned an invalid default model. Reconnect in Settings.")?
                .into();
            model_source = match effective {
                Some(model) => {
                    format!("catalog default (your configured model '{model}' is unavailable)")
                }
                None => "catalog default (no configured model)".into(),
            };
        }
    }
    if selection.effort.is_empty() {
        let model = catalog.iter().find(|m| m["model"] == selection.model);
        let configured = config
            .as_ref()
            .ok()
            .and_then(|c| c["config"]["model_reasoning_effort"].as_str())
            .filter(|e| !e.is_empty());
        if let Some(effort) = configured.filter(|effort| {
            model
                .and_then(|m| m["supportedReasoningEfforts"].as_array())
                .is_some_and(|efforts| efforts.iter().any(|e| e["reasoningEffort"] == *effort))
        }) {
            selection.effort = effort.into();
            effort_source = "your Codex configuration".into();
        } else {
            selection.effort = model.and_then(|m| m["defaultReasoningEffort"].as_str())
                .filter(|e| !e.is_empty())
                .ok_or("The selected model or its default effort is unavailable. Choose a model and effort in Settings.")?.into();
            effort_source = match configured {
                Some(effort) => format!("model default (your configured effort '{effort}' isn't supported by this model)"),
                None if config.is_err() => "model default (Codex configuration could not be read)".into(),
                None => "model default (no configured effort)".into(),
            };
        }
    }
    selection.validate(catalog)?;
    let notice = (selection != ModelSelection::workspace(settings)).then(|| format!(
        "Resolved review selection to {} · {}. Model: {model_source}. Effort: {effort_source}. This explicit selection is saved for new reviews.",
        selection.model, selection.effort));
    Ok(Resolution { selection, notice })
}

/// Historical metadata must never be inferred from workspace or chat settings.
pub fn report_labels(review: &Review) -> (String, String) {
    if let Some(selection) = &review.confirmed_review_selection {
        return (
            format!("{} (confirmed)", selection.model),
            format!("{} (confirmed)", selection.effort),
        );
    }
    let label = |value: &str| {
        if value.is_empty() {
            "not recorded".into()
        } else {
            format!("{value} (requested, unverified)")
        }
    };
    (label(&review.model), label(&review.effort))
}
pub fn report_selection_label(review: &Review) -> String {
    let (model, effort) = report_labels(review);
    format!("Model: {model} · Effort: {effort}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn report_selection_format_matches_shared_ui_and_export_cases() {
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../test-fixtures/report-selection-labels.json"
        ))
        .unwrap();
        for case in cases {
            let mut r = Review::new(&crate::types::tests::pr(), &Settings::default(), false);
            r.model = case["model"].as_str().unwrap().into();
            r.effort = case["effort"].as_str().unwrap().into();
            r.confirmed_review_selection =
                serde_json::from_value(case["confirmedReviewSelection"].clone()).unwrap();
            r.legacy_chat_selection = Some(ModelSelection {
                model: "later-chat".into(),
                effort: "high".into(),
            });
            assert_eq!(report_selection_label(&r), case["label"].as_str().unwrap());
        }
    }
    pub fn catalog() -> Vec<Value> {
        vec![
            json!({"id":"ui-a","model":"model-a","displayName":"Model A","isDefault":true,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]}),
            json!({"id":"ui-b","model":"model-b","displayName":"Model B","isDefault":false,"defaultReasoningEffort":"high","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}),
        ]
    }
    #[test]
    fn resolves_effective_model_then_unique_default_using_model_slugs_and_configured_effort() {
        let s = Settings::default();
        let effective = resolve(
            &s,
            &catalog(),
            &Ok(json!({"config":{"model":"model-a","model_reasoning_effort":"low"}})),
        )
        .unwrap();
        assert_eq!(
            effective.selection,
            ModelSelection {
                model: "model-a".into(),
                effort: "low".into()
            }
        );
        let notice = effective.notice.unwrap();
        assert!(notice.contains("Model: your Codex configuration."));
        assert!(notice.contains("Effort: your Codex configuration."));
        for model in [Value::Null, json!("unlisted"), json!("ui-b")] {
            let selected = resolve(&s, &catalog(), &Ok(json!({"config":{"model":model}}))).unwrap();
            assert_eq!(selected.selection.model, "model-a");
            assert_eq!(selected.selection.effort, "medium");
            assert!(selected.notice.unwrap().contains("Model: catalog default"));
        }
    }
    #[test]
    fn empty_effort_uses_supported_config_for_an_explicit_model_and_explains_fallbacks() {
        let settings = Settings {
            model: "model-a".into(),
            ..Settings::default()
        };
        for (config, effort, reason) in [
            // Config effort applies even though its model differs from Orbit's.
            (
                Ok(json!({"config":{"model":"model-b", "model_reasoning_effort":"high"}})),
                "high",
                "your Codex configuration",
            ),
            (
                Ok(json!({"config":{"model_reasoning_effort":"xhigh"}})),
                "medium",
                "model default (your configured effort 'xhigh' isn't supported by this model)",
            ),
            (
                Ok(json!({"config":{"model_reasoning_effort":null}})),
                "medium",
                "model default (no configured effort)",
            ),
            (
                Ok(json!({"config":{"model_reasoning_effort":""}})),
                "medium",
                "model default (no configured effort)",
            ),
            (
                Ok(json!({"config":{"reasoningEffort":"high"}})),
                "medium",
                "model default (no configured effort)",
            ),
            (
                Err("offline".into()),
                "medium",
                "model default (Codex configuration could not be read)",
            ),
        ] {
            let resolved = resolve(&settings, &catalog(), &config).unwrap();
            assert_eq!(resolved.selection.model, "model-a");
            assert_eq!(resolved.selection.effort, effort);
            let notice = resolved.notice.unwrap();
            assert!(notice.contains("Model: your saved Orbit selection."));
            assert!(notice.contains(&format!("Effort: {reason}.")), "{notice}");
        }
    }
    #[test]
    fn unsupported_config_falls_back_but_an_unusable_default_still_blocks() {
        let settings = Settings::default();
        let configured = Ok(json!({"config":{"model":"model-b", "model_reasoning_effort":"low"}}));
        let resolved = resolve(&settings, &catalog(), &configured).unwrap();
        assert_eq!(resolved.selection.effort, "high");
        assert!(resolved
            .notice
            .unwrap()
            .contains("your configured effort 'low' isn't supported"));
        for invalid_default in [Value::Null, json!(""), json!("unsupported")] {
            let mut models = catalog();
            models[1]["defaultReasoningEffort"] = invalid_default;
            assert!(resolve(&settings, &models, &configured).is_err());
            // A supported configured effort takes precedence even over a bad default.
            let supported =
                Ok(json!({"config":{"model":"model-b", "model_reasoning_effort":"high"}}));
            assert_eq!(
                resolve(&settings, &models, &supported)
                    .unwrap()
                    .selection
                    .effort,
                "high"
            );
        }
    }
    #[test]
    fn explicit_values_are_never_replaced_and_invalid_defaults_block() {
        let mut s = Settings {
            model: "model-a".into(),
            effort: "low".into(),
            ..Settings::default()
        };
        let resolved = resolve(
            &s,
            &catalog(),
            &Ok(json!({"config":{"model":"model-b", "model_reasoning_effort":"high"}})),
        )
        .unwrap();
        assert_eq!(resolved.selection, ModelSelection::workspace(&s));
        assert!(resolved.notice.is_none());
        assert_eq!(
            resolve(&s, &catalog(), &Err("offline".into()))
                .unwrap()
                .selection
                .effort,
            "low"
        );
        s.model = "missing".into();
        assert!(resolve(&s, &catalog(), &Ok(Value::Null)).is_err());
        s.model = "model-b".into();
        assert!(resolve(&s, &catalog(), &Ok(Value::Null)).is_err());
        s.effort.clear();
        assert_eq!(
            resolve(&s, &catalog(), &Ok(Value::Null))
                .unwrap()
                .selection
                .effort,
            "high"
        );
        let mut models = catalog();
        models[1]["defaultReasoningEffort"] = json!("low");
        assert!(resolve(&s, &models, &Ok(Value::Null)).is_err());
        let s = Settings::default();
        assert!(resolve(&s, &catalog(), &Err("offline".into())).is_err());
        models[1]["isDefault"] = json!(true);
        assert!(resolve(&s, &models, &Ok(Value::Null)).is_err());
        assert!(resolve(&s, &[], &Ok(Value::Null)).is_err());
        models[0]["isDefault"] = json!(false);
        models[1]["isDefault"] = json!(false);
        assert!(resolve(&s, &models, &Ok(Value::Null)).is_err());
    }
    #[test]
    fn partial_explicit_settings_preserve_the_nonempty_value() {
        let s = Settings {
            effort: "low".into(),
            ..Settings::default()
        };
        let resolved = resolve(
            &s,
            &catalog(),
            &Ok(json!({"config":{"model":"model-a", "model_reasoning_effort":"high"}})),
        )
        .unwrap();
        assert_eq!(resolved.selection.effort, "low");
        assert!(resolved
            .notice
            .unwrap()
            .contains("Effort: your saved Orbit selection."));
        assert!(resolve(&s, &catalog(), &Ok(json!({"config":{"model":"model-b"}}))).is_err());
        assert_eq!(
            resolve(&s, &catalog(), &Ok(Value::Null))
                .unwrap()
                .selection
                .effort,
            "low"
        );
    }
    #[test]
    fn old_state_loads_and_later_chat_metadata_never_changes_report_labels() {
        let r = Review::new(&crate::types::tests::pr(), &Settings::default(), false);
        let mut value = serde_json::to_value(r).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("confirmedReviewSelection");
        value.as_object_mut().unwrap().remove("legacyChatSelection");
        let mut r: Review = serde_json::from_value(value).unwrap();
        r.legacy_chat_selection = Some(ModelSelection {
            model: "chat-model".into(),
            effort: "high".into(),
        });
        assert_eq!(
            report_labels(&r),
            ("not recorded".into(), "not recorded".into())
        );
        r.model = "old-model".into();
        r.effort = "low".into();
        assert!(report_labels(&r).0.contains("requested, unverified"));
        r.confirmed_review_selection = Some(ModelSelection {
            model: "confirmed-model".into(),
            effort: "medium".into(),
        });
        assert_eq!(report_labels(&r).0, "confirmed-model (confirmed)");
    }
}
