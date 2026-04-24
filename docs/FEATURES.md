# QuickDep 功能清单

## 项目概述

**QuickDep** 是一个 Rust 实现的 MCP 服务，用于扫描项目代码的接口依赖关系，提供给 Agent 工具实时查找。

**核心目标**：
- 快速在大规模代码项目中搜索依赖关系
- 减少 Agent 工具调用次数，节省 tokens
- 后台实时扫描，监控文件变更，实时更新依赖关系
- 依赖关系落盘持久化

---

## 功能模块清单

### M1: MCP Server

**功能描述**：提供 MCP 协议服务，供 Agent 工具调用

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M1.1 | initialize handler | 处理 MCP 初始化请求，返回 capabilities |
| M1.2 | initialized handler | 初始化完成后触发后台监控启动 |
| M1.3 | tools handler | 处理工具调用请求 |
| M1.4 | resources handler | 处理资源读取请求 |

**依赖**：M3, M4, M5, M6

---

### M2: HTTP Server (可选)

**功能描述**：提供 HTTP API，供 Web 前端或第三方工具使用

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M2.1 | REST API | RESTful 接口，与 MCP Tools 功能对应 |
| M2.2 | WebSocket | 实时推送变更通知 |
| M2.3 | CORS 支持 | 跨域请求支持 |

**依赖**：M3, M4, M5, M6

**配置**：默认关闭，通过 `--http <port>` 参数启用

---

### M3: Project Manager

**功能描述**：管理多项目状态，懒加载监控

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M3.1 | 项目注册 | 注册项目路径，生成 project_id |
| M3.2 | 状态管理 | 管理 NotLoaded/Loading/Loaded 状态 |
| M3.3 | 懒加载触发 | 首次调用时触发扫描和监控 |
| M3.4 | 监控暂停/恢复 | 空闲 5 分钟暂停，调用时恢复 |
| M3.5 | 项目隔离 | 基于 path hash 的 project_id，确保隔离 |

**依赖**：M4, M5

---

### M4: Storage

**功能描述**：持久化存储符号和依赖关系

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M4.1 | SQLite 存储 | 使用 SQLite + WAL 模式 |
| M4.2 | 符号表管理 | symbols 表 CRUD |
| M4.3 | 依赖表管理 | dependencies 表 CRUD |
| M4.4 | 文件状态管理 | file_state 表，存储哈希和错误信息 |
| M4.5 | 导入表管理 | imports 表，存储 import 语句 |
| M4.6 | manifest 管理 | 项目元信息 JSON 文件 |
| M4.7 | 版本检查 | schema_version 检查，不匹配时提示重建 |
| M4.8 | Hash 快速验证 | 启动时对比 hash，只解析变更文件 |

**依赖**：无

**存储位置**：`.quickdep/` 目录

---

### M5: Parser

**功能描述**：解析源代码，提取符号和引用

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M5.1 | Parser trait | 语言解析器抽象接口 |
| M5.2 | Rust Parser | 基于 tree-sitter-rust 实现 |
| M5.3 | TypeScript Parser | 基于 tree-sitter-typescript 实现 |
| M5.4 | Python Parser | 基于 tree-sitter-python 实现 |
| M5.5 | Go Parser | 基于 tree-sitter-go 实现 |
| M5.6 | 语言检测 | 扩展名匹配 + 配置覆盖 |
| M5.7 | 错误容忍 | 忽略错误节点，记录错误数量 |

**依赖**：无

**符号类型**：
- Function, Method, Class, Struct, Enum, EnumVariant
- Interface, Trait, TypeAlias, Module, Constant, Variable
- Property, Macro

---

### M6: Resolver

**功能描述**：解析 import 语句，建立跨文件符号映射

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M6.1 | Import 解析 | 解析 use/import 语句 |
| M6.2 | 模块路径解析 | 解析 Rust mod、TS 相对路径等 |
| M6.3 | 符号匹配 | 将引用匹配到符号定义 |
| M6.4 | glob import 处理 | 标记为 Glob，不建立依赖 |
| M6.5 | 别名处理 | 存储别名映射关系 |
| M6.6 | 外部符号标记 | 标记 external/builtin 符号 |

