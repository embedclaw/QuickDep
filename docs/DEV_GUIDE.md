# QuickDep 开发准备清单

## 一、可直接沿用功能

### 从 code-review-graph 沿用

| 功能 | 沿用程度 | 实现方式 |
|------|----------|----------|
| **SQLite Schema** | 高 | 直接采用 nodes + edges 表结构，微调字段 |
| **Qualified Name 格式** | 高 | `file_path::SymbolName` 格式 |
| **Tree-sitter 解析流程** | 高 | AST 遍历 + node type 匹配 |
| **递归 CTE 图查询** | 高 | SQLite 递归查询依赖关系 |
| **WAL 模式配置** | 高 | `PRAGMA journal_mode = WAL` |
| **MCP Server 架构** | 高 | FastMCP stdio 模式 |
| **Hash 增量更新** | 高 | SHA-256/blake3 + git diff |
| **工具过滤机制** | 中 | `--tools` 参数控制暴露工具 |

### 从 autodocs 沿用

| 功能 | 沿用程度 | 实现方式 |
|------|----------|----------|
| **每仓库独立 SQLite** | 高 | `.quickdep/{project_id}.db` |
| **FTS5 全文搜索** | 中 | 添加 symbols_fts 虚拟表 |
| **拓扑扫描顺序** | 中 | 依赖 DAG 确定预热顺序 |
| **Git diff 增量** | 高 | commit hash 对比 |

### 从 potpie 沿用

| 功能 | 沿用程度 | 实现方式 |
|------|----------|----------|
| **Tree-sitter queries (.scm)** | 高 | 定义每种语言的提取规则 |
| **符号类型分类** | 高 | Function/Class/Method/Struct 等 |
| **边类型定义** | 高 | CALLS/IMPORTS/INHERITS 等 |

---

## 二、开发规范

### 代码规范

```
1. Rust 版本: 1.75+ (支持 async trait 等特性)
2. 使用 clippy 严格检查
3. 所有 public 函数必须有文档注释
4. 错误处理统一使用 thiserror
5. 异步使用 tokio runtime
6. 日志使用 tracing
```

### 模块组织规范

```
src/
├── core/           # 核心数据结构（无外部依赖）
├── parser/         # Tree-sitter 解析（可独立测试）
├── resolver/       # 跨文件解析（依赖 parser）
├── storage/        # SQLite 存储（可独立测试）
├── cache/          # 内存缓存
├── watcher/        # 文件监控
├── project/        # 项目管理
├── mcp/            # MCP 服务
├── http/           # HTTP API（可选）
├── config/         # 配置管理
├── security/       # 安全验证
└── cli/            # CLI 入口

原则：
- 每个模块可独立编译测试
- 依赖单向：上层依赖下层
- 避免循环依赖
```

### 测试规范

```
1. 单元测试：每个模块 _test.rs 或 #[cfg(test)]
2. 集成测试：tests/ 目录
3. Fixture 项目：tests/fixtures/sample_project/
4. 覆盖率目标：核心模块 80%+
5. CI：每次提交运行测试
```

### Git 规范

```
分支策略：
- main: 稳定版本
- develop: 开发分支
- feature/*: 功能分支

提交格式：
<type>(<scope>): <subject>

type: feat/fix/refactor/test/docs/chore
scope: parser/storage/mcp/watcher 等

示例：
feat(parser): add rust function extraction
fix(storage): handle cascade delete for symbols
test(mcp): add find_interfaces test cases
```

### 文档规范

```
docs/
├── FEATURES.md           # 功能清单
├── REFERENCE_ANALYSIS.md # 参考项目分析
├── DEV_GUIDE.md          # 开发指南（本文件）
├── AGENT_GUIDE.md        # Agent 执行指南
├── API.md                # MCP/HTTP API 文档
└── ARCHITECTURE.md       # 架构设计（后续补充）

代码文档：
- README.md: 项目介绍、安装、使用
- CLAUDE.md: Claude Code 项目配置（后续）
- CHANGELOG.md: 版本变更记录
```

