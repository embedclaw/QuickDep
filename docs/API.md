# QuickDep API

This document describes the public interfaces exposed by QuickDep:

- MCP over stdio
- MCP over streamable HTTP at `/mcp`
- REST endpoints under `/api`
- WebSocket project status updates at `/ws/projects`

When the HTTP server is enabled, QuickDep listens on `127.0.0.1:<port>`.

## Starting the Server

```bash
# stdio MCP only
quickdep

# stdio MCP + HTTP
quickdep --http 8080

# HTTP only
quickdep --http 8080 --http-only

# Restrict the exposed tools
quickdep --http 8080 --tools list_projects,scan_project,find_interfaces
```

If `--tools` is set, disabled MCP tools are omitted from the MCP tool list, and the matching REST endpoints return an error.

## Project Selector

Most tool and HTTP request bodies accept the same project selector:

```json
{
  "project": {
    "project_id": "a1b2c3d4e5f6a7b8",
    "path": "/absolute/path/to/project"
  }
}
```

Rules:

- Omit both fields to target the server workspace.
- Set `project_id` to use a known registered project.
- Set `path` to auto-register and target a project by filesystem path.
- If both are set, they must point to the same project.

## MCP Tools

All MCP tool responses are JSON objects.

### `list_projects`

Request:

```json
{}
```

Response shape:

```json
{
  "default_project_id": "a1b2c3d4e5f6a7b8",
  "projects": [
    {
      "id": "a1b2c3d4e5f6a7b8",
      "name": "quickdep",
      "path": "/absolute/path",
      "state": "NotLoaded",
      "is_default": true
    }
  ]
}
```

### `scan_project`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "rebuild": false
}
```

Response fields:

- `project`: project record with current state
- `rebuild`: whether a rebuild was requested
- `stats`: storage counts for `symbols`, `dependencies`, `imports`, and `files`

### `get_scan_status`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  }
}
```

Response:

```json
{
  "project": {
    "id": "a1b2c3d4e5f6a7b8",
    "name": "project-name",
    "path": "/absolute/path/to/project",
    "state": {
      "Loaded": {
        "file_count": 12,
        "symbol_count": 40,
        "dependency_count": 55,
        "loaded_at": 1710000000,
        "watching": true
      }
    },
    "is_default": false
  }
}
```

`state` is one of:

- `NotLoaded`
- `Loading`
- `Loaded`
- `WatchPaused`
- `Failed`

### `cancel_scan`

Request body matches `get_scan_status`.

Response:

```json
{
  "cancel_requested": true,
  "project": { "...": "..." }
}
```

### `find_interfaces`

Prefer this as a low-level fallback when you already know part of the symbol name. For natural-language engineering questions, prefer `get_task_context` or the scene-specific context tools first.

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "query": "helper",
  "limit": 10
}
```

Notes:

- `query` is required and must be non-empty.
- `limit` defaults to `20`.
- Search uses exact symbol index hits first, then SQLite FTS5 when available, otherwise `LIKE`.

Response:

```json
{
  "query": "helper",
  "limit": 10,
  "interfaces": [
    {
      "id": "symbol_id",
      "name": "helper",
      "qualified_name": "src/lib.rs::helper",
      "kind": "Function",
      "file_path": "src/lib.rs",
      "line": 2,
      "column": 0,
      "visibility": "Public",
      "source": "Local"
    }
  ]
}
```

### `get_interface`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "interface": "src/lib.rs::helper"
}
```

`interface` may be a symbol ID, qualified name, or exact symbol name.

Response:

```json
{
  "interface": {
    "...": "full symbol record"
  }
}
```

### `get_dependencies`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "interface": "src/lib.rs::helper",
  "direction": "outgoing",
  "max_depth": 3
}
```

Notes:

- `direction` may be `outgoing`, `incoming`, or `both`
- `max_depth` defaults to `3`

Response:

- `outgoing` or `incoming`: `interface`, `direction`, `max_depth`, `dependencies`
- `both`: `interface`, `direction`, `max_depth`, `outgoing`, `incoming`

### `get_call_chain`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "from_interface": "src/lib.rs::entry",
  "to_interface": "src/lib.rs::helper",
  "max_depth": 3
}
```

Response:

```json
{
  "from": { "...": "source symbol" },
  "to": { "...": "target symbol" },
  "max_depth": 3,
  "path": []
}
```

