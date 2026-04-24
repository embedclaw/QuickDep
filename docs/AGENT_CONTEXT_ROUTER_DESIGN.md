# QuickDep Agent 场景路由与自适应上下文 API 设计

## 1. 背景

QuickDep 当前已经证明了一件事：

- 它能显著缩小大型仓库里的候选文件范围
- 它能减少 Agent 的盲目源码阅读
- 它在 watcher / 增量更新场景上已经具备真实产出能力

但现阶段的主要问题也已经很明确：

- Agent 仍然需要自己决定先调用哪个 MCP 工具
- 旧的“单次聚合胖包”思路已经证明不稳，当前仍需避免上下文重新膨胀
- 对复杂问题，Agent 会反复补查，导致 MCP 往返和 payload 膨胀
- “定位问题”和“解释行为”这两类任务需要的上下文并不相同

因此，下一阶段不应继续单纯堆底层查询，而应补一层面向 Agent 的场景路由和自适应上下文供给。

这层能力的目标不是替代源码阅读，而是让 Agent 在大型项目里：

1. 更快进入正确代码区域
2. 更少自己编排工具调用
3. 在上下文不够时，明确知道该如何扩张
4. 在图谱能力不足时，及时退回源码阅读

---

## 2. 问题定义

当前 QuickDep 的 MCP 工具主要是底层索引工具：

- `find_interfaces`
- `get_interface`
- `get_dependencies`
- `get_call_chain`
- `get_file_interfaces`
- `batch_query`

在此之上，当前代码已经补了一层高层入口：

- `get_task_context`

这些工具本身没有问题，问题在于 Agent 使用它们时要自己完成下面这条链路：

1. 理解当前问题属于哪类工作阶段
2. 选择先调哪些工具
3. 判断返回是否已经足够
4. 如果不够，再决定往哪个方向扩上下文

这会带来两个直接问题：

1. 同样的问题，不同 Agent 很容易走出不同的工具调用路径
2. QuickDep 的“结构收敛优势”不一定能稳定转化成更好的实际工作流

所以这轮设计的核心不是“更强搜索”，而是：

> 让服务端替 Agent 承担一部分场景判断、上下文裁剪和下一步建议。

---

## 3. 设计目标

### 3.1 目标

本设计要实现的能力是：

1. 接收一个自然语言任务和少量锚点
2. 自动判断当前更像哪一类 Agent 子任务场景
3. 先返回一个最小但够用的上下文包
4. 当上下文不足时，返回明确的扩张方向
5. 通过机器字段告诉 Agent 当前结果的置信度和覆盖度

### 3.2 非目标

本期明确不做：

1. 全量业务语义理解
2. 动态运行时行为还原
3. 数据流和控制流的完整分析
4. 用一个工具替代所有源码阅读
5. 让服务端代替 LLM 直接完成最终解释

QuickDep 仍然是：

- 静态结构收敛器
- Agent 的前置定位层
- 与原生搜索和源码阅读互补的工具

---

## 4. 核心原则

### 4.1 场景不是永久标签，而是当前子任务阶段

同一个用户问题，通常会经历多个阶段：

1. 先定位
2. 再解释
3. 再评估影响
4. 再修改和验证

因此不应把“场景”理解为问题的唯一分类，而应理解为 Agent 当前最需要的上下文类型。

### 4.2 不能静态硬限制，也不能完全放开

在大仓库里，一刀切的固定返回策略不合理：

- 一上来全放开会导致 payload 膨胀
- 一上来硬卡死 `top 3` 会让复杂问题信息不足

正确做法是：

- 先给最小包
- 再按规则逐步扩张
- 把扩张原因显式返回

### 4.3 服务端只做“概率型收敛”，不做“绝对理解”

本设计不要求服务端 100% 读懂业务语义。

服务端需要做到的是：

- 结合问题文本、图谱形状和当前锚点
- 给出一个结构上合理的场景猜测
- 如果不确定，就明确返回低置信度和 fallback 建议

### 4.4 默认走 Hybrid

QuickDep 的目标不是禁止 Agent 读代码，而是让它在读代码前先收敛范围。

因此本设计默认服务于 Hybrid 工作流：

1. QuickDep 先缩小搜索空间
2. Agent 再读少量关键实现
3. 行为细节仍以源码确认

---

## 5. 场景模型

