use serde_json::{json, Value as JsonValue};

pub const DEFAULT_EXCLUDED_BUNDLE_IDS: &[&str] = &[
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
];

/// Redact focus payload when bundle_id is in the exclusion list.
pub fn redact_focus_payload(
    bundle_id: &str,
    app_name: &str,
    excluded_bundle_ids: &[String],
) -> JsonValue {
    if excluded_bundle_ids.iter().any(|id| id == bundle_id) {
        json!({
            "bundle_id": bundle_id,
            "app_name": "Hidden",
            "excluded": true
        })
    } else {
        json!({
            "bundle_id": bundle_id,
            "app_name": app_name
        })
    }
}

pub fn default_excluded_bundle_ids() -> Vec<String> {
    DEFAULT_EXCLUDED_BUNDLE_IDS
        .iter()
        .map(|s| s.to_string())
        .collect()
}
