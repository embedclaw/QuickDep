use std::path::Path;

use anyhow::Result;
use quickdep::mcp::QuickDepServer;
use rmcp::{
    model::{CallToolRequestParams, ReadResourceRequestParams, ResourceContents},
    ClientHandler, ServiceExt,
};
use serde_json::{json, Value};

use crate::common::create_simple_rust_workspace;

#[derive(Debug, Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        rmcp::model::ClientInfo::default()
    }
}

async fn start_test_server(
    project_root: &Path,
) -> Result<rmcp::service::RunningService<rmcp::service::RoleClient, TestClient>> {
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let server = QuickDepServer::from_workspace(project_root).await?;
    tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });

    Ok(TestClient.serve(client_transport).await?)
}

#[tokio::test]
async fn serves_dependency_resources_after_scan() -> Result<()> {
    let workspace = create_simple_rust_workspace();
    let client = start_test_server(workspace.path()).await?;

    client
        .call_tool(
            CallToolRequestParams::new("scan_project").with_arguments(
                json!({
                    "project": {
                        "path": workspace.path().display().to_string()
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;

    let search = client
        .call_tool(
            CallToolRequestParams::new("find_interfaces").with_arguments(
                json!({
                    "project": {
                        "path": workspace.path().display().to_string()
                    },
                    "query": "entry"
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let symbol_id = search.structured_content.unwrap()["interfaces"][0]["id"]
        .as_str()
        .expect("missing symbol id")
        .to_string();

    let projects = client
        .read_resource(ReadResourceRequestParams::new("quickdep://projects"))
        .await?;
    let projects_text = match &projects.contents[0] {
        ResourceContents::TextResourceContents { text, .. } => text,
        _ => panic!("expected text resource"),
    };
    let projects_json: Value = serde_json::from_str(projects_text)?;
    let project_id = projects_json["projects"][0]["id"]
        .as_str()
        .expect("missing project id");

    let deps_uri = format!("quickdep://project/{project_id}/interface/{symbol_id}/deps");
    let dependencies = client
        .read_resource(ReadResourceRequestParams::new(deps_uri))
        .await?;
    let deps_text = match &dependencies.contents[0] {
        ResourceContents::TextResourceContents { text, .. } => text,
        _ => panic!("expected text resource"),
    };
    let deps_json: Value = serde_json::from_str(deps_text)?;
    assert_eq!(deps_json["interface"]["name"], "entry");
    assert_eq!(deps_json["direction"], "outgoing");
    assert!(
        deps_json["dependencies"]
            .as_array()
            .expect("dependencies should be an array")
            .len()
            >= 2
    );

    Ok(())
}
