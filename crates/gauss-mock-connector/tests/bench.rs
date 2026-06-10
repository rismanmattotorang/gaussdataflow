//! Throughput benchmark for the replication engine. Ignored by default; run
//! with `cargo test -p gauss-mock-connector --test bench --release -- --ignored --nocapture`.

use gauss_connector_runtime::ProcessLauncher;
use gauss_sync::{run_sync, SyncOptions, SyncRequest};
use serde_json::json;
use tokio::sync::watch;

#[tokio::test]
#[ignore = "benchmark; run explicitly with --ignored --nocapture"]
async fn replication_throughput() {
    const RECORDS: u64 = 200_000;

    let launcher = ProcessLauncher::new(env!("CARGO_BIN_EXE_gauss-mock-connector"));
    let out = tempfile::NamedTempFile::new().unwrap();
    let request = SyncRequest {
        source_config: json!({"record_count": RECORDS}),
        destination_config: json!({"out_path": out.path()}),
        catalog: json!({"streams": [{
            "stream": {"name": "users", "json_schema": {}},
            "sync_mode": "full_refresh",
            "destination_sync_mode": "append"
        }]}),
        state: None,
    };
    let (_cancel, cancel_rx) = watch::channel(false);

    let started = std::time::Instant::now();
    let summary = run_sync(
        &launcher,
        &launcher,
        &request,
        &SyncOptions::default(),
        cancel_rx,
        |_| async { Ok(()) },
    )
    .await
    .expect("sync succeeds");
    let elapsed = started.elapsed();

    assert_eq!(summary.records_synced, RECORDS);
    let rate = RECORDS as f64 / elapsed.as_secs_f64();
    let mb = summary.bytes_synced as f64 / (1024.0 * 1024.0);
    println!(
        "replicated {RECORDS} records ({mb:.1} MiB) in {elapsed:.2?} → {rate:.0} records/s, {:.1} MiB/s",
        mb / elapsed.as_secs_f64()
    );
}
