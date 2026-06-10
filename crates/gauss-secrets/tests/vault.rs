//! VaultSecretsBackend against a fake Vault KV-v2 server: write/read/delete,
//! token enforcement, and not-found semantics.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::Json;
use gauss_secrets::{SecretsBackend, SecretsError, VaultSecretsBackend};
use serde_json::{json, Value};

type Vault = Arc<Mutex<HashMap<String, String>>>;

fn authed(headers: &HeaderMap) -> bool {
    headers.get("x-vault-token").and_then(|v| v.to_str().ok()) == Some("root-token")
}

async fn fake_vault() -> String {
    async fn write(
        State(vault): State<Vault>,
        Path(path): Path<String>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        if !authed(&headers) {
            return StatusCode::FORBIDDEN;
        }
        let value = body["data"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        vault.lock().unwrap().insert(path, value);
        StatusCode::OK
    }
    async fn read(
        State(vault): State<Vault>,
        Path(path): Path<String>,
        headers: HeaderMap,
    ) -> Result<Json<Value>, StatusCode> {
        if !authed(&headers) {
            return Err(StatusCode::FORBIDDEN);
        }
        match vault.lock().unwrap().get(&path) {
            Some(value) => Ok(Json(json!({"data": {"data": {"value": value}}}))),
            None => Err(StatusCode::NOT_FOUND),
        }
    }
    async fn remove(
        State(vault): State<Vault>,
        Path(path): Path<String>,
        headers: HeaderMap,
    ) -> StatusCode {
        if !authed(&headers) {
            return StatusCode::FORBIDDEN;
        }
        vault.lock().unwrap().remove(&path);
        StatusCode::NO_CONTENT
    }

    let vault: Vault = Arc::new(Mutex::new(HashMap::new()));
    let app = axum::Router::new()
        .route("/v1/secret/data/{*path}", post(write))
        .route("/v1/secret/data/{*path}", get(read))
        .route("/v1/secret/data/{*path}", delete(remove))
        .with_state(vault);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn vault_backend_round_trip() {
    let addr = fake_vault().await;
    let backend = VaultSecretsBackend::new(&addr, "root-token", "secret", "gaussdataflow");

    backend.put("id-1", "hunter2").await.unwrap();
    assert_eq!(backend.get("id-1").await.unwrap(), "hunter2");

    // Overwrite.
    backend.put("id-1", "rotated").await.unwrap();
    assert_eq!(backend.get("id-1").await.unwrap(), "rotated");

    backend.delete("id-1").await.unwrap();
    assert!(matches!(
        backend.get("id-1").await,
        Err(SecretsError::NotFound(_))
    ));
    // Deleting again is fine.
    backend.delete("id-1").await.unwrap();
}

#[tokio::test]
async fn vault_backend_full_envelope_flow() {
    let addr = fake_vault().await;
    let backend = VaultSecretsBackend::new(&addr, "root-token", "secret", "gaussdataflow");

    // The same split/hydrate cycle the platform runs, against Vault.
    let schema = json!({"type": "object", "properties": {
        "password": {"type": "string", "gauss_secret": true}
    }});
    let config = json!({"host": "db", "password": "pg-pass"});
    let (redacted, secrets) = gauss_secrets::split_config(&schema, &config);
    for (id, value) in &secrets {
        backend.put(id, value).await.unwrap();
    }
    let hydrated = gauss_secrets::hydrate_config(&redacted, &backend)
        .await
        .unwrap();
    assert_eq!(hydrated, config);
}

#[tokio::test]
async fn vault_backend_rejects_bad_token() {
    let addr = fake_vault().await;
    let backend = VaultSecretsBackend::new(&addr, "wrong-token", "secret", "gaussdataflow");
    assert!(matches!(
        backend.put("id", "v").await,
        Err(SecretsError::Backend(_))
    ));
}
