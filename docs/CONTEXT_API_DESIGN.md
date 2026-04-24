# QuickDep 上下文打包 API 设计

## 1. 背景

QuickDep 当前已经能提供高质量的静态依赖查询能力：

- `find_interfaces`
- `get_interface`
- `get_dependencies`
- `get_call_chain`
- `get_file_interfaces`
- `batch_query`

这已经能明显减少 Agent 在大仓库里的盲目搜索和盲目读文件。

但当前模式仍然以“底层查询”为主：

1. Agent 先问符号详情
2. 再问 incoming dependencies
3. 再问 outgoing dependencies
4. 再问文件内其他接口
5. 再自己拼装上下文

这个流程虽然比 `grep + cat` 高效很多，但仍然存在两个问题：

- 需要多次 MCP 往返
- Agent 仍然需要自己决定“哪些内容最值得读”

因此需要补一层“任务导向的上下文组装”能力。

---

## 2. 目标

本阶段的目标不是做“全量代码理解”，而是做一个最小可用的场景化上下文工具：

> 给定一个符号，返回足够支持重构影响分析的结构化上下文。

这个工具暂定名为：

- `get_refactoring_context`

它的职责不是替代现有底层查询，而是在现有图查询之上进行：

- 聚合
- 排序
- 裁剪
- 返回建议阅读集合

---

## 3. 非目标

本期明确不做：

- 数据流分析
- 控制流分析
- 宏展开后的真实语义建模
- 动态分派的运行时类型推断
- 全量源码打包
- 自动生成补丁或重构方案

本期仍然坚持 QuickDep 的核心定位：

- 静态符号
- 直接依赖
- 面向 Agent 的结构化上下文供给

---

## 4. 核心判断

QuickDep 当前已经较好完成了“依赖发现”：

- 改一个函数，能快速知道谁调用它、它调用谁

但还没有完成“上下文打包”：

- 改一个函数时，Agent 还不能一次性拿到“最值得读的上下文”

因此本期应补的不是更多底层查询，而是一个新的聚合层。

---

## 5. 本期范围：方案 A

### 5.1 功能名称

- `get_refactoring_context`

### 5.2 最小可用能力

输入一个符号后，返回：

- 符号本身
- 直接调用者
- 直接被调用者
- 同文件其他接口
- 相关文件列表
- 建议阅读顺序
- 风险等级与简短理由

### 5.3 本期不包含

以下能力不进入 A 方案：

- 函数体源码片段
- token 精确预算裁剪
- bugfix / test impact 等其他场景工具
- 多层递归上下文扩张
- 外部库实现追踪

这些能力全部作为未来扩展记录在本文档中。

---

## 6. API 设计

### 6.1 MCP Tool 名称

- `get_refactoring_context`

### 6.2 请求结构

```json
{
  "project": {
    "path": "/path/to/project"
  },
  "interface": "src/lib.rs::helper",
  "max_callers": 20,
  "max_callees": 20,
  "max_related_files": 8,
  "include_file_interfaces": true
}
```

### 6.3 字段说明

| 字段 | 必填 | 说明 |
|------|------|------|
| `project` | 否 | 目标项目，复用现有 `ProjectTarget` |
| `interface` | 是 | 符号 ID、qualified name 或精确名称 |
| `max_callers` | 否 | 最多返回多少直接调用者，默认 20 |
| `max_callees` | 否 | 最多返回多少直接依赖，默认 20 |
| `max_related_files` | 否 | 最多返回多少相关文件，默认 8 |
| `include_file_interfaces` | 否 | 是否包含同文件接口列表，默认 true |

### 6.4 返回结构

```json
{
  "symbol": {},
  "callers": [],
  "callees": [],
  "same_file_interfaces": [],
  "related_files": [],
  "suggested_reads": [],
  "summary": {
    "risk": "medium",
    "reasons": [
      "被多个文件直接调用",
      "依赖跨越多个文件"
    ]
  }
}
```

---

## 7. 返回字段定义

### 7.1 `symbol`

目标符号本身，直接复用 `get_interface` 返回的结构。

### 7.2 `callers`

目标符号的直接调用者，来源于：

- `get_dependencies(direction = "incoming", max_depth = 1)`

### 7.3 `callees`

目标符号的直接依赖，来源于：

- `get_dependencies(direction = "outgoing", max_depth = 1)`

### 7.4 `same_file_interfaces`

目标文件中的其他接口，来源于：

- `get_file_interfaces(file_path = symbol.file_path)`

说明：

- 需要排除目标符号自身
- 需要优先保留与目标符号同文件的高相关接口

### 7.5 `related_files`

根据以下来源聚合并去重：

- callers 的 `file_path`
- callees 的 `file_path`
- target symbol 的 `file_path`

返回时建议包含：

```json
{
  "file_path": "src/service.rs",
  "reason": "contains direct caller",
  "symbol_count": 3
}
```

### 7.6 `suggested_reads`

这是本工具最关键的输出之一。

建议返回“应该优先读哪些符号或文件”，例如：

```json
[
  {
    "kind": "symbol",
    "qualified_name": "src/api.rs::handle_request",
    "reason": "direct caller"
  },
  {
    "kind": "symbol",
    "qualified_name": "src/repo.rs::save_user",
    "reason": "direct callee"
  },
  {
    "kind": "file",
    "file_path": "src/service.rs",
    "reason": "same file as target"
  }
]
```

