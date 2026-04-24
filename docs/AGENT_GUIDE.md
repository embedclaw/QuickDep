# QuickDep Agent 开发指南

## 概述

本文档为执行开发任务的 Agent 提供指导，说明如何参考项目文档进行功能开发。

---

## 文档索引

| 文档 | 用途 | 使用时机 |
|------|------|----------|
| `docs/FEATURES.md` | 功能清单、数据模型、架构设计 | 开发任何功能前必读 |
| `docs/REFERENCE_ANALYSIS.md` | 参考项目对比、借鉴点 | 实现具体功能时参考 |
| `docs/DEV_GUIDE.md` | 任务清单、开发规范、里程碑 | 执行具体任务时参考 |
| `docs/AGENT_GUIDE.md` | 本文档，Agent 执行指南 | 每次 Agent 任务开始时必读 |
| `docs/REFERENCE_ANALYSIS.md` | 参考项目对比、借鉴点 | 需要外部实现思路时阅读 |

---

## 开发流程

### Step 1：理解任务

```
1. 从 DEV_GUIDE.md 找到任务 ID（如 T2.1）
2. 确认任务描述、依赖关系、预估时间
3. 检查依赖任务是否已完成
```

### Step 2：阅读相关文档

```
必读顺序：
1. FEATURES.md → 找到对应功能模块（如 M4 Storage）
   - 理解模块定位和依赖关系
   - 查看子功能列表
   
2. FEATURES.md → 数据模型部分
   - 确认数据结构定义
   - 查看 SQLite Schema
   
3. REFERENCE_ANALYSIS.md → 找到借鉴点
   - code-review-graph 是最佳参考
   - 查看具体实现建议
```

### Step 3：参考外部项目实现

```
优先参考顺序：
1. code-review-graph → 最相似项目
2. autodocs → 增量更新参考
3. potpie → tree-sitter queries 参考

注意：
- 需要时去上游仓库查看实现，不要把第三方仓库源码提交到本仓库
- 本仓库保留分析文档，不再 vendored 外部项目代码

关键文件对照表：
| 功能 | 参考文件 | 项目 |
|------|----------|------|
| Tree-sitter 解析 | code_review_graph/parser.py | code-review-graph |
| SQLite 图存储 | code_review_graph/graph.py | code-review-graph |
| MCP 工具注册 | code_review_graph/main.py | code-review-graph |
| 增量更新 | code_review_graph/incremental.py | code-review-graph |
| Tree-sitter queries | queries/*.scm | potpie |
```

### Step 4：实现代码

```
遵循规范（见 DEV_GUIDE.md 第二节）：
1. Rust 版本 1.75+
2. clippy 严格检查
3. public 函数必须有文档注释
4. 错误处理用 thiserror
5. 异步用 tokio
6. 日志用 tracing
```

### Step 5：编写测试

```
测试规范（见 DEV_GUIDE.md）：
1. 单元测试：#[cfg(test)] 或 _test.rs
2. Fixture：tests/fixtures/
3. 覆盖率目标：80%+
```

### Step 6：提交代码

```
Git 提交格式（见 DEV_GUIDE.md）：
<type>(<scope>): <subject>

示例：
feat(storage): add symbols CRUD operations
fix(parser): handle error nodes gracefully
test(resolver): add import parsing tests
```

---

## 任务执行 Prompt 模板

以下 Prompt 可直接用于启动 Agent 执行具体任务：

### 模板 A：单任务执行

```
执行 QuickDep 项目任务 T{ID}。

任务描述：{从 DEV_GUIDE.md 复制}

依赖任务：{检查依赖任务是否完成}

执行步骤：
1. 阅读 docs/FEATURES.md 对应模块（M{module}）
2. 理解数据模型和 SQLite Schema
3. 参考 docs/REFERENCE_ANALYSIS.md 借鉴点
4. 如有必要，查看上游参考项目实现
5. 按开发规范实现代码
6. 编写单元测试
7. 运行 cargo test 验证
8. 提交代码：{type}({scope}): {subject}

验收标准：
- 功能实现正确
- 测试通过
- clippy 无警告
```

### 模板 B：模块开发

```
开发 QuickDep 模块 M{ID}: {模块名}。

功能描述：{从 FEATURES.md 复制}

包含任务：{从 DEV_GUIDE.md 复制任务列表}

执行步骤：
1. 阅读模块依赖关系，确认依赖模块已完成
2. 阅读 docs/FEATURES.md 数据模型部分
3. 阅读 docs/REFERENCE_ANALYSIS.md 借鉴点
4. 参考外部项目公开实现
5. 按任务顺序逐个实现
6. 每个任务完成后运行测试
7. 模块完成后编写集成测试

验收标准：
- 所有子功能实现
- 所有任务测试通过
- 模块集成测试通过
```

### 模板 C：里程碑执行

