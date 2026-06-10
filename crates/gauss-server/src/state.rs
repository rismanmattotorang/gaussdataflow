use std::sync::Arc;

use gauss_secrets::SecretsBackend;
use gauss_store::Store;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub secrets: Arc<dyn SecretsBackend>,
}

impl AppState {
    /// Standard wiring: Postgres store with its co-located secrets backend.
    pub fn new(store: Store) -> Self {
        let secrets = Arc::new(store.secrets_backend());
        Self { store, secrets }
    }
}
