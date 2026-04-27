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

---

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

---

## 3. 当前基础

现有代码已经具备 daemon 所需的大部分核心能力：

- [src/project/manager.rs](/Users/luozx/work/quickdep/src/project/manager.rs)
  - 持有项目表、manifest、后台 scan channel
  - 已经是一个长期存活 runtime 的雏形
- [src/http/mod.rs](/Users/luozx/work/quickdep/src/http/mod.rs)
  - 已支持 HTTP / WebSocket / streamable MCP over HTTP
- [src/mcp/mod.rs](/Users/luozx/work/quickdep/src/mcp/mod.rs)
  - MCP tools 已经是纯“请求 -> 项目管理/存储查询”
- [src/cli/mod.rs](/Users/luozx/work/quickdep/src/cli/mod.rs)
  - 当前 CLI 通过 `ProjectRuntime` 访问项目状态，但每次都会自己 new manager
- [src/main.rs](/Users/luozx/work/quickdep/src/main.rs)
  - 当前进程模型把“服务入口”和“状态持有”绑死在 `serve` 里

因此本次改造不应重写能力，而应重写“进程模型”和“生命周期归属”。

---

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

最终运行关系：

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

---

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

---

## 6. 单实例策略

### 6.1 全局单实例

一台机器在同一用户域下只允许一个 QuickDep daemon。

建议机制：

- 启动时创建全局锁文件，例如 `~/.quickdep/daemon.lock`
- 锁文件内容记录：
  - pid
  - 启动时间
  - socket 路径
  - 版本号
- 若锁存在：
  - 先检查 pid 是否存活
  - 存活则拒绝第二个 daemon 启动
  - 死锁则清理并重建

### 6.2 项目级串行化

即便只有一个 daemon，也必须保证同一项目不会并发进入多个加载/扫描流程。

建议机制：

- `project_id -> inflight task`
- 当项目状态为 `Loading` 时：
  - 新请求复用已有 future 或等待同一个结果
- 不允许重复创建 watcher
- 不允许多个扫描任务同时写同一个 `.quickdep/symbols.db`

---

## 7. 本地通信设计

### 7.1 选择

本期建议：

- macOS / Linux：Unix Domain Socket
- Windows：Named Pipe

原因：

1. 本地通信，不需要暴露 TCP 端口
2. 权限边界清楚
3. 更适合 daemon
4. 可以避免用户误把 daemon 当成公网服务

### 7.2 路径建议

- macOS / Linux：`~/.quickdep/daemon.sock`
- Windows：`\\\\.\\pipe\\quickdep-daemon`

### 7.3 协议

本期不要发明复杂协议，直接使用 JSON RPC 风格即可。

请求可以复用现有 MCP / HTTP 参数结构：

- `scan_project`
- `get_scan_status`
- `find_interfaces`
- `get_dependencies`
- `get_task_context`
- `get_project_overview`

这样可以最大化复用现有请求模型，避免重复维护两套 DTO。

---

## 8. 三阶段实施方案

## Phase A：引入 daemon，先接管 CLI 生命周期

### 目标

先建立单实例后台服务，不立刻改 MCP。

### 交付

新增命令：

- `quickdep daemon`
- `quickdep daemon status`
- `quickdep daemon stop`

新增组件：

- `QuickDepRuntime`
- `DaemonServer`
- 全局锁文件
- 本地 socket / named pipe

CLI 变更：

- `quickdep scan <path>`
- `quickdep status <path>`
- `quickdep debug <path>`

优先走 daemon：

1. 尝试连接 daemon
2. daemon 不存在时：
   - 可选自动拉起
   - 或返回明确提示

### 好处

1. 先把重复创建 `ProjectManager` 的问题收住
2. 不影响现有 MCP tool 逻辑
3. 改动面相对可控

### 风险

1. MCP 依然会重复起 `serve` 进程
2. 项目状态此时仍分裂成 “daemon 管 CLI” 和 “serve 管 MCP”

所以这是止血阶段，不是终局。

---

## Phase B：MCP 改为 daemon 前端

### 目标

让 MCP 不再直接持有项目状态。

### 交付

新增命令：

- `quickdep mcp`

行为变化：

- `quickdep mcp` 只负责：
  - 接 stdio MCP 请求
  - 转发给 daemon
  - 把 daemon 响应包装成 MCP 响应

旧的 `quickdep serve` 可以：

1. 保留为兼容别名，内部等价于 `quickdep mcp`
2. 或在一段过渡期后废弃

### 好处

1. Codex / Claude / OpenCode 即使起多个 MCP 进程，也只是多个轻量代理
2. 真正的项目状态只在 daemon 内部存在一份
3. 彻底解决“同项目已经 loading 又来一个 loading”的多进程竞争

### 风险

1. MCP 前端需要处理 daemon 不可用场景
2. 需要设计好 MCP 代理层错误映射

---

## Phase C：统一 HTTP / Web / Watcher 与运维能力

### 目标

把所有入口和后台管理都收敛到 daemon。

### 交付

1. `quickdep --http` 改成 daemon 模式下暴露 HTTP
2. Web UI 默认连 daemon HTTP
3. watcher 生命周期完全由 daemon 统一管理
4. 增加 daemon 运维能力：
   - `list projects`
   - `pause/resume watcher`
   - `rebuild project`
   - `show daemon diagnostics`
   - `show active sessions`