本期统一定义 5 类一线场景。

### 5.1 `locate`

适合问题：

- 谁在调用这个函数
- 这个接口在哪里定义
- 哪几个文件最值得先看

目标：

- 产出第一批嫌疑文件和嫌疑符号

默认返回：

- target symbol
- top callers / callees
- top related files
- same-file public neighbors

### 5.2 `behavior`

适合问题：

- 为什么这个失败会升级成 turn failure
- 某个状态是怎么传播的
- 某个时序/调度问题可能在哪条路径上发生

目标：

- 找到关键 producer / consumer / caller
- 给出最少但可解释的跨层链路

默认返回：

- target symbol
- key consumers / callers
- key edges
- limited source snippets
- suggested reads

### 5.3 `impact`

适合问题：

- 改这个函数会影响谁
- 重构这里的风险点是什么
- 哪些恢复点、状态转移点、回归点最值得关注

目标：

- 产出修改风险面和关键影响点

默认返回：

- target symbol
- top incoming dependents
- top outgoing collaborators
- related files
- risk summary
- suggested reads

### 5.4 `call_chain`

适合问题：

- 从 A 到 B 的调用链是什么
- 某个能力最终是怎么委托到另一个模块的

目标：

- 产出最短或最可信的路径
- 如果路径断裂，说明断裂点和可能原因

默认返回：

- path
- path breakpoints
- related files
- confidence

### 5.5 `watcher`

适合问题：

- 我刚改了代码，索引是否已经更新
- 新增 helper 后谁已经能看见它
- 增量更新是否捕获到了新依赖

目标：

- 产出变更是否被索引观察到的证据

默认返回：

- observed changed files
- newly discovered symbols
- newly discovered dependencies
- refresh summary

---

## 6. 新工具设计

本期建议新增一个高层 MCP 工具：

- `get_task_context`

命名理由：

- 它面向“当前任务”，而不是单个固定场景
- 它不把接口定位死在“refactoring”
- 它适合在 `mode=auto` 下做场景路由

现有底层工具全部保留，不做替代。

### 6.1 首版支持矩阵

为了避免 `auto` 被描述得比首版实现更强，首版支持范围需要明确收窄：

| 能力 | Phase A | Phase B | Phase C |
|------|---------|---------|---------|
| `locate` | 稳定支持 | 稳定支持 | 稳定支持 |
| `impact` | 稳定支持 | 稳定支持 | 稳定支持 |
| 显式 `call_chain` | 稳定支持 | 稳定支持 | 稳定支持 |
| `behavior` | 仅做弱识别和 `needs_code_read` 回退 | 稳定支持 | 稳定支持 |
| `watcher` | 不进入 `auto` 默认承诺 | 不进入 `auto` 默认承诺 | 稳定支持 |

这里“稳定支持”的含义是：

- 服务端能给出相对一致的场景判断
- 返回包主要建立在现有图能力上
- 不需要引入额外的 consumer / producer 语义层

这里“弱识别”的含义是：

- 服务端可以判断问题更像行为解释题
- 但首版不会承诺给出完整的行为上下文包
- 会优先返回收敛后的嫌疑区域和 `needs_code_read`

### 6.2 当前代码对齐状态（2026-04-24）

当前实现已经落地了 Phase A 的可用版本，但有几条必须明确：

1. `get_task_context` 已经是正式 MCP / HTTP 接口
2. 请求体已经支持：
   - `workspace`
   - `runtime`
   - `conversation`
3. 返回体已经包含：
   - `resolved_anchors`
   - `scene`
   - `confidence`
   - `coverage`
   - `status`
4. `behavior` 当前仍以 `needs_code_read` 为主回退
5. `watcher` 只在显式 `mode=watcher` 下支持，不属于 `auto` 的默认承诺
6. `next_tool_calls` 当前实现里只会返回现有底层工具模板，主要是 `batch_query`
7. `needs_expansion` 仍属于设计预留，当前实现尚未主动返回这一状态

---

## 7. 请求结构

