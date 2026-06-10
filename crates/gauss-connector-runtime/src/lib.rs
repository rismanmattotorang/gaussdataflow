//! Runs Airbyte-protocol connectors and streams their output as typed
//! messages.
//!
//! A connector is just a program: `docker run -i <image> <op> --config …` or
//! a local binary with the same argument convention. [`Launcher`]
//! implementations turn a [`ConnectorCommand`] into a runnable
//! [`tokio::process::Command`]; [`ConnectorProcess`] streams STDOUT lines as
//! [`gauss_protocol::AirbyteMessage`]s; [`ConnectorRunner`] provides the
//! high-level `spec`/`check`/`discover`/`read` operations.

mod error;
mod launcher;
mod process;
mod runner;

pub use error::RuntimeError;
pub use launcher::{ConnectorCommand, DockerLauncher, Launcher, ProcessLauncher};
pub use process::{ConnectorOutput, ConnectorProcess};
pub use runner::{stage_json, ConnectorRunner, ReadEvent, ReadSummary};
