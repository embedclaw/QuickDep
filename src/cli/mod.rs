//! CLI command helpers for QuickDep.

mod install_mcp;

use std::path::Path;

use serde_json::Value;

pub use install_mcp::{ClaudeScope, InstallMcpOptions, McpClient};

use crate::daemon::DaemonClient;

/// Run the `scan` command.
pub async fn run_scan(path: &Path, rebuild: bool) -> anyhow::Result<Value> {
    DaemonClient::connect_or_start()
        .await?
        .cli_scan(path, rebuild)
        .await
}

/// Run the `status` command.
pub async fn run_status(path: &Path) -> anyhow::Result<Value> {
    DaemonClient::connect_or_start()
        .await?
        .cli_status(path)
        .await
}

/// Run the `debug` command.
pub async fn run_debug(
    path: &Path,
    stats: bool,
    deps: Option<&str>,
    file: Option<&str>,
) -> anyhow::Result<Value> {
    DaemonClient::connect_or_start()
        .await?
        .cli_debug(path, stats, deps, file)
        .await
}

/// Install QuickDep as an MCP server for a supported client.
pub fn run_install_mcp(options: InstallMcpOptions) -> anyhow::Result<Value> {
    install_mcp::install_mcp(options)
}