```json
{
  "project": {
    "path": "/path/to/project"
  },
  "question": "为什么 verify_pre_dispatch 的失败会升级成 turn failure？",
  "anchor_symbols": [
    "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch"
  ],
  "anchor_files": [],
  "mode": "auto",
  "budget": "lean",
  "allow_source_snippets": true,
  "max_expansions": 1,
  "workspace": {
    "active_file": "crates/ark-runtime/src/core_flow_service.rs",
    "selection_symbol": "process_turn",
    "selection_line": 336,
    "recent_files": [
      "crates/ark-runtime-verification/src/lib.rs"
    ]
  },
  "runtime": {
    "stacktrace_symbols": [
      "verify_pre_dispatch"
    ],
    "failing_test": "turn_failure_escalates_on_pre_dispatch_rejection"
  },
  "conversation": {
    "previous_targets": [
      "verify_pre_dispatch"
    ],
    "previous_scene": "behavior"
  }
}
```

### 7.1 字段定义

| 字段 | 必填 | 说明 |
|------|------|------|
| `project` | 否 | 复用现有 `ProjectTarget` |
| `question` | 条件必填 | `mode=auto` 时必填；显式 `mode` 且锚点充分时可省略 |
| `anchor_symbols` | 否 | 已知符号锚点，支持 symbol ID、qualified name、exact name |
| `anchor_files` | 否 | 已知文件锚点，帮助收敛 |
| `mode` | 否 | `auto`、`locate`、`behavior`、`impact`、`call_chain`、`watcher` |
| `budget` | 否 | `lean`、`normal`、`wide` |
| `allow_source_snippets` | 否 | 是否允许返回少量源码片段 |
| `max_expansions` | 否 | 服务端单次自动扩张的最大轮次，默认 `1` |
| `workspace` | 否 | 编辑器侧上下文：`active_file`、`selection_symbol`、`selection_line`、`recent_files` |
| `runtime` | 否 | 运行时线索：`stacktrace_symbols`、`failing_test` |
| `conversation` | 否 | 会话线索：`previous_targets`、`previous_scene` |

### 7.2 默认值

- `mode`: `auto`
- `budget`: `lean`
- `allow_source_snippets`: `false`
- `max_expansions`: `1`

设计意图：

- 让 Agent 默认先拿最小够用包
- 对复杂问题，服务端可受控扩一层

### 7.3 `mode` 的语义

- `mode=auto`
  - 由服务端完成场景路由
  - 首版只对 `locate / impact / 显式 call_chain` 做稳定承诺
- `mode!=auto`
  - 视为客户端显式覆盖场景选择
  - 服务端不再重新改判场景
  - 但仍然要返回 `confidence`、`coverage`、`status`

这样设计的原因是：

- `auto` 适合默认 Agent 工作流
- 手动指定 `mode` 适合 benchmark、调试和高级客户端

### 7.4 锚点要求

首版实现里，锚点不是“可有可无的增强项”，而是稳定收敛的核心输入。

约束建议：

1. `mode=auto`
   - 推荐至少提供一个 `anchor_symbol` 或 `anchor_file`
2. `mode=call_chain`
   - 推荐显式提供两个 `anchor_symbols`
3. `mode=impact`
   - 推荐显式提供一个 `anchor_symbol`
4. 完全无锚点时
   - 只允许做一次受限的弱解析尝试
   - 弱解析失败后直接返回 `needs_anchor`

---

## 8. 返回结构