### `get_file_interfaces`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "file_path": "src/lib.rs"
}
```

Response:

```json
{
  "file_path": "src/lib.rs",
  "interfaces": []
}
```

### `get_task_context`

This is the default high-level entry point for natural-language engineering questions. It auto-routes to the best scene (`locate`, `behavior`, `impact`, `workflow`, `call_chain`, or `watcher`) from the question, anchors, workspace hints, and runtime hints.

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "question": "为什么这里失败会升级？",
  "anchor_symbols": [
    "src/lib.rs::helper"
  ],
  "anchor_files": [],
  "mode": "auto",
  "budget": "lean",
  "allow_source_snippets": true,
  "workspace": {
    "active_file": "src/lib.rs",
    "selection_symbol": "helper",
    "selection_line": 8
  },
  "runtime": {
    "stacktrace_symbols": [
      "helper"
    ]
  },
  "conversation": {
    "previous_targets": [
      "helper"
    ]
  }
}
```

Notes:

- `mode` may be `auto`, `locate`, `behavior`, `impact`, `workflow`, `call_chain`, or `watcher`.
- `budget` may be `lean`, `normal`, or `wide`.
- `anchor_symbols` accepts symbol IDs, qualified names, or exact symbol names.
- `anchor_files`, `workspace`, `runtime`, and `conversation` are optional evidence sources for agents.
- `workflow` packages may include `workflow_phases[*].supporting_symbols` so one phase can surface a small cluster of helper/gating functions instead of forcing a single linear symbol.
- In `workflow` mode, `primary_symbols` can include those phase supports when they materially explain the transition.
- `allow_source_snippets` defaults to `false`.
- `max_expansions` defaults to `1`.

Response:

```json
{
  "scene": "behavior",
  "confidence": 0.78,
  "coverage": "partial",
  "status": "needs_code_read",
  "budget": {
    "requested": "lean",
    "applied": "normal",
    "expanded": true,
    "max_expansions": 1,
    "estimated_tokens": 512,
    "truncated": false
  },
  "evidence": {
    "question_signals": [
      "question looks like behavior analysis"
    ],
    "anchor_sources": [
      "runtime.stacktrace_symbols"
    ],
    "graph_signals": [
      "1 direct callers found",
      "2 direct callees found"
    ],
    "penalties": []
  },
  "resolved_anchors": {
    "symbols": [
      {
        "...": "symbol summary"
      }
    ],
    "files": [
      "src/lib.rs"
    ]
  },
  "package": {
    "target": {
      "...": "symbol summary"
    },
    "primary_symbols": [],
    "primary_files": [],
    "key_edges": [],
    "related_files": [],
    "suggested_reads": [],
    "source_snippets": [],
    "risk_summary": null
  },
  "expansion_hint": "read_implementation",
  "next_tool_calls": [],
  "fallback_to_code": true,
  "note": "Static graph narrowed the likely code region, but behavior confirmation still needs source reading."
}
```

### `analyze_workflow_context`

Request and response shapes match `get_task_context`, but the server forces `mode = "workflow"`.

Use it first for:

- workflow or state-transition questions
- approval / queue / scheduler / dispatch issues
- prompts like “为什么还停留在 Queued”

### `analyze_change_impact`

Request and response shapes match `get_task_context`, but the server forces `mode = "impact"`.

Use it first for:

- refactor impact analysis
- rename risk
- “改这个会影响谁”

### `analyze_behavior_context`

Request and response shapes match `get_task_context`, but the server forces `mode = "behavior"`.

Use it first for:

- “为什么会这样”
- failure explanation
- stack trace or runtime debugging questions

### `locate_relevant_code`

Request and response shapes match `get_task_context`, but the server forces `mode = "locate"`.

Use it first for:

- narrowing down which files to read next
- editor/file anchored discovery
- “这个功能先看哪里”

### `batch_query`

Request:

```json
{
  "project": {
    "path": "/absolute/path/to/project"
  },
  "queries": [
    {
      "kind": "find_interfaces",
      "query": "helper",
      "limit": 5
    },
    {
      "kind": "get_dependencies",
      "interface": "src/lib.rs::helper",
      "direction": "outgoing",
      "max_depth": 2
    }
  ]
}
```

Supported `kind` values:

