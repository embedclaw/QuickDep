# QuickDep 参考项目对比分析

## 概述

本文档对比分析 7 个相关开源项目，为 QuickDep 设计提供参考借鉴。

---

## 项目对比总览

| 项目 | 语言 | 存储方式 | 解析方式 | MCP 支持 | 核心定位 |
|------|------|----------|----------|----------|----------|
| **QuickDep** | Rust | SQLite | Tree-sitter | ✅ 主要 | Agent 依赖查询工具 |
| potpie | Python | Neo4j + PostgreSQL | Tree-sitter | ✅ | AI Agent + 知识图谱平台 |
| autodocs | Python + TypeScript | PostgreSQL + SQLite | Tree-sitter + SCIP | ✅ | 自动文档生成 |
| code-review-graph | Python | SQLite + NetworkX | Tree-sitter | ✅ | Code Review 知识图谱 |
| prometheus | Python | Neo4j + PostgreSQL | Tree-sitter | ✅ | GitHub Issue 自动处理 |
| understand-anything | TypeScript | JSON 文件 | Tree-sitter (WASM) | ❌ (Plugin) | 代码库理解可视化 |
| wallfacer | Go | 文件系统 | 外部 Agent | ❌ | 任务编排平台 |
| graph4code | Python | Neo4j | Tree-sitter | ❓ | 代码知识图谱 |

---

## 详细对比分析

### 1. potpie

**项目地址**: https://github.com/potpie-ai/potpie

**核心特点**:
- 知识图谱平台，将代码库转换为 Neo4j 图结构
- 支持多种 AI Agent（debug、qna、codegen 等）
- 40+ 工具系统（KG查询、文件操作、Git、Jira/Linear集成）

**可借鉴点**:

| 维度 | potpie 实现 | QuickDep 可借鉴 |
|------|-------------|-----------------|
| **图存储** | Neo4j 属性图 | SQLite 也可实现邻接表，更轻量 |
| **节点类型** | FILE, CLASS, FUNCTION, INTERFACE | 可采用相同分类 |
| **关系类型** | CONTAINS, REFERENCES, CALLS | 依赖关系设计参考 |
| **tree-sitter queries** | `.scm` 文件定义提取规则 | 采用相同方式提取符号 |
| **向量嵌入** | SentenceTransformers + Qdrant | 暂不实现，后续扩展 |
| **增量更新** | Celery 异步任务 | 可简化为后台线程 |
| **MCP 工具** | 40+ 工具注册 | 11 个工具精简设计 |

**差异点**:
- potpie 是完整平台，QuickDep 是轻量 MCP 服务
- potpie 用 Neo4j，QuickDep 用 SQLite 更简单
- potpie 有完整 Web UI，QuickDep 暂不做前端

---

### 2. autodocs

**项目地址**: https://github.com/TrySita/AutoDocs

**核心特点**:
- 自动文档生成 + AI 摘要
- 混合解析：Tree-sitter + SCIP（跨文件引用）
- 每仓库独立 SQLite 数据库
- 按依赖顺序生成文档（DAG 拓扑）

**可借鉴点**:

| 维度 | autodocs 实现 | QuickDep 可借鉴 |
|------|---------------|-----------------|
| **混合解析** | Tree-sitter + SCIP | 可考虑 SCIP 作为可选增强 |
| **每仓库独立 DB** | `{repo_slug}.db` | `.quickdep/symbols.db` 相同设计 |
| **依赖图构建** | NetworkX DAG | SQLite 递归 CTE 替代 |
| **拓扑顺序** | 用于 AI 摘要生成顺序 | 可用于预热扫描顺序 |
| **增量解析** | Git diff + commit hash | 已采用 hash 加速策略 |
| **FTS5 搜索** | 全文搜索虚拟表 | 可添加 FTS5 支持 |

**差异点**:
- autodocs 有完整前后端分离架构
- autodocs 依赖 PostgreSQL 作为主数据库
- autodocs 有 AI 摘要生成，QuickDep 暂不需要