```json
{
  "scene": "behavior",
  "confidence": 0.74,
  "coverage": "partial",
  "status": "needs_code_read",
  "budget": {
    "requested": "lean",
    "applied": "normal",
    "expanded": true,
    "max_expansions": 1,
    "estimated_tokens": 1800,
    "truncated": false
  },
  "evidence": {
    "question_signals": [
      "question looks like behavior analysis"
    ],
    "anchor_sources": [
      "anchor_symbols",
      "runtime.stacktrace_symbols"
    ],
    "graph_signals": [
      "2 direct callers found",
      "2 direct callees found"
    ],
    "penalties": []
  },
  "resolved_anchors": {
    "symbols": [
      {
        "qualified_name": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch",
        "anchor_source": "anchor_symbols"
      }
    ],
    "files": []
  },
  "package": {
    "target": {
      "qualified_name": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch"
    },
    "primary_symbols": [
      {
        "qualified_name": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch"
      },
      {
        "qualified_name": "crates/ark-runtime-verification/src/lib.rs::VerificationService::verify_pre_dispatch"
      }
    ],
    "primary_files": [
      {
        "file_path": "crates/ark-verification/src/lib.rs"
      },
      {
        "file_path": "crates/ark-runtime-verification/src/lib.rs"
      }
    ],
    "key_edges": [
      {
        "from": "crates/ark-runtime-verification/src/lib.rs::VerificationService::verify_pre_dispatch",
        "to": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch",
        "dependency_kind": "call"
      }
    ],
    "related_files": [
      {
        "file_path": "crates/ark-runtime/src/core_flow_service.rs"
      }
    ],
    "suggested_reads": [
      {
        "kind": "symbol",
        "qualified_name": "crates/ark-runtime-verification/src/lib.rs::VerificationService::verify_pre_dispatch",
        "reason": "direct caller"
      }
    ],
    "source_snippets": [
      {
        "qualified_name": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch",
        "reason": "behavior anchor"
      }
    ],
    "risk_summary": null
  },
  "expansion_hint": "read_implementation",
  "next_tool_calls": [
    {
      "tool": "batch_query",
      "arguments": {
        "project": {
          "path": "/path/to/project"
        },
        "queries": [
          {
            "kind": "get_dependencies",
            "interface": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch",
            "direction": "incoming",
            "max_depth": 2
          },
          {
            "kind": "get_dependencies",
            "interface": "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch",
            "direction": "outgoing",
            "max_depth": 2
          }
        ]
      },
      "reason": "Expand one more hop around the anchor if you need more static structure before reading implementations."
    }
  ],
  "fallback_to_code": true,
  "note": "Static graph narrowed the likely code region, but behavior confirmation still needs source reading."
}
```

### 8.1 顶层字段定义

| 字段 | 说明 |
|------|------|
| `scene` | 服务端最终判断出的当前场景 |
| `confidence` | 当前场景和返回结果的结构置信度，范围 `0.0` 到 `1.0` |
| `coverage` | `strong`、`partial`、`minimal` |
| `status` | `ready`、`needs_expansion`、`needs_code_read`、`needs_anchor`、`insufficient_graph` |
| `budget` | 实际采用的上下文预算信息 |
| `evidence` | 场景判断证据，包含 `question_signals`、`anchor_sources`、`graph_signals`、`penalties` |
| `resolved_anchors` | 服务端最终成功解析出的符号锚点和文件锚点；这里只记录锚点，不等于 `related_files` |
| `package` | 当前最小可用上下文包 |
| `expansion_hint` | 如果不够，下一步应该扩什么 |
| `next_tool_calls` | 基于现有 QuickDep MCP 工具的下一步调用模板 |
| `fallback_to_code` | 是否建议直接读源码 |
| `note` | 面向 Agent 的简短说明 |

### 8.2 `package` 内字段定义

统一保留这些字段，但不同场景可部分为空：

| 字段 | 说明 |
|------|------|
| `target` | 目标符号或目标文件摘要 |
| `primary_symbols` | 当前最值得关注的符号列表 |
| `primary_files` | 当前最值得关注的文件列表 |
| `key_edges` | 与当前任务最相关的边 |
| `related_files` | 候选相关文件摘要 |
| `suggested_reads` | 推荐阅读顺序 |
| `source_snippets` | 可选的少量源码片段 |
| `risk_summary` | 风险等级和原因，仅对 `impact` 等场景有意义 |

### 8.3 `next_tool_calls` 契约

`next_tool_calls` 必须是严格的、客户端可直接消费的工具调用模板，而不是自由文本建议。

约束：

1. `tool` 必须来自现有 QuickDep MCP 工具集合
   - `find_interfaces`
   - `get_interface`
   - `get_dependencies`
   - `get_call_chain`
   - `get_file_interfaces`
   - `batch_query`
2. `arguments` 必须能直接映射到对应工具的现有参数
3. 不允许返回未定义的动作名，例如 `read_symbol`
4. 源码阅读不通过本字段表达
   - 源码阅读使用 `fallback_to_code` + `suggested_reads` 表达

补充约束：

5. Phase A 当前实现里，`next_tool_calls` 主要返回 `batch_query` 模板
6. 不应返回已删除的旧接口名或递归调用 `get_task_context` 自身

设计目的：

- 避免不同 Agent 客户端各自发明下一步动作协议
- 让 `get_task_context` 返回结果能直接接到现有工具面上

---

## 9. 场景识别策略

场景识别采用纯代码的规则打分器，不依赖额外模型训练。

### 9.1 第一层：问题文本信号

