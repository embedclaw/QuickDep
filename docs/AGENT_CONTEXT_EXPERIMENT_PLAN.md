# QuickDep Agent Context 实验计划

## 1. 目标

这轮实验不再把“省 token”作为唯一目标。

主问题改为：

> 在大型真实仓库里，`get_task_context` 是否能让 agent 更快进入正确代码区域，并减少盲目搜索与误判？

需要验证的不是某一个接口响应快不快，而是整个 agent 工作流是否更稳：

1. 更快命中正确文件或符号
2. 在命中前阅读更少源码
3. 更少重复补查
4. 在图谱不足时更早、且更诚实地回退到源码阅读

---

## 2. 核心假设

### H1：更快定位

相较原生 `rg/sed/cat` 流程，`get_task_context-first` 或 `task_context + native hybrid` 应该降低：

- `time_to_first_gold`
- `fan_out_before_first_hit`
- `raw_source_chars_before_first_hit`

### H2：更少无效补查

相较 QuickDep 低层工具自由组合，`get_task_context-first` 应该降低：

- `redundant_tool_calls`
- `payload_per_success`

### H3：行为题更诚实

在 `behavior` 场景里，`get_task_context` 应更早返回：

- `scene = behavior`
- `status = needs_code_read`

而不是给出看似完整、但其实不可靠的静态结论。

### H4：无锚点时尽快止损

在无有效锚点的真实提问里，`get_task_context` 应该快速返回：

- `status = needs_anchor`

而不是带着 agent 大范围乱搜。

---

## 3. 实验对象

### 3.1 主战场：`ark-runtime`

- 仓库路径：`/Users/luozx/work/ark-runtime`
- 用途：验证“大型 Rust 多 crate 工程”下的真实工作流收益
- 原因：
  - 代码量足够大
  - 存在跨 crate 委托链
  - 同时覆盖 `impact`、`behavior`、`call_chain`、大文件边界理解

### 3.2 次级正确性场景

以下场景不用于讲“大仓库效率故事”，而用于补齐功能边界：

- watcher / 增量更新
- 无锚点止损
- 跨语言 sanity check

这些场景应使用更可控的 fixture 或中型仓库，而不是强行塞进 `ark-runtime` 主 benchmark。

---

## 4. 对比组

每个主场景固定跑 4 条路线，且一次最多并发 4 个任务。

### A. Native Only

约束：

- 禁用 QuickDep MCP
- 仅允许原生工具
  - `rg`
  - `sed`
  - `cat`
  - 必要时 `find`

用途：

- 作为真实 agent 默认工作流基线

### B. QuickDep Low-Level

约束：

- 允许 QuickDep MCP
- 禁止 `get_task_context`
- 只允许：
  - `find_interfaces`
  - `get_interface`
  - `get_dependencies`
  - `get_call_chain`
  - `get_file_interfaces`
  - `batch_query`

用途：

- 测“只有图谱、没有路由层”时的 agent 真实表现

### C. TaskContext-First

约束：

1. 第一跳必须调用 `get_task_context`
2. 在 `status != needs_code_read && status != insufficient_graph` 前，不允许直接读源码
3. 若 `next_tool_calls` 非空，只允许优先使用其给出的模板
4. 不允许绕过 `resolved_anchors` 直接盲搜

用途：

- 测 `get_task_context` 作为纯路由入口时的价值

### D. Hybrid Recommended

约束：

1. 第一跳必须调用 `get_task_context`
2. 若返回 `needs_code_read`，允许最小量源码阅读
3. 若返回 `needs_anchor`，允许补锚点或明确请求更多上下文
4. 允许在 QuickDep 缩小范围后配合原生工具做少量确认

用途：

- 这是最接近真实产品故事的推荐工作流

---

## 5. 统一实验协议

### 5.1 运行前置条件

所有路线都必须满足：

1. 使用同一模型版本
2. 使用全新会话，不复用记忆
3. 使用同一份仓库和同一 commit
4. QuickDep 索引提前扫描完成
5. 每轮实验前记录仓库路径、commit、QuickDep 扫描统计