```
执行 QuickDep 里程碑 M{N}: {里程碑名}。

目标：{从 DEV_GUIDE.md 复制}

包含任务：{从 DEV_GUIDE.md 复制 Phase 列表}

执行步骤：
1. 检查前置里程碑是否完成
2. 检查依赖任务状态
3. 按任务依赖顺序执行
4. 每完成一组任务进行集成验证
5. 里程碑完成后进行验收测试

验收标准：{从 DEV_GUIDE.md 复制验收标准}
```

---

## 具体任务 Prompt 示例

### T2.1 SQLite 连接管理

```
执行 QuickDep 项目任务 T2.1。

任务描述：SQLite 连接管理
依赖任务：T1.1（已完成）
预估时间：1h

执行步骤：
1. 阅读 docs/FEATURES.md M4 Storage 模块
2. 理解 SQLite + WAL 模式要求
3. 查看 docs/FEATURES.md SQLite Schema 部分
4. 参考 code-review-graph 中的连接管理思路
5. 创建 src/storage/mod.rs 和 sqlite.rs
6. 实现 Storage 结构体和连接池
7. 配置 WAL 模式：PRAGMA journal_mode = WAL
8. 编写连接测试
9. 运行 cargo test --package quickdep --lib storage

代码规范：
- 使用 rusqlite crate（bundled feature）
- thiserror 定义 StorageError
- pub fn 需文档注释

验收标准：
- Storage::new(path) 创建连接
- WAL 模式生效
- 测试通过
- clippy 无警告

提交格式：feat(storage): add sqlite connection management
```

### T3.3 Rust Parser

```
执行 QuickDep 项目任务 T3.3。

任务描述：Rust Parser（tree-sitter-rust）
依赖任务：T3.1 Parser trait 定义（已完成）、T1.1 数据结构（已完成）
预估时间：4h

执行步骤：
1. 阅读 docs/FEATURES.md M5 Parser 模块
2. 理解符号类型：Function/Method/Struct/Enum/Trait/Macro
3. 查看 docs/FEATURES.md 数据模型 Symbol 结构
4. 参考 potpie 的 tree-sitter query 设计思路
5. 参考 code-review-graph 的解析流程设计
6. 创建 src/parser/rust.rs
7. 实现 RustParser struct（实现 Parser trait）
8. 定义 tree-sitter queries（或直接用 node type 匹配）
9. 提取函数/结构体/枚举定义
10. 提取 use 语句（imports）
11. 提取函数调用（references）
12. 处理错误节点（忽略 + 记录数量）
13. 编写 Parser 测试
14. 创建 tests/fixtures/rust/ 样例文件

代码规范：
- 使用 tree-sitter-rust crate
- 实现 Parser trait
- 返回 Result<Vec<Symbol>, ParseError>
- 错误节点不阻塞解析

验收标准：
- 可解析 Rust 函数定义
- 可解析 struct/enum
- 可提取 use 语句
- 可提取函数调用
- 测试通过

提交格式：feat(parser): add rust parser with tree-sitter
```

### T9.5 find_interfaces MCP Tool

```
执行 QuickDep 项目任务 T9.5。

任务描述：find_interfaces MCP tool
依赖任务：T2.3 Symbols CRUD（已完成）、T7.1 符号名索引缓存（已完成）、T8.4 MCP Tools 注册（已完成）
预估时间：2h

执行步骤：
1. 阅读 docs/FEATURES.md M9 MCP Tools 模块
2. 理解 T5 find_interfaces 功能：模糊匹配搜索接口
3. 参考 code-review-graph 的 query_graph_tool 设计思路
4. 实现 find_interfaces tool handler
5. 参数定义：
   - project_id: String
   - query: String（模糊匹配）
   - kind: Option<SymbolKind>
   - file: Option<String>
   - limit: usize（默认 10）
6. 返回 Vec<InterfaceSummary>（id, name, file, line, kind）
7. 使用 Cache 符号名索引加速
8. 编写 MCP 模拟客户端测试

代码规范：
- 使用 rmcp crate 定义 tool
- #[tool] 注解
- 返回 JSON 格式结果

验收标准：
- MCP find_interfaces 可调用
- 模糊搜索返回结果
- 测试通过

提交格式：feat(mcp): add find_interfaces tool
```

---

## 文档内容速查

### 数据模型速查（FEATURES.md）

```rust
// Symbol
struct Symbol {
    id: String,              // 全局唯一 ID
    name: String,            // 符号名称
    qualified_name: String,  // file_path::SymbolName 格式
    kind: SymbolKind,        // Function/Method/Class/Struct/Enum...
    file_path: String,       // 相对路径
    line: u32,
    column: u32,
    visibility: Visibility,  // Public/Private
    signature: Option<String>,
    source: SymbolSource,    // Local/External/Builtin
}

// Dependency
struct Dependency {
    id: String,
    from_symbol: String,
    to_symbol: String,
    from_file: String,
    from_line: u32,
    kind: DependencyKind,    // Call/Inherit/Implement/TypeUse
}
```