根据问题文本打初始分。

示例信号：

- `locate`
  - `who calls`
  - `谁调用`
  - `where defined`
  - `哪里定义`
- `behavior`
  - `why`
  - `为什么`
  - `failure`
  - `时序`
  - `调度`
  - `状态传播`
- `impact`
  - `impact`
  - `影响`
  - `refactor`
  - `风险`
  - `改了会怎样`
- `call_chain`
  - `path`
  - `调用链`
  - `from A to B`
  - `从 ... 到 ...`
- `watcher`
  - `updated`
  - `refresh`
  - `watcher`
  - `索引更新`
  - `刚改完`

### 9.2 第二层：锚点形状

根据请求中的锚点数量和类型调整分数。

规则示例：

- 两个明确 `anchor_symbols` 时，提高 `call_chain` 分数
- 一个 `anchor_symbol` 且问题包含“影响”，提高 `impact`
- 只有 `anchor_files` 且问题包含“边界”“同文件”，提高 `locate`

### 9.3 锚点解析规则

锚点解析应尽量保持保守。

规则建议：

1. `anchor_symbols` 全部为空
   - 只做一次受限的弱解析尝试
   - 仅从 `question` 中提取 identifier-like token 或显式代码标识符
   - 复用 `find_interfaces` 做最多 `5` 个候选解析
   - 如果没有唯一高置信命中，则直接返回 `needs_anchor`
2. 仅一个锚点且唯一解析成功
   - 进入正常流程
3. 多个锚点但存在歧义
   - 不进入正常场景路由
   - 返回 `needs_anchor`
   - 在 `evidence.penalties` 中记录 `ambiguous anchor resolution`
4. 锚点无法解析
   - 返回 `needs_anchor`
   - `next_tool_calls` 为空
   - `note` 明确要求客户端先补符号或文件锚点

### 9.4 第三层：图谱结构信号

根据图查询结果继续调整。

规则示例：

- direct callers / callees 很集中，利好 `locate`
- Phase B 起，如果结果涉及多个跨文件 consumer，利好 `behavior`
- incoming 多且 related files 扩散，利好 `impact`
- path 存在且长度较短，利好 `call_chain`
- 无法找到路径但两端都可解析时，降低 `call_chain` 置信度并提示 delegate gap

### 9.5 第四层：处罚项

以下情况降低整体置信度：

- anchor 解析不唯一
- 调用链断裂
- 只命中测试符号
- 大量结果来自同文件私有 helper
- 解析路径中包含已知 delegate / wrapper 缺口

### 9.6 输出原则

服务端不要求“绝对正确分类”，只要求：

1. 产出当前最像的场景
2. 返回明确的 `confidence`
3. 如果信心不足，就说明下一步该怎么扩

---

## 10. 自适应扩张策略

本设计不采用静态硬限制，而采用分层预算。

### 10.1 预算等级

#### `lean`

默认最小包：

- `primary_symbols`: `3`
- `primary_files`: `3`
- `related_files`: `3`
- `source_snippets`: `0`

适用：

- 首次定位
- 大仓库默认入口

#### `normal`

中等扩张：

- `primary_symbols`: `5`
- `primary_files`: `5`
- `related_files`: `5`
- `source_snippets`: `1` 到 `2`

适用：

- `confidence` 偏低
- 问题明显是 `impact`
- 显式 `mode=call_chain` 且首轮路径不足以形成 `ready`

#### `wide`

保守上限：

- `primary_symbols`: `8`
- `primary_files`: `8`
- `related_files`: `8`
- `source_snippets`: `2` 到 `4`

适用：

- 复杂失败传播
- 风险面梳理
- 用户明确要求更完整覆盖

### 10.2 扩张触发条件

满足任一条件即可从 `lean` 升到 `normal`：

1. `confidence < 0.65`
2. `coverage == "minimal"`
3. `scene == impact`
4. 关键路径缺失但存在明显候选分叉

满足任一条件即可从 `normal` 升到 `wide`：

1. `confidence < 0.5`
2. 用户显式请求完整影响范围
3. `impact` 场景需要风险点而非仅文件列表
4. 当前场景已经是显式 `mode=call_chain` 且路径候选不止一条

### 10.3 停止扩张条件

满足任一条件后停止扩张：