### 5.2 运行约束

1. 同一场景的 4 条路线尽量在相近时间窗口内运行
2. 一次最多并发 4 个 agent 任务
3. 每个场景至少跑 3 轮
4. 每轮都保存完整 transcript 和工具日志
5. 同一场景的原始证据必须等价暴露给所有路线

这里的“等价暴露”指：

- 如果场景定义了 `workspace` / `runtime` / `conversation` 线索
- A/B 路线也必须在提示词附录里看到同样的信息
- C/D 路线除了看到这些信息，还允许把它们原样传给 `get_task_context`

否则测到的就不只是“路由能力”，还混入了“哪条路线拿到了更多先验上下文”。

### 5.3 建议落盘目录

```text
/tmp/quickdep-experiments-v2/
  ark-runtime/
    s1-impact/
      native-run1.jsonl
      lowlevel-run1.jsonl
      task-context-run1.jsonl
      hybrid-run1.jsonl
    s2-behavior/
    s3-large-file/
    s4-call-chain/
    s5-editor-context/
    s6-no-anchor/
  watcher-fixture/
    s7-watcher/
```

### 5.4 标准化 Agent 提示模板

为了让 4 条路线可复现，建议固定使用同一套任务提示，只替换“允许工具”和“路线约束”。

公共任务模板：

```text
你正在评估一个大型仓库问题定位任务。

仓库：{repo_path}
场景 ID：{scenario_id}
用户问题：{user_question}
附加证据：{extra_context}
Gold 不对你公开，你只能依据工具结果作答。

执行要求：
1. 严格遵守本轮允许工具和路线约束。
2. 每次读源码前，先判断是否已经拿到足够锚点。
3. 结束时输出：
   - final_answer
   - files_touched
   - symbols_touched
   - why_these_files
   - confidence
4. 不要为了显得完整而猜测行为细节；不确定时明确说明。
```

Route A: Native Only

```text
允许工具：rg、sed、cat、find。
禁止 QuickDep MCP。
目标：仅用原生工具回答问题。
```

Route B: QuickDep Low-Level

```text
允许工具：find_interfaces、get_interface、get_dependencies、get_call_chain、get_file_interfaces、batch_query。
禁止 get_task_context。
禁止直接读源码，除非你已经无法继续并在答案中明确说明为什么失败。
目标：只用底层图谱工具回答问题。
```

Route C: TaskContext-First

```text
第一跳必须调用 get_task_context。
在 status 不是 needs_code_read 或 insufficient_graph 之前，不允许直接读源码。
若 next_tool_calls 非空，优先使用返回模板，不允许跳过 resolved_anchors 去盲搜。
目标：尽量只依靠 get_task_context 及其推荐动作完成定位。
```

Route D: Hybrid Recommended

```text
第一跳必须调用 get_task_context。
如果返回 needs_code_read，可以读取最少量源码验证行为。
如果返回 needs_anchor，可以补锚点或明确说明还缺什么。
允许 QuickDep 与少量原生工具协作，但必须先用 QuickDep 收敛范围。
目标：模拟真实推荐工作流。
```

`extra_context` 的建议格式：

```text
workspace:
- active_file: crates/ark-verification/src/lib.rs
- selection_symbol: verify_pre_dispatch
- selection_line: 70

runtime:
- stacktrace_symbols: verify_pre_dispatch

conversation:
- previous_scene: behavior
```

如果某场景没有这些线索，显式写 `extra_context: none`，避免不同路线因为提示词长度差异引入额外偏差。

### 5.5 统一记录模板

每轮实验建议至少记录以下字段，便于后续统计：

```json
{
  "scenario_id": "s2-behavior",
  "route": "hybrid",
  "run_id": 1,
  "repo_path": "/Users/luozx/work/ark-runtime",
  "repo_commit": "<git-sha>",
  "time_to_first_gold_ms": 0,
  "fan_out_before_first_hit": 0,
  "raw_source_chars_before_first_hit": 0,
  "redundant_tool_calls": 0,
  "fallback_rate_to_native": false,
  "answer_completeness": 0.0,
  "wrong_confident_answer": false,
  "scene_match": true,
  "status_quality": true,
  "anchor_resolution_success": true,
  "notes": []
}
```

