use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::error::RuntimeError;

/// One of the standard connector operations and its input files.
#[derive(Debug, Clone)]
pub enum ConnectorCommand {
    Spec,
    Check {
        config: PathBuf,
    },
    Discover {
        config: PathBuf,
    },
    Read {
        config: PathBuf,
        catalog: PathBuf,
        state: Option<PathBuf>,
    },
    /// Destination operation; records arrive on the connector's STDIN.
    Write {
        config: PathBuf,
        catalog: PathBuf,
    },
}

impl ConnectorCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Spec => "spec",
            Self::Check { .. } => "check",
            Self::Discover { .. } => "discover",
            Self::Read { .. } => "read",
            Self::Write { .. } => "write",
        }
    }

    /// `(--flag, file)` pairs this operation passes to the connector.
    fn files(&self) -> Vec<(&'static str, &Path)> {
        match self {
            Self::Spec => vec![],
            Self::Check { config } | Self::Discover { config } => {
                vec![("--config", config)]
            }
            Self::Read {
                config,
                catalog,
                state,
            } => {
                let mut files = vec![
                    ("--config", config.as_path()),
                    ("--catalog", catalog.as_path()),
                ];
                if let Some(state) = state {
                    files.push(("--state", state.as_path()));
                }
                files
            }
            Self::Write { config, catalog } => vec![
                ("--config", config.as_path()),
                ("--catalog", catalog.as_path()),
            ],
        }
    }
}

/// Turns a [`ConnectorCommand`] into a runnable process.
pub trait Launcher: Send + Sync {
    fn build(&self, command: &ConnectorCommand) -> Result<Command, RuntimeError>;

    /// Human-readable identity for logs and error messages.
    fn describe(&self) -> String;
}

/// Runs a connector image via the local Docker daemon, mounting input files
/// read-only into the container.
#[derive(Debug, Clone)]
pub struct DockerLauncher {
    pub image: String,
    pub docker_bin: String,
}

impl DockerLauncher {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            docker_bin: "docker".to_string(),
        }
    }
}

impl Launcher for DockerLauncher {
    fn build(&self, command: &ConnectorCommand) -> Result<Command, RuntimeError> {
        let mut cmd = Command::new(&self.docker_bin);
        cmd.arg("run")
            .arg("--rm")
            .arg("-i")
            .arg("--log-driver=none");

        // Mount each input file at a stable in-container path.
        let mut connector_args: Vec<String> = vec![command.name().to_string()];
        for (flag, host_path) in command.files() {
            let abs = host_path
                .canonicalize()
                .map_err(|_| RuntimeError::InvalidPath(host_path.to_path_buf()))?;
            let container_path = format!("/staging{}.json", flag.trim_start_matches('-'));
            cmd.arg("-v")
                .arg(format!("{}:{}:ro", abs.display(), container_path));
            connector_args.push(flag.to_string());
            connector_args.push(container_path);
        }

        cmd.arg(&self.image).args(connector_args);
        Ok(cmd)
    }

    fn describe(&self) -> String {
        format!("docker image {}", self.image)
    }
}

/// Runs a connector as a local binary (native Rust connectors, tests, or
/// `python main.py`-style dev runs).
#[derive(Debug, Clone)]
pub struct ProcessLauncher {
    pub program: PathBuf,
    pub base_args: Vec<String>,
}

impl ProcessLauncher {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            base_args: vec![],
        }
    }
}

impl Launcher for ProcessLauncher {
    fn build(&self, command: &ConnectorCommand) -> Result<Command, RuntimeError> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.base_args);
        cmd.arg(command.name());
        for (flag, path) in command.files() {
            if !path.exists() {
                return Err(RuntimeError::InvalidPath(path.to_path_buf()));
            }
            cmd.arg(flag).arg(path);
        }
        Ok(cmd)
    }

    fn describe(&self) -> String {
        format!("local process {}", self.program.display())
    }
}
