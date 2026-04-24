# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Initial QuickDep implementation for project scanning, symbol extraction, dependency resolution, SQLite storage, caching, and file watching.
- Tree-sitter parsers for Rust, TypeScript, Python, and Go, plus configurable extension-to-language overrides through `parser.map`.
- MCP server with project management, scan control, interface search, interface detail lookup, dependency traversal, call-chain lookup, file interface listing, batch queries, and database rebuild support.
- MCP resources for project lists, project status, project interface lists, interface detail, and dependency views.
- Optional localhost HTTP server with REST endpoints, streamable MCP over `/mcp`, WebSocket project status streaming, health checks, and CORS support.
- FTS5-backed interface search with automatic SQL `LIKE` fallback, plus server-side tool filtering via `--tools`.
- Integration test suites for parser, resolver, storage, MCP, and end-to-end flows.
- GitHub Actions CI for `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings`.

### Changed

- Updated `README.md` with the current CLI, configuration, HTTP transport, and usage examples.
- Added `docs/API.md` to document MCP, HTTP, WebSocket, resource, and error contracts.