`answer_completeness` 建议采用 `0.0 / 0.5 / 1.0` 三档：

- `0.0`：未覆盖 minimum complete answer
- `0.5`：命中部分 gold，但遗漏关键链路或关键文件
- `1.0`：覆盖 minimum complete answer

如果需要保留人工审计痕迹，可再附一份 markdown 记录表：

| route | first_gold | files_before_hit | source_chars_before_hit | completeness | wrong_confident | note |
|------|------|------|------|------|------|------|
| hybrid | `core_flow_service.rs` | `2` | `640` | `1.0` | `false` | 首跳返回 `needs_code_read`，随后只读一个实现文件 |

---

## 6. 指标

### 6.1 主指标

| 指标 | 含义 |
|------|------|
| `time_to_first_gold` | 首次命中 gold 文件或 gold 符号的耗时 |
| `fan_out_before_first_hit` | 首次命中前显式触达的文件数 |
| `raw_source_chars_before_first_hit` | 首次命中前读取的源码字符数 |
| `redundant_tool_calls` | 明显重复、低收益的工具调用数 |
| `fallback_rate_to_native` | QuickDep 路线最终仍需大范围原生搜索的概率 |
| `answer_completeness` | 最终答案对 gold 关键点的覆盖率 |
| `wrong_confident_answer_rate` | 明显错误但语气肯定的答案比例 |

### 6.2 次指标

| 指标 | 含义 |
|------|------|
| `scene_match_rate` | `get_task_context(mode=auto)` 与人工场景标注是否一致 |
| `status_quality` | `ready / needs_code_read / needs_anchor / insufficient_graph` 是否符合场景预期 |
| `anchor_resolution_success` | `resolved_anchors` 是否解析到正确目标 |
| `payload_per_success` | 成功命中前累计的 QuickDep payload 规模 |

---

## 7. Gold 判定方法

### 7.1 Gold 的定义

每个场景预先人工标注：

1. `gold_files`
2. `gold_symbols`
3. `gold_path`（若场景是调用链）
4. `minimum_complete_answer`

### 7.2 首次命中的判定

满足任一条件即可算 `first_gold`：

1. agent 显式读取了 `gold_files` 中的文件
2. agent 明确提到了 `gold_symbols` 中的符号
3. `get_task_context.package.target / primary_symbols / related_files` 已经覆盖 gold

### 7.3 硬失败规则

以下情况直接记为硬失败：

1. `call_chain` 场景返回错误的空路径且语气肯定
2. `behavior` 场景在未读实现前给出确定性行为结论
3. 无锚点场景未止损，进入大范围盲搜
4. 大文件边界场景在命中前读取了与问题无关的大量文件

---

## 8. 主实验矩阵

## S1：显式锚点的影响分析

用户问题：

> 我要改 `VerificationEngine::verify_pre_dispatch`，先帮我评估影响范围，哪些入口和文件必须先看？

Task Context 输入：

```json
{
  "question": "我要改 verify_pre_dispatch，先帮我评估影响范围，哪些入口和文件必须先看？",
  "anchor_symbols": [
    "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch"
  ],
  "mode": "auto",
  "budget": "normal"
}
```

Gold：

- `gold_files`
  - `crates/ark-verification/src/lib.rs`
  - `crates/ark-runtime-verification/src/lib.rs`
  - `crates/ark-runtime/src/core_flow_service.rs`
- `gold_symbols`
  - `VerificationEngine::verify_pre_dispatch`
  - `VerificationService::verify_pre_dispatch`
  - `RuntimeFlowService::process_turn`

通过标准：

1. 命中 target 文件
2. 最终答案至少覆盖 3 个 gold 文件中的 2 个
3. TaskContext-First / Hybrid 的 `fan_out_before_first_hit` 不高于 Native

## S2：带运行时线索的行为题

用户问题：

> 为什么 `verify_pre_dispatch` 失败会升级成 turn failure？

Task Context 输入：

