//! Typed client for the gauss-server REST control plane.
//!
//! The TUI talks to the same `/api/v1` surface as the web console and MCP
//! gateway, so it works against any deployment — local or remote — and
//! respects `--require-auth` via a bearer token.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    #[serde(rename = "workspaceId")]
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    #[serde(rename = "connectionId")]
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub schedule: Option<Value>,
}

impl Connection {
    /// Human form of the schedule JSON: "every 30m", "cron 0 * * * *", "manual".
    pub fn schedule_label(&self) -> String {
        match &self.schedule {
            Some(s) => {
                if let Some(m) = s.get("intervalMinutes").and_then(Value::as_i64) {
                    format!("every {m}m")
                } else if let Some(c) = s.get("cron").and_then(Value::as_str) {
                    format!("cron {c}")
                } else {
                    "custom".to_string()
                }
            }
            None => "manual".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: i64,
    pub job_type: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOverview {
    pub id: i64,
    pub connection_id: Uuid,
    pub connection_name: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub records_synced: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStats {
    pub sources: i64,
    pub destinations: i64,
    pub connections: i64,
    pub jobs_pending: i64,
    pub jobs_running: i64,
    pub jobs_succeeded_24h: i64,
    pub jobs_failed_24h: i64,
    pub records_synced_24h: i64,
    pub last_success_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub attempt_number: i32,
    pub status: String,
    pub records_synced: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDetail {
    pub id: i64,
    pub status: String,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
}

#[derive(Deserialize)]
struct DataEnvelope<T> {
    data: Vec<T>,
}

#[derive(Deserialize)]
struct StateEnvelope {
    state: Option<Value>,
}

impl ApiClient {
    pub fn new(base: String, token: Option<String>) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            // Bounded timeouts: a hung or unreachable API surfaces as the
            // offline indicator instead of silently stalling the fetch task
            // (commands are handled sequentially behind it).
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("reqwest client"),
        }
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> anyhow::Result<T> {
        let req = match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        };
        let res = req.send().await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(Value::as_str).map(String::from))
                .unwrap_or_else(|| format!("HTTP {status}"));
            anyhow::bail!(msg);
        }
        Ok(serde_json::from_str(&body)?)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        self.send(self.http.get(format!("{}{path}", self.base)))
            .await
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: Option<Value>,
    ) -> anyhow::Result<T> {
        let mut req = self.http.post(format!("{}{path}", self.base));
        if let Some(b) = body {
            req = req.json(&b);
        }
        self.send(req).await
    }

    async fn patch<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: Value,
    ) -> anyhow::Result<T> {
        self.send(self.http.patch(format!("{}{path}", self.base)).json(&body))
            .await
    }

    pub async fn workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        Ok(self
            .get::<DataEnvelope<Workspace>>("/api/v1/workspaces")
            .await?
            .data)
    }

    pub async fn create_workspace(&self, name: &str) -> anyhow::Result<Workspace> {
        self.post(
            "/api/v1/workspaces",
            Some(serde_json::json!({ "name": name })),
        )
        .await
    }

    pub async fn stats(&self, workspace: Option<Uuid>) -> anyhow::Result<PlatformStats> {
        let path = match workspace {
            Some(id) => format!("/api/v1/stats?workspaceId={id}"),
            None => "/api/v1/stats".to_string(),
        };
        self.get(&path).await
    }

    pub async fn recent_jobs(
        &self,
        workspace: Option<Uuid>,
        limit: usize,
    ) -> anyhow::Result<Vec<JobOverview>> {
        let path = match workspace {
            Some(id) => format!("/api/v1/jobs?workspaceId={id}&limit={limit}"),
            None => format!("/api/v1/jobs?limit={limit}"),
        };
        Ok(self.get::<DataEnvelope<JobOverview>>(&path).await?.data)
    }

    pub async fn connections(&self, workspace: Uuid) -> anyhow::Result<Vec<Connection>> {
        Ok(self
            .get::<DataEnvelope<Connection>>(&format!(
                "/api/v1/connections?workspaceId={workspace}"
            ))
            .await?
            .data)
    }

    pub async fn actors(&self, workspace: Uuid, kind: &str) -> anyhow::Result<Vec<Actor>> {
        Ok(self
            .get::<DataEnvelope<Actor>>(&format!("/api/v1/{kind}?workspaceId={workspace}"))
            .await?
            .data)
    }

    pub async fn connection(&self, id: Uuid) -> anyhow::Result<Connection> {
        self.get(&format!("/api/v1/connections/{id}")).await
    }

    pub async fn set_connection_status(
        &self,
        id: Uuid,
        status: &str,
    ) -> anyhow::Result<Connection> {
        self.patch(
            &format!("/api/v1/connections/{id}"),
            serde_json::json!({ "status": status }),
        )
        .await
    }

    pub async fn connection_jobs(&self, connection: Uuid) -> anyhow::Result<Vec<Job>> {
        Ok(self
            .get::<DataEnvelope<Job>>(&format!("/api/v1/connections/{connection}/jobs"))
            .await?
            .data)
    }

    pub async fn connection_state(&self, connection: Uuid) -> anyhow::Result<Option<Value>> {
        Ok(self
            .get::<StateEnvelope>(&format!("/api/v1/connections/{connection}/state"))
            .await?
            .state)
    }

    pub async fn trigger_sync(&self, connection: Uuid) -> anyhow::Result<Job> {
        self.post(&format!("/api/v1/connections/{connection}/sync"), None)
            .await
    }

    pub async fn cancel_job(&self, job: i64) -> anyhow::Result<Job> {
        self.post(&format!("/api/v1/jobs/{job}/cancel"), None).await
    }

    pub async fn job_detail(&self, job: i64) -> anyhow::Result<JobDetail> {
        self.get(&format!("/api/v1/jobs/{job}")).await
    }
}
