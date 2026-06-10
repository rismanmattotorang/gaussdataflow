//! Phase-6 governance tests: token auth + RBAC, audit logging, OAuth2
//! exchange, and deployment import. Skips without `DATABASE_URL`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use gauss_server::{auth, AppState};
use gauss_store::Store;
use serde_json::{json, Value};
use tower::util::ServiceExt;

async fn test_state() -> Option<AppState> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping");
        return None;
    };
    let admin = sqlx::PgPool::connect(&url).await.expect("admin connect");
    let name = format!("gauss_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
        .execute(&admin)
        .await
        .expect("create test database");
    let (base, _) = url.rsplit_once('/').unwrap();
    let store = Store::connect(&format!("{base}/{name}")).await.unwrap();
    Some(AppState::new(store))
}

/// Mint a token directly in the store; returns the raw bearer value.
async fn mint(state: &AppState, name: &str, role: &str) -> String {
    let raw = auth::generate_token();
    state
        .store
        .tokens()
        .create(name, role, &auth::hash_token(&raw))
        .await
        .unwrap();
    raw
}

async fn req(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        Some(json) => {
            builder = builder.header("content-type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

#[tokio::test]
async fn rbac_enforcement() {
    let Some(state) = test_state().await else {
        return;
    };
    let viewer = mint(&state, "ro", "viewer").await;
    let editor = mint(&state, "rw", "editor").await;
    let admin = mint(&state, "root", "admin").await;
    let app = gauss_server::app(state.clone().require_auth(true));

    // No token / bad token → 401. Health stays open.
    let (status, _) = req(&app, "GET", "/api/v1/workspaces", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = req(&app, "GET", "/api/v1/workspaces", Some("gauss_bogus"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = req(&app, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);

    // Viewer: read yes, write no.
    let (status, _) = req(&app, "GET", "/api/v1/workspaces", Some(&viewer), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(&viewer),
        Some(json!({"name": "nope"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Editor: writes yes, token management no.
    let (status, _) = req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(&editor),
        Some(json!({"name": "ws"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = req(&app, "GET", "/api/v1/tokens", Some(&editor), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = req(&app, "GET", "/api/v1/audit", Some(&editor), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Admin: mints a token over the API; raw value returned exactly once.
    let (status, minted) = req(
        &app,
        "POST",
        "/api/v1/tokens",
        Some(&admin),
        Some(json!({"name": "ci", "role": "viewer"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{minted}");
    let ci_token = minted["token"].as_str().unwrap().to_string();
    assert!(ci_token.starts_with("gauss_"));
    let (status, listed) = req(&app, "GET", "/api/v1/tokens", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !listed.to_string().contains(&ci_token),
        "raw token never listed"
    );

    // The minted token works, then revocation kills it.
    let (status, _) = req(&app, "GET", "/api/v1/workspaces", Some(&ci_token), None).await;
    assert_eq!(status, StatusCode::OK);
    let id = minted["id"].as_str().unwrap();
    let (status, _) = req(
        &app,
        "DELETE",
        &format!("/api/v1/tokens/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = req(&app, "GET", "/api/v1/workspaces", Some(&ci_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Bad role rejected.
    let (status, _) = req(
        &app,
        "POST",
        "/api/v1/tokens",
        Some(&admin),
        Some(json!({"name": "x", "role": "superuser"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn audit_records_mutations() {
    let Some(state) = test_state().await else {
        return;
    };
    let editor = mint(&state, "deployer", "editor").await;
    let admin = mint(&state, "root", "admin").await;
    let app = gauss_server::app(state.clone().require_auth(true));

    req(
        &app,
        "POST",
        "/api/v1/workspaces",
        Some(&editor),
        Some(json!({"name": "audited"})),
    )
    .await;

    // The audit write is fire-and-forget; poll briefly.
    let mut entries = Value::Null;
    for _ in 0..40 {
        let (_, body) = req(&app, "GET", "/api/v1/audit", Some(&admin), None).await;
        if body["data"].as_array().is_some_and(|a| !a.is_empty()) {
            entries = body;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let entry = &entries["data"][0];
    assert_eq!(entry["subject"], "deployer");
    assert_eq!(entry["method"], "POST");
    assert_eq!(entry["path"], "/api/v1/workspaces");
    assert_eq!(entry["status"], 201);
}

#[tokio::test]
async fn oauth_exchange_seals_tokens() {
    let Some(state) = test_state().await else {
        return;
    };
    let app = gauss_server::app(state.clone());

    // Fake provider: /token validates the form and returns credentials.
    let provider = {
        use axum::routing::post;
        async fn token(axum::Form(form): axum::Form<Value>) -> axum::Json<Value> {
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["code"], "the-code");
            axum::Json(json!({
                "access_token": "at-123",
                "refresh_token": "rt-456",
                "expires_in": 3600,
                "token_type": "Bearer"
            }))
        }
        let router = axum::Router::new().route("/token", post(token));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{addr}")
    };

    let (status, authorize) = req(
        &app,
        "POST",
        "/api/v1/oauth/authorize_url",
        None,
        Some(json!({
            "authorizationUrl": "https://provider.example.com/oauth/authorize",
            "clientId": "client-1",
            "redirectUri": "http://localhost:3000/callback",
            "scopes": ["read", "write"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let url = authorize["url"].as_str().unwrap();
    let csrf = authorize["state"].as_str().unwrap().to_string();
    assert!(url.contains("response_type=code"));
    assert!(url.contains("scope=read%20write"));
    assert!(url.contains(&format!("state={csrf}")));

    let complete_body = json!({
        "tokenUrl": format!("{provider}/token"),
        "clientId": "client-1",
        "clientSecret": "shh",
        "code": "the-code",
        "redirectUri": "http://localhost:3000/callback",
        "state": csrf
    });
    let (status, completed) = req(
        &app,
        "POST",
        "/api/v1/oauth/complete",
        None,
        Some(complete_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{completed}");

    // Token material is sealed, not returned raw; metadata passes through.
    let credentials = &completed["credentials"];
    assert!(!completed.to_string().contains("at-123"));
    assert!(!completed.to_string().contains("rt-456"));
    let access_ref = credentials["access_token"]["_secret"].as_str().unwrap();
    assert_eq!(state.secrets.get(access_ref).await.unwrap(), "at-123");
    assert_eq!(credentials["expires_in"], 3600);

    // The state is single-use.
    let (status, _) = req(
        &app,
        "POST",
        "/api/v1/oauth/complete",
        None,
        Some(complete_body),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn deployment_import_round_trip() {
    let Some(state) = test_state().await else {
        return;
    };
    let doc: gauss_server::import::ImportDocument = serde_json::from_value(json!({
        "workspace": "imported",
        "sources": [{
            "name": "crm",
            "definition": {
                "name": "Test API",
                "dockerRepository": "example/source-crm",
                "dockerImageTag": "1.0",
                "spec": {"connectionSpecification": {
                    "type": "object",
                    "properties": {"api_key": {"type": "string", "airbyte_secret": true}}
                }}
            },
            "configuration": {"api_key": "raw-import-secret"}
        }],
        "destinations": [{
            "name": "warehouse",
            "definition": {
                "name": "Test WH",
                "dockerRepository": "example/destination-wh",
                "dockerImageTag": "1.0"
            },
            "configuration": {"host": "wh.internal"}
        }],
        "connections": [{
            "name": "crm → warehouse",
            "source": "crm",
            "destination": "warehouse",
            "catalog": {"streams": [{
                "stream": {"name": "accounts", "json_schema": {}},
                "sync_mode": "full_refresh",
                "destination_sync_mode": "append"
            }]},
            "schedule": {"intervalMinutes": 120},
            "notifications": {"webhookUrl": "https://hooks.example.com/x"}
        }]
    }))
    .unwrap();

    let summary = gauss_server::import::import(&state.store, state.secrets.as_ref(), doc)
        .await
        .unwrap();
    assert_eq!(summary.sources, 1);
    assert_eq!(summary.destinations, 1);
    assert_eq!(summary.connections, 1);

    // The imported secret is sealed, the connection fully wired.
    let workspace = &state.store.workspaces().list().await.unwrap()[0];
    let sources = state
        .store
        .actors()
        .list(workspace.id, gauss_store::ActorType::Source)
        .await
        .unwrap();
    let config = serde_json::to_string(&sources[0].configuration.0);
    assert!(!config.unwrap().contains("raw-import-secret"));
    let connections = state.store.connections().list(workspace.id).await.unwrap();
    assert_eq!(connections[0].name, "crm → warehouse");
    assert_eq!(
        connections[0].notifications.as_ref().unwrap().0["webhookUrl"],
        "https://hooks.example.com/x"
    );

    // Unknown actor references fail loudly.
    let bad: gauss_server::import::ImportDocument = serde_json::from_value(json!({
        "workspace": "broken",
        "connections": [{
            "name": "dangling", "source": "ghost", "destination": "ghost",
            "catalog": {"streams": []}
        }]
    }))
    .unwrap();
    assert!(
        gauss_server::import::import(&state.store, state.secrets.as_ref(), bad)
            .await
            .is_err()
    );
}
