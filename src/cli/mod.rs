//! CLI command helpers for QuickDep.

mod install_mcp;

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use serde_json::{json, Map, Value};

pub use install_mcp::{ClaudeScope, InstallMcpOptions, McpClient};

use crate::{
    config::{load_settings, Settings},
    project::{get_manifest_path, ProjectConfig, ProjectId, ProjectManager, ProjectState},
    storage::Storage,
    CACHE_DIR, DB_FILE,
};

/// Run the `scan` command.
pub async fn run_scan(path: &Path, rebuild: bool) -> anyhow::Result<Value> {
    let runtime = ProjectRuntime::load(path).await?;
    let value = runtime.scan(rebuild).await;
    runtime.shutdown().await;
    value
}

/// Run the `status` command.
pub async fn run_status(path: &Path) -> anyhow::Result<Value> {
    let runtime = ProjectRuntime::load(path).await?;
    let value = runtime.status().await;
    runtime.shutdown().await;
    value
}

/// Run the `debug` command.
pub async fn run_debug(
    path: &Path,
    stats: bool,
    deps: Option<&str>,
    file: Option<&str>,
) -> anyhow::Result<Value> {
    let runtime = ProjectRuntime::load(path).await?;
    let value = runtime.debug(stats, deps, file).await;
    runtime.shutdown().await;
    value
}

/// Install QuickDep as an MCP server for a supported client.
pub fn run_install_mcp(options: InstallMcpOptions) -> anyhow::Result<Value> {
    install_mcp::install_mcp(options)
}

fn project_config_from_settings(settings: &Settings) -> ProjectConfig {
    ProjectConfig {
        include: settings.scan.include.clone(),
        exclude: settings.scan.exclude.clone(),
        languages: settings.scan.languages.clone(),
        include_tests: settings.scan.include_tests,
        parser_map: settings.parser.map.clone(),
        idle_timeout_secs: settings.watcher.idle_timeout.as_secs(),
    }
}

fn normalize_project_root(path: &Path) -> anyhow::Result<PathBuf> {
    let root = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize project path {}", path.display()))?;
    if !root.is_dir() {
        bail!("project path must be a directory: {}", root.display());
    }
    Ok(root)
}

