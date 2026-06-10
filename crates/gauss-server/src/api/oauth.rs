//! Generic OAuth2 authorization-code plumbing for connector credentials.
//!
//! The platform doesn't hardcode providers: the client supplies the
//! provider's endpoints (from the connector's `advanced_auth` spec or its
//! docs), the server contributes what must not live in a browser — CSRF
//! state issuance/validation and the code-for-token exchange with the client
//! secret. Returned tokens are sealed into the secrets backend immediately;
//! the response carries `{"_secret": id}` references ready to paste into a
//! source configuration.

use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::ApiError;
use crate::AppState;

const STATE_TTL: Duration = Duration::from_secs(600);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeUrlRequest {
    pub authorization_url: String,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub extra_params: Option<Value>,
}

pub async fn authorize_url(
    State(state): State<AppState>,
    Json(body): Json<AuthorizeUrlRequest>,
) -> Result<Json<Value>, ApiError> {
    let csrf = uuid::Uuid::new_v4().simple().to_string();
    {
        let mut states = state.oauth_states.lock().await;
        states.retain(|_, issued| issued.elapsed() < STATE_TTL);
        states.insert(csrf.clone(), Instant::now());
    }

    let mut params = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), body.client_id),
        ("redirect_uri".to_string(), body.redirect_uri),
        ("state".to_string(), csrf.clone()),
    ];
    if !body.scopes.is_empty() {
        params.push(("scope".to_string(), body.scopes.join(" ")));
    }
    if let Some(Value::Object(extra)) = body.extra_params {
        for (key, value) in extra {
            params.push((
                key,
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string()),
            ));
        }
    }
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let separator = if body.authorization_url.contains('?') {
        '&'
    } else {
        '?'
    };

    Ok(Json(json!({
        "url": format!("{}{}{}", body.authorization_url, separator, query),
        "state": csrf,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteRequest {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub code: String,
    pub redirect_uri: String,
    pub state: String,
}

pub async fn complete(
    State(state): State<AppState>,
    Json(body): Json<CompleteRequest>,
) -> Result<Json<Value>, ApiError> {
    // CSRF: the state must be one we issued, recently, exactly once.
    let valid = {
        let mut states = state.oauth_states.lock().await;
        states
            .remove(&body.state)
            .map(|issued| issued.elapsed() < STATE_TTL)
            .unwrap_or(false)
    };
    if !valid {
        return Err(ApiError::bad_request("unknown or expired OAuth state"));
    }

    let response = reqwest::Client::new()
        .post(&body.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &body.code),
            ("client_id", &body.client_id),
            ("client_secret", &body.client_secret),
            ("redirect_uri", &body.redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("token exchange request failed: {e}"),
        })?;
    let status = response.status();
    let payload: Value = response.json().await.map_err(|e| ApiError {
        status: StatusCode::BAD_GATEWAY,
        message: format!("provider returned non-JSON token response: {e}"),
    })?;
    if !status.is_success() {
        return Err(ApiError::bad_request(format!(
            "provider rejected the code exchange ({status}): {payload}"
        )));
    }

    // Seal token material; pass through non-sensitive metadata.
    let mut credentials = serde_json::Map::new();
    for field in ["access_token", "refresh_token", "id_token"] {
        if let Some(Value::String(value)) = payload.get(field) {
            let id = uuid::Uuid::new_v4().to_string();
            state.secrets.put(&id, value).await?;
            credentials.insert(field.to_string(), json!({"_secret": id}));
        }
    }
    for field in ["expires_in", "token_type", "scope"] {
        if let Some(value) = payload.get(field) {
            credentials.insert(field.to_string(), value.clone());
        }
    }
    Ok(Json(json!({ "credentials": credentials })))
}

fn urlencode(input: &str) -> String {
    input
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