目标：

- 告诉 Agent 先读什么
- 减少它自己试探式读文件的成本

### 7.7 `summary`

返回一个小型风险摘要，例如：

```json
{
  "risk": "high",
  "reasons": [
    "direct callers exceed 10",
    "touches more than 5 files"
  ]
}
```

---

## 8. 排序与裁剪规则

本期虽然不做精确 token budget，但仍然要做基础裁剪，否则返回会迅速失控。

### 8.1 callers 排序

优先级建议：

1. 非目标自身
2. 本地符号优先于 builtin / external
3. 文件路径稳定排序
4. 符号名排序

### 8.2 callees 排序

优先级建议：

1. 非目标自身
2. 本地符号优先
3. 同文件符号优先
4. 文件路径与符号名排序

### 8.3 same_file_interfaces 裁剪

建议：

- 排除目标符号自身
- 仅保留前 N 个，默认可设 20
- 优先函数/方法，再保留类型定义

### 8.4 related_files 裁剪

建议：

- 目标文件永远保留
- direct caller/callee 文件优先
- 去重后限制在 `max_related_files`

---

## 9. 风险等级规则

本期可以采用简单规则，不需要复杂评分模型。

### 9.1 `low`

满足全部：

- callers <= 3
- callees <= 5
- related_files <= 3

### 9.2 `medium`

满足任一：

- callers > 3
- callees > 5
- related_files > 3

### 9.3 `high`

满足任一：

- callers > 10
- callees > 15
- related_files > 8

说明：

- 本期只需要给 Agent 一个可解释、可扫描的风险分层
- 不需要引入复杂评分公式

---

## 10. 复用现有能力

该工具不应重复发明底层逻辑，应直接复用现有实现：

- 符号解析：`get_interface_value`
- 依赖查询：`get_dependencies_value`
- 文件接口：`get_file_interfaces_value`
- 缓存与项目加载：沿用 `QuickDepServer` 现有能力

实现方式建议：

1. 先解析目标符号
2. 查询 incoming/outgoing depth=1
3. 查询同文件接口
4. 聚合 related_files
5. 生成 suggested_reads
6. 生成 summary

---

## 11. 测试建议

### 11.1 单元测试

覆盖：

- callers/callees 聚合
- related_files 去重与裁剪
- risk 级别计算
- suggested_reads 排序

### 11.2 集成测试

建议使用小型 fixture 覆盖：

- 单文件低风险函数
- 多调用者高风险函数
- 跨文件调用链函数
- 同文件接口较多的函数

### 11.3 真实仓库验证

可复用本轮已验证过的仓库：

- `nest`
- `requests`
- `cobra`
- `ripgrep`

验证方式：

- 对核心函数调用 `get_refactoring_context`
- 检查返回是否比多次底层查询更适合直接喂给 Agent

---

## 12. 工作量评估

### 12.1 A 方案最小实现

包含：

- MCP 请求/响应结构
- 聚合逻辑
- 排序与裁剪
- 风险摘要
- 单元测试
- 基础集成测试
- 文档

预计工作量：

- 设计细化：0.5 天
- 实现：1 到 1.5 天
- 测试与修正：1 天
- 文档和对外说明：0.5 天

合计：

- `3 到 3.5 天`

### 12.2 风险

主要风险不在实现难度，而在“返回多少才合适”：

- 太少：Agent 还是得继续追问
- 太多：失去节省 token 的意义

因此本期最重要的是：

- 先做保守版
- 让返回明显优于手工多次查询拼装

---

## 13. 未来扩展

以下内容不纳入 A 方案，但建议明确保留为后续方向。

### 13.1 源码片段

可选字段：

- `include_source`

支持：

- `none`
- `symbol_only`
- `minimal`

作用：

- 在需要时附带目标符号及相关符号附近的小片段

### 13.2 token budget

未来可加：

```json
{
  "token_budget": 4000
}
```

由 QuickDep 在预算内进行：

- 排序
- 裁剪
- 片段选取

### 13.3 其他场景工具

未来可扩展为：

- `get_bugfix_context`
- `get_test_impact_context`
- `get_feature_entry_context`

### 13.4 更丰富的 related_files 理由

未来可细化 reason：

- `contains direct caller`
- `contains direct callee`
- `same file as target`
- `highest call density`

### 13.5 外部依赖摘要

未来可增加：

- 外部包命中摘要
- 标准库引用摘要

但仍不建议在本期尝试追踪外部库内部实现。

---

## 14. 推荐实施顺序

建议按以下顺序推进：

1. 补 MCP 请求/响应结构
2. 实现 `get_refactoring_context`
3. 单元测试 risk / sort / dedupe
4. 集成测试最小 fixture
5. 在真实仓库上手工回归
6. 更新 `README` / `API` / `USAGE`

---

## 15. 结论

QuickDep 当前已经较好完成了“依赖发现”，但还未完成“面向 Agent 的上下文打包”。

A 方案的意义不在于增加一个新查询，而在于把：

- 多次底层依赖查询

收敛成：

- 一次面向重构任务的结构化上下文返回

这将是 QuickDep 从“依赖查询器”走向“Agent 上下文供给器”的第一步。
