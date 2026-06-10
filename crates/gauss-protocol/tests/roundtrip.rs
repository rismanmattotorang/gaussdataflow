//! Round-trip tests against wire-format fixtures shaped like the examples in
//! the Gauss Protocol docs. Each fixture is deserialized, re-serialized,
//! and compared as JSON values to prove wire-exactness.

use gauss_protocol::*;
use serde_json::{json, Value};

fn roundtrip(fixture: Value) -> GaussMessage {
    let msg: GaussMessage =
        serde_json::from_value(fixture.clone()).expect("fixture must deserialize");
    let back = serde_json::to_value(&msg).expect("message must serialize");
    assert_eq!(fixture, back, "wire form must round-trip unchanged");
    msg
}

#[test]
fn record_message() {
    let msg = roundtrip(json!({
        "type": "RECORD",
        "record": {
            "stream": "users",
            "namespace": "public",
            "data": {"id": 1, "name": "ada", "updated_at": "2026-01-01T00:00:00Z"},
            "emitted_at": 1767225600000_i64
        }
    }));
    assert_eq!(msg.message_type, GaussMessageType::Record);
    let record = msg.record.unwrap();
    assert_eq!(record.stream, "users");
    assert_eq!(record.data["name"], "ada");
}

#[test]
fn stream_state_message() {
    let msg = roundtrip(json!({
        "type": "STATE",
        "state": {
            "type": "STREAM",
            "stream": {
                "stream_descriptor": {"name": "users", "namespace": "public"},
                "stream_state": {"cursor": "2026-01-01T00:00:00Z"}
            },
            "sourceStats": {"recordCount": 100.0}
        }
    }));
    let state = msg.state.unwrap();
    assert_eq!(state.state_type, Some(GaussStateType::Stream));
    assert_eq!(state.source_stats.unwrap().record_count, Some(100.0));
}

#[test]
fn global_state_message() {
    let msg = roundtrip(json!({
        "type": "STATE",
        "state": {
            "type": "GLOBAL",
            "global": {
                "shared_state": {"cdc_lsn": "0/16B3748"},
                "stream_states": [
                    {"stream_descriptor": {"name": "users"}, "stream_state": {"cursor": "5"}}
                ]
            }
        }
    }));
    let global = msg.state.unwrap().global.unwrap();
    assert_eq!(global.stream_states.len(), 1);
}

#[test]
fn legacy_state_message() {
    let msg = roundtrip(json!({
        "type": "STATE",
        "state": {"data": {"whole_source_cursor": 42}}
    }));
    let state = msg.state.unwrap();
    assert_eq!(state.state_type, None);
    assert_eq!(state.data.unwrap()["whole_source_cursor"], 42);
}

#[test]
fn log_message() {
    let msg = roundtrip(json!({
        "type": "LOG",
        "log": {"level": "WARN", "message": "rate limited, backing off"}
    }));
    assert_eq!(msg.log.unwrap().level, GaussLogLevel::Warn);
}

#[test]
fn spec_message() {
    let msg = roundtrip(json!({
        "type": "SPEC",
        "spec": {
            "documentationUrl": "https://docs.example.com/source-pg",
            "connectionSpecification": {
                "type": "object",
                "required": ["host"],
                "properties": {"host": {"type": "string"}}
            },
            "supportsIncremental": true,
            "protocol_version": "0.2.0"
        }
    }));
    let spec = msg.spec.unwrap();
    assert_eq!(spec.supports_incremental, Some(true));
    assert_eq!(spec.protocol_version.as_deref(), Some("0.2.0"));
}

#[test]
fn connection_status_message() {
    let msg = roundtrip(json!({
        "type": "CONNECTION_STATUS",
        "connectionStatus": {"status": "FAILED", "message": "bad credentials"}
    }));
    assert_eq!(
        msg.connection_status.unwrap().status,
        ConnectionStatus::Failed
    );
}

#[test]
fn catalog_message() {
    let msg = roundtrip(json!({
        "type": "CATALOG",
        "catalog": {
            "streams": [{
                "name": "users",
                "json_schema": {"type": "object", "properties": {"id": {"type": "integer"}}},
                "supported_sync_modes": ["full_refresh", "incremental"],
                "source_defined_cursor": true,
                "default_cursor_field": ["updated_at"],
                "source_defined_primary_key": [["id"]],
                "is_resumable": true
            }]
        }
    }));
    let stream = &msg.catalog.unwrap().streams[0];
    assert_eq!(
        stream.supported_sync_modes,
        Some(vec![SyncMode::FullRefresh, SyncMode::Incremental])
    );
}

#[test]
fn configured_catalog() {
    let fixture = json!({
        "streams": [{
            "stream": {
                "name": "users",
                "json_schema": {"type": "object"}
            },
            "sync_mode": "incremental",
            "cursor_field": ["updated_at"],
            "destination_sync_mode": "append_dedup",
            "primary_key": [["id"]]
        }]
    });
    let catalog: ConfiguredGaussCatalog = serde_json::from_value(fixture.clone()).unwrap();
    assert_eq!(catalog.streams[0].sync_mode, SyncMode::Incremental);
    assert_eq!(
        catalog.streams[0].destination_sync_mode,
        DestinationSyncMode::AppendDedup
    );
    assert_eq!(fixture, serde_json::to_value(&catalog).unwrap());
}

#[test]
fn error_trace_message() {
    let msg = roundtrip(json!({
        "type": "TRACE",
        "trace": {
            "type": "ERROR",
            "emitted_at": 1767225600000.0,
            "error": {
                "message": "Something went wrong",
                "internal_message": "stacktrace head",
                "failure_type": "config_error"
            }
        }
    }));
    let trace = msg.trace.unwrap();
    assert_eq!(trace.trace_type, GaussTraceType::Error);
    assert_eq!(
        trace.error.unwrap().failure_type,
        Some(FailureType::ConfigError)
    );
}

#[test]
fn stream_status_trace_message() {
    let msg = roundtrip(json!({
        "type": "TRACE",
        "trace": {
            "type": "STREAM_STATUS",
            "emitted_at": 1767225600000.0,
            "stream_status": {
                "stream_descriptor": {"name": "users"},
                "status": "COMPLETE"
            }
        }
    }));
    let status = msg.trace.unwrap().stream_status.unwrap();
    assert_eq!(status.status, StreamStatus::Complete);
}

#[test]
fn control_message() {
    let msg = roundtrip(json!({
        "type": "CONTROL",
        "control": {
            "type": "CONNECTOR_CONFIG",
            "emitted_at": 1767225600000.0,
            "connectorConfig": {"config": {"refresh_token": "new-token"}}
        }
    }));
    let control = msg.control.unwrap();
    assert_eq!(
        control.control_type,
        GaussControlMessageType::ConnectorConfig
    );
}

#[test]
fn unknown_fields_are_tolerated() {
    // Protocol evolution: newer connectors may emit fields we don't know yet.
    let msg = parse_message(
        r#"{"type":"RECORD","record":{"stream":"s","data":{},"emitted_at":1,"some_future_field":true},"another_future_field":1}"#,
    )
    .expect("unknown fields must not break parsing");
    assert_eq!(msg.record.unwrap().stream, "s");
}

#[test]
fn wire_form_is_single_line() {
    let msg = GaussMessage::record(GaussRecordMessage {
        namespace: None,
        stream: "users".into(),
        data: json!({"id": 1}),
        emitted_at: 1,
        meta: None,
    });
    let line = to_wire(&msg).unwrap();
    assert!(!line.contains('\n'));
    // None fields must be omitted, not serialized as null.
    assert!(!line.contains("null"));
}