- `find_interfaces`
- `get_interface`
- `get_dependencies`
- `get_call_chain`
- `get_file_interfaces`

Response:

```json
{
  "results": [
    {
      "index": 0,
      "kind": "find_interfaces",
      "ok": true,
      "result": { "...": "tool result" }
    },
    {
      "index": 1,
      "kind": "get_dependencies",
      "ok": false,
      "error": "message"
    }
  ]
}
```

### `rebuild_database`

Request body matches `get_scan_status`.

Response shape matches `scan_project` with `rebuild: true`.

## MCP Resources

QuickDep exposes these resource URIs:

- `quickdep://projects`
- `quickdep://project/{project_id}/status`
- `quickdep://project/{project_id}/interfaces`

Resource templates:

- `quickdep://project/{project_id}/status`
- `quickdep://project/{project_id}/interfaces`
- `quickdep://project/{project_id}/interface/{symbol_id}`
- `quickdep://project/{project_id}/interface/{symbol_id}/deps`

Notes:

- Resources return pretty-printed JSON text with MIME type `application/json`.
- `.../interface/{symbol_id}/deps` uses the same dependency query as `get_dependencies` with `direction = "outgoing"` and `max_depth = 3`.

## HTTP Endpoints

The REST API mirrors the MCP tool contract.

### Health

`GET /health`

Response:

```json
{
  "status": "ok"
}
```

### REST Routes

| Method | Path | Equivalent tool |
|--------|------|-----------------|
| `GET` | `/api/projects` | `list_projects` |
| `POST` | `/api/projects/scan` | `scan_project` |
| `POST` | `/api/projects/status` | `get_scan_status` |
| `POST` | `/api/projects/cancel` | `cancel_scan` |
| `POST` | `/api/projects/rebuild` | `rebuild_database` |
| `POST` | `/api/interfaces/search` | `find_interfaces` |
| `POST` | `/api/interfaces/detail` | `get_interface` |
| `POST` | `/api/dependencies` | `get_dependencies` |
| `POST` | `/api/call-chain` | `get_call_chain` |
| `POST` | `/api/files/interfaces` | `get_file_interfaces` |
| `POST` | `/api/task-context` | `get_task_context` |
| `POST` | `/api/query/batch` | `batch_query` |

REST responses return the same JSON payload as the corresponding MCP tool result.

Examples:

```bash
curl http://127.0.0.1:8080/health
```

```bash
curl -X POST http://127.0.0.1:8080/api/projects/scan \
  -H 'content-type: application/json' \
  -d '{
    "project": { "path": "/absolute/path/to/project" },
    "rebuild": false
  }'
```

```bash
curl -X POST http://127.0.0.1:8080/api/interfaces/search \
  -H 'content-type: application/json' \
  -d '{
    "project": { "path": "/absolute/path/to/project" },
    "query": "helper",
    "limit": 10
  }'
```

### Streamable MCP over HTTP

`/mcp` hosts the rmcp streamable HTTP transport. Use it when your MCP client prefers HTTP instead of stdio.

## WebSocket

`GET /ws/projects`

Query parameters:

- `project_id`: optional registered project ID
- `path`: optional project path
- `interval_ms`: optional polling interval in milliseconds

`interval_ms` defaults to `1000` and is clamped to `100..30000`.

Behavior:

- sends one initial status message immediately
- sends a new status message only when the project state changes
- sending the text frame `refresh` forces an immediate status push
- errors are sent as a final `type = "error"` message and then the socket closes

Status message:

```json
{
  "type": "status",
  "data": {
    "project": { "...": "same payload as get_scan_status" }
  }
}
```

Error message:

```json
{
  "type": "error",
  "error": {
    "code": -32602,
    "message": "invalid params",
    "data": null
  }
}
```

## CORS

QuickDep allows cross-origin HTTP requests with:

- origins: `*`
- methods: `GET`, `POST`, `OPTIONS`
- headers: `content-type`, `accept`, `origin`

## Error Handling

REST errors use this shape:

```json
{
  "error": {
    "code": -32602,
    "message": "interface cannot be empty",
    "data": null
  }
}
```

Status mapping:

- `400 Bad Request`: invalid params, invalid request, or parse errors
- `404 Not Found`: unknown resources or disabled tools
- `500 Internal Server Error`: storage, scan, serialization, or other runtime failures
