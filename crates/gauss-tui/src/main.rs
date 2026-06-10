//! gauss-tui — an interactive terminal console for the Gauss-DataFlow
//! control plane, built on Ratatui.
//!
//! It speaks the same REST API as the web console, so it works against any
//! deployment: `gauss-tui --api http://host:8000 [--token gauss_…]`.

mod api;
mod app;
mod fetch;
mod ui;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "gauss-tui",
    version,
    about = "Terminal console for Gauss-DataFlow: fleet pulse, pipelines, jobs, and one-key syncs"
)]
struct Cli {
    /// Base URL of the gauss-server API.
    #[arg(long, env = "GAUSS_API", default_value = "http://127.0.0.1:8000")]
    api: String,

    /// Bearer token, required when the server runs with --require-auth.
    #[arg(long, env = "GAUSS_TOKEN")]
    token: Option<String>,

    /// Auto-refresh interval in seconds.
    #[arg(long, default_value_t = 3)]
    refresh: u64,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let runtime = tokio::runtime::Runtime::new()?;
    let client = api::ApiClient::new(cli.api.clone(), cli.token);

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (upd_tx, upd_rx) = std::sync::mpsc::channel();
    runtime.spawn(fetch::run(client, cmd_rx, upd_tx));

    let terminal = ratatui::init();
    let result = app::App::new(cli.api, cmd_tx, upd_rx, cli.refresh).run(terminal);
    ratatui::restore();
    result
}