**依赖**：M4, M5

---

### M7: Watcher

**功能描述**：监控文件变更，触发增量更新

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M7.1 | 文件监控 | 基于 notify 监控文件系统 |
| M7.2 | Hash 计算 | blake3 计算文件内容哈希 |
| M7.3 | Hash 对比 | 与存储的旧哈希对比，过滤无效变更 |
| M7.4 | 事件防抖 | 500ms 防抖 + 批量处理 |
| M7.5 | 符号增量对比 | diff 新旧符号，计算 added/removed/modified |
| M7.6 | 依赖增量更新 | 只更新变更符号的依赖关系 |
| M7.7 | 错误处理 | 记录解析失败，不阻塞监控 |

**依赖**：M3, M4, M5, M6

---

### M8: Cache

**功能描述**：内存缓存，加速查询

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M8.1 | 符号名索引 | HashMap<String, Vec<SymbolId>> 缓存 |
| M8.2 | 查询结果缓存 | 缓存查询结果，5 分钟 TTL |
| M8.3 | 缓存失效 | 文件变更时失效相关缓存 |

**依赖**：M4

---

### M9: MCP Tools

**功能描述**：MCP 工具定义

**工具列表**：
| ID | 工具名 | 说明 |
|----|--------|------|
| T1 | list_projects | 列出已知项目 |
| T2 | scan_project | 触发项目扫描 |
| T3 | get_scan_status | 获取扫描状态和进度 |
| T4 | cancel_scan | 取消正在进行的扫描 |
| T5 | find_interfaces | 搜索接口（模糊匹配） |
| T6 | get_interface | 获取接口详情 |
| T7 | get_dependencies | 获取依赖关系图 |
| T8 | get_call_chain | 获取调用链路径 |
| T9 | get_file_interfaces | 获取文件内接口列表 |
| T10 | get_task_context | 获取场景化任务上下文 |
| T11 | analyze_workflow_context | 强制走 workflow 场景分析 |
| T12 | analyze_change_impact | 强制走 impact 场景分析 |
| T13 | analyze_behavior_context | 强制走 behavior 场景分析 |
| T14 | locate_relevant_code | 强制走 locate 场景分析 |
| T15 | batch_query | 批量查询 |
| T16 | rebuild_database | 重建数据库 |

**依赖**：M3, M4, M8

---

### M10: MCP Resources

**功能描述**：MCP 资源定义

**资源列表**：
| URI | 说明 |
|-----|------|
| quickdep://projects | 项目列表 |
| quickdep://project/{id}/status | 项目扫描状态 |
| quickdep://project/{id}/interfaces | 接口列表摘要 |
| quickdep://project/{id}/interface/{id} | 接口详情 |
| quickdep://project/{id}/interface/{id}/deps | 依赖关系 |

**依赖**：M3, M4

---

### M11: CLI

**功能描述**：命令行接口

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M11.1 | MCP 模式启动 | 默认模式，stdio MCP 服务 |
| M11.2 | HTTP 模式启动 | --http 参数，MCP + HTTP |
| M11.3 | HTTP only 模式 | --http-only 参数，仅 HTTP |
| M11.4 | debug 子命令 | 调试工具，查看符号/依赖/状态 |
| M11.5 | version 参数 | --version 输出版本信息 |

**依赖**：M1, M2

---

### M12: Configuration

**功能描述**：配置管理

**配置项**：
| 配置 | 说明 | 默认值 |
|------|------|--------|
| scan.include | 扫描路径模式 | ["src/**"] |
| scan.exclude | 排除路径模式 | ["target/**", "node_modules/**"] |
| scan.include_tests | 是否包含测试 | false |
| scan.languages | 语言列表 | ["rust", "typescript"] |
| parser.map | 文件扩展名映射 | {} |
| server.http_enabled | HTTP 是否启用 | false |
| server.http_port | HTTP 端口 | 8080 |
| log.level | 日志级别 | "info" |
| watcher.idle_timeout | 监控空闲超时 | 5m |

**配置文件**：`quickdep.toml` 或 `.quickdep/config.toml`

