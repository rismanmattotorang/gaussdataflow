use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use gauss_secrets::SecretsBackend;
use gauss_store::Store;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub secrets: Arc<dyn SecretsBackend>,
    /// When true every `/api/v1` request needs a valid API token.
    pub require_auth: bool,
    /// Allowed CORS origins. Empty means permissive (self-hosted default);
    /// production deployments pin the console's origin(s).
    pub cors_origins: Vec<axum::http::HeaderValue>,
    /// Outstanding OAuth CSRF states (value = issue time, TTL-checked).
    pub oauth_states: Arc<tokio::sync::Mutex<HashMap<String, Instant>>>,
}

impl AppState {
    /// Standard wiring: Postgres store with its co-located secrets backend.
    pub fn new(store: Store) -> Self {
        let secrets = Arc::new(store.secrets_backend());
        Self::with_secrets(store, secrets)
    }

    /// Custom secrets backend (e.g. Vault).
    pub fn with_secrets(store: Store, secrets: Arc<dyn SecretsBackend>) -> Self {
        Self {
            store,
            secrets,
            require_auth: false,
            cors_origins: Vec::new(),
            oauth_states: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn require_auth(mut self, on: bool) -> Self {
        self.require_auth = on;
        self
    }

    pub fn cors_origins(mut self, origins: Vec<axum::http::HeaderValue>) -> Self {
        self.cors_origins = origins;
        self
    }
}
