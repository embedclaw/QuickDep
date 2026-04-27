# QuickDep 使用说明

## 1. QuickDep 是什么

QuickDep 是一个面向代码仓库的依赖分析服务。它会扫描项目源码，提取符号和依赖关系，写入本地 SQLite 数据库，然后通过以下几种方式提供查询能力：

- `stdio MCP`：给 Agent / MCP 客户端使用
- `HTTP`：给本地工具或 Web 前端使用
- `CLI debug`：给开发者直接排查项目图谱使用

当前支持的语言：

- Rust
- TypeScript
- JavaScript
- Java
- C#
- Kotlin
- PHP
- Ruby
- Swift
- Objective-C (`objc`)
- Python
- Go
- C
- C++

QuickDep 会把项目数据存到项目目录下的 `.quickdep/` 中，并支持增量扫描、文件监控、缓存、重建数据库、调用链查询等能力。

---

## 2. 最终交付物

项目最终交付的核心内容包括：

- 一个可执行的 `quickdep` CLI
- 一个默认以 `stdio` 运行的 MCP Server
- 一套面向项目、符号、依赖和任务上下文的 MCP Tools
- 一组 MCP Resources
- 一个可选启用的本地 HTTP Server
- 一个本地 SQLite 图数据库
- 增量扫描与文件变更监控能力
- 基于 `.quickdep/` 的日志、数据库、manifest 持久化

运行后你会在项目目录看到：

- `.quickdep/symbols.db`：SQLite 图数据库
- `.quickdep/manifest.json`：项目注册与统计信息
- `.quickdep/logs/quickdep.log`：滚动日志文件

---

## 3. 安装方式

截至 `2026-04-27`，当前已经实测有效的安装方式包括源码安装；GitHub Release 也已经公开发布：

```bash
cargo install --path .
quickdep --version
```

公开分发链路当前状态如下：

- GitHub Releases：已经公开发布，仓库地址是 `https://github.com/embedclaw/QuickDep/releases`
- Homebrew：目标命令是 `brew install embedclaw/tap/quickdep`，当前公式还未公开可用
- npm 二进制包装器：目标命令是 `npm i -g @embedclaw/quickdep`，当前包还未发布

如果你想让 Claude Code / Codex / OpenCode 帮你自动完成安装和接入，直接复制：

- [AGENT_INSTALL_PROMPT.md](AGENT_INSTALL_PROMPT.md)

如果你正在仓库根目录开发，也可以先本地编译：

```bash
cargo build --release
```

安装或编译完成后，可直接验证：

```bash
quickdep --version
```

分发和 Agent 集成细节见 [INTEGRATIONS.md](INTEGRATIONS.md)。

---

## 4. 最常见的三种使用方式

### 4.1 作为 MCP Server 使用

默认直接运行：

```bash
quickdep
```

这会以当前目录作为工作区，启动 `stdio MCP` 服务。

适合：

- 给本地 Agent 工具接入
- 给支持 MCP 的客户端接入

### 4.2 作为 HTTP 服务使用

同时开启 MCP stdio 和 HTTP：

```bash
quickdep --http 8080
```

只开启 HTTP：

```bash
quickdep --http 8080 --http-only
```

默认监听：

- `http://127.0.0.1:8080/mcp`
- `http://127.0.0.1:8080/api/...`
- `ws://127.0.0.1:8080/ws/projects`
- `http://127.0.0.1:8080/health`

### 4.3 作为本地排查工具使用

直接扫描一个项目：

```bash
quickdep scan /path/to/project
```

查看状态：

```bash
quickdep status /path/to/project
```

查看某个接口的依赖：

```bash
quickdep debug /path/to/project --deps src/lib.rs::entry
```

查看某个文件里有哪些接口：

```bash
quickdep debug /path/to/project --file src/lib.rs
```

---

## 5. CLI 命令说明

基础形式：

```bash
quickdep [OPTIONS] [COMMAND]
```

命令列表：

- `serve`：启动 MCP 服务，默认命令
- `scan <path>`：扫描指定项目
- `status <path>`：查看项目状态
- `debug <path>`：调试查询

常用全局参数：

- `--http <port>`：启用 HTTP 服务
- `--http-only`：只启用 HTTP，不启用 stdio MCP
- `--tools a,b,c`：只暴露指定工具
- `--log-level <debug|info|warn|error>`：设置日志级别

---

## 6. MCP Tools

QuickDep 当前提供以下 17 个工具：

