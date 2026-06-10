//! `gauss-declarative` — one binary, any HTTP-API connector.
//!
//! Register it once (`exec:/path/to/gauss-declarative`); each source's config
//! carries its own manifest (under the `manifest` key) plus the user fields
//! the manifest interpolates. Container-free low-code connectors.

use gauss_declarative::DeclarativeSource;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    gauss_cdk::cli::run_source(DeclarativeSource).await
}