1. 达到 `max_expansions`
2. 已经足够形成 `ready`
3. 图谱信息不足，继续扩张只会返回噪音
4. 继续扩张的收益低于直接读源码

---

## 11. 排名策略

上下文不是“找全”，而是“排对顺序”。

### 11.1 统一排名信号

首版只能依赖当前图谱和摘要层已经稳定暴露出来的字段。

每个候选符号或文件可以按下列维度累加：

1. 与 target 同文件
2. direct caller / direct callee
3. 同时出现在多条证据来源中
4. public / exported
5. 本地项目内定义
6. 函数/方法优先于类型声明

### 11.2 降权规则

以下规则中，只有当前可直接判断的信号才能进入 Phase A：

1. 私有 helper 长列表
2. 仅类型引用但非关键调用关系
3. 外部库符号且无法继续解析实现

以下规则推迟到 Phase B / Phase C：

1. 测试函数和 `#[cfg(test)]` 模块的稳定过滤
2. consumer / producer 语义加权
3. 状态转移点、恢复点等高阶语义信号

### 11.3 场景特化

不同场景在统一排名之上加权：

- `locate`
  - 更重 direct caller / callee
- `behavior`
  - 首版不承诺 consumer / producer 排名
  - 仅返回收敛后的候选文件和候选符号，并优先给 `needs_code_read`
- `impact`
  - 更重 incoming dependents / public boundary
- `call_chain`
  - 更重路径中间节点和断裂点
- `watcher`
  - 更重最近变化文件和新增依赖边

---

## 12. 与现有工具的关系

### 12.1 不替代底层工具

以下工具保留原样：

- `get_interface`
- `get_dependencies`
- `get_call_chain`
- `get_file_interfaces`
- `batch_query`

原因：

- 它们仍然适合手工调试和精细查询
- 新工具应建立在这些能力之上

### 12.2 与旧 `get_refactoring_context` 的关系

旧的 `get_refactoring_context` 已经从代码中删除。

原因不是功能目标消失，而是它存在两个根本问题：

1. 语义过窄，只服务“重构影响分析”
2. 默认聚合结果容易重新膨胀成胖包

当前替代关系应理解为：

- `get_task_context`
  - 面向 Agent 总入口的场景路由工具
- 底层工具组合
  - 面向精细补查和调试

也就是说：

- 旧 `get_refactoring_context` 承担的“专用打包”角色，已经拆分到
  - `get_task_context`
  - `batch_query`
  - `get_dependencies`
  - `get_file_interfaces`

这更符合“先收敛，再补查”的 Hybrid 工作流。

### 12.3 HTTP 暴露

当前已经暴露：

- `POST /api/task-context`

便于 CLI、Web UI 和非 MCP 客户端共用。

---

## 13. 实现方案

### 13.1 新增数据结构

当前代码已经存在：

- `TaskContextRequest`
- `TaskContextWorkspace`
- `TaskContextRuntime`
- `TaskContextConversation`
- `TaskContextMode`
- `TaskContextBudget`

仍可在后续版本继续抽象为强类型结构：

- `TaskContextScene`
- `TaskContextStatus`
- `TaskContextEvidence`
- `TaskContextResponse`

### 13.2 服务端结构

建议把实现拆成 3 层：

1. `scene_router`
   - 负责文本信号、锚点信号和图谱信号打分
2. `context_packager`
   - 负责不同场景的最小上下文打包
3. `budget_controller`
   - 负责 `lean -> normal -> wide` 扩张和裁剪

### 13.3 复用现有代码

优先复用现有能力：

- 符号解析逻辑
- `direct_dependency_entries`
- `related_files_value`
- `suggested_reads_value`
- `source_snippets_for_symbols`
- `estimate_value_tokens`

设计要求：

- 先复用，再抽象
- 第一版不做过度重构

---

## 14. 结果状态定义

### 14.1 `ready`

表示：

- 当前返回足够让 Agent 进入下一步阅读或分析

### 14.2 `needs_expansion`

表示：

- 当前已经收敛到某个区域，但还不足以支持当前问题
- 仍建议优先扩图谱，而不是马上盲读大量代码

当前状态：

- 这是设计预留状态
- 当前代码实现尚未主动返回 `needs_expansion`
- Phase A 主要返回：
  - `ready`
  - `needs_code_read`
  - `needs_anchor`
  - `insufficient_graph`

约束：

