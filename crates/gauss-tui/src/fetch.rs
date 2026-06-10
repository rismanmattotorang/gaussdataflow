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
    CancelJob { job: i64, connection: Uuid },
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
        id: Uuid,
        jobs: Vec<Job>,
        state: Option<Value>,
    },
    JobDetail(JobDetail),
    Notice(String),
    Error(String),
}

pub async fn run(
    api: ApiClient,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    updates: std::sync::mpsc::Sender<Update>,
) {
    while let Some(cmd) = commands.recv().await {
        let update = handle(&api, cmd, &updates).await;
        if updates.send(update).is_err() {
            return; // UI is gone
        }
    }
}

async fn handle(
    api: &ApiClient,
    cmd: Command,
    updates: &std::sync::mpsc::Sender<Update>,
) -> Update {
    match cmd {
        Command::Home => {
            match tokio::try_join!(api.workspaces(), api.stats(None), api.recent_jobs(None, 30)) {
                Ok((workspaces, stats, jobs)) => Update::Home {
                    workspaces,
                    stats,
                    jobs,
                },
                Err(e) => Update::Error(e.to_string()),
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
                Err(e) => Update::Error(e.to_string()),
            }
        }
        Command::Connection(id) => {
            match tokio::try_join!(api.connection_jobs(id), api.connection_state(id)) {
                Ok((jobs, state)) => Update::Connection { id, jobs, state },
                Err(e) => Update::Error(e.to_string()),
            }
        }
        Command::JobDetail(id) => match api.job_detail(id).await {
            Ok(detail) => Update::JobDetail(detail),
            Err(e) => Update::Error(e.to_string()),
        },
        Command::TriggerSync(connection) => match api.trigger_sync(connection).await {
            Ok(job) => {
                refresh_connection(api, connection, updates).await;
                Update::Notice(format!("sync queued as job #{}", job.id))
            }
            Err(e) => Update::Error(format!("sync failed: {e}")),
        },
        Command::CancelJob { job, connection } => match api.cancel_job(job).await {
            Ok(_) => {
                refresh_connection(api, connection, updates).await;
                Update::Notice(format!("cancellation requested for job #{job}"))
            }
            Err(e) => Update::Error(format!("cancel failed: {e}")),
        },
        Command::CreateWorkspace(name) => match api.create_workspace(&name).await {
            Ok(ws) => {
                if let Ok((workspaces, stats, jobs)) =
                    tokio::try_join!(api.workspaces(), api.stats(None), api.recent_jobs(None, 30))
                {
                    let _ = updates.send(Update::Home {
                        workspaces,
                        stats,
                        jobs,
                    });
                }
                Update::Notice(format!("workspace \"{}\" created", ws.name))
            }
            Err(e) => Update::Error(format!("create failed: {e}")),
        },
    }
}

/// Push fresh connection data after a mutation so the action's effect is
/// visible on the very next frame.
async fn refresh_connection(api: &ApiClient, id: Uuid, updates: &std::sync::mpsc::Sender<Update>) {
    if let Ok((jobs, state)) = tokio::try_join!(api.connection_jobs(id), api.connection_state(id)) {
        let _ = updates.send(Update::Connection { id, jobs, state });
    }
}