- `list_projects`
- `scan_project`
- `get_scan_status`
- `cancel_scan`
- `find_interfaces`
- `get_interface`
- `get_dependencies`
- `get_call_chain`
- `get_file_interfaces`
- `get_verification_context`
- `get_task_context`
- `analyze_workflow_context`
- `analyze_change_impact`
- `analyze_behavior_context`
- `locate_relevant_code`
- `batch_query`
- `rebuild_database`

典型用途：

- `scan_project`：首次加载或重新扫描项目
- `find_interfaces`：按名称搜索符号；更适合你已经知道部分符号名时做低层查找
- `get_interface`：查看单个符号详情；返回 `evidence`，包含 `assessment / static_incoming_count / dynamic_risk / verification_hints`
- `get_dependencies`：查看上游或下游依赖；同时返回当前符号的 `evidence`
- `get_call_chain`：查两点之间调用链
- `get_verification_context`：查看删除/清理/单符号判断时的验证证据包，包括直接调用者、直接被调方、相关文件、搜索词和下一步验证动作
- `get_task_context`：默认的高层入口；适合自然语言工程问题，按场景自动路由到 `locate / behavior / impact / workflow / call_chain / watcher`
- `analyze_workflow_context`：强制走 `workflow`，适合状态流转、审批、调度、排队类问题
- `analyze_change_impact`：强制走 `impact`，适合重构、改动影响面、风险分析
- `analyze_behavior_context`：强制走 `behavior`，适合“为什么会这样”、失败原因、运行时行为
- `locate_relevant_code`：强制走 `locate`，适合先缩小到最该读的文件和符号
- `rebuild_database`：数据库需要完整重建时使用

低层符号查询现在默认返回“证据包”语义，而不是“判决”语义：

- `assessment = unused_candidate` 只表示“当前静态图里没发现调用者”
- `assessment = dynamic_entry_candidate` 表示“静态图里没发现调用者，但名字/路径看起来像动态注册入口，不能直接删”
- `verification_hints` 会告诉你下一步该查全文、查注册点，还是优先看哪些相关文件

---

## 7. MCP Resources

可读取的资源包括：

- `quickdep://projects`
- `quickdep://project/{id}/status`
- `quickdep://project/{id}/interfaces`
- `quickdep://project/{id}/interface/{id}`
- `quickdep://project/{id}/interface/{id}/deps`

适合在只读场景下直接获取结构化资源，而不是主动调用工具。

---

## 8. HTTP API 快速示例

触发扫描：

```bash
curl -X POST http://127.0.0.1:8080/api/projects/scan \
  -H 'content-type: application/json' \
  -d '{}'
```

搜索接口：

```bash
curl -X POST http://127.0.0.1:8080/api/interfaces/search \
  -H 'content-type: application/json' \
  -d '{
    "project": { "path": "/path/to/project" },
    "query": "helper",
    "limit": 10
  }'
```

查看依赖：

```bash
curl -X POST http://127.0.0.1:8080/api/dependencies \
  -H 'content-type: application/json' \
  -d '{
    "project": { "path": "/path/to/project" },
    "interface": "src/lib.rs::entry",
    "direction": "outgoing",
    "max_depth": 5
  }'
```

查看任务上下文：

```bash
curl -X POST http://127.0.0.1:8080/api/task-context \
  -H 'content-type: application/json' \
  -d '{
    "project": { "path": "/path/to/project" },
    "question": "改 helper 会影响谁？",
    "anchor_symbols": ["src/lib.rs::helper"],
    "mode": "auto",
    "budget": "normal",
    "allow_source_snippets": true
  }'
```

---

## 9. 配置文件

QuickDep 会按以下顺序查找配置：

1. `quickdep.toml`
2. `.quickdep/config.toml`

一个常用配置示例：

```toml
[scan]
include = ["src/**"]
exclude = [
  "target/**",
  "node_modules/**",
  ".research/**",
  ".cache/**",
  "coverage/**",
  "artifacts/**",
  "tmp/**",
  ".tmp/**",
  "temp/**",
]
include_tests = false
languages = ["rust", "typescript", "ruby", "swift", "objc", "python", "go", "c", "cpp"]

[parser.map]
".vue" = "typescript"
".pyi" = "python"
".ipp" = "cpp"

[server]
http_enabled = false
http_port = 8080

[log]
level = "info"

[watcher]
idle_timeout = "5m"
```

说明：

- 默认配置已经开启 Rust、TypeScript、JavaScript、Java、C#、Kotlin、PHP、Ruby、Swift、Objective-C、Python、Go、C、C++
- 默认会排除常见构建目录、缓存目录、分析产物和临时目录；像 `.research/`、`coverage/`、`artifacts/`、`tmp/` 这类高噪声路径不会进入索引
- `scan.languages` 控制启用哪些语言解析器
- `parser.map` 可把自定义扩展映射到已有解析器
- `watcher.idle_timeout` 控制项目空闲后多久暂停监控