```json
{
  "question": "为什么 verify_pre_dispatch 失败会升级成 turn failure？",
  "anchor_symbols": [
    "crates/ark-verification/src/lib.rs::VerificationEngine::verify_pre_dispatch"
  ],
  "mode": "auto",
  "budget": "lean",
  "allow_source_snippets": true,
  "runtime": {
    "stacktrace_symbols": [
      "verify_pre_dispatch"
    ]
  }
}
```

Gold：

- `gold_files`
  - `crates/ark-verification/src/lib.rs`
  - `crates/ark-runtime-verification/src/lib.rs`
  - `crates/ark-runtime/src/core_flow_service.rs`
- `gold_symbols`
  - `VerificationService::verify_pre_dispatch`
  - `RuntimeFlowService::process_turn`

通过标准：

1. `get_task_context` 首跳返回 `scene = behavior`
2. `get_task_context` 首跳返回 `status = needs_code_read`
3. `package.primary_files` 或 `related_files` 命中 `crates/ark-runtime/src/core_flow_service.rs`
4. 最终解释必须明确“失败分支在 `core_flow_service` 中进入 `apply_turn_failure` 路径”

## S3：大文件边界理解

用户问题：

> 我需要理解 `PlatformServer::health_report` 的模块边界，先看哪几个局部函数和入口？

Task Context 输入：

```json
{
  "question": "我需要理解 PlatformServer::health_report 的模块边界，先看哪几个局部函数和入口？",
  "anchor_symbols": [
    "crates/ark-platform-server/src/lib.rs::PlatformServer::health_report"
  ],
  "mode": "auto",
  "budget": "normal"
}
```

Gold：

- `gold_files`
  - `crates/ark-platform-server/src/lib.rs`
- `gold_symbols`
  - `PlatformServer::health_report`
  - `worker_health_projection`
  - `http_health`
  - `http_readiness`

通过标准：

1. 首次命中应落在同一文件
2. 最终答案至少覆盖 4 个 gold 符号中的 3 个
3. TaskContext-First / Hybrid 在命中前不应读取 2 个以上源码文件

## S4：显式调用链

用户问题：

> 从 `RuntimeCore::next_conflict_queue_head` 到 `Scheduler::dispatchable_head` 的静态调用链是什么？

Task Context 输入：

```json
{
  "question": "从 RuntimeCore::next_conflict_queue_head 到 Scheduler::dispatchable_head 的静态调用链是什么？",
  "anchor_symbols": [
    "crates/ark-runtime/src/flow.rs::RuntimeCore::next_conflict_queue_head",
    "crates/ark-scheduler/src/lib.rs::Scheduler::dispatchable_head"
  ],
  "mode": "call_chain",
  "budget": "normal"
}
```

Gold：

- `gold_path`
  1. `crates/ark-runtime/src/flow.rs::RuntimeCore::next_conflict_queue_head`
  2. `crates/ark-execution/src/lib.rs::ExecutionService::next_conflict_queue_head`
  3. `crates/ark-scheduler/src/lib.rs::Scheduler::dispatchable_head`

通过标准：

1. 返回完整 3 跳路径，或明确返回 `insufficient_graph`
2. 返回错误空路径但语气肯定，记为硬失败
3. Hybrid 若回退源码阅读，必须先用 `get_task_context` 或 `get_call_chain` 尝试一次

## S5：用户不说符号，但编辑器有上下文

用户问题：

> 这个改起来风险大吗？

Task Context 输入：

```json
{
  "question": "这个改起来风险大吗？",
  "mode": "auto",
  "workspace": {
    "active_file": "crates/ark-verification/src/lib.rs",
    "selection_symbol": "verify_pre_dispatch",
    "selection_line": 70
  }
}
```

Gold：

- 与 S1 相同

通过标准：

1. 不应返回 `needs_anchor`
2. `resolved_anchors.symbols[0].anchor_source` 应来自 workspace 证据
3. 路由应落在 `impact`

## S6：真实无锚点提问

用户问题：

> 这个失败是哪里传上来的？

Task Context 输入：

