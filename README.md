<p align="right">
  <a href="./README.zh-CN.md">简体中文</a>
</p>

# QuickDep

> Local-first dependency intelligence that helps agents get to the right code faster.

![License: MIT](https://img.shields.io/badge/License-MIT-0f766e.svg)
![Rust 1.75+](https://img.shields.io/badge/Rust-1.75%2B-d97706.svg)
![Protocol](https://img.shields.io/badge/MCP-stdio%20%7C%20HTTP-2563eb.svg)
![Surface](https://img.shields.io/badge/Surface-CLI%20%7C%20API%20%7C%20Web-111827.svg)

QuickDep turns a repository into a queryable symbol and dependency graph that both humans and agents can use.
Instead of making Claude, Codex, or your own tooling grep a large codebase blindly, QuickDep narrows the search space first and answers structural questions like:

- What calls this function?
- What depends on this interface?
- What is the shortest call path between two symbols?
- Which file owns these related declarations?

It runs locally, persists graph data in SQLite, updates incrementally, and exposes the result through MCP, HTTP, WebSocket, and a local web UI.
QuickDep is MIT licensed and designed for real coding workflows where agents need to find the right files before they can reason correctly.

## The Question QuickDep Answers Better Than grep

**"What breaks if I change `helper()`?"**

`grep` can show where `helper` appears.
QuickDep tells you who **calls** it, what **depends** on it, and how the **impact chain** fans out across the repository.

```text
get_dependencies("helper", direction="incoming")
```

That is the core value proposition: not more text hits, but a better first suspect list.

## Why QuickDep

Most code agents still reconstruct architecture from raw text search.
The real problem is not just token cost. It is that agents waste time reading the wrong files and manually filtering noisy grep results before they even reach the likely cause.

QuickDep precomputes the code graph once, keeps it warm as files change, and gives agents a fast structural narrowing layer for:

- refactor planning and impact analysis
- symbol lookup without brute-force search
- call-chain tracing across files and modules
- local code exploration through CLI, API, or web UI

If you want an LLM to answer "what should I inspect first?" with something better than guesswork, this is the missing layer.

## What Current Benchmarking Supports

We ran a realistic agent benchmark on `ark-runtime` with three routes:

- `Q`: QuickDep-first
- `N`: Native-only
- `H`: Hybrid (QuickDep first, then limited raw code reading)

Across the shared scenarios `S1-S5`, the current data supports this story:

- `Hybrid` hit the first gold file or symbol in `9.2s` on average
- `Native-only` needed `16.9s` on average
- `Hybrid` reduced average file fan-out from `35.8` to `7.6`
- `Hybrid` reduced average raw source reading from about `42.1k chars` to `22.7k chars`

What the benchmark does **not** support yet:

- QuickDep-only does not yet give stable total-context savings
- On behavior-heavy questions, agents still need to read implementation details

So the honest claim today is:

> QuickDep already helps agents localize the right part of a large codebase faster. Token savings are a secondary benefit, not the primary promise.

Detailed benchmark notes:

- [docs/AGENT_HYBRID_BENCHMARK_REPORT.md](docs/AGENT_HYBRID_BENCHMARK_REPORT.md)

## Good Fit

- Agent-assisted development with Claude Code, Codex, or OpenCode
- Large local repositories where text search is too noisy
- Refactors, migrations, and dependency-aware code review
- Building your own code intelligence workflow on top of MCP or HTTP

## Supported Languages

QuickDep currently supports these languages in the local graph pipeline:

| Language | Notes |
| --- | --- |
| Rust | `rs` |
| TypeScript | `ts`, `tsx` |
| JavaScript | `js`, `jsx`, `mjs`, `cjs` |
| Java | `java` |
| C# | `cs` |
| Kotlin | `kt`, `kts` |
| PHP | `php`, `phtml` |
| Ruby | `rb`, `rake` |
| Swift | `swift` |
| Objective-C | `m` |
| Python | `py`, `pyi` |
| Go | `go` |
| C | `c`, `h` |
| C++ | `cc`, `cpp`, `cxx`, `hh`, `hpp`, `hxx` |

## Best Current Usage Pattern

Today, QuickDep works best as a **hybrid workflow**:

1. Use QuickDep to narrow to the right 3-10 files, symbols, and call paths
2. Let the agent read a much smaller amount of raw code
3. Use implementation reading only where behavior details still matter

That is a better fit for the current product than pretending the graph alone can answer every complex semantic question.

## QuickDep vs Common Alternatives

| Tool | MCP-native | Local-first setup | Graph traversal | Impact-oriented queries |
| --- | --- | --- | --- | --- |
| `grep` / `rg` | No | Yes | No | No |
| LSP "find references" | No | Yes | Weak | Weak |
| Sourcegraph-style code intelligence | No | Mixed | Partial | Partial |
| **QuickDep** | **Yes** | **Yes** | **Yes** | **Yes** |

QuickDep is built specifically for local MCP agents that need dependency and call-chain answers, not just symbol search.

## Quick Install

For published releases, the intended install paths are:

```bash
# Homebrew
brew install northcipher/tap/quickdep

# npm binary wrapper
npm i -g @northcipher/quickdep

# from source / latest repo state
cargo install --path .
```

Verify that the local service is alive:

```bash
# terminal 1
quickdep --http 8080 --http-only

# terminal 2
curl http://127.0.0.1:8080/health
# {"status":"ok"}
```

Then register QuickDep into your agent client:

```bash
quickdep install-mcp claude
quickdep install-mcp codex
quickdep install-mcp opencode
```

More distribution and integration details:

- [docs/INTEGRATIONS.md](docs/INTEGRATIONS.md)

## 30-Second Start and Verify

QuickDep defaults to `serve`, so `quickdep` starts the local stdio MCP server immediately:

```bash
# Start local stdio MCP in the current workspace
quickdep

# Start MCP stdio + HTTP on localhost:8080
quickdep --http 8080

# HTTP only
quickdep --http 8080 --http-only
```

If you want a fast health check before wiring it into an MCP client:

```bash
# run this from another terminal after starting QuickDep with HTTP enabled
curl http://127.0.0.1:8080/health
# {"status":"ok"}
```

Optional local web console:

```bash
cd web
npm install
npm run dev
```

The HTTP server exposes:

- streamable MCP at `/mcp`
- REST endpoints under `/api`
- project status updates at `/ws/projects`
- health checks at `/health`

## Example Questions QuickDep Can Answer

| You want to know | QuickDep surface |
| --- | --- |
| What calls `helper()`? | `get_dependencies` with `incoming` |
| What does this symbol depend on? | `get_dependencies` with `outgoing` |
| How do `entry` and `helper` connect? | `get_call_chain` |
| What interfaces live in one file? | `get_file_interfaces` |
| Can I browse this visually? | local web UI in [`web/`](web) |

## What Ships Today

- Tree-sitter parsers for Rust, TypeScript/JavaScript, Java, C#, Kotlin, PHP, Ruby, Swift, Objective-C, Python, Go, C, and C++
- SQLite-backed graph storage with WAL mode and FTS5-backed symbol search
- Incremental scanning with file watching, debounce, and pause/resume behavior
- MCP server with project, symbol, dependency, and call-chain tools
- HTTP API plus WebSocket status streaming
- Local web UI for project state, search, graph view, tables, and batch queries
- Agent installers for Claude Code, Codex, and OpenCode
- Tool filtering with `--tools` for tighter deployments

## What Makes It Attractive

- Local-first: your code stays on your machine
- Agent-native: built to be consumed by MCP clients, not retrofitted later
- Fast to adopt: install the binary, run `install-mcp`, start querying
- Practical surfaces: CLI for scripts, HTTP for integrations, web UI for humans
- Open and permissive: MIT licensed

## CLI Snapshot

```bash
quickdep [OPTIONS] [COMMAND]
```

Key commands:

- `serve`
- `scan <path>`
- `status <path>`
- `debug <path> --stats`
- `debug <path> --deps <interface>`
- `debug <path> --file <relative-path>`
- `install-mcp <claude|codex|opencode>`

Useful server flags:

- `--http <port>`
- `--http-only`
- `--tools <tool1,tool2,...>`
- `--log-level <trace|debug|info|warn|error>`

## Docs

- [docs/USAGE.md](docs/USAGE.md)
- [docs/API.md](docs/API.md)
- [docs/INTEGRATIONS.md](docs/INTEGRATIONS.md)
- [docs/QUICKDEP_PLAIN_LANGUAGE_GUIDE.md](docs/QUICKDEP_PLAIN_LANGUAGE_GUIDE.md)
- [docs/TEST_REPORT.md](docs/TEST_REPORT.md)
- [web/README.md](web/README.md)
- [CHANGELOG.md](CHANGELOG.md)

## Development

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## License

MIT. Use it, fork it, ship it.