### C/C++ 项目推荐配置

如果你的项目包含头文件目录，建议把 `include/**` 也加入扫描范围：

```toml
[scan]
include = ["src/**", "include/**"]
exclude = ["build/**", "third_party/**"]
languages = ["c", "cpp"]
include_tests = false

[parser.map]
".ipp" = "cpp"
```

当前 C/C++ 支持覆盖：

- C：函数、结构体、枚举、typedef、全局变量/常量、`#include`、基础函数调用
- C++：namespace、class/struct、方法、构造/析构声明、继承、类型别名、`#include`、基础方法/函数调用
- include 解析：会优先尝试当前文件相对路径，并补充同名头/源文件候选，例如 `foo.h -> foo.c`、`foo.hpp -> foo.cpp`

当前已知边界：

- 不做宏展开和条件编译求值
- 不读取 `compile_commands.json`
- 不做模板特化、重载分派的精确语义分析
- `.h` 默认按 C 解析；如果你的仓库把 `.h` 作为 C++ 头文件使用，建议通过 `parser.map` 覆盖

---

## 10. QuickDep 的工作方式

### 首次扫描

首次扫描会：

1. 发现符合规则的源文件
2. 用 tree-sitter 提取符号、imports、raw dependencies
3. 解析跨文件依赖
4. 写入 SQLite
5. 构建缓存

### 增量更新

文件变更后会：

1. 通过 watcher 捕获事件
2. 用 blake3 比较文件 hash
3. 只重新解析变更文件
4. 对变更文件做符号级 diff
5. 只更新变更符号的依赖关系

### 非本地符号

QuickDep 还会把以下符号物化进图谱：

- `builtin`：语言内建符号
- `external`：外部库/包的符号

这意味着你在查询依赖时，不只会看到本地代码之间的边，也会看到部分内建/外部调用落点。

---

## 11. 典型使用流程

### 场景一：本地调试一个项目

```bash
quickdep scan /path/to/project
quickdep debug /path/to/project --stats
quickdep debug /path/to/project --deps src/lib.rs::entry
```

### 场景二：给 MCP 客户端接入当前仓库

```bash
cd /path/to/project
quickdep
```

然后让客户端连接这个 `stdio MCP` 服务。

### 场景三：给本地前端或脚本提供查询接口

```bash
cd /path/to/project
quickdep --http 8080 --http-only
```

之后通过 REST、WebSocket 或 streamable MCP over HTTP 访问。

---

## 12. 常见问题

### 为什么没有扫到 Java / C# / Kotlin / PHP / Ruby / Swift / Objective-C / Python / Go / JavaScript 文件？

默认配置已经包含这些语言。如果没扫到，通常是下面几种原因：

- 你在 `quickdep.toml` 里覆写了 `scan.languages`
- 你把文件排除在 `scan.include` / `scan.exclude` 之外
- 目标文件是测试文件，而默认 `include_tests = false`

### 为什么没有扫到 C / C++ 文件？

默认配置已经支持 `c` / `cpp`。如果仓库把头文件放在 `include/`、`vendor/` 之类的目录，建议显式补上扫描范围：

```toml
[scan]
include = ["src/**", "include/**"]
```

如果仓库把 `.h` 当作 C++ 头文件，还需要补一个扩展映射：

```toml
[parser.map]
".h" = "cpp"
```

### 数据库坏了或 schema 不匹配怎么办？

使用：

- MCP Tool：`rebuild_database`
- HTTP：`POST /api/projects/rebuild`

### 日志写到哪里？

日志会写到目标工作区下：

- `.quickdep/logs/quickdep.log`

对于 `quickdep scan /path/to/project` 这类命令，日志会落到被操作项目的 `.quickdep/logs/`，不是执行命令时所在的任意目录。

### 为什么项目空闲后似乎不再继续监控？

这是正常行为。QuickDep 会在空闲后暂停 watcher；再次访问项目、查询接口或触发扫描时会自动恢复。

---

## 13. 相关文档

- [README.zh-CN.md](../README.zh-CN.md)
- [QUICKDEP_PLAIN_LANGUAGE_GUIDE.md](QUICKDEP_PLAIN_LANGUAGE_GUIDE.md)
- [API.md](API.md)
- [INTEGRATIONS.md](INTEGRATIONS.md)
- [TEST_REPORT.md](TEST_REPORT.md)
- [web/README.md](../web/README.md)
- [CHANGELOG.md](../CHANGELOG.md)
