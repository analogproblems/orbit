//! The Codex account's usage limits, as `account/rateLimits/read` reports them
//! and `account/rateLimits/updated` keeps them current. Field names follow the
//! Codex protocol so the app can show them as they arrive.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How much of one usage window is used, and when it starts over.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageWindow {
    pub used_percent: f64,
    pub window_duration_mins: Option<i64>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageCredits {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}
/// One metered limit: the short window (`primary`) and the long one
/// (`secondary`, usually a week).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageLimit {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    /// The model this limit belongs to, for a limit tied to one model.
    pub normal_model_slug: Option<String>,
    pub primary: Option<UsageWindow>,
    pub secondary: Option<UsageWindow>,
    pub credits: Option<UsageCredits>,
    pub plan_type: Option<String>,
    /// Set while a limit is reached, such as `rate_limit_reached`.
    pub rate_limit_reached_type: Option<String>,
}
impl UsageLimit {
    fn has_windows(&self) -> bool {
        self.primary.is_some() || self.secondary.is_some()
    }
    /// Takes the values a sparse update carries. A missing value in an update
    /// means "not reported", so it leaves the last known one in place.
    fn merge(&mut self, update: UsageLimit) {
        macro_rules! take {
            ($($field:ident),*) => {$(
                if update.$field.is_some() {
                    self.$field = update.$field;
                }
            )*};
        }
        take!(
            limit_id,
            limit_name,
            normal_model_slug,
            primary,
            secondary,
            credits,
            plan_type,
            rate_limit_reached_type
        );
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    /// The account's main limit, the one Codex shows first.
    pub rate_limits: UsageLimit,
    /// Every other metered limit, such as one tied to a particular model.
    pub other_limits: Vec<UsageLimit>,
    /// When Orbit last heard from Codex about usage.
    pub updated_at: String,
}
fn limit(value: &Value) -> Option<UsageLimit> {
    serde_json::from_value(value.clone()).ok()
}
impl Usage {
    /// Reads an `account/rateLimits/read` response. An account with no usage
    /// windows, such as an API key login, has nothing to show.
    pub fn from_read(response: &Value, at: String) -> Option<Self> {
        let main = limit(&response["rateLimits"])?;
        let mut other_limits: Vec<UsageLimit> = response["rateLimitsByLimitId"]
            .as_object()
            .map(|by_id| {
                by_id
                    .iter()
                    .filter_map(|(id, value)| {
                        let mut l = limit(value)?;
                        l.limit_id.get_or_insert_with(|| id.clone());
                        Some(l)
                    })
                    .filter(|l| l.has_windows() && l.limit_id != main.limit_id)
                    .collect()
            })
            .unwrap_or_default();
        other_limits.sort_by(|a, b| a.limit_id.cmp(&b.limit_id));
        if !main.has_windows() && other_limits.is_empty() {
            return None;
        }
        Some(Self {
            rate_limits: main,
            other_limits,
            updated_at: at,
        })
    }
    /// Applies an `account/rateLimits/updated` notification to the last read.
    pub fn apply_update(current: Option<Self>, params: &Value, at: String) -> Option<Self> {
        let Some(update) = limit(&params["rateLimits"]) else {
            return current;
        };
        let Some(mut usage) = current else {
            return update.has_windows().then(|| Self {
                rate_limits: update,
                other_limits: vec![],
                updated_at: at,
            });
        };
        let main_id = usage.rate_limits.limit_id.clone();
        let target = match update.limit_id.as_ref() {
            Some(id) if main_id.as_ref() != Some(id) => {
                match usage
                    .other_limits
                    .iter_mut()
                    .position(|l| l.limit_id.as_ref() == Some(id))
                {
                    Some(i) => &mut usage.other_limits[i],
                    None if update.has_windows() => {
                        usage.other_limits.push(UsageLimit::default());
                        usage.other_limits.last_mut().unwrap()
                    }
                    None => return Some(usage),
                }
            }
            _ => &mut usage.rate_limits,
        };
        target.merge(update);
        usage.updated_at = at;
        Some(usage)
    }
}
/// Why a review stopped, when Codex says the account ran out of usage.
pub fn limit_message(error: &Value) -> Option<String> {
    (error["codexErrorInfo"] == "usageLimitExceeded").then(|| {
        "Your Codex usage limit was reached, so this review stopped. Nothing is wrong with the \
         review itself: start it again once the limit resets. The usage meter shows when."
            .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn window(used: i64, mins: i64, resets: i64) -> Value {
        json!({"usedPercent": used, "windowDurationMins": mins, "resetsAt": resets})
    }
    fn read() -> Value {
        json!({
            "ordinaryUsageAllowed": true,
            "rateLimits": {
                "limitId": "codex",
                "primary": window(10, 300, 1_000),
                "secondary": window(42, 10_080, 9_000),
                "credits": {"hasCredits": true, "unlimited": false, "balance": "12.50"},
                "planType": "pro",
                "rateLimitReachedType": null
            },
            "rateLimitsByLimitId": {
                "codex": {"limitId": "codex", "primary": window(10, 300, 1_000)},
                "codex_bengalfox": {
                    "limitName": "GPT-5.5 Codex",
                    "normalModelSlug": "gpt-5.5-codex",
                    "primary": window(3, 300, 1_200)
                },
                "empty": {"limitId": "empty"}
            }
        })
    }

    #[test]
    fn reads_the_main_limit_and_the_per_model_limits() {
        let usage = Usage::from_read(&read(), "t1".into()).unwrap();
        let main = &usage.rate_limits;
        assert_eq!(main.secondary.as_ref().unwrap().used_percent, 42.0);
        assert_eq!(
            main.primary.as_ref().unwrap().window_duration_mins,
            Some(300)
        );
        assert_eq!(main.plan_type.as_deref(), Some("pro"));
        assert_eq!(
            main.credits.as_ref().unwrap().balance.as_deref(),
            Some("12.50")
        );
        // The main limit is not listed twice, and a limit without windows is left out.
        assert_eq!(usage.other_limits.len(), 1);
        let other = &usage.other_limits[0];
        assert_eq!(other.limit_id.as_deref(), Some("codex_bengalfox"));
        assert_eq!(other.normal_model_slug.as_deref(), Some("gpt-5.5-codex"));
    }
    #[test]
    fn an_account_without_usage_windows_shows_nothing() {
        assert_eq!(Usage::from_read(&json!({}), "t".into()), None);
        let api_key = json!({"rateLimits": {"limitId": null, "primary": null, "secondary": null}});
        assert_eq!(Usage::from_read(&api_key, "t".into()), None);
    }
    #[test]
    fn a_sparse_update_keeps_what_it_does_not_report() {
        let usage = Usage::from_read(&read(), "t1".into());
        let update = json!({"rateLimits": {
            "limitId": "codex",
            "primary": window(55, 300, 1_000),
            "planType": null,
            "rateLimitReachedType": "rate_limit_reached"
        }});
        let usage = Usage::apply_update(usage, &update, "t2".into()).unwrap();
        let main = &usage.rate_limits;
        assert_eq!(main.primary.as_ref().unwrap().used_percent, 55.0);
        assert_eq!(main.secondary.as_ref().unwrap().used_percent, 42.0);
        assert_eq!(main.plan_type.as_deref(), Some("pro"));
        assert_eq!(
            main.rate_limit_reached_type.as_deref(),
            Some("rate_limit_reached")
        );
        assert_eq!(usage.updated_at, "t2");
    }
    #[test]
    fn an_update_for_another_limit_changes_only_that_limit() {
        let usage = Usage::from_read(&read(), "t1".into());
        let update = json!({"rateLimits": {
            "limitId": "codex_bengalfox",
            "primary": window(80, 300, 1_200)
        }});
        let usage = Usage::apply_update(usage, &update, "t2".into()).unwrap();
        assert_eq!(
            usage.rate_limits.primary.as_ref().unwrap().used_percent,
            10.0
        );
        assert_eq!(
            usage.other_limits[0].primary.as_ref().unwrap().used_percent,
            80.0
        );
        assert_eq!(
            usage.other_limits[0].limit_name.as_deref(),
            Some("GPT-5.5 Codex")
        );
        let new = json!({"rateLimits": {"limitId": "new", "secondary": window(5, 10_080, 9)}});
        let usage = Usage::apply_update(Some(usage), &new, "t3".into()).unwrap();
        assert_eq!(usage.other_limits.len(), 2);
    }
    #[test]
    fn an_update_before_any_read_starts_the_meter_only_with_windows() {
        let empty = json!({"rateLimits": {"limitId": "codex"}});
        assert_eq!(Usage::apply_update(None, &empty, "t".into()), None);
        let update = json!({"rateLimits": {"limitId": "codex", "primary": window(1, 300, 9)}});
        let usage = Usage::apply_update(None, &update, "t".into()).unwrap();
        assert_eq!(usage.rate_limits.primary.unwrap().used_percent, 1.0);
        let unreadable = json!({"rateLimits": "nope"});
        assert_eq!(Usage::apply_update(None, &unreadable, "t".into()), None);
    }
    #[test]
    fn names_a_usage_limit_failure() {
        assert!(limit_message(&json!({"codexErrorInfo": "usageLimitExceeded"})).is_some());
        assert!(limit_message(&json!({"codexErrorInfo": "serverOverloaded"})).is_none());
        assert!(limit_message(&json!({"message": "boom"})).is_none());
    }
}
