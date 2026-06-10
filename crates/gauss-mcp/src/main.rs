//! `gauss-mcp` — MCP server over stdio.
//!
//! Add to any MCP client config:
//! `{"command": "gauss-mcp", "env": {"DATABASE_URL": "postgres://…"}}`

use anyhow::Context;
use clap::Parser;
use gauss_mcp::Gateway;
use gauss_store::Store;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Parser)]
#[command(
    name = "gauss-mcp",
    version,
    about = "gaussdataflow MCP gateway (stdio)"
)]
struct Cli {
    /// Postgres connection string
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout is the protocol channel; logs go to stderr only.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let store = Store::connect(&cli.database_url)
        .await
        .context("connecting to database")?;
    let gateway = Gateway::new(store);

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: serde_json::Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(%error, "ignoring non-JSON input line");
                continue;
            }
        };
        if let Some(response) = gateway.handle(message).await {
            stdout.write_all(response.to_string().as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
