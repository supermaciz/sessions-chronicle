use serde_json::Value;
use tracing::debug;

/// Normalizes a raw model value from session JSON into a clean model slug.
///
/// Rules:
/// 1. `None` input returns `None`.
/// 2. Non-string JSON values (numbers, objects, null, arrays) return `None`.
/// 3. Whitespace is trimmed.
/// 4. Empty string after trimming returns `None`.
/// 5. The sentinel `<synthetic>` returns `None`.
/// 6. Otherwise the raw slug is preserved as-is (no case rewrite, no splitting).
pub fn normalize_model(raw: Option<&Value>) -> Option<String> {
    let value = raw?;

    let s = match value.as_str() {
        Some(s) => s,
        None => {
            debug!(?value, "non-string model value, skipping");
            return None;
        }
    };

    let trimmed = s.trim();

    if trimmed.is_empty() {
        return None;
    }

    if trimmed == "<synthetic>" {
        return None;
    }

    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_model_slug_preserved() {
        let v = json!("claude-opus-4-6");
        assert_eq!(
            normalize_model(Some(&v)),
            Some("claude-opus-4-6".to_string())
        );
    }

    #[test]
    fn none_input_returns_none() {
        assert_eq!(normalize_model(None), None);
    }

    #[test]
    fn non_string_returns_none() {
        let v = json!(42);
        assert_eq!(normalize_model(Some(&v)), None);
    }

    #[test]
    fn empty_string_returns_none() {
        let v = json!("");
        assert_eq!(normalize_model(Some(&v)), None);
    }

    #[test]
    fn whitespace_only_returns_none() {
        let v = json!("   ");
        assert_eq!(normalize_model(Some(&v)), None);
    }

    #[test]
    fn synthetic_sentinel_returns_none() {
        let v = json!("<synthetic>");
        assert_eq!(normalize_model(Some(&v)), None);
    }

    #[test]
    fn whitespace_trimmed() {
        let v = json!("  gpt-4o  ");
        assert_eq!(normalize_model(Some(&v)), Some("gpt-4o".to_string()));
    }

    #[test]
    fn null_value_returns_none() {
        let v = json!(null);
        assert_eq!(normalize_model(Some(&v)), None);
    }

    #[test]
    fn object_value_returns_none() {
        let v = json!({"id": "model"});
        assert_eq!(normalize_model(Some(&v)), None);
    }
}
