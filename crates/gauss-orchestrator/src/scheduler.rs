//! Schedule evaluation: connections carry a `schedule` JSON of either
//! `{"intervalMinutes": N}` or `{"cron": "<expr>"}` (5-field standard cron
//! or 6/7-field with seconds). Jobs are enqueued when due; the partial
//! unique index makes double-enqueueing impossible.

use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use gauss_store::{Store, StoreError};
use serde_json::Value;

use crate::OrchestratorError;

/// When the next run is due, given the schedule and the previous job's
/// creation time (`None` = never ran → due immediately).
pub fn next_due(
    schedule: &Value,
    last_job_at: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>, OrchestratorError> {
    if let Some(minutes) = schedule.get("intervalMinutes").and_then(Value::as_i64) {
        if minutes < 0 {
            return Err(OrchestratorError::Schedule(
                "intervalMinutes must be >= 0".to_string(),
            ));
        }
        return Ok(match last_job_at {
            Some(last) => last + Duration::minutes(minutes),
            None => Utc::now(),
        });
    }

    if let Some(expr) = schedule.get("cron").and_then(Value::as_str) {
        // Accept standard 5-field cron by prepending a seconds field.
        let normalized = if expr.split_whitespace().count() == 5 {
            format!("0 {expr}")
        } else {
            expr.to_string()
        };
        let parsed = cron::Schedule::from_str(&normalized)
            .map_err(|err| OrchestratorError::Schedule(format!("bad cron `{expr}`: {err}")))?;
        let base = last_job_at.unwrap_or_else(Utc::now);
        return parsed
            .after(&base)
            .next()
            .ok_or_else(|| OrchestratorError::Schedule(format!("cron `{expr}` never fires")));
    }

    Err(OrchestratorError::Schedule(
        "schedule must contain intervalMinutes or cron".to_string(),
    ))
}

pub(crate) async fn schedule_due(store: &Store) -> Result<usize, OrchestratorError> {
    let mut created = 0;
    for (connection, last_job_at) in store.connections().list_schedulable().await? {
        let Some(schedule) = &connection.schedule else {
            continue;
        };
        let due = match next_due(&schedule.0, last_job_at) {
            Ok(due) => due,
            Err(err) => {
                tracing::warn!(connection = %connection.id, %err, "skipping bad schedule");
                continue;
            }
        };
        if due <= Utc::now() {
            match store.jobs().create(connection.id, "sync").await {
                Ok(_) => created += 1,
                // A job appeared concurrently — exactly what the unique
                // index is for; skip.
                Err(StoreError::Conflict(_)) => {}
                Err(err) => return Err(err.into()),
            }
        }
    }
    Ok(created)
}
