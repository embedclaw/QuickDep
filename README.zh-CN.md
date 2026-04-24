<p align="right">
  <a href="./README.md">English</a>
</p>

# QuickDep

> 让 Agent 在大仓库里先缩小范围，再读源码。

![许可证: MIT](https://img.shields.io/badge/License-MIT-0f766e.svg)
![Rust 1.75+](https://img.shields.io/badge/Rust-1.75%2B-d97706.svg)
![协议](https://img.shields.io/badge/MCP-stdio%20%7C%20HTTP-2563eb.svg)
![能力面](https://img.shields.io/badge/Surface-CLI%20%7C%20API%20%7C%20Web-111827.svg)

QuickDep 会把一个代码仓库预处理成可查询的符号和依赖图，让人和 Agent 都能直接消费。
它解决的不是“搜到几段文本”，而是“先把搜索空间缩小，再拿到结构化关系”：

- 谁在调用这个函数？
- 改这个接口会影响哪些地方？
- 两个符号之间最短调用链是什么？
- 一个文件里到底声明了哪些关键接口？

它完全本地运行，把图数据存进 SQLite，支持增量更新，并通过 MCP、HTTP、WebSocket 和本地 Web UI 暴露出来。
QuickDep 采用 MIT 开源协议，定位就是一个能直接放进真实开发流程的工程工具，让 Agent 在开始深入阅读源码之前，先找到更可能有问题的那几处代码。

## QuickDep 比 grep 更擅长回答的问题

**“如果我改了 `helper()`，到底会影响谁？”**

`grep` 能告诉你 `helper` 在哪里出现过。
QuickDep 能告诉你谁**调用**它、谁**依赖**它，以及这条**影响链**会怎样沿着仓库继续扩散。

```text
get_dependencies("helper", direction="incoming")
```

这就是它最核心的价值：不是更多文本命中，而是更好的“第一批怀疑对象”。

## 项目价值

现在很多 Agent 处理代码时，本质上还是在做“高级版全文检索”。
真正的问题不只是 token，而是它们往往要先读错很多文件，才能慢慢靠近真正相关的代码区域。

这会带来几个很实际的问题：

- 上下文太碎，跨文件关系难以稳定恢复
- token 浪费在盲搜和重复读取上
- 影响分析常常靠猜，不够可验证

QuickDep 的价值就是先把仓库变成一层结构化收敛层，再让 Agent 去问：

- 重构前先看依赖影响面
- 修改接口前先看入边和出边
- 跨模块定位调用链
- 用统一的本地图谱服务支撑 CLI、脚本、Web 和 Agent

如果你希望 LLM 先回答“我应该先看哪几处代码”，而不是在一大堆 grep 结果里盲猜，QuickDep 就是中间那一层。

## 在真实项目上，它现在表现如何

我们在 `ark-runtime` 上做了共同场景 `S1-S5` 的对比实验，分别测试三条路线：

- 原生 Agent 自带工具
- 只用 QuickDep
- QuickDep + 原生工具混合使用

核心数据如下：

| 路线 | 平均得分 | 平均耗时 ms | 平均上下文 tokens | 平均涉及文件数 | 平均源码读取 chars | 平均 MCP 返回 chars |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 原生 Agent 自带工具 | `3.2` | `70,365` | `278,350` | `35.8` | `42,056` | `0` |
| 只用 QuickDep | `3.2` | `89,825` | `379,111` | `5.0` | `8,811` | `24,700` |
| QuickDep + 原生工具 | `3.2` | `72,045` | `272,461` | `7.6` | `22,748` | `7,781` |

这张表最值得看的不是“谁绝对更快”，而是 Agent 在多大范围里乱翻文件：

- 混合路线把平均涉及文件数从 `35.8` 压到 `7.6`
- 混合路线把平均源码读取量从 `42,056 chars` 压到 `22,748 chars`
- 在分数基本不退化的前提下，Agent 更少跑偏到无关区域

这也是 QuickDep 当前最可信的价值：

> 它不是替代源码阅读，而是让 Agent 更快找到值得读的那几处代码。

完整实验记录见：

- [docs/EXPERIMENTS.md](docs/EXPERIMENTS.md)
- [docs/AGENT_HYBRID_BENCHMARK_REPORT.md](docs/AGENT_HYBRID_BENCHMARK_REPORT.md)

## 适用场景

- Claude Code、Codex、OpenCode 这类 Agent 驱动开发
- 中大型本地仓库，全文搜索噪音已经很高
- 重构、迁移、依赖梳理、影响分析
- 想基于 MCP 或 HTTP 自己做代码智能工作流

## 当前支持语言

QuickDep 当前已经接入到本地图谱流水线的语言如下：

| 语言 | 典型扩展名 |
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

## 当前最推荐的使用方式

现阶段最推荐的是 **Hybrid 工作流**：

1. 先用 QuickDep 把范围缩到正确的 3 到 10 个文件 / 符号 / 调用链
2. 再让 Agent 去读少量关键实现
3. 把原生搜索和源码阅读留给真正需要行为细节确认的地方

这比把 QuickDep 描述成“替代所有源码阅读”的工具更真实，也更符合当前版本的实际效果。

## QuickDep 和常见方案的区别

| 工具 | MCP 原生 | 本地优先 | 图遍历 | 影响分析查询 |
| --- | --- | --- | --- | --- |
| `grep` / `rg` | 否 | 是 | 否 | 否 |
| LSP 的 find references | 否 | 是 | 弱 | 弱 |
| Sourcegraph 一类代码智能工具 | 否 | 混合 | 部分支持 | 部分支持 |
| **QuickDep** | **是** | **是** | **是** | **是** |

QuickDep 的定位不是替代所有代码工具，而是给本地 MCP Agent 补上一层依赖图和调用链收敛能力。

## 安装与接入

截至 `2026-04-24`，当前实测有效的安装路径是“源码安装 + `install-mcp` 接入客户端”。公开分发渠道已经准备好，但还没有真正发布上线。

| 方式 | 当前状态 | 我们的验证结果 |
| --- | --- | --- |
| `cargo install --path .` | 已可用 | 已实测安装成功，`quickdep --version` 返回 `0.1.0` |
| `quickdep install-mcp claude` | 已可用 | 已实测写入成功，`claude mcp list` 显示已连接 |
| `quickdep install-mcp codex` | 已可用 | 已实测写入成功，`codex mcp list` 可见 |
| `quickdep install-mcp opencode` | 已可用 | 已实测写入成功，`opencode mcp list` 显示已连接 |
| GitHub Release | 未发布 | `releases/latest/download/...` 当前返回 `404` |
| Homebrew | 未发布 | `Formula/quickdep.rb` 当前返回 `404` |
| npm | 未发布 | `npm view @northcipher/quickdep` 当前返回 `E404` |

今天想真正装起来，直接用：

```bash
cargo install --path .
quickdep --version
```

然后一条命令接进 Agent：

```bash
quickdep install-mcp claude
quickdep install-mcp codex
quickdep install-mcp opencode
```

如果你想把安装动作直接交给 Claude Code / Codex / OpenCode，可以把这份提示词原样贴给它：

- [docs/AGENT_INSTALL_PROMPT.md](docs/AGENT_INSTALL_PROMPT.md)

先验证本地服务正常：

```bash
# 终端 1
quickdep --http 8080 --http-only

# 终端 2
curl http://127.0.0.1:8080/health
# {"status":"ok"}
```

更多分发和集成说明见：

- [docs/INTEGRATIONS.md](docs/INTEGRATIONS.md)

## 30 秒启动与验证

QuickDep 默认子命令就是 `serve`，所以直接运行即可启动本地 `stdio MCP`：

```bash
# 在当前目录启动本地 stdio MCP
quickdep

# 同时暴露 MCP stdio 和 HTTP
quickdep --http 8080

# 仅启用 HTTP
quickdep --http 8080 --http-only
```

如果你想在接入 MCP 前先做一个最快速的活性检查：

```bash
# 先在另一个终端启动带 HTTP 的 QuickDep，再执行这里
curl http://127.0.0.1:8080/health
# {"status":"ok"}
```

可选的本地 Web 控制台：

```bash
cd web
npm install
npm run dev
```

HTTP 服务会提供：

- `/mcp`：streamable MCP
- `/api`：REST API
- `/ws/projects`：项目状态推送
- `/health`：健康检查

## QuickDep 能回答什么

| 你想知道什么 | 对应能力 |
| --- | --- |
| 谁在调用 `helper()`？ | `get_dependencies` 的 `incoming` |
| 这个符号依赖了什么？ | `get_dependencies` 的 `outgoing` |
| `entry` 和 `helper` 怎么连起来？ | `get_call_chain` |
| 一个文件里有哪些接口？ | `get_file_interfaces` |
| 能不能直接图形化查看？ | [`web/`](web) 本地 Web UI |

## 当前已经交付的内容

- Rust、TypeScript/JavaScript、Java、C#、Kotlin、PHP、Ruby、Swift、Objective-C、Python、Go、C、C++ 的 Tree-sitter 解析
- 基于 SQLite 的图存储，开启 WAL，支持 FTS5 符号搜索
- 增量扫描、文件监控、防抖、暂停/恢复
- MCP 服务，提供项目、符号、依赖、调用链等工具
- HTTP API 和 WebSocket 状态流
- 本地 Web UI，可查看项目状态、搜索、依赖图、表格和批量查询
- Claude Code、Codex、OpenCode 的一键 `install-mcp`
- `--tools` 级别的工具裁剪能力

## 吸引人的点

- 本地优先：代码不出机器
- 面向 Agent：不是后补的接口层，而是从 MCP 使用场景反推设计
- 上手快：装好二进制，执行 `install-mcp`，马上就能用
- 入口完整：CLI、HTTP、Web UI 都有
- MIT 开源：可直接集成、二开、商用友好

## CLI 概览

```bash
quickdep [OPTIONS] [COMMAND]
```

核心命令：

- `serve`
- `scan <path>`
- `status <path>`
- `debug <path> --stats`
- `debug <path> --deps <interface>`
- `debug <path> --file <relative-path>`
- `install-mcp <claude|codex|opencode>`

常用服务参数：

- `--http <port>`
- `--http-only`
- `--tools <tool1,tool2,...>`
- `--log-level <trace|debug|info|warn|error>`

## 文档入口

- [docs/USAGE.md](docs/USAGE.md)
- [docs/API.md](docs/API.md)
- [docs/INTEGRATIONS.md](docs/INTEGRATIONS.md)
- [docs/QUICKDEP_PLAIN_LANGUAGE_GUIDE.md](docs/QUICKDEP_PLAIN_LANGUAGE_GUIDE.md)
- [docs/TEST_REPORT.md](docs/TEST_REPORT.md)
- [web/README.md](web/README.md)
- [CHANGELOG.md](CHANGELOG.md)

## 开发验证

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## License

MIT。拿去用，拿去改，拿去集成。