**依赖**：无

---

### M13: Logging

**功能描述**：日志系统

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M13.1 | 文件日志 | RollingFileAppender，每日滚动 |
| M13.2 | stderr 输出 | 关键信息实时输出 |
| M13.3 | 日志级别控制 | tracing 配置 |

**依赖**：无

**日志位置**：`.quickdep/logs/quickdep.log`

---

### M14: Security

**功能描述**：安全防护

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M14.1 | 路径验证 | 防止路径遍历攻击 |
| M14.2 | project_id 生成 | 基于 path hash，防止伪造 |

**依赖**：无

---

### M15: Testing

**功能描述**：测试框架

**子功能**：
| ID | 功能 | 说明 |
|----|------|------|
| M15.1 | Parser 测试 | fixture 文件解析测试 |
| M15.2 | Resolver 测试 | import/symbol 解析测试 |
| M15.3 | Storage 测试 | 数据库操作测试 |
| M15.4 | MCP 测试 | 模拟 MCP 客户端测试 |
| M15.5 | E2E 测试 | 完整流程测试 |

**依赖**：所有模块

---

## 功能依赖关系图

```
                              ┌─────────────┐
                              │    CLI      │
                              │   (M11)     │
                              └──────┬──────┘
                                     │
              ┌──────────────────────┴──────────────────────┐
              │                                              │
    ┌─────────▼─────────┐                    ┌───────────────▼───────────┐
    │    MCP Server     │                    │     HTTP Server (可选)    │
    │      (M1)         │                    │         (M2)              │
    └─────────┬─────────┘                    └───────────────┬───────────┘
              │                                              │
              │          ┌───────────────────────┐          │
              │          │   Configuration (M12) │          │
              │          └───────────────────────┘          │
              │                                              │
              └────┬─────────────────┬─────────────────┬─────┘
                   │                 │                 │
          ┌────────▼────────┐ ┌──────▼──────┐ ┌───────▼───────┐
          │   MCP Tools     │ │ MCP Resources│ │   Logging     │
          │     (M9)        │ │    (M10)     │ │    (M13)      │
          └───────┬─────────┘ └──────┬───────┘ └───────────────┘
                  │                  │
                  │    ┌─────────────▼─────────────┐
                  │    │     Project Manager       │
                  │    │         (M3)              │
                  │    └─────────────┬─────────────┘
                  │                  │
         ┌────────▼──────────────────▼───────────────┐
         │                 Cache (M8)                 │
         └─────────────────────┬─────────────────────┘
                               │
         ┌─────────────────────▼─────────────────────┐
         │               Storage (M4)                │
         └─────────────────────┬─────────────────────┘
                               │
         ┌─────────────────────▼─────────────────────┐
         │               Resolver (M6)               │
         └────────────┬─────────────────┬───────────┘
                      │                 │
         ┌────────────▼───────┐ ┌───────▼─────────────┐
         │    Parser (M5)     │ │   Watcher (M7)      │
         └────────────────────┘ └─────────────────────┘
                      │                 │
         ┌────────────▼─────────────────▼─────────────┐
         │              Security (M14)                │
         └────────────────────────────────────────────┘

         ┌────────────────────────────────────────────┐
         │              Testing (M15)                 │
         │         (依赖所有模块进行测试)              │
         └────────────────────────────────────────────┘
```

---

## 数据模型

### Symbol（符号）

```rust
struct Symbol {
    id: String,              // 全局唯一 ID
    name: String,            // 符号名称
    qualified_name: String,  // 完整限定名
    kind: SymbolKind,        // 类型
    file_path: String,       // 所在文件
    line: u32,               // 行号
    column: u32,             // 列号
    visibility: Visibility,  // 可见性
    signature: Option<String>,// 签名
    source: SymbolSource,    // Local/External/Builtin
}
```

### Dependency（依赖关系）

```rust
struct Dependency {
    id: String,
    from_symbol: String,     // 源符号 ID
    to_symbol: String,       // 目标符号 ID
    from_file: String,       // 调用文件
    from_line: u32,          // 调用行号
    kind: DependencyKind,    // Call/Inherit/Implement/TypeUse
}
```

