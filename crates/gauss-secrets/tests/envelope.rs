use gauss_secrets::*;
use serde_json::json;

fn pg_like_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "host": {"type": "string"},
            "password": {"type": "string", "airbyte_secret": true},
            "tunnel": {
                "type": "object",
                "properties": {
                    "ssh_key": {"type": "string", "airbyte_secret": true},
                    "port": {"type": "integer"}
                }
            },
            "auth": {
                "oneOf": [
                    {"properties": {"method": {"const": "token"}, "token": {"type": "string", "airbyte_secret": true}}},
                    {"properties": {"method": {"const": "none"}}}
                ]
            }
        }
    })
}

#[test]
fn split_extracts_marked_fields_only() {
    let config = json!({
        "host": "db.example.com",
        "password": "hunter2",
        "tunnel": {"ssh_key": "PRIVATE", "port": 22},
        "auth": {"method": "token", "token": "tok-123"}
    });

    let (redacted, secrets) = split_config(&pg_like_schema(), &config);

    assert_eq!(secrets.len(), 3);
    let values: Vec<&str> = secrets.iter().map(|(_, v)| v.as_str()).collect();
    assert!(values.contains(&"hunter2"));
    assert!(values.contains(&"PRIVATE"));
    assert!(values.contains(&"tok-123"));

    // Non-secrets untouched; secrets replaced by references.
    assert_eq!(redacted["host"], "db.example.com");
    assert_eq!(redacted["tunnel"]["port"], 22);
    assert!(redacted["password"]["_secret"].is_string());
    assert!(redacted["tunnel"]["ssh_key"]["_secret"].is_string());
    assert!(redacted["auth"]["token"]["_secret"].is_string());

    // Raw values must not appear anywhere in the redacted form.
    let serialized = redacted.to_string();
    assert!(!serialized.contains("hunter2"));
    assert!(!serialized.contains("PRIVATE"));
    assert!(!serialized.contains("tok-123"));
}

#[test]
fn split_preserves_existing_refs() {
    // A config update that round-trips the stored (already redacted) form
    // must not double-wrap or lose references.
    let stored = json!({
        "host": "new-host.example.com",
        "password": {"_secret": "11111111-1111-1111-1111-111111111111"}
    });
    let (redacted, secrets) = split_config(&pg_like_schema(), &stored);
    assert!(secrets.is_empty());
    assert_eq!(
        redacted["password"]["_secret"],
        "11111111-1111-1111-1111-111111111111"
    );
}

#[tokio::test]
async fn hydrate_restores_original_config() {
    let backend = MemorySecretsBackend::default();
    let config = json!({
        "host": "db.example.com",
        "password": "hunter2",
        "tunnel": {"ssh_key": "PRIVATE", "port": 22}
    });

    let (redacted, secrets) = split_config(&pg_like_schema(), &config);
    for (id, value) in &secrets {
        backend.put(id, value).await.unwrap();
    }

    let hydrated = hydrate_config(&redacted, &backend).await.unwrap();
    assert_eq!(hydrated, config);
}

#[tokio::test]
async fn hydrate_fails_on_missing_secret() {
    let backend = MemorySecretsBackend::default();
    let config = json!({"password": {"_secret": "missing-id"}});
    let err = hydrate_config(&config, &backend).await.unwrap_err();
    assert!(matches!(err, SecretsError::NotFound(_)));
}

#[test]
fn collect_refs_finds_nested_references() {
    let config = json!({
        "a": {"_secret": "id-1"},
        "nested": {"b": {"_secret": "id-2"}},
        "list": [{"c": {"_secret": "id-3"}}]
    });
    let mut refs = collect_refs(&config);
    refs.sort();
    assert_eq!(refs, vec!["id-1", "id-2", "id-3"]);
}