fn normalize_file_path(project_root: &Path, file_path: &str) -> anyhow::Result<String> {
    let canonical =
        crate::security::validate_path(project_root, file_path).map_err(|error| anyhow!(error))?;
    let relative = canonical
        .strip_prefix(project_root)
        .map_err(|_| anyhow!("file '{}' is outside the project root", file_path))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn resolve_symbol(storage: &Storage, identifier: &str) -> anyhow::Result<crate::core::Symbol> {
    if let Some(symbol) = storage.get_symbol(identifier)? {
        return Ok(symbol);
    }

    if let Some(symbol) = storage.get_symbol_by_qualified_name(identifier)? {
        return Ok(symbol);
    }

    let exact = storage
        .search_symbols(identifier, 25)?
        .into_iter()
        .filter(|symbol| symbol.name == identifier)
        .collect::<Vec<_>>();

    match exact.len() {
        1 => Ok(exact.into_iter().next().expect("exact match missing")),
        0 => bail!("interface '{}' not found", identifier),
        _ => bail!(
            "interface '{}' is ambiguous; try a qualified name or symbol id",
            identifier
        ),
    }
}

struct ProjectRuntime {
    root: PathBuf,
    name: String,
    manager: ProjectManager,
    id: ProjectId,
}

impl ProjectRuntime {
    async fn load(path: &Path) -> anyhow::Result<Self> {
        let root = normalize_project_root(path)?;
        let settings = load_settings(&root)
            .with_context(|| format!("failed to load settings from {}", root.display()))?;
        let config = project_config_from_settings(&settings);
        let manifest_path = get_manifest_path(&root);
        let manager = ProjectManager::with_scanner(&manifest_path).await;
        let id = ProjectId::from_path(&root).map_err(|error| anyhow!(error))?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_string();
        manager
            .register_or_update(&root, name.clone(), Some(config))
            .await
            .map_err(|error| anyhow!(error))?;

        Ok(Self {
            root,
            name,
            manager,
            id,
        })
    }

    async fn scan(&self, rebuild: bool) -> anyhow::Result<Value> {
        self.manager
            .scan(&self.id, rebuild)
            .await
            .map_err(|error| anyhow!(error))?;
        let project = self.ensure_loaded_project().await?;

        Ok(json!({
            "project": self.project_header(),
            "state": project.state,
            "stats": self.database_stats()?,
            "rebuild": rebuild,
        }))
    }

    async fn status(&self) -> anyhow::Result<Value> {
        Ok(json!({
            "project": self.project_header(),
            "state": self.manager.status(&self.id).await.map_err(|error| anyhow!(error))?,
            "manifest": self.manifest_value().await?,
            "stats": self.database_stats()?,
            "database_path": self.database_path().display().to_string(),
        }))
    }

    async fn debug(
        &self,
        stats: bool,
        deps: Option<&str>,
        file: Option<&str>,
    ) -> anyhow::Result<Value> {
        let show_stats = stats || (deps.is_none() && file.is_none());
        let mut response = Map::new();
        response.insert("project".to_string(), self.project_header());

        if show_stats {
            response.insert("stats".to_string(), self.status().await?);
        }

        if let Some(interface) = deps {
            response.insert(
                "dependencies".to_string(),
                self.debug_dependencies(interface).await?,
            );
        }

        if let Some(file_path) = file {
            response.insert(
                "file_interfaces".to_string(),
                self.debug_file_interfaces(file_path).await?,
            );
        }

        Ok(Value::Object(response))
    }

    async fn debug_dependencies(&self, interface: &str) -> anyhow::Result<Value> {
        let project = self.ensure_loaded_project().await?;
        let storage = Storage::new(&project.database_path())?;
        let symbol = resolve_symbol(&storage, interface)?;

        Ok(json!({
            "interface": symbol,
            "outgoing": storage.get_dependency_chain_forward(&symbol.id, 5)?,
            "incoming": storage.get_dependency_chain_backward(&symbol.id, 5)?,
        }))
    }

    async fn debug_file_interfaces(&self, file_path: &str) -> anyhow::Result<Value> {
        let project = self.ensure_loaded_project().await?;
        let relative_path = normalize_file_path(&project.path, file_path)?;
        let storage = Storage::new(&project.database_path())?;

        Ok(json!({
            "file_path": relative_path,
            "interfaces": storage.get_symbols_by_file(&relative_path)?,
        }))
    }

    async fn ensure_loaded_project(&self) -> anyhow::Result<crate::project::Project> {
        let project = self
            .manager
            .get(&self.id)
            .await
            .map_err(|error| anyhow!(error))?
            .ok_or_else(|| anyhow!("project {} not found", self.id))?;

        if let ProjectState::Failed { error, .. } = &project.state {
            bail!("project scan failed: {}", error);
        }

        Ok(project)
    }

    fn database_path(&self) -> PathBuf {
        self.root.join(CACHE_DIR).join(DB_FILE)
    }

    fn database_stats(&self) -> anyhow::Result<Option<Value>> {
        let database_path = self.database_path();
        if !database_path.exists() {
            return Ok(None);
        }

        let storage = Storage::new(&database_path)?;
        let stats = storage.get_stats()?;
        Ok(Some(serde_json::to_value(stats)?))
    }

    async fn manifest_value(&self) -> anyhow::Result<Value> {
        let manifest = self.manager.get_manifest().await;
        let entry = manifest
            .get_project(&self.id)
            .ok_or_else(|| anyhow!("manifest entry missing for {}", self.id))?;

        Ok(json!({
            "registered_at": entry.registered_at,
            "last_accessed": entry.last_accessed,
            "last_scanned": entry.last_scanned,
            "file_count": entry.file_count,
            "symbol_count": entry.symbol_count,
            "dependency_count": entry.dependency_count,
            "config": entry.config,
        }))
    }

    fn project_header(&self) -> Value {
        json!({
            "id": self.id.as_str(),
            "name": self.name,
            "path": self.root.display().to_string(),
        })
    }

    async fn shutdown(&self) {
        self.manager.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_sample_project(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            r#"
pub fn entry() {
    helper();
}

pub fn helper() {}
"#,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_run_scan_and_status() {
        let project_dir = TempDir::new().unwrap();
        write_sample_project(project_dir.path());

        let scan = run_scan(project_dir.path(), false).await.unwrap();
        assert_eq!(scan["stats"]["symbols"], 2);
        assert_eq!(scan["stats"]["dependencies"], 1);

        let status = run_status(project_dir.path()).await.unwrap();
        assert_eq!(status["manifest"]["symbol_count"], 2);
        assert_eq!(status["stats"]["symbols"], 2);
    }

    #[tokio::test]
    async fn test_run_debug_outputs_dependencies_and_file_interfaces() {
        let project_dir = TempDir::new().unwrap();
        write_sample_project(project_dir.path());

        let debug = run_debug(project_dir.path(), false, Some("entry"), Some("src/lib.rs"))
            .await
            .unwrap();
        assert_eq!(debug["dependencies"]["interface"]["name"], "entry");
        assert_eq!(
            debug["file_interfaces"]["interfaces"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn test_run_scan_refreshes_config_from_settings() {
        let project_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(project_dir.path().join("src")).unwrap();
        std::fs::create_dir_all(project_dir.path().join("examples")).unwrap();
        std::fs::write(
            project_dir.path().join("src/lib.rs"),
            "pub fn src_only() {}\n",
        )
        .unwrap();
        std::fs::write(
            project_dir.path().join("examples/demo.rs"),
            "pub fn example_only() {}\n",
        )
        .unwrap();
        std::fs::write(
            project_dir.path().join(crate::CONFIG_FILE),
            r#"
[scan]
include = ["src/**"]
exclude = []
languages = ["rust"]
"#,
        )
        .unwrap();

        let initial = run_scan(project_dir.path(), false).await.unwrap();
        assert_eq!(initial["stats"]["symbols"], 1);

        std::fs::write(
            project_dir.path().join(crate::CONFIG_FILE),
            r#"
[scan]
include = ["examples/**"]
exclude = []
languages = ["rust"]
"#,
        )
        .unwrap();

        let rescanned = run_scan(project_dir.path(), true).await.unwrap();
        assert_eq!(rescanned["stats"]["symbols"], 1);

        let status = run_status(project_dir.path()).await.unwrap();
        assert_eq!(status["manifest"]["config"]["include"][0], "examples/**");

        let debug_example = run_debug(project_dir.path(), false, None, Some("examples/demo.rs"))
            .await
            .unwrap();
        assert_eq!(
            debug_example["file_interfaces"]["interfaces"][0]["name"],
            "example_only"
        );

        let debug_src = run_debug(project_dir.path(), false, None, Some("src/lib.rs"))
            .await
            .unwrap();
        assert_eq!(
            debug_src["file_interfaces"]["interfaces"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn test_run_scan_without_config_discovers_python_project() {
        let project_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(project_dir.path().join("src/app")).unwrap();
        std::fs::write(
            project_dir.path().join("src/app/main.py"),
            "def greet(name):\n    return name\n",
        )
        .unwrap();

        let scanned = run_scan(project_dir.path(), true).await.unwrap();
        assert_eq!(scanned["stats"]["files"], 1);
        assert_eq!(scanned["stats"]["symbols"], 1);

        let debug = run_debug(project_dir.path(), false, None, Some("src/app/main.py"))
            .await
            .unwrap();
        assert_eq!(debug["file_interfaces"]["interfaces"][0]["name"], "greet");
    }
}