### 好处

1. QuickDep 真正成为本地代码索引服务
2. Web / CLI / MCP / HTTP 数据面完全一致
3. 后续再加性能监控和调度策略会简单很多

### 风险

1. 需要补更完整的 daemon 状态可观测性
2. 需要明确升级/重启时对在途请求的影响

---

## 9. 运行时结构设计

建议新增：

- `src/runtime/mod.rs`
- `src/daemon/mod.rs`
- `src/daemon/ipc.rs`
- `src/daemon/lock.rs`

### 9.1 `QuickDepRuntime`

建议职责：

- 持有 `ProjectManager`
- 统一处理项目注册、加载、扫描、查询
- 提供“纯业务”异步方法

例如：

- `register_project`
- `scan_project`
- `get_project_status`
- `find_interfaces`
- `get_dependencies`
- `get_task_context`

这样：

- CLI、MCP、HTTP 都只依赖 runtime 抽象
- 进程边界之外的协议层不直接碰 `ProjectManager`

### 9.2 `DaemonServer`

建议职责：

- 维护全局锁
- 打开本地 socket
- 反序列化请求
- 调用 `QuickDepRuntime`
- 返回 JSON

### 9.3 前端适配层

- CLI adapter：本地命令 -> daemon request
- MCP adapter：MCP tool -> daemon request
- HTTP adapter：REST endpoint -> daemon request

---

## 10. 项目状态机

当前已有：

- `NotLoaded`
- `Loading`
- `Loaded`
- `WatchPaused`
- `Failed`

daemon 化后建议补充运行时语义：

- `NotRegistered`
- `Registered`
- `Loading`
- `LoadedWatching`
- `LoadedPaused`
- `Rebuilding`
- `Failed`

映射上不一定需要外露全部状态，但 daemon 内部最好明确区分：

1. 项目是否已注册
2. 项目是否已建库
3. watcher 是否运行
4. 是否有 inflight scan / rebuild

---

## 11. 错误与恢复策略

### 11.1 daemon 不存在

前端收到：

- `DaemonUnavailable`

行为：

- CLI 可选自动拉起
- MCP 不自动拉起，返回明确错误更稳

### 11.2 daemon 锁脏掉

行为：

1. 检查 pid
2. 若不存在则回收锁
3. 重建 daemon

### 11.3 项目加载超时

行为：

1. 不让第二个请求再起新的加载流程
2. 返回当前 inflight 状态
3. 提供取消或强制重建入口

### 11.4 watcher 崩溃

行为：

1. watcher 状态降级
2. 项目仍保持可查询
3. daemon 记录诊断信息
4. 支持手动恢复 watcher

---

## 12. 向后兼容

兼容策略建议：

1. 保留 `quickdep serve` 一段时间
2. 内部实现逐步切换到 daemon 代理
3. `install-mcp` 暂时不改命令名，仍注册 `quickdep serve`
4. 等 `serve` 内部已经变成轻代理后，再考虑是否切到 `quickdep mcp`

这样可以避免一次性打断 Claude / Codex / OpenCode 的接入方式。

---

## 13. 为什么不直接用 HTTP 代替 daemon IPC

本地 `localhost:port` 当然也能跑，但本期不建议把它当 daemon 的唯一内部通道。

原因：

1. 端口占用冲突多
2. 用户容易把 daemon 当成对外服务
3. 本地进程协调用 UDS / named pipe 更自然
4. 以后仍然可以让 daemon 选择性暴露 HTTP，而不是把 HTTP 当内部强依赖

HTTP 更适合作为：

- Web UI
- 第三方工具
- 调试入口

而不是后台服务的唯一骨干协议。

---

## 14. 第一阶段落地建议

建议第一期只做以下内容：

1. 引入 `QuickDepRuntime`
2. 引入 `quickdep daemon`
3. 增加全局单实例锁
4. 用本地 socket 跑 `scan/status/debug`
5. 保持现有 MCP 与 HTTP 行为不变

这样做的原因：

1. 改动面最小
2. 可以先验证 daemon 生命周期是否稳定
3. 可以先验证项目状态机是否收敛
4. 不会一次性把 MCP / HTTP / Web 全部牵进来

---

## 15. 成功标准

Phase A 成功标准：

1. 同一用户下只能启动一个 daemon
2. `scan/status/debug` 不再各自 new `ProjectManager`
3. 同一项目不会在 CLI 并发下重复 loading

Phase B 成功标准：

1. Codex/Claude/OpenCode 多开时不再产生多个重状态实例
2. MCP 查询不再重复创建 watcher
3. `already loading` / `timed out waiting` 大幅下降

Phase C 成功标准：

1. Web / CLI / MCP 看到同一份项目状态
2. watcher 生命周期只存在于 daemon
3. QuickDep 成为稳定的本地索引服务

---

## 16. 当前结论

QuickDep daemon 改造不是“另起炉灶的新架构”，而是：

- 把已经存在的 `ProjectManager + Scanner + Watcher + Storage + Cache`
- 从短命 `serve` 进程里提取出来
- 放进一个单实例、可复用、可观测的后台服务

最重要的变化不是功能列表，而是状态归属：

- 当前：状态属于每个 `serve` 进程
- 目标：状态属于唯一的 daemon

这一步如果不做，QuickDep 在大项目、长会话、多 agent 下会持续被多进程和状态竞争拖住。