---

## 三、任务清单

### Phase 0：项目初始化

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T0.1 | 创建 Cargo.toml + 项目结构 | - | 0.5h |
| T0.2 | 配置 clippy + 基础 lint | T0.1 | 0.5h |
| T0.3 | 设置 CI (GitHub Actions) | T0.1 | 1h |
| T0.4 | 创建测试 fixtures 项目 | T0.1 | 1h |

### Phase 1：核心数据结构

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T1.1 | Symbol/Dependency 数据结构 | T0.1 | 1h |
| T1.2 | SymbolKind 枚举定义 | T1.1 | 0.5h |
| T1.3 | DependencyKind 枚举定义 | T1.1 | 0.5h |
| T1.4 | Graph 结构（内存图） | T1.1 | 2h |

### Phase 2：Storage 模块

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T2.1 | SQLite 连接管理 | T1.1 | 1h |
| T2.2 | Schema 创建（symbols/dependencies/imports/file_state） | T2.1 | 2h |
| T2.3 | Symbols CRUD | T2.2 | 2h |
| T2.4 | Dependencies CRUD | T2.2 | 2h |
| T2.5 | Imports CRUD | T2.2 | 1h |
| T2.6 | FileState 管理 | T2.2 | 1h |
| T2.7 | 递归 CTE 图查询 | T2.4 | 3h |
| T2.8 | 批量插入优化（事务） | T2.3 | 1h |
| T2.9 | WAL 模式配置 | T2.1 | 0.5h |

### Phase 3：Parser 模块

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T3.1 | Parser trait 定义 | T1.1 | 1h |
| T3.2 | 语言检测（扩展名映射） | T3.1 | 0.5h |
| T3.3 | Rust Parser（tree-sitter-rust） | T3.1 | 4h |
| T3.4 | TypeScript Parser | T3.1 | 4h |
| T3.5 | Tree-sitter queries (.scm) | T3.3 | 2h |
| T3.6 | 错误容忍处理 | T3.3 | 1h |
| T3.7 | Parser 单元测试 | T3.3, T3.4 | 2h |

### Phase 4：Resolver 模块

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T4.1 | Import 解析逻辑 | T2.5, T3.3 | 3h |
| T4.2 | 模块路径解析（Rust mod） | T4.1 | 2h |
| T4.3 | 符号匹配（bare-name → qualified） | T4.1, T2.3 | 3h |
| T4.4 | Glob import 标记 | T4.1 | 0.5h |
| T4.5 | 别名映射处理 | T4.1 | 1h |
| T4.6 | Resolver 单元测试 | T4.1-T4.5 | 2h |

### Phase 5：Project Manager

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T5.1 | Project 结构定义 | T1.1 | 1h |
| T5.2 | ProjectId 生成（path hash） | T5.1 | 0.5h |
| T5.3 | 状态管理（NotLoaded/Loading/Loaded） | T5.1 | 1h |
| T5.4 | 懒加载触发逻辑 | T5.3 | 2h |
| T5.5 | Manifest 管理（JSON） | T5.1 | 1h |

### Phase 6：Watcher 模块

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T6.1 | notify 文件监控 | T5.1 | 2h |
| T6.2 | Hash 计算（blake3） | T6.1 | 1h |
| T6.3 | 事件防抖（500ms） | T6.1 | 1h |
| T6.4 | 增量更新逻辑 | T6.2, T2.6 | 3h |
| T6.5 | 批量变更处理 | T6.3 | 1h |
| T6.6 | 监控暂停/恢复 | T6.1, T5.4 | 1h |

### Phase 7：Cache 模块

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T7.1 | 符号名索引缓存 | T2.3 | 2h |
| T7.2 | 查询结果缓存 | T2.7 | 2h |
| T7.3 | 缓存失效策略 | T7.1, T6.4 | 1h |

