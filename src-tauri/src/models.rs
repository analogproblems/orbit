//! Selection policy shared by migration, settings, and execution. Catalog IDs
//! are UI identifiers; only the `model` slug is sent to Codex.
use crate::types::{Review, Settings, Snapshot};
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

/// Only empty values are resolved. Call with the current state under its lock,
/// after discovery, so intervening explicit Settings edits always win.
pub fn resolve(
    settings: &Settings,
    catalog: &[Value],
    config: &Result<Value, String>,
) -> Result<ModelSelection, String> {
    let mut selection = ModelSelection::workspace(settings);
    if selection.model.is_empty() {
        let config = config.as_ref().map_err(|e| format!("Could not read effective Codex settings: {e}. Reconnect or select a model in Settings."))?;
        let effective = config["config"]["model"].as_str().filter(|m| !m.is_empty());
        if let Some(model) = effective.filter(|m| catalog.iter().any(|entry| entry["model"] == *m))
        {
            selection.model = model.into();
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
        }
    }
    if selection.effort.is_empty() {
        selection.effort = catalog.iter().find(|m| m["model"] == selection.model)
            .and_then(|m| m["defaultReasoningEffort"].as_str()).filter(|e| !e.is_empty())
            .ok_or("The selected model or its default effort is unavailable. Choose a model and effort in Settings.")?.into();
    }
    selection.validate(catalog)?;
    Ok(selection)
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    pub fn catalog() -> Vec<Value> {
        vec![
            json!({"id":"ui-a","model":"model-a","displayName":"Model A","isDefault":true,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"},{"reasoningEffort":"high"}]}),
            json!({"id":"ui-b","model":"model-b","displayName":"Model B","isDefault":false,"defaultReasoningEffort":"high","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}),
        ]
    }
    #[test]
    fn resolves_effective_model_then_unique_default_using_model_slugs_and_supported_defaults() {
        let s = Settings::default();
        let effective = resolve(
            &s,
            &catalog(),
            &Ok(json!({"config":{"model":"model-b","model_reasoning_effort":"low"}})),
        )
        .unwrap();
        assert_eq!(
            effective,
            ModelSelection {
                model: "model-b".into(),
                effort: "high".into()
            }
        );
        for model in [Value::Null, json!("unlisted"), json!("ui-b")] {
            let selected = resolve(&s, &catalog(), &Ok(json!({"config":{"model":model}}))).unwrap();
            assert_eq!(selected.model, "model-a");
            assert_eq!(selected.effort, "medium");
        }
    }
    #[test]
    fn explicit_values_are_never_replaced_and_invalid_defaults_block() {
        let mut s = Settings {
            model: "model-a".into(),
            effort: "low".into(),
            ..Settings::default()
        };
        assert_eq!(
            resolve(&s, &catalog(), &Err("offline".into()))
                .unwrap()
                .effort,
            "low"
        );
        s.model = "missing".into();
        assert!(resolve(&s, &catalog(), &Ok(Value::Null)).is_err());
        s.model = "model-b".into();
        assert!(resolve(&s, &catalog(), &Ok(Value::Null)).is_err());
        s.effort.clear();
        assert_eq!(
            resolve(&s, &catalog(), &Ok(Value::Null)).unwrap().effort,
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
        assert!(resolve(&s, &catalog(), &Ok(json!({"config":{"model":"model-b"}}))).is_err());
        assert_eq!(
            resolve(&s, &catalog(), &Ok(Value::Null)).unwrap().effort,
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