---

### 3. code-review-graph

**项目地址**: https://github.com/tirth8205/code-review-graph

**核心特点**:
- **与 QuickDep 最相似**：专为 Agent 设计的代码图谱 MCP 服务
- 28 个 MCP 工具 + 5 个 MCP Prompts
- SQLite + WAL 存储（轻量）
- Blast-radius 影响分析（BFS遍历）
- 执行流追踪 + 关键性评分
- 23+ 语言支持

**高度可借鉴点**:

| 维度 | code-review-graph 实现 | QuickDep 可借鉴 |
|------|------------------------|-----------------|
| **存储** | SQLite + WAL，本地 `.code-review-graph/graph.db` | 完全相同设计 |
| **节点标识** | Qualified Name: `file.py::ClassName.method` | 可采用相同格式 |
| **边类型** | CALLS, IMPORTS_FROM, INHERITS, IMPLEMENTS, CONTAINS, TESTED_BY | 扩展依赖类型参考 |
| **影响分析** | BFS + SQLite 递归 CTE | get_dependencies 实现 |
| **工具设计** | 28 工具分类：build/review/query/flow/community/refactor | 工具分类参考 |
| **工具过滤** | `--tools` 参数限制暴露工具集 | Token 优化策略参考 |
| **MCP Prompts** | 5 个预定义模板 | 可考虑添加 |
| **语言支持** | tree-sitter-language-pack (23+) | 使用相同依赖 |
| **增量更新** | git diff + SHA-256 hash | 已采用类似策略 |
| **社团检测** | Leiden 算法分组 | 可作为可选功能 |

**核心差异**:
- code-review-graph 有更多分析功能（执行流、社团检测、重构）
- QuickDep 更聚焦依赖查询，更轻量

---

### 4. prometheus

**项目地址**: https://github.com/EuniAI/Prometheus

**核心特点**:
- GitHub Issue 自动处理平台
- LangGraph 多 Agent 协作
- Docker 容器隔离执行构建/测试
- Knowledge Graph 存储到 Neo4j

**可借鉴点**:

| 维度 | prometheus 实现 | QuickDep 可借鉴 |
|------|-----------------|-----------------|
| **AST 存储** | Neo4j 存储 AST 层级结构 | SQLite 邻接表替代 |
| **上下文检索** | Memory-first + KG遍历 | 查询缓存策略参考 |
| **LangGraph** | 状态机编排 Agent | 暂不需要，QuickDep 是纯工具 |

**差异点**:
- prometheus 是完整 Issue 处理系统
- 使用 Neo4j + PostgreSQL，复杂度高
- 有 Docker 容器执行环境

---

### 5. understand-anything

**项目地址**: https://github.com/Lum1104/Understand-Anything

**核心特点**:
- 代码库理解 + 可视化 Dashboard
- Multi-Agent Pipeline（5-6 个 Agent）
- Tree-sitter WASM (浏览器端解析)
- JSON 文件存储知识图谱
- 21 种节点类型 + 35 种边类型

**可借鉴点**:

| 维度 | understand-anything 实现 | QuickDep 可借鉴 |
|------|--------------------------|-----------------|
| **节点/边类型丰富** | 21 节点 + 35 边 | 可扩展类型定义 |
| **文件存储** | `.understand-anything/knowledge-graph.json` | SQLite 更高效 |
| **增量更新** | fingerprints.json + staleness 检测 | Hash 策略类似 |
| **模糊搜索** | Fuse.js | 可添加类似搜索 |
| **框架检测** | manifest 文件关键词匹配 | 语言/框架识别参考 |

**差异点**:
- TypeScript 实现，浏览器端 WASM 解析
- Plugin 架构而非 MCP 服务
- 有完整可视化 Dashboard

---

### 6. wallfacer

**项目地址**: https://github.com/changkun/wallfacer

**核心特点**:
- Go 实现的任务编排平台
- 文件系统优先存储（无数据库）
- REST + SSE API（无 MCP）
- Git worktree 隔离执行