### Import（导入语句）

```rust
struct Import {
    id: String,
    source: String,          // import 来源
    alias: Option<String>,   // 别名
    file_path: String,
    line: u32,
    kind: ImportKind,        // Named/Glob/Self/Alias
}
```

### FileState（文件状态）

```rust
struct FileState {
    path: String,
    hash: String,            // 内容哈希
    last_modified: u64,
    status: FileStatus,      // Ok/Failed/Pending
    error_message: Option<String>,
}
```

---

## 增量更新策略（Hash 加速）

### 核心原理

通过文件内容 Hash 快速判断文件是否真正变更，避免无效解析，加速增量更新。

### Hash 计算

```rust
use blake3::Hash;

fn compute_file_hash(path: &Path) -> Result<String> {
    let content = std::fs::read(path)?;
    let hash = blake3::hash(&content);
    Ok(hash.to_hex().to_string())
}
```

**选择 blake3 的理由**：
- 极快：比 MD5/SHA256 快 5-10 倍
- 安全：密码学强度哈希
- 短输出：32 字节十六进制

### 启动时快速验证

```
流程：
1. 读取 file_state 表，获取已扫描文件的 hash
2. 遍历项目文件，计算当前 hash
3. 对比 hash：
   - 相同 → 跳过解析，保留旧数据
   - 不同 → 重新解析，更新数据
4. 处理新增文件：解析并存储
5. 处理删除文件：级联删除符号和依赖

优化效果：
- 99% 文件未变更 → 启动验证 < 1 秒（10k 文件）
- 只解析变更文件 → 大幅减少启动时间
```

### 监控时过滤无效变更

```
流程：
1. 文件变更事件触发
2. 计算新 hash
3. 与 file_state 中的旧 hash 对比
4. 相同 → 跳过（可能是 touch 或权限变更）
5. 不同 → 触发解析

场景示例：
- git checkout 切换分支 → 大量 touch，但内容可能未变
- IDE 保存 → 实际内容变更
- chmod/chown → 权限变更，内容未变

优化效果：
- git checkout 后的批量 touch 不会触发大量解析
- 只解析真正变更的文件
```

### 符号增量对比

```rust
struct SymbolDiff {
    added: Vec<Symbol>,      // 新增符号
    removed: Vec<SymbolId>,  // 删除符号
    modified: Vec<Symbol>,   // 修改符号（位置/签名变化）
}

fn diff_symbols(old: Vec<Symbol>, new: Vec<Symbol>) -> SymbolDiff {
    // 按 qualified_name 匹配
    let old_map: HashMap<String, Symbol> = old.into_map();
    let new_map: HashMap<String, Symbol> = new.into_map();
    
    SymbolDiff {
        added: new_map.keys().filter(|k| !old_map.contains(k)).map(...),
        removed: old_map.keys().filter(|k| !new_map.contains(k)).map(...),
        modified: new_map.iter().filter(|(k, v)| old_map.get(k) != Some(v)).map(...),
    }
}
```

### 依赖关系增量更新

```
流程：
1. 获取文件关联的旧符号列表
2. 解析文件，获取新符号列表
3. diff_symbols() → 计算差异
4. 更新数据库：
   - added → INSERT symbols
   - removed → DELETE symbols + 级联 DELETE dependencies
   - modified → UPDATE symbols
5. 重新解析依赖关系：
   - 删除旧依赖（from_symbol 或 to_symbol 在 removed 中）
   - 解析新符号的依赖 → INSERT dependencies
```

### 增量更新流程图