### SQLite Schema 速查（FEATURES.md）

```sql
CREATE TABLE symbols (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    visibility TEXT,
    signature TEXT,
    source TEXT DEFAULT 'local'
);

CREATE TABLE dependencies (
    id TEXT PRIMARY KEY,
    from_symbol TEXT NOT NULL,
    to_symbol TEXT NOT NULL,
    from_file TEXT NOT NULL,
    from_line INTEGER NOT NULL,
    kind TEXT NOT NULL
);

CREATE TABLE file_state (
    path TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    last_modified INTEGER NOT NULL,
    status TEXT DEFAULT 'ok',
    error_message TEXT
);
```

### 模块依赖速查（FEATURES.md）

```
依赖关系：
CLI (M11) → MCP Server (M1) → MCP Tools (M9) → Project Manager (M3)
                                          → Cache (M8) → Storage (M4)
                                                       → Resolver (M6) → Parser (M5)

开发顺序：
Parser (M5) → Storage (M4) → Resolver (M6) → Project Manager (M3)
→ Cache (M8) → Watcher (M7) → MCP Server (M1) → MCP Tools (M9)
```

### 借鉴点速查（REFERENCE_ANALYSIS.md）

| 功能 | 最佳参考 | 关键点 |
|------|----------|--------|
| SQLite 存储 | code-review-graph | WAL 模式 + 递归 CTE |
| Tree-sitter 解析 | code-review-graph | AST 遍历 + node type |
| MCP 工具 | code-review-graph | FastMCP stdio |
| 增量更新 | autodocs | Git diff + hash |
| Tree-sitter queries | potpie | .scm 文件格式 |

---

## 常见问题处理

### Q1：如何处理 glob import？

```
决策（见 FEATURES.md）：
- 标记为 ImportKind::Glob
- 不建立依赖关系
- 返回时提示用户存在 glob import

代码：
if import.source.ends_with("*") {
    import.kind = ImportKind::Glob;
    // 不匹配具体符号
}
```

### Q2：如何处理解析错误？

```
决策（见 FEATURES.md）：
- 忽略错误节点
- 记录错误数量到 FileState
- 不阻塞解析流程

代码：
let error_count = count_error_nodes(&tree);
result.errors = error_count;
result.partial = error_count > 0;
// 继续提取有效符号
```

### Q3：Qualified Name 格式？

```
决策（见 FEATURES.md）：
格式：相对路径::符号名

示例：
src/utils.rs::helper          # 函数
src/utils.rs::Utils::process  # 方法
src/models.rs::User           # 结构体
src/models.rs::User::new      # 结构体方法

注意：使用相对路径而非绝对路径
```

### Q4：如何实现增量更新？

```
决策（见 FEATURES.md 增量更新策略）：
1. 计算 blake3 hash
2. 对比 file_state 表中旧 hash
3. 相同 → 跳过
4. 不同 → diff_symbols() → 增量更新数据库

参考：code-review-graph 的 incremental.py 设计思路
```

### Q5：递归 CTE 如何实现依赖查询？

```
参考：code-review-graph 的 graph.py 设计思路

SQL：
WITH RECURSIVE dep_chain AS (
    SELECT to_symbol, 1 as depth
    FROM dependencies WHERE from_symbol = ?
    
    UNION ALL
    
    SELECT d.to_symbol, dc.depth + 1
    FROM dependencies d
    JOIN dep_chain dc ON d.from_symbol = dc.to_symbol
    WHERE dc.depth < ?
)
SELECT * FROM dep_chain;
```

---

## 检查清单

### 任务开始前

```
- [ ] 已阅读 FEATURES.md 对应模块
- [ ] 已理解数据模型
- [ ] 已查看参考项目实现
- [ ] 依赖任务已完成
```

### 任务完成后

```
- [ ] 功能实现正确
- [ ] 单元测试通过
- [ ] cargo clippy 无警告
- [ ] 文档注释完整
- [ ] 代码已提交
```

### 模块完成后

```
- [ ] 所有子功能实现
- [ ] 所有任务测试通过
- [ ] 模块集成测试通过
- [ ] 与依赖模块接口正确
```

---

## 总结

Agent 执行开发任务时应遵循：

```
1. 先读文档（FEATURES → REFERENCE → DEV_GUIDE）
2. 再看参考（code-review-graph 源代码）
3. 按规范实现（Rust/clippy/thiserror）
4. 写测试验证（单元/集成）
5. 提交代码（规范格式）
```

核心文档路径：
- `docs/FEATURES.md`
- `docs/REFERENCE_ANALYSIS.md`
- `docs/DEV_GUIDE.md`
