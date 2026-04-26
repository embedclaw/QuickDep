# QuickDep Daemon 设计

## 1. 背景

QuickDep 当前默认运行形态是 `quickdep serve`：

- MCP 客户端连接时启动一个新的 `quickdep serve` 进程
- 该进程同时持有项目注册、项目加载、扫描、数据库、缓存、watcher、MCP 请求处理
- CLI 子命令 `scan/status/debug` 也会临时创建自己的 `ProjectManager`

这套模型在无状态工具上可行，但对 QuickDep 这种重状态索引服务不合适。已经暴露出两个核心问题：

1. 同一个 MCP 客户端会拉起多个 `quickdep serve` 进程
2. 多个进程会并发触发同一项目的加载、扫描和 watcher，出现 `already loading`、`timed out waiting`、`Operation cancelled`

结论很明确：

- QuickDep 不是“每次调用临时启动的一次性工具”
- QuickDep 更像“本地代码索引后台服务”
- MCP / HTTP / Web / CLI 都应该是这个后台服务的访问入口

## 2. 目标

Daemon 改造要达成以下目标：

1. 同一台机器上只保留一个 QuickDep 后台实例
2. 同一项目在同一时刻只允许一个可写加载/扫描流程
3. MCP 客户端可以重复连接，但不会重复创建项目级状态
4. CLI、MCP、HTTP、Web 共享同一套项目状态和数据库
5. 项目首次加载是重操作，后续查询是轻操作
6. 保留当前 SQLite / ProjectManager / Watcher / HTTP / MCP 的大部分实现，不推倒重写

非目标：

1. 本期不做远程托管服务
2. 本期不做多机共享索引
3. 本期不重写 MCP tool 契约

## 3. 当前基础

现有代码已经具备 daemon 所需的大部分核心能力：

- `src/project/manager.rs`
  - 持有项目表、manifest、后台 scan channel
  - 已经是一个长期存活 runtime 的雏形
- `src/http/mod.rs`
  - 已支持 HTTP / WebSocket / streamable MCP over HTTP
- `src/mcp/mod.rs`
  - MCP tools 已经是纯“请求 -> 项目管理/存储查询”
- `src/cli/mod.rs`
  - 当前 CLI 通过 `ProjectRuntime` 访问项目状态，但每次都会自己 new manager
- `src/main.rs`
  - 当前进程模型把“服务入口”和“状态持有”绑死在 `serve` 里

因此本次改造不应重写能力，而应重写“进程模型”和“生命周期归属”。

## 4. 目标架构

### 4.1 角色划分

QuickDep 未来应分成三层：

1. `runtime`
   - 真正的服务本体
   - 持有 `ProjectManager`、manifest、数据库访问、watcher、缓存

2. `daemon`
   - 常驻进程
   - 独占持有一份 `runtime`
   - 对外暴露本地 RPC / HTTP

3. `frontends`
   - `quickdep scan/status/debug`
   - `quickdep mcp`
   - `quickdep --http`
   - Web UI
   - Claude / Codex / OpenCode MCP 客户端

这些前端都不再直接持有项目状态，而是请求 daemon。

### 4.2 运行关系

```text
Agent / CLI / Web
        |
        v
  MCP / HTTP / Local RPC
        |
        v
   QuickDep Daemon
        |
        v
ProjectManager + Scanner + Watcher + Storage + Cache
```

### 4.3 关键原则

1. 后台状态只存在于 daemon 一处
2. 前端入口可多开，但只能读写 daemon，不直接建状态
3. 同项目的扫描、加载、watcher 只能由 daemon 内部统一调度

## 5. 生命周期设计

### 5.1 安装期

- 安装二进制
- 安装 MCP 客户端配置
- 不触发项目扫描

### 5.2 项目期

第一次遇到一个项目时：

1. daemon 注册项目
2. daemon 加载配置
3. daemon 扫描项目
4. daemon 建立 SQLite / cache / watcher

这部分是重操作，应明确归属于 daemon。

### 5.3 会话期

Agent 会话开始时：

1. MCP 客户端连接 QuickDep
2. QuickDep 前端入口把请求转发给 daemon
3. daemon 返回结构化结果

Agent 会话不应重新创建项目级状态。

## 6. 单实例策略

### 6.1 全局单实例

一台机器在同一用户域下只允许一个 QuickDep daemon。

建议机制：

- 启动时创建全局锁文件，例如 `~/.quickdep/daemon/daemon.lock`
- 锁文件内容记录：
  - pid
  - 启动时间
  - socket 路径
  - 版本号
- 若锁存在：
  - 先检查本地 endpoint 是否可达
  - 可达则拒绝第二个 daemon 启动
  - 不可达则清理陈旧锁并重建

### 6.2 项目级串行化

即便只有一个 daemon，也必须保证同一项目不会并发进入多个加载/扫描流程。

建议机制：

- `project_id -> inflight task`
- 当项目状态为 `Loading` 时：
  - 新请求复用已有 future 或等待同一个结果
- 不允许重复创建 watcher
- 不允许多个扫描任务同时写同一个 `.quickdep/symbols.db`

## 7. 本地通信设计

### 7.1 选择

本期建议：

- macOS / Linux：Unix Domain Socket
- Windows：本地回环端口，后续再切 Named Pipe

原因：

1. 本地通信，不需要暴露公网端口
2. 权限边界清楚
3. 更适合 daemon
4. 可以避免用户误把 daemon 当成公网服务

### 7.2 路径建议

- macOS / Linux：`~/.quickdep/daemon/daemon.sock`
- Windows：`127.0.0.1:41237`

### 7.3 协议

本期使用轻量 JSON line RPC，统一封装：

- CLI 请求
- MCP tool 请求
- HTTP 启动请求
- daemon 状态 / 停止请求

## 8. 三阶段实施方案

### Phase A：引入 daemon，先接管 CLI 生命周期

目标：

- 建立单实例后台服务
- 让 `scan/status/debug` 不再创建临时 `ProjectManager`

交付：

- `quickdep daemon`
- `quickdep daemon status`
- `quickdep daemon stop`
- `QuickDepRuntime`
- `DaemonServer`
- 全局锁文件
- 本地 socket / 回环端口

### Phase B：MCP 改为 daemon 代理

目标：

- `quickdep serve` 不再持有项目状态
- stdio MCP 请求全部转发到 daemon

交付：

- `QuickDepServer::from_daemon_proxy(...)`
- MCP tool 调用转发到 daemon
- CLI 与 MCP 共享同一项目状态

### Phase C：HTTP / Web 接入 daemon

目标：

- HTTP / Web 也不再直接持有运行态
- daemon 内统一托管 HTTP 服务

交付：

- `quickdep serve --http` 通过 daemon 确保 HTTP 监听
- Web UI 连接 daemon 暴露的本地 HTTP / MCP 服务
- daemon 状态输出包含 HTTP listener 信息