```
┌─────────────────────────────────────────────────────────────┐
│                     文件变更检测                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   文件事件 ──→ 计算 hash ──→ 与旧 hash 对比                  │
│                     │                                       │
│                     ├─ 相同 ──→ 跳过（结束）                 │
│                     │                                       │
│                     └─ 不同 ──→ 继续                         │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│                     符号解析                                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   解析文件 ──→ 提取符号 ──→ 提取 imports ──→ 提取 references │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│                     增量对比                                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   查询旧符号 ──→ diff_symbols() ──→ SymbolDiff              │
│                                                             │
│   ┌─────────────────────────────────────────┐               │
│   │ SymbolDiff:                             │               │
│   │   added: [新增符号]                     │               │
│   │   removed: [删除符号ID]                 │               │
│   │   modified: [修改符号]                  │               │
│   └─────────────────────────────────────────┘               │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│                     数据库更新                               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   1. DELETE symbols WHERE id IN (removed)                   │
│   2. DELETE dependencies WHERE from_symbol IN (removed)     │
│   3. DELETE dependencies WHERE to_symbol IN (removed)       │
│   4. INSERT symbols (added)                                 │
│   5. UPDATE symbols (modified)                              │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│                     依赖重建                                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   1. 解析 imports → 建立 visible_symbols 映射               │
│   2. 匹配 references → 建立依赖关系                         │
│   3. INSERT dependencies (新依赖)                           │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│                     缓存失效                                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   1. 失效该文件的符号缓存                                   │
│   2. 失效涉及 removed 符号的查询缓存                        │
│   3. 失效涉及 added 符号的查询缓存                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 性能预估

| 场景 | 文件数 | 预估耗时 |
|------|--------|----------|
| 首次全量扫描 | 1000 | ~30s |
| 启动验证（无变更） | 1000 | <1s |
| 启动验证（10 个变更） | 1000 | ~3s |
| 单文件变更 | 1 | <100ms |
| git checkout（100 变更） | 100 | ~10s |

### 关键优化点

1. **Hash 计算并行化**：启动验证时并行计算多个文件 hash
2. **批量数据库操作**：使用事务批量 INSERT/DELETE
3. **缓存智能失效**：只失效受影响的部分缓存
4. **依赖关系懒解析**：先更新符号，后台重建依赖

---

## SQLite Schema

```sql
-- 符号表
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
    source TEXT DEFAULT 'local',
    
    INDEX idx_name (name),
    INDEX idx_qualified (qualified_name),
    INDEX idx_file (file_path)
);

-- 依赖表
CREATE TABLE dependencies (
    id TEXT PRIMARY KEY,
    from_symbol TEXT NOT NULL,
    to_symbol TEXT NOT NULL,
    from_file TEXT NOT NULL,
    from_line INTEGER NOT NULL,
    kind TEXT NOT NULL,
    
    INDEX idx_from (from_symbol),
    INDEX idx_to (to_symbol)
);

-- 导入表
CREATE TABLE imports (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    alias TEXT,
    file_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    kind TEXT NOT NULL,
    
    INDEX idx_file (file_path)
);

-- 文件状态表
CREATE TABLE file_state (
    path TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    last_modified INTEGER NOT NULL,
    status TEXT DEFAULT 'ok',
    error_message TEXT
);

