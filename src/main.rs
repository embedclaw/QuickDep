//! QuickDep CLI entry point

use clap::{Parser, Subcommand};
use quickdep::{
    cli::{self, ClaudeScope, InstallMcpOptions, McpClient},
    config::load_settings,
    daemon::{self, DaemonClient},
    log::{init_logging, LogLevel},
    mcp::QuickDepServer,
    VERSION,
};
use rmcp::ServiceExt;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::info;

/// QuickDep - Fast code dependency analysis for AI agents
#[derive(Parser)]
#[command(name = "quickdep")]
#[command(version = VERSION)]
#[command(about = "A Rust MCP service for scanning code dependencies")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// HTTP port. When set, QuickDep serves both stdio MCP and streamable HTTP.
    #[arg(long, global = true)]
    http: Option<u16>,

    /// Serve HTTP only without stdio MCP.
    #[arg(long, global = true)]
    http_only: bool,

    /// Log level
    #[arg(short, long, global = true)]
    log_level: Option<String>,

    /// Comma-separated list of MCP tools to expose.
    #[arg(long, global = true, value_delimiter = ',')]
    tools: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the QuickDep MCP server
    Serve,

    /// Manage the shared QuickDep daemon
    Daemon {
        #[command(subcommand)]
        command: Option<DaemonCommands>,
    },

    /// Debug and inspect project
    Debug {
        /// Project path
        path: PathBuf,

        /// Show statistics
        #[arg(short, long)]
        stats: bool,

        /// Show dependencies for an interface
        #[arg(short, long)]
        deps: Option<String>,

        /// Show interfaces in a file
        #[arg(short, long)]
        file: Option<String>,
    },

    /// Scan a project
    Scan {
        /// Project path
        path: PathBuf,

        /// Force rebuild (ignore cached data)
        #[arg(short, long)]
        rebuild: bool,
    },

    /// Show project status
    Status {
        /// Project path
        path: PathBuf,
    },

    /// Install QuickDep into a supported MCP client configuration
    InstallMcp {
        /// Target MCP client
        #[arg(value_enum)]
        client: InstallMcpClient,

        /// MCP server name inside the target client
        #[arg(long, default_value = "quickdep")]
        name: String,

        /// Explicit path to the quickdep binary to register
        #[arg(long)]
        quickdep_bin: Option<PathBuf>,

        /// Only print the intended changes without modifying client config
        #[arg(long)]
        dry_run: bool,

        /// Claude Code installation scope
        #[arg(long, value_enum, default_value_t = InstallMcpScope::Local)]
        scope: InstallMcpScope,

        /// Override the OpenCode config file path
        #[arg(long)]
        opencode_config: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DaemonCommands {
    /// Run the daemon in the foreground
    Run,
    /// Show daemon status
    Status,
    /// Stop the running daemon
    Stop,
}

#[derive(clap::ValueEnum, Clone, Copy)]
enum InstallMcpClient {
    Claude,
    Codex,
    #[value(name = "opencode")]
    OpenCode,
}

#[derive(clap::ValueEnum, Clone, Copy, Default)]
enum InstallMcpScope {
    #[default]
    Local,
    User,
    Project,
}

fn default_log_dir(command: &Option<Commands>, current_dir: &std::path::Path) -> PathBuf {
    let root = match command {
        Some(Commands::Debug { path, .. })
        | Some(Commands::Scan { path, .. })
        | Some(Commands::Status { path })
            if path.exists() && path.is_dir() =>
        {
            path
        }
        Some(Commands::Daemon { .. }) => current_dir,
        _ => current_dir,
    };

    root.join(".quickdep").join("logs")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let current_dir = std::env::current_dir()?;
    let log_settings = match &cli.command {
        Some(Commands::Debug { path, .. })
        | Some(Commands::Scan { path, .. })
        | Some(Commands::Status { path })
            if path.exists() && path.is_dir() =>
        {
            load_settings(path).ok()
        }
        Some(Commands::Daemon { .. }) => load_settings(&current_dir).ok(),
        _ if current_dir.exists() => load_settings(&current_dir).ok(),
        _ => None,
    };

    // Initialize logging
    let level_name = cli
        .log_level
        .clone()
        .or_else(|| log_settings.map(|settings| settings.log.level))
        .unwrap_or_else(|| "info".to_string());
    let level = LogLevel::from_str(&level_name).unwrap_or_default();
    init_logging(level, Some(default_log_dir(&cli.command, &current_dir)))?;

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => {
            let settings = load_settings(&current_dir)?;
            let http_port = if cli.http_only {
                Some(cli.http.unwrap_or(settings.server.http_port))
            } else {
                cli.http.or(settings
                    .server
                    .http_enabled
                    .then_some(settings.server.http_port))
            };

            info!("Starting QuickDep MCP proxy v{}", VERSION);
            let daemon_client = DaemonClient::connect_or_start().await?;
            let tool_filter = (!cli.tools.is_empty()).then_some(cli.tools.clone());

            if let Some(port) = http_port {
                let response = daemon_client
                    .ensure_http(&current_dir, port, tool_filter.clone())
                    .await?;
                info!("Daemon HTTP endpoint ready at {}", response["url"]);
                if cli.http_only {
                    println!("{}", serde_json::to_string_pretty(&response)?);
                    return Ok(());
                }
            }

            let server = if cli.tools.is_empty() {
                QuickDepServer::from_daemon_proxy(&current_dir, None).await?
            } else {
                QuickDepServer::from_daemon_proxy(&current_dir, Some(cli.tools.clone())).await?
            };

            server
                .serve(rmcp::transport::io::stdio())
                .await?
                .waiting()
                .await?;
        }

        Commands::Daemon { command } => match command.unwrap_or(DaemonCommands::Run) {
            DaemonCommands::Run => daemon::run_foreground().await?,
            DaemonCommands::Status => {
                let output = daemon::daemon_status().await?;
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            DaemonCommands::Stop => {
                let output = daemon::stop_daemon().await?;
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
        }

        Commands::Debug {
            path,
            stats,
            deps,
            file,
        } => {
            info!("Debug mode for project: {}", path.display());
            let output = cli::run_debug(&path, stats, deps.as_deref(), file.as_deref()).await?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        Commands::Scan { path, rebuild } => {
            info!("Scanning project: {}", path.display());
            let output = cli::run_scan(&path, rebuild).await?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        Commands::Status { path } => {
            info!("Status for project: {}", path.display());
            let output = cli::run_status(&path).await?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }

        Commands::InstallMcp {
            client,
            name,
            quickdep_bin,
            dry_run,
            scope,
            opencode_config,
        } => {
            let output = cli::run_install_mcp(InstallMcpOptions {
                client: match client {
                    InstallMcpClient::Claude => McpClient::Claude,
                    InstallMcpClient::Codex => McpClient::Codex,
                    InstallMcpClient::OpenCode => McpClient::OpenCode,
                },
                name,
                quickdep_bin,
                dry_run,
                claude_scope: match scope {
                    InstallMcpScope::Local => ClaudeScope::Local,
                    InstallMcpScope::User => ClaudeScope::User,
                    InstallMcpScope::Project => ClaudeScope::Project,
                },
                opencode_config,
            })?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_log_dir_uses_target_project_for_scan_commands() {
        let current_dir = TempDir::new().expect("create current dir");
        let project_dir = TempDir::new().expect("create project dir");

        let command = Some(Commands::Scan {
            path: project_dir.path().to_path_buf(),
            rebuild: false,
        });

        assert_eq!(
            default_log_dir(&command, current_dir.path()),
            project_dir.path().join(".quickdep").join("logs")
        );
    }

    #[test]
    fn test_default_log_dir_falls_back_to_current_dir_for_serve() {
        let current_dir = TempDir::new().expect("create current dir");

        assert_eq!(
            default_log_dir(&Some(Commands::Serve), current_dir.path()),
            current_dir.path().join(".quickdep").join("logs")
        );
        assert_eq!(
            default_log_dir(&None, current_dir.path()),
            current_dir.path().join(".quickdep").join("logs")
        );
    }
}
