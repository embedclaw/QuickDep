use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpClient {
    Claude,
    Codex,
    OpenCode,
}

impl McpClient {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeScope {
    Local,
    User,
    Project,
}

impl ClaudeScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallMcpOptions {
    pub client: McpClient,
    pub name: String,
    pub quickdep_bin: Option<PathBuf>,
    pub dry_run: bool,
    pub claude_scope: ClaudeScope,
    pub opencode_config: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallMcpResult {
    pub client: String,
    pub name: String,
    pub command: Vec<String>,
    pub target: String,
    pub method: String,
    pub config_path: Option<String>,
    pub dry_run: bool,
}

pub fn install_mcp(options: InstallMcpOptions) -> anyhow::Result<Value> {
    let quickdep_bin = resolve_quickdep_bin(options.quickdep_bin.as_deref())?;
    let command = vec![quickdep_bin.display().to_string(), "serve".to_string()];

    let result = match options.client {
        McpClient::Claude => install_claude(&options, &quickdep_bin, &command)?,
        McpClient::Codex => install_codex(&options, &quickdep_bin, &command)?,
        McpClient::OpenCode => install_opencode(&options, &quickdep_bin, &command)?,
    };

    serde_json::to_value(result).map_err(|error| anyhow!(error))
}

fn resolve_quickdep_bin(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = explicit {
        return canonicalize_executable(path);
    }

    let current_exe =
        std::env::current_exe().context("failed to locate current quickdep binary")?;
    canonicalize_executable(&current_exe)
}

fn canonicalize_executable(path: &Path) -> anyhow::Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize executable path {}", path.display()))?;
    if !canonical.is_file() {
        bail!("quickdep binary is not a file: {}", canonical.display());
    }
    Ok(canonical)
}

fn install_claude(
    options: &InstallMcpOptions,
    quickdep_bin: &Path,
    command: &[String],
) -> anyhow::Result<InstallMcpResult> {
    let args = vec![
        "mcp".to_string(),
        "add".to_string(),
        "--scope".to_string(),
        options.claude_scope.as_str().to_string(),
        options.name.clone(),
        "--".to_string(),
        quickdep_bin.display().to_string(),
        "serve".to_string(),
    ];

    if !options.dry_run {
        run_command("claude", &args)?;
    }

    Ok(InstallMcpResult {
        client: options.client.as_str().to_string(),
        name: options.name.clone(),
        command: command.to_vec(),
        target: format!("Claude Code ({})", options.claude_scope.as_str()),
        method: "cli".to_string(),
        config_path: None,
        dry_run: options.dry_run,
    })
}

fn install_codex(
    options: &InstallMcpOptions,
    quickdep_bin: &Path,
    command: &[String],
) -> anyhow::Result<InstallMcpResult> {
    let args = vec![
        "mcp".to_string(),
        "add".to_string(),
        options.name.clone(),
        "--".to_string(),
        quickdep_bin.display().to_string(),
        "serve".to_string(),
    ];

    if !options.dry_run {
        run_command("codex", &args)?;
    }

    Ok(InstallMcpResult {
        client: options.client.as_str().to_string(),
        name: options.name.clone(),
        command: command.to_vec(),
        target: "Codex".to_string(),
        method: "cli".to_string(),
        config_path: Some("~/.codex/config.toml".to_string()),
        dry_run: options.dry_run,
    })
}

fn install_opencode(
    options: &InstallMcpOptions,
    quickdep_bin: &Path,
    command: &[String],
) -> anyhow::Result<InstallMcpResult> {
    let config_path = options
        .opencode_config
        .clone()
        .unwrap_or_else(default_opencode_config_path);
    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow!("invalid OpenCode config path: {}", config_path.display()))?;

    let mut config = load_json5_document(&config_path)?;
    let root = config
        .as_object_mut()
        .ok_or_else(|| anyhow!("OpenCode config root must be an object"))?;

    let mcp_entry = json!({
        "type": "local",
        "command": [
            quickdep_bin.display().to_string(),
            "serve"
        ]
    });

