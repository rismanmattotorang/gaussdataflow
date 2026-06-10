//! HashiCorp Vault backend (KV v2) — production-grade storage for raw
//! secret values, behind the same [`SecretsBackend`] trait as the local
//! Postgres backend. Configure with the Vault address, a token, the KV
//! mount, and a path prefix; values live at
//! `<mount>/data/<prefix>/<secret-id>`.

use serde_json::{json, Value};

use crate::{SecretsBackend, SecretsError};

pub struct VaultSecretsBackend {
    address: String,
    token: String,
    mount: String,
    prefix: String,
    client: reqwest::Client,
}

impl VaultSecretsBackend {
    pub fn new(
        address: impl Into<String>,
        token: impl Into<String>,
        mount: impl Into<String>,
        prefix: impl Into<String>,
    ) -> Self {
        Self {
            address: address.into().trim_end_matches('/').to_string(),
            token: token.into(),
            mount: mount.into(),
            prefix: prefix.into(),
            client: reqwest::Client::new(),
        }
    }

    fn url(&self, id: &str) -> String {
        format!(
            "{}/v1/{}/data/{}/{}",
            self.address, self.mount, self.prefix, id
        )
    }
}

fn transport(err: reqwest::Error) -> SecretsError {
    SecretsError::Backend(format!("vault request failed: {err}"))
}

#[async_trait::async_trait]
impl SecretsBackend for VaultSecretsBackend {
    async fn put(&self, id: &str, value: &str) -> Result<(), SecretsError> {
        let response = self
            .client
            .post(self.url(id))
            .header("X-Vault-Token", &self.token)
            .json(&json!({"data": {"value": value}}))
            .send()
            .await
            .map_err(transport)?;
        if !response.status().is_success() {
            return Err(SecretsError::Backend(format!(
                "vault write returned {}",
                response.status()
            )));
        }
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<String, SecretsError> {
        let response = self
            .client
            .get(self.url(id))
            .header("X-Vault-Token", &self.token)
            .send()
            .await
            .map_err(transport)?;
        if response.status().as_u16() == 404 {
            return Err(SecretsError::NotFound(id.to_string()));
        }
        if !response.status().is_success() {
            return Err(SecretsError::Backend(format!(
                "vault read returned {}",
                response.status()
            )));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|e| SecretsError::Backend(format!("vault returned non-JSON: {e}")))?;
        body["data"]["data"]["value"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| SecretsError::NotFound(id.to_string()))
    }

    async fn delete(&self, id: &str) -> Result<(), SecretsError> {
        let response = self
            .client
            .delete(self.url(id))
            .header("X-Vault-Token", &self.token)
            .send()
            .await
            .map_err(transport)?;
        // Deleting a missing secret is fine; surface other failures.
        if !response.status().is_success() && response.status().as_u16() != 404 {
            return Err(SecretsError::Backend(format!(
                "vault delete returned {}",
                response.status()
            )));
        }
        Ok(())
    }
}