- 只应在锚点已经唯一解析成功之后出现
- 不应用于表达“缺少锚点”或“锚点歧义”

### 14.3 `needs_code_read`

表示：

- 图谱已经成功收敛范围
- 但行为解释依赖实现细节

首版里这也是 `behavior` 场景最常见的安全回退状态。

### 14.4 `needs_anchor`

表示：

- 当前无法稳定确定目标符号或目标文件
- 继续做图谱扩张的收益低于先补锚点

常见原因：

- 完全无锚点且弱解析失败
- 候选命中太多
- 多个 anchor 之间存在歧义

### 14.5 `insufficient_graph`

表示：

- 当前静态图不足以稳定支持这类问题
- 常见原因包括 delegate gap、动态分派、外部库不可见

---

## 15. 验证指标

这个设计是否“更好用”，不能只看响应速度。

应重点看 Agent 工作流指标。

### 15.1 主指标

1. `time_to_first_gold`
   - 第一次命中正确文件或符号的耗时
2. `fan_out_before_first_hit`
   - 首次命中前显式触达的文件数
3. `raw_source_chars_before_first_hit`
   - 首次命中前读取的源码字符数
4. `redundant_tool_calls`
   - 是否出现明显重复补查
5. `fallback_rate_to_native`
   - QuickDep 返回后仍需大范围 `rg/cat` 的概率
6. `answer_completeness`
   - 最终答案对关键路径、风险点或消费者的覆盖程度

### 15.2 次指标

1. `scene_match_rate`
   - `auto` 路由是否与人工标注场景一致
2. `expansion_effectiveness`
   - 扩张是否真的提升了 completeness
3. `payload_per_success`
   - 每次成功定位的平均 MCP payload

---

## 16. 回归场景

下一轮回归至少覆盖：

1. 单符号定位
2. 大文件边界理解
3. 失败传播
4. 跨 crate 风险分析
5. 调用链追踪
6. watcher / 增量更新

并且每个场景都要同时比较：

1. `mode=auto`
2. 人工指定 `mode`
3. `get_task_context-first`
4. 底层工具组合
5. `get_task_context + native` hybrid

---

## 17. 风险与限制

### 17.1 纯代码场景识别不是语义理解

它只能做到：

- 结构合理
- 规则一致
- 可解释

不能做到：

- 绝对理解业务意图

### 17.2 解析缺边会直接影响上层判断

如果底层调用图漏掉：

- `self.field.method()`
- wrapper / delegate 路径
- 动态分派消费点

那么上层场景路由和包生成也会降质。

### 17.3 不应掩盖“不知道”

服务端必须允许自己说：

- `confidence` 低
- `coverage` 不够
- 应回退源码阅读

这比返回一个看似完整但方向错误的胖包更重要。

---

## 18. 分阶段实施建议

### Phase A

实现最小可用版：

1. `get_task_context`
2. `mode=auto`
3. `locate / impact / 显式 call_chain` 三类先行
4. `confidence / coverage / status / expansion_hint`
5. `lean / normal` 两级预算

额外约束：

1. `behavior` 在 Phase A 只做弱识别
2. 对行为题优先返回嫌疑区域 + `needs_code_read`
3. `watcher` 不纳入 `auto` 的首版承诺
4. 无锚点时只做一次受限弱解析，失败即 `needs_anchor`

### Phase B

增强行为问题支持：

1. `behavior` 场景
2. consumer / producer 聚合
3. limited source snippets
4. `needs_code_read` 路由

### Phase C

增强 watcher 和交互式收敛：

1. `watcher` 场景
2. 更稳定的自动扩张
3. benchmark regression 固化

---

## 19. 结论

这轮设计的核心不是把 QuickDep 变成“一个回答所有问题的超级工具”，而是把它变成：

> 一个能识别当前子任务阶段、先给最小可用上下文、再明确告诉 Agent 下一步该怎么做的结构收敛器。

这比继续增加更多底层查询更贴近当前真实问题，也更符合 QuickDep 在大型项目中的实际价值：

1. 帮 Agent 更快进入正确代码区域
2. 减少盲目源码阅读和工具编排
3. 在该读代码时明确提示，而不是假装图谱已经足够

如果这套设计落地成功，QuickDep 的故事将从“静态依赖查询工具”进一步升级为：

> 面向大型仓库 Agent 工作流的场景化上下文路由层。