    match root.get_mut("mcp") {
        Some(Value::Object(mcp)) => {
            mcp.insert(options.name.clone(), mcp_entry);
        }
        Some(_) => {
            bail!("existing OpenCode `mcp` config is not an object");
        }
        None => {
            let mut mcp = Map::new();
            mcp.insert(options.name.clone(), mcp_entry);
            root.insert("mcp".to_string(), Value::Object(mcp));
        }
    }

    if !options.dry_run {
        fs::create_dir_all(config_dir).with_context(|| {
            format!(
                "failed to create OpenCode config directory {}",
                config_dir.display()
            )
        })?;
        let serialized =
            serde_json::to_string_pretty(&config).context("failed to serialize OpenCode config")?;
        fs::write(&config_path, format!("{serialized}\n")).with_context(|| {
            format!("failed to write OpenCode config {}", config_path.display())
        })?;
    }

    Ok(InstallMcpResult {
        client: options.client.as_str().to_string(),
        name: options.name.clone(),
        command: command.to_vec(),
        target: "OpenCode".to_string(),
        method: "file".to_string(),
        config_path: Some(config_path.display().to_string()),
        dry_run: options.dry_run,
    })
}

fn run_command(program: &str, args: &[String]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to execute `{program}`"))?;

    if !status.success() {
        bail!(
            "`{program} {}` exited with status {}",
            args.join(" "),
            status
        );
    }

    Ok(())
}

fn load_json5_document(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read OpenCode config {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    json5::from_str(&content)
        .with_context(|| format!("failed to parse OpenCode config {}", path.display()))
}

fn default_opencode_config_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("opencode")
            .join("opencode.json");
    }

    PathBuf::from(".config")
        .join("opencode")
        .join("opencode.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_install_opencode_updates_mcp_entry() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("opencode.json");
        fs::write(
            &config_path,
            r#"
            {
              "$schema": "https://opencode.ai/config.json",
              "provider": {
                "demo": {
                  "name": "demo"
                }
              }
            }
            "#,
        )
        .unwrap();

        let result = install_mcp(InstallMcpOptions {
            client: McpClient::OpenCode,
            name: "quickdep".to_string(),
            quickdep_bin: Some(std::env::current_exe().unwrap()),
            dry_run: false,
            claude_scope: ClaudeScope::Local,
            opencode_config: Some(config_path.clone()),
        })
        .unwrap();

        assert_eq!(result["client"], "opencode");
        let updated: Value =
            serde_json::from_str(&fs::read_to_string(config_path).unwrap()).unwrap();
        assert_eq!(updated["mcp"]["quickdep"]["type"], "local");
        assert_eq!(updated["mcp"]["quickdep"]["command"][1], "serve");
    }

    #[test]
    fn test_install_opencode_accepts_empty_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("opencode.json");
        fs::write(&config_path, "").unwrap();

        let result = install_mcp(InstallMcpOptions {
            client: McpClient::OpenCode,
            name: "quickdep".to_string(),
            quickdep_bin: Some(std::env::current_exe().unwrap()),
            dry_run: false,
            claude_scope: ClaudeScope::Local,
            opencode_config: Some(config_path.clone()),
        })
        .unwrap();

        assert_eq!(result["client"], "opencode");
        let updated: Value =
            serde_json::from_str(&fs::read_to_string(config_path).unwrap()).unwrap();
        assert_eq!(updated["mcp"]["quickdep"]["type"], "local");
        assert_eq!(updated["mcp"]["quickdep"]["command"][1], "serve");
    }

    #[test]
    fn test_install_opencode_rejects_non_object_mcp() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("opencode.json");
        fs::write(&config_path, r#"{ "mcp": [] }"#).unwrap();

        let error = install_mcp(InstallMcpOptions {
            client: McpClient::OpenCode,
            name: "quickdep".to_string(),
            quickdep_bin: Some(std::env::current_exe().unwrap()),
            dry_run: false,
            claude_scope: ClaudeScope::Local,
            opencode_config: Some(config_path),
        })
        .unwrap_err();

        assert!(error.to_string().contains("`mcp` config is not an object"));
    }
}
