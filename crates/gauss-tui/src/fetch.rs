//! Background data plane for the TUI: the render loop stays non-blocking by
//! sending [`Command`]s here and draining [`Update`]s each frame.

use serde_json::Value;
use uuid::Uuid;

use crate::api::{
    Actor, ApiClient, Connection, Job, JobDetail, JobOverview, PlatformStats, Workspace,
};

#[derive(Debug)]
pub enum Command {
    Home,
    Workspace(Uuid),
    Connection(Uuid),
    JobDetail(i64),
    TriggerSync(Uuid),
    CancelJob(i64),
    SetConnectionStatus { id: Uuid, status: &'static str },
    CreateWorkspace(String),
}

pub enum Update {
    Home {
        workspaces: Vec<Workspace>,
        stats: PlatformStats,
        jobs: Vec<JobOverview>,
    },
    Workspace {
        id: Uuid,
        stats: PlatformStats,
        connections: Vec<Connection>,
        sources: Vec<Actor>,
        destinations: Vec<Actor>,
        jobs: Vec<JobOverview>,
    },
    Connection {
        connection: Connection,
        jobs: Vec<Job>,
        state: Option<Value>,
    },
    JobDetail(JobDetail),
    /// A successful mutation; the app re-fetches its current screen.
    Notice(String),
    /// A failed mutation: surfaced in the footer.
    Error(String),
    /// A failed screen load: the API is unreachable or unhealthy. Shown as a
    /// persistent offline indicator rather than a transient notice.
    RefreshFailed(String),
}

pub async fn run(
    api: ApiClient,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    updates: std::sync::mpsc::Sender<Update>,
) {
    while let Some(cmd) = commands.recv().await {
        let update = handle(&api, cmd).await;
        if updates.send(update).is_err() {
            return; // UI is gone
        }
    }
}

async fn handle(api: &ApiClient, cmd: Command) -> Update {
    match cmd {
        Command::Home => {
            match tokio::try_join!(api.workspaces(), api.stats(None), api.recent_jobs(None, 30)) {
                Ok((workspaces, stats, jobs)) => Update::Home {
                    workspaces,
                    stats,
                    jobs,
                },
                Err(e) => Update::RefreshFailed(e.to_string()),
            }
        }
        Command::Workspace(id) => {
            match tokio::try_join!(
                api.stats(Some(id)),
                api.connections(id),
                api.actors(id, "sources"),
                api.actors(id, "destinations"),
                api.recent_jobs(Some(id), 50),
            ) {
                Ok((stats, connections, sources, destinations, jobs)) => Update::Workspace {
                    id,
                    stats,
                    connections,
                    sources,
                    destinations,
                    jobs,
                },
                Err(e) => Update::RefreshFailed(e.to_string()),
            }
        }
        Command::Connection(id) => {
            match tokio::try_join!(
                api.connection(id),
                api.connection_jobs(id),
                api.connection_state(id)
            ) {
                Ok((connection, jobs, state)) => Update::Connection {
                    connection,
                    jobs,
                    state,
                },
                Err(e) => Update::RefreshFailed(e.to_string()),
            }
        }
        Command::JobDetail(id) => match api.job_detail(id).await {
            Ok(detail) => Update::JobDetail(detail),
            Err(e) => Update::Error(e.to_string()),
        },
        Command::TriggerSync(connection) => match api.trigger_sync(connection).await {
            Ok(job) => Update::Notice(format!("sync queued as job #{}", job.id)),
            Err(e) => Update::Error(format!("sync failed: {e}")),
        },
        Command::CancelJob(job) => match api.cancel_job(job).await {
            Ok(_) => Update::Notice(format!("cancellation requested for job #{job}")),
            Err(e) => Update::Error(format!("cancel failed: {e}")),
        },
        Command::SetConnectionStatus { id, status } => {
            match api.set_connection_status(id, status).await {
                Ok(conn) => Update::Notice(match status {
                    "active" => format!("\"{}\" resumed — schedule live again", conn.name),
                    _ => format!("\"{}\" paused — no syncs until resumed", conn.name),
                }),
                Err(e) => Update::Error(format!("status change failed: {e}")),
            }
        }
        Command::CreateWorkspace(name) => match api.create_workspace(&name).await {
            Ok(ws) => Update::Notice(format!("workspace \"{}\" created", ws.name)),
            Err(e) => Update::Error(format!("create failed: {e}")),
        },
    }
}