-- 扫描状态表
CREATE TABLE scan_state (
    id TEXT PRIMARY KEY,
    status TEXT,
    files_total INTEGER,
    files_scanned INTEGER,
    started_at INTEGER,
    updated_at INTEGER
);
```

---

## 实现优先级

### Phase 1：核心功能（MVP）

| 优先级 | 模块 | 说明 |
|--------|------|------|
| P0 | M4 Storage | 基础存储层 |
| P0 | M5 Parser | Rust Parser（可自测） |
| P0 | M6 Resolver | 跨文件解析 |
| P0 | M3 Project Manager | 项目管理 |
| P1 | M9 MCP Tools | 核心工具实现 |
| P1 | M1 MCP Server | MCP 协议 |
| P1 | M7 Watcher | 文件监控 |
| P1 | M8 Cache | 内存缓存 |
| P2 | M5 Parser | TypeScript Parser |
| P2 | M10 MCP Resources | 资源实现 |
| P2 | M11 CLI | 命令行 |
| P2 | M12 Configuration | 配置系统 |
| P2 | M13 Logging | 日志系统 |
| P2 | M14 Security | 安全防护 |

### Phase 2：扩展功能

| 优先级 | 模块 | 说明 |
|--------|------|------|
| P3 | M2 HTTP Server | Web API |
| P3 | M5 Parser | Python Parser |
| P3 | M5 Parser | Go Parser |
| P3 | M15 Testing | 完整测试覆盖 |

---

## 目录结构

```
quickdep/
├── Cargo.toml
├── quickdep.toml              # 配置文件示例
├── docs/
│   └── FEATURES.md            # 本文档
├── src/
│   ├── main.rs                # CLI 入口
│   ├── lib.rs
│   │
│   ├── core/
│   │   ├── mod.rs
│   │   ├── symbol.rs          # 符号定义
│   │   ├── dependency.rs      # 依赖关系
│   │   └── graph.rs           # 图结构
│   │
│   ├── parser/
│   │   ├── mod.rs
│   │   ├── trait.rs           # Parser trait
│   │   ├── rust.rs            # Rust parser
│   │   ├── typescript.rs      # TypeScript parser
│   │   ├── python.rs          # Python parser
│   │   ├── go.rs              # Go parser
│   │   └── language.rs        # 语言检测
│   │
│   ├── resolver/
│   │   ├── mod.rs
│   │   ├── import.rs          # Import 解析
│   │   ├── module.rs          # 模块路径解析
│   │   └── symbol.rs          # 符号匹配
│   │
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── sqlite.rs          # SQLite 存储
│   │   ├── schema.rs          # Schema 定义
│   │   └── manifest.rs        # Manifest 管理
│   │
│   ├── cache/
│   │   ├── mod.rs
│   │   ├── index.rs           # 符号索引缓存
│   │   └── query.rs           # 查询结果缓存
│   │
│   ├── watcher/
│   │   ├── mod.rs
│   │   ├── fs.rs              # 文件监控
│   │   └── debounce.rs        # 防抖处理
│   │
│   ├── project/
│   │   ├── mod.rs
│   │   ├── manager.rs         # 项目管理
│   │   ├── state.rs           # 项目状态
│   │   └── id.rs              # ID 生成
│   │
│   ├── mcp/
│   │   ├── mod.rs
│   │   ├── server.rs          # MCP server
│   │   ├── tools.rs           # MCP tools
│   │   ├── resources.rs       # MCP resources
│   │   └── handlers.rs        # 请求处理
│   │
│   ├── http/
│   │   ├── mod.rs
│   │   ├── server.rs          # HTTP server
│   │   ├── api.rs             # REST API
│   │   ├── websocket.rs       # WebSocket
│   │   └── cors.rs            # CORS
│   │
│   ├── config/
│   │   ├── mod.rs
│   │   ├── settings.rs        # 配置定义
│   │   └── loader.rs          # 配置加载
│   │
│   ├── log/
│   │   ├── mod.rs
│   │   ├── setup.rs           # 日志初始化
│   │
│   ├── security/
│   │   ├── mod.rs
│   │   ├── path.rs            # 路径验证
│   │
│   └── cli/
│       ├── mod.rs
│       ├── args.rs            # CLI 参数
│       ├── debug.rs           # debug 子命令
│       └
│   └── tests/
│       ├── parser/
│       ├── resolver/
│       ├── storage/
│       ├── mcp/
│       └── e2e/
│       └── fixtures/
│           ├── rust/
│           ├── typescript/
│           ├── python/
│           └── go/
│
└── .quickdep/                  # 缓存目录（项目运行时生成）
    ├── manifest.json
    ├── symbols.db
    ├── logs/
    └
```

---

## 关键技术决策汇总

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 解析器 | Tree-sitter | 轻量、增量解析、容错性好 |
| 存储 | SQLite + WAL | 简单可靠、单文件部署、并发友好 |
| Hash 算法 | blake3 | 极快、安全、短输出 |
| 增量更新 | Hash 对比 + diff | 避免无效解析、只更新差异 |
| 监控模式 | 懒加载 + 暂停 | 按需节省资源 |
| 缓存 | 内存索引 + 查询缓存 | 加速频繁查询 |
| HTTP | 同进程共享组件 | 避免重复逻辑 |
| 错误处理 | 记录 + 不阻塞 | 单个失败不影响整体 |
| 安全 | 路径验证 + ID hash | 基本防护 |
| 语言优先级 | Rust → TypeScript | 自测 + 广泛使用 |