### Phase 8：MCP Server

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T8.1 | MCP 协议实现（rmcp crate） | - | 3h |
| T8.2 | Server capabilities | T8.1 | 0.5h |
| T8.3 | initialize/initialized handler | T8.1 | 1h |
| T8.4 | MCP Tools 注册 | T8.1, T5.1 | 1h |

### Phase 9：MCP Tools

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T9.1 | list_projects | T5.1, T8.4 | 1h |
| T9.2 | scan_project | T5.1, T3.3, T2.2 | 2h |
| T9.3 | get_scan_status | T5.3 | 0.5h |
| T9.4 | cancel_scan | T5.3, T6.1 | 1h |
| T9.5 | find_interfaces | T2.3, T7.1 | 2h |
| T9.6 | get_interface | T2.3 | 1h |
| T9.7 | get_dependencies | T2.7, T7.2 | 3h |
| T9.8 | get_call_chain | T2.7 | 2h |
| T9.9 | get_file_interfaces | T2.3 | 1h |
| T9.10 | batch_query | T9.5-T9.9 | 2h |
| T9.11 | rebuild_database | T2.2 | 1h |

### Phase 10：MCP Resources

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T10.1 | Resources 注册 | T8.1 | 0.5h |
| T10.2 | quickdep://projects | T5.1 | 1h |
| T10.3 | quickdep://project/{id}/status | T5.3 | 1h |
| T10.4 | quickdep://project/{id}/interfaces | T2.3 | 1h |
| T10.5 | quickdep://project/{id}/interface/{id} | T2.3 | 1h |
| T10.6 | quickdep://project/{id}/interface/{id}/deps | T2.7 | 1h |

### Phase 11：Configuration

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T11.1 | Settings 结构定义 | - | 1h |
| T11.2 | TOML 配置加载 | T11.1 | 1h |
| T11.3 | 默认配置 | T11.1 | 0.5h |
| T11.4 | 配置验证 | T11.2 | 1h |

### Phase 12：Security

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T12.1 | 路径验证函数 | - | 1h |
| T12.2 | 路径遍历检测 | T12.1 | 1h |
| T12.3 | project_id 验证 | T5.2, T12.1 | 0.5h |

### Phase 13：CLI

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T13.1 | CLI 参数定义（clap） | - | 1h |
| T13.2 | MCP 模式启动 | T8.1, T13.1 | 1h |
| T13.3 | HTTP 模式启动 | T13.1 | 1h |
| T13.4 | debug 子命令 | T13.1, T2.2 | 2h |
| T13.5 | version 参数 | T13.1 | 0.5h |

### Phase 14：Logging

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T14.1 | tracing 初始化 | - | 1h |
| T14.2 | 文件日志（RollingFileAppender） | T14.1 | 1h |
| T14.3 | stderr 输出 | T14.1 | 0.5h |
| T14.4 | 日志级别控制 | T14.1, T11.1 | 0.5h |

### Phase 15：Testing

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T15.1 | Parser 测试 fixtures | T3.3 | 2h |
| T15.2 | Resolver 测试 | T4.6 | 1h |
| T15.3 | Storage 测试 | T2.9 | 2h |
| T15.4 | MCP 模拟客户端测试 | T9.11 | 3h |
| T15.5 | E2E 测试（完整扫描） | T15.1-T15.4 | 3h |

### Phase 16：文档完善

| ID | 任务 | 依赖 | 预估 |
|----|------|------|------|
| T16.1 | README.md 完善 | T13.5 | 1h |
| T16.2 | API.md 文档 | T9.11, T10.6 | 2h |
| T16.3 | 使用示例 | T16.1 | 1h |
| T16.4 | CHANGELOG.md | T16.1 | 0.5h |

---

## 四、依赖清单

### Cargo.toml 核心依赖

