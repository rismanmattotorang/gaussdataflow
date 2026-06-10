//! API-token authentication, role-based authorization, and audit logging.
//!
//! Tokens are bearer credentials (`Authorization: Bearer gauss_…`); only
//! their SHA-256 hash is stored. Three roles:
//!
//! | role   | may                                            |
//! |--------|------------------------------------------------|
//! | viewer | read everything (GET)                          |
//! | editor | viewer + all mutations except token management |
//! | admin  | everything, incl. `/tokens` and `/audit`       |
//!
//! With `--require-auth` every `/api/v1` request needs a valid token; without
//! it the API is open (dev mode) but presented tokens are still validated and
//! attributed. Every mutating request is recorded in the audit log either
//! way.

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sha2::Digest;

use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Editor,
    Admin,
}

impl Role {
    pub fn parse(role: &str) -> Option<Self> {
        match role {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub subject: String,
    pub role: Role,
}

pub fn generate_token() -> String {
    format!(
        "gauss_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn hash_token(raw: &str) -> String {
    let digest = sha2::Sha256::digest(raw.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn required_role(method: &Method, path: &str) -> Role {
    if path.starts_with("/api/v1/tokens") || path.starts_with("/api/v1/audit") {
        return Role::Admin;
    }
    match *method {
        Method::GET | Method::HEAD => Role::Viewer,
        _ => Role::Editor,
    }
}

fn deny(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"message": message}))).into_response()
}

pub async fn layer(State(state): State<AppState>, mut req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    // Health stays open; CORS preflight is handled by the CORS layer.
    if !path.starts_with("/api/") || req.method() == Method::OPTIONS {
        return next.run(req).await;
    }

    let bearer = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string);

    let context = match bearer {
        Some(raw) => {
            // A presented token must be valid even in open mode.
            match state.store.tokens().authenticate(&hash_token(&raw)).await {
                Ok(Some(token)) => match Role::parse(&token.role) {
                    Some(role) => AuthContext {
                        subject: token.name,
                        role,
                    },
                    None => return deny(StatusCode::FORBIDDEN, "token has an unknown role"),
                },
                Ok(None) => return deny(StatusCode::UNAUTHORIZED, "invalid API token"),
                Err(_) => return deny(StatusCode::INTERNAL_SERVER_ERROR, "token lookup failed"),
            }
        }
        None if state.require_auth => {
            return deny(
                StatusCode::UNAUTHORIZED,
                "missing API token (Authorization: Bearer …)",
            )
        }
        None => AuthContext {
            subject: "anonymous".to_string(),
            role: Role::Admin,
        },
    };

    if context.role < required_role(req.method(), &path) {
        return deny(
            StatusCode::FORBIDDEN,
            "this token's role does not permit the operation",
        );
    }

    let method = req.method().clone();
    let mutating = !matches!(method, Method::GET | Method::HEAD);
    let subject = context.subject.clone();
    req.extensions_mut().insert(context);
    let response = next.run(req).await;

    if mutating {
        let status = response.status().as_u16() as i32;
        let store = state.store.clone();
        // Best-effort, off the request path.
        tokio::spawn(async move {
            if let Err(err) = store
                .audit()
                .record(&subject, method.as_str(), &path, status)
                .await
            {
                tracing::warn!(%err, "audit write failed");
            }
        });
    }
    response
}
