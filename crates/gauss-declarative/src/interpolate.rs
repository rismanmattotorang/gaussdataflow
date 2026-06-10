//! `{{ config.key }}` interpolation for manifest strings.
//!
//! Supported forms: `{{ config.api_key }}`, `{{ config['api_key'] }}`,
//! `{{ config["api_key"] }}`. Values must be scalars; unknown references are
//! config errors (silent empty strings hide typos in credentials).

use gauss_cdk::CdkError;
use serde_json::Value;

pub fn interpolate(template: &str, config: &Value) -> Result<String, CdkError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| CdkError::Config(format!("unclosed `{{{{` in template `{template}`")))?;
        let expr = after[..end].trim();
        out.push_str(&resolve(expr, config, template)?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve(expr: &str, config: &Value, template: &str) -> Result<String, CdkError> {
    let key = expr
        .strip_prefix("config")
        .and_then(|rest| {
            rest.strip_prefix('.').map(str::to_string).or_else(|| {
                rest.strip_prefix("['")
                    .and_then(|r| r.strip_suffix("']"))
                    .or_else(|| rest.strip_prefix("[\"").and_then(|r| r.strip_suffix("\"]")))
                    .map(str::to_string)
            })
        })
        .ok_or_else(|| {
            CdkError::Config(format!(
                "unsupported expression `{{{{ {expr} }}}}` in `{template}` \
                 (only `config.<key>` references are supported)"
            ))
        })?;

    match config.get(key.trim()) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(Value::Bool(b)) => Ok(b.to_string()),
        Some(other) => Err(CdkError::Config(format!(
            "config key `{key}` is not a scalar (found {other})"
        ))),
        None => Err(CdkError::Config(format!(
            "config key `{key}` referenced by `{template}` is missing"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn interpolates_all_forms() {
        let config = json!({"api_key": "sk-123", "port": 5432, "tls": true});
        for template in [
            "{{ config.api_key }}",
            "{{config.api_key}}",
            "{{ config['api_key'] }}",
            "{{ config[\"api_key\"] }}",
        ] {
            assert_eq!(interpolate(template, &config).unwrap(), "sk-123");
        }
        assert_eq!(
            interpolate("host:{{ config.port }}/{{ config.tls }}", &config).unwrap(),
            "host:5432/true"
        );
        assert_eq!(
            interpolate("no templates", &config).unwrap(),
            "no templates"
        );
    }

    #[test]
    fn rejects_unknown_and_malformed() {
        let config = json!({});
        assert!(interpolate("{{ config.missing }}", &config).is_err());
        assert!(interpolate("{{ secrets.x }}", &config).is_err());
        assert!(interpolate("{{ config.x", &config).is_err());
    }
}
