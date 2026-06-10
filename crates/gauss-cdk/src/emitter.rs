//! Wire-format output with protocol helpers.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use gauss_protocol::*;
use serde_json::Value;

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_millis() as i64
}

/// Writes protocol messages as newline-delimited JSON. Connectors get one
/// pointed at STDOUT; tests use [`Emitter::buffer`] and assert on the
/// captured messages.
pub struct Emitter {
    out: Box<dyn Write + Send>,
}

impl Emitter {
    pub fn new(out: Box<dyn Write + Send>) -> Self {
        Self { out }
    }

    pub fn stdout() -> Self {
        Self::new(Box::new(std::io::stdout()))
    }

    /// Capture emitter for tests: returns the emitter and a shared buffer;
    /// parse it with [`Emitter::parse_buffer`] once the connector finishes.
    pub fn buffer() -> (Self, Arc<Mutex<Vec<u8>>>) {
        #[derive(Clone)]
        struct Shared(Arc<Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let shared = Arc::new(Mutex::new(Vec::new()));
        (Self::new(Box::new(Shared(shared.clone()))), shared)
    }

    pub fn parse_buffer(buffer: &Arc<Mutex<Vec<u8>>>) -> Vec<GaussMessage> {
        String::from_utf8_lossy(&buffer.lock().unwrap())
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| parse_message(l).expect("emitter output must be protocol messages"))
            .collect()
    }

    pub fn message(&mut self, message: &GaussMessage) -> Result<(), crate::CdkError> {
        let line = to_wire(message)?;
        writeln!(self.out, "{line}")?;
        self.out.flush()?;
        Ok(())
    }

    pub fn record(
        &mut self,
        stream: &str,
        namespace: Option<&str>,
        data: Value,
    ) -> Result<(), crate::CdkError> {
        self.message(&GaussMessage::record(GaussRecordMessage {
            namespace: namespace.map(str::to_string),
            stream: stream.to_string(),
            data,
            emitted_at: now_millis(),
            meta: None,
        }))
    }

    /// Per-stream state checkpoint with optional source stats.
    pub fn stream_state(
        &mut self,
        stream: &str,
        state: Value,
        record_count: Option<f64>,
    ) -> Result<(), crate::CdkError> {
        let mut message = GaussStateMessage::stream(GaussStreamState {
            stream_descriptor: StreamDescriptor::new(stream),
            stream_state: Some(state),
        });
        message.source_stats = record_count.map(|count| GaussStateStats {
            record_count: Some(count),
        });
        self.message(&GaussMessage::state(message))
    }

    pub fn stream_status(
        &mut self,
        stream: &str,
        status: StreamStatus,
    ) -> Result<(), crate::CdkError> {
        self.message(&GaussMessage::trace(GaussTraceMessage::stream_status(
            now_millis() as f64,
            GaussStreamStatusTraceMessage {
                stream_descriptor: StreamDescriptor::new(stream),
                status,
                reasons: None,
            },
        )))
    }

    pub fn log(&mut self, level: GaussLogLevel, text: &str) -> Result<(), crate::CdkError> {
        self.message(&GaussMessage::log(GaussLogMessage {
            level,
            message: text.to_string(),
            stack_trace: None,
        }))
    }

    pub fn error_trace(
        &mut self,
        message: &str,
        failure_type: FailureType,
    ) -> Result<(), crate::CdkError> {
        self.message(&GaussMessage::trace(GaussTraceMessage {
            trace_type: GaussTraceType::Error,
            emitted_at: now_millis() as f64,
            error: Some(GaussErrorTraceMessage {
                message: message.to_string(),
                internal_message: None,
                stack_trace: None,
                failure_type: Some(failure_type),
                stream_descriptor: None,
            }),
            estimate: None,
            stream_status: None,
            analytics: None,
        }))
    }
}