**可借鉴点**:

| 维度 | wallfacer 实现 | QuickDep 可借鉴 |
|------|-----------------|-----------------|
| **文件系统存储** | task.json + traces/ | SQLite 更适合结构化查询 |
| **事件溯源** | TaskEvent 记录 | 可用于扫描进度追踪 |
| **并发模型** | RWMutex + per-repo mutex | Rust 的并发策略参考 |

**差异点**:
- 不解析代码结构，依赖外部 Agent
- 无 MCP 支持
- Go 语言实现

---

### 7. graph4code

**项目地址**: https://github.com/wala/graph4code

**信息来源**: 项目 README + 学术论文

**核心特点**:
- 学术项目，代码属性图（CPG）研究
- Neo4j 存储大规模代码图谱
- Tree-sitter 解析多语言
- 跨仓库调用关系分析

**可借鉴点**:
- 代码属性图（CPG）理论模型
- 大规模图谱性能优化经验
- 跨仓库依赖分析（未来扩展）

---

## QuickDep 设计决策对照

### 决策合理性验证

| QuickDep 决策 | 参考项目验证 | 结论 |
|---------------|--------------|------|
| Tree-sitter 解析 | 6/7 项目使用 | ✅ 正确选择 |
| SQLite 存储 | code-review-graph、autodocs 使用 | ✅ 轻量可行 |
| WAL 模式 | code-review-graph 使用 | ✅ 并发友好 |
| 本地 `.quickdep/` 目录 | autodocs、understand-anything、code-review-graph 类似 | ✅ 项目隔离 |
| MCP 协议 | potpie、autodocs、code-review-graph、prometheus 支持 | ✅ 主流标准 |
| Hash 增量更新 | autodocs、code-review-graph、understand-anything 使用 | ✅ 正确策略 |
| Qualified Name ID | code-review-graph 使用 | ✅ 可读性强 |

### 可能的扩展借鉴

| 功能 | 来源 | 优先级 | 说明 |
|------|------|--------|------|
| MCP Prompts | code-review-graph | P3 | 预定义查询模板 |
| FTS5 全文搜索 | autodocs、code-review-graph | P2 | 加速符号名搜索 |
| SCIP 跨文件解析 | autodocs | P3 | 可选增强，更精确 |
| 执行流追踪 | code-review-graph | P3 | 入口点检测 + 关键性评分 |
| 社团检测 | code-review-graph | P4 | Leiden 算法分组 |
| 向量嵌入/语义搜索 | potpie、autodocs | P4 | 后续扩展 |
| 工具过滤机制 | code-review-graph | P2 | Token 优化策略 |

---

## code-review-graph 详细借鉴

**最相似的参考项目**，值得深入研究：

### MCP 工具分类对比

| code-review-graph 工具 | QuickDep 对应 |
|------------------------|---------------|
| `build_or_update_graph_tool` | `scan_project` |
| `detect_changes_tool` | Hash 验证内部逻辑 |
| `get_review_context_tool` | - (暂不需要) |
| `get_impact_radius_tool` | `get_dependencies(direction=Incoming)` |
| `query_graph_tool` | `find_interfaces` |
| `semantic_search_nodes_tool` | - (暂不需要) |
| `traverse_graph_tool` | `get_call_chain` |
| `list_flows_tool` | - (暂不需要) |
| `get_hub_nodes_tool` | - (暂不需要) |
| `refactor_tool` | - (暂不需要) |
| `list_repos_tool` | `list_projects` |

### 数据模型对比

```python
# code-review-graph nodes 表
CREATE TABLE nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,           -- File/Class/Function/Type/Test
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL, -- /path/to/file.py::ClassName.method
    file_path TEXT NOT NULL,
    line_start INTEGER,
    line_end INTEGER,
    language TEXT,
    community_id INTEGER,         -- 社团检测结果
    signature TEXT                -- 函数签名
);

# QuickDep symbols 表设计（参考后）
CREATE TABLE symbols (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,           -- Function/Method/Class/Struct...
    file_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    visibility TEXT,
    signature TEXT,
    source TEXT DEFAULT 'local'   -- Local/External/Builtin
);
```