```json
{
  "question": "这个失败是哪里传上来的？",
  "mode": "auto"
}
```

Gold：

- 首跳应止损，而不是试图“猜对业务”

通过标准：

1. `get_task_context` 在 1 次调用内返回 `needs_anchor`
2. 在 `needs_anchor` 之前不应读取源码文件
3. 若 agent 继续盲搜，记为路线失败

---

## 9. watcher / 增量更新专项

这个专项不建议放在 `ark-runtime` 主 benchmark 里，因为大仓库实时改写噪音太高。

建议使用独立 fixture：

1. 扫描一个小型 Rust fixture
2. 修改 `src/lib.rs`
3. 新增函数和调用边
4. 用 `mode=watcher` + `anchor_files` 查询

Gold：

- `package.primary_files` 命中变更文件
- `package.primary_symbols` 命中新符号
- 索引状态不应停留在旧版本

---

## 10. 次级跨语言 sanity check

这些不是主 benchmark，但用于确认 `get_task_context` 不只在 Rust 仓库讲故事：

| 语言 | 仓库 | 代表场景 |
|------|------|----------|
| TypeScript | `nest` | `locate` / `impact` |
| Python | `requests` | `impact` |
| Go | `gin` | `impact` |
| Rust | `tokio` | `call_chain` |
| Rust | `ripgrep` | `locate` |
| C++ | `fmt` | `locate` |
| C | `redis` | `impact` |

最低要求：

1. 场景路由不离谱
2. `resolved_anchors` 能落到正确符号
3. `related_files` 不出现明显无关噪音爆炸

---

## 11. 通过标准

如果要支持“QuickDep 能帮助 agent 在大型项目里更快定位问题”这个故事，建议至少满足以下条件：

### P1：主故事通过

在 `S1 + S3 + S5` 上，`Hybrid Recommended` 相比 `Native Only`：

1. `time_to_first_gold` 中位数下降至少 `25%`
2. `fan_out_before_first_hit` 中位数不高于 native，目标下降 `30%`
3. `raw_source_chars_before_first_hit` 中位数不高于 native

### P2：行为题不乱承诺

在 `S2` 上：

1. `scene = behavior` 命中率至少 `80%`
2. `status = needs_code_read` 命中率至少 `80%`
3. `wrong_confident_answer_rate = 0`

### P3：显式调用链不允许错而不知

在 `S4` 上：

1. 返回正确路径，或明确 `insufficient_graph`
2. “空路径但确信不存在”记为不通过

### P4：无锚点场景要止损

在 `S6` 上：

1. `needs_anchor` 一跳命中率至少 `90%`
2. 不允许出现显著大范围盲搜

如果以上条件不满足，就说明：

- `get_task_context` 还可以用
- 但还不足以支撑“更快定位问题”的主故事

---

## 12. 建议的执行顺序

1. 先跑 `ark-runtime` 的 `S1-S6`
2. 汇总 4 条路线在 6 个主场景的中位数
3. 再补 `watcher` 专项
4. 最后做跨语言 sanity check

原因：

- 主 benchmark 先回答核心故事
- watcher 和跨语言更多是边界验证

---

## 13. 建议输出物

每轮实验结束后至少产出：

1. 原始 transcript / tool logs
2. 每场景一张结果表
3. 一份失败样例分析
4. 一份“下一轮要修什么”的问题清单

建议最终写成：

- `docs/ARK_RUNTIME_AGENT_COMPARISON_V2.md`
- `docs/AGENT_CONTEXT_FAILURE_ANALYSIS.md`

---

## 14. 结论

这份计划的目的不是把实验做成“接口压测”，而是把它做成一套能真实回答下面这个问题的工作流 benchmark：

> 当用户在大仓库里提出一个并不完美的问题时，QuickDep 是否能帮 agent 更快收敛到正确代码区域？

如果实验要服务这个问题，就必须同时比较：

1. Native
2. QuickDep 低层工具
3. `get_task_context-first`
4. `task_context + native` hybrid

只有这样，实验结果才足够说明产品价值，而不是只说明“某个工具调用看起来挺结构化”。