```toml
[dependencies]
# Tree-sitter
tree-sitter = "0.20"
tree-sitter-rust = "0.20"
tree-sitter-typescript = "0.20"
tree-sitter-python = "0.20"
tree-sitter-go = "0.20"

# 存储
rusqlite = { version = "0.31", features = ["bundled"] }

# 异步
tokio = { version = "1", features = ["full"] }

# MCP 协议
rmcp = "0.1"  # 或使用官方 mcp crate

# 文件监控
notify = "6"

# Hash
blake3 = "1"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# 错误处理
thiserror = "1"
anyhow = "1"

# 日志
tracing = "0.1"
tracing-subscriber = "0.3"
tracing-appender = "0.2"

# CLI
clap = { version = "4", features = ["derive"] }

# 路径处理
walkdir = "2"
glob-match = "0.1"

# 安全
path-clean = "0.1"

[dev-dependencies]
tempfile = "3"
pretty_assertions = "1"
```

---

## 五、里程碑规划

### M1：最小可用版本（Week 1-2）

**目标**: Rust Parser + SQLite 存储 + 基础 MCP

**包含任务**:
- T0.1-T0.4（项目初始化）
- T1.1-T1.4（数据结构）
- T2.1-T2.9（Storage）
- T3.1-T3.7（Rust Parser）
- T8.1-T8.4（MCP Server 基础）
- T9.1, T9.5-T9.7（基础 Tools）
- T13.1-T13.2（CLI MCP 模式）

**验收标准**:
- 可扫描 Rust 项目
- 存储符号和依赖到 SQLite
- MCP find_interfaces/get_dependencies 可调用

### M2：完整功能版本（Week 3-4）

**目标**: Resolver + Watcher + Cache + 完整 MCP Tools

**包含任务**:
- T4.1-T4.6（Resolver）
- T5.1-T5.5（Project Manager）
- T6.1-T6.6（Watcher）
- T7.1-T7.3（Cache）
- T9.2-T9.4, T9.8-T9.11（剩余 Tools）
- T10.1-T10.6（MCP Resources）
- T11.1-T11.4（Configuration）
- T12.1-T12.3（Security）
- T14.1-T14.4（Logging）

**验收标准**:
- 跨文件依赖解析正确
- 文件变更实时更新
- 所有 MCP Tools 可用
- 完整的懒加载和缓存

### M3：多语言支持（Week 5-6）

**目标**: TypeScript Parser + Python Parser + 测试完善

**包含任务**:
- T3.4（TypeScript Parser）
- T15.1-T15.5（完整测试）
- T16.1-T16.4（文档完善）

**验收标准**:
- 支持 Rust + TypeScript + Python
- 测试覆盖率 80%+
- 文档完整

### M4：可选功能（Week 7-8）

**目标**: HTTP API + Go Parser + 性能优化

**包含任务**:
- HTTP Server（预留接口）
- Go Parser
- FTS5 搜索
- 工具过滤机制

---

## 六、开发前检查清单

### 环境准备

- [ ] Rust 1.75+ 安装
- [x] cargo/clippy 配置
- [ ] Git 初始化
- [ ] IDE 配置（rust-analyzer）

### 文档准备

- [x] docs/FEATURES.md
- [x] docs/REFERENCE_ANALYSIS.md
- [x] docs/DEV_GUIDE.md（本文档）
- [x] docs/AGENT_GUIDE.md
- [x] README.md
- [x] Cargo.toml

### 测试准备

- [x] tests/fixtures/ 目录
- [x] sample Rust 项目
- [x] sample TypeScript 项目

### CI 准备

- [x] GitHub Actions workflow
- [x] 测试运行配置
- [x] Clippy 检查配置

---

## 七、启动命令

```bash
# 初始化项目
cd /Users/luozx/work/quickdep

# 创建 Cargo.toml（下一步执行）
cargo init --name quickdep

# 添加依赖
cargo add tree-sitter tree-sitter-rust rusqlite tokio ...

# 运行测试
cargo test

# MCP 模式运行
cargo run

# Debug 模式
cargo run -- debug ./path/to/project --stats
```