### Qualified Name 格式

```
code-review-graph:
/absolute/path/to/file.py                      # File
/absolute/path/to/file.py::function_name      # Function
/absolute/path/to/file.py::ClassName.method   # Method

QuickDep (可采用相对路径):
src/utils.rs::helper                           # Function
src/utils.rs::Utils::process                   # Method
src/models.rs::User                            # Struct
```

---

## 技术栈对比总结

### Tree-sitter 使用方式

| 项目 | Tree-sitter 集成 | 语言支持 |
|------|------------------|----------|
| potpie | tree-sitter + tree-sitter-language-pack | 15+ |
| autodocs | tree-sitter + tree-sitter-language-pack | 3 (TS/JS/Python) |
| code-review-graph | tree-sitter-language-pack | 23+ |
| prometheus | tree-sitter + tree-sitter-language-pack | 18+ |
| understand-anything | web-tree-sitter (WASM) | 10 |
| QuickDep | tree-sitter-rust/typescript/python/go crate | 4 (初期) |

**结论**: 使用 `tree-sitter-language-pack` 可支持更多语言，但单独 crate 更可控。

### 存储方式选择

| 存储方案 | 项目使用 | 适用场景 |
|----------|----------|----------|
| Neo4j | potpie, prometheus, graph4code | 大规模图谱、复杂图查询 |
| SQLite | code-review-graph, autodocs | 轻量、单文件、无外部依赖 |
| JSON 文件 | understand-anything | 简单、浏览器可读 |
| PostgreSQL | potpie, autodocs, prometheus | 用户数据、关系数据 |

**QuickDep 选择**: SQLite + WAL，与 code-review-graph 一致，验证合理。

---

## 实现建议

### Phase 1 MVP 借鉴优先级

1. **code-review-graph**：核心架构、MCP 工具设计、SQLite schema
2. **autodocs**：增量更新策略、SCIP 参考
3. **potpie**：tree-sitter queries 文件格式

### 代码直接参考

| 功能 | 参考文件 | 项目 |
|------|----------|------|
| Tree-sitter 解析 | `parser.py` | code-review-graph |
| SQLite 图存储 | `graph.py` | code-review-graph |
| MCP 工具注册 | `main.py` | code-review-graph |
| 增量更新 | `incremental.py` | code-review-graph |
| Tree-sitter queries | `queries/*.scm` | potpie |
| 语言配置 | `languages/configs/` | understand-anything |

---

## 风险与局限

### 已发现问题

| 项目 | 问题 | QuickDep 避免 |
|------|------|---------------|
| potpie | Neo4j 部署复杂 | SQLite 轻量部署 |
| prometheus | 多数据库依赖 | 单一 SQLite |
| understand-anything | WASM 性能限制 | Rust native |
| wallfacer | 无代码解析 | Tree-sitter 内置 |

### 需要注意

| 问题 | 解决方案 |
|------|----------|
| glob import 无法分析 | 标记为 Glob，不建立依赖 |
| 宏展开丢失 | 标记为 Macro，不深入 |
| 动态 import | 标记为 Dynamic，不分析 |
| 大项目性能 | 内存缓存 + 分页查询 |

---

## 总结

**最佳参考项目**: code-review-graph

**理由**:
- 与 QuickDep 目标高度一致（Agent 依赖查询）
- SQLite + WAL 轻量存储
- 完整 MCP 工具设计
- 23+ 语言支持
- 增量更新成熟
- 开源可借鉴

**核心借鉴清单**:
1. SQLite schema 设计
2. Qualified Name 格式
3. MCP 工具分类
4. 工具过滤机制
5. Hash 增量更新
6. Tree-sitter 解析流程
7. 递归 CTE 图查询