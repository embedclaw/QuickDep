# QuickDep on `ark-runtime`: Claude 实验对比 V2

> 状态说明：这是历史实验记录。
> 当前如果要引用最新结果，请优先看 [AGENT_HYBRID_BENCHMARK_REPORT.md](AGENT_HYBRID_BENCHMARK_REPORT.md) 和 [ARK_RUNTIME_AGENT_COMPARISON_V3.md](ARK_RUNTIME_AGENT_COMPARISON_V3.md)。
> V2 保留的价值主要是展示当时暴露过哪些问题，以及后续修复前的基线。

## 1. 结论摘要

这轮实验基本支撑了一个更准确的产品故事：

> QuickDep 的价值不在于“替代源码阅读”，而在于帮助 agent 在大型仓库里更快进入正确代码区域，并在该止损时及时止损。

更具体地说：

1. `get_task_context` 对 `impact`、`large-file boundary`、`no-anchor stop-loss` 这三类场景帮助非常明显。
2. `behavior` 和 `call-chain` 场景里，最有效的路线不是“纯图”，而是 `get_task_context + 少量源码阅读` 的 hybrid。
3. 单独依赖 QuickDep 低层工具时，Claude 容易在静态图里越查越散，或者被图缺边误导。
4. 原生工具最终也能得到答案，但通常会触达更多文件、读更多实现、并更容易偏到无关支线。

因此，当前版本最适合讲的不是“全自动依赖打包”，而是：

> `task_context -> 收敛范围 -> 最小量读码 -> 完成解释`

---

## 2. 实验设置

- 仓库：`/Users/luozx/work/ark-runtime`
- 仓库 commit：`44754e0`
- 实验日期：`2026-04-24`
- 执行入口：`claude` CLI
- Claude CLI 日志里实际模型标识：`glm-5`
- 原始日志目录：`/tmp/quickdep-experiments-v2/ark-runtime/`

比较路线：

1. `Native Only`
2. `QuickDep Low-Level`
3. `TaskContext-First`
4. `Hybrid Recommended`

说明：

1. `S4` 初次运行出现过 Claude API `terminated`，后续使用 `run2` 重跑。
2. `S2` 和 `S5` 的 low-level / native 部分路线出现明显 wandering，保留了原始失败样例，同时增加了带止损规则的 `run1b`。
3. 这轮先完成了主实验矩阵 `S1-S6` 的单轮可比版本，还没有做每场景 3 次重复取中位数。

---

## 3. 总体判断

| 场景 | 最优路线 | 结论 |
|------|------|------|
| `S1 impact` | `Hybrid` | `task_context` 先收敛，再读 3 个关键文件，答案最完整 |
| `S2 behavior` | `Hybrid` | `task_context` 先诚实停在 `needs_code_read`，hybrid 再补出真实失败传播链 |
| `S3 large-file` | `TaskContext` / `Hybrid` | 几乎不需要读码就能收敛到同文件局部函数和入口 |
| `S4 call_chain` | `Native` / `Hybrid` | 当前图存在缺边，`task_context` 和 `low-level` 会报 `insufficient_graph` 或假阴性 |
| `S5 editor-context` | `TaskContext` / `Hybrid` | 模糊问题配合编辑器上下文时，`task_context` 价值非常直接 |
| `S6 no-anchor` | `TaskContext` / `Hybrid` | 明确止损，避免大范围盲搜 |

一句话总结：

- `TaskContext-First` 是最好的“前置定位层”。
- `Hybrid` 是当前版本最好的真实工作流。
- `Low-Level` 适合调试，不适合作为 Claude 的默认入口。

---

## 4. 场景详情

### S1：显式锚点的影响分析

问题：

> 我要改 `VerificationEngine::verify_pre_dispatch`，先帮我评估影响范围，哪些入口和文件必须先看？

代表结果：

| 路线 | 触达文件数 | 源码读取数 | 结果质量 |
|------|------|------|------|
| `Native` | `11` | `8` | 完整，但明显过读 |
| `Low-Level` | `7` | `4` | 找到目标和部分上下游，但 incoming caller 不完整 |
| `TaskContext-First` | `1` | `0` | 收敛极快，但覆盖不足，只给出同文件风险面 |
| `Hybrid` | `3` | `3` | 最平衡，命中 `VerificationService::verify_pre_dispatch` 和 `RuntimeFlowService::process_turn` |

判断：

1. `TaskContext-First` 证明了“先收窄代码区域”这件事是成立的。
2. 但只靠当前 `task_context` 包，还不足以直接回答完整影响范围。
3. `Hybrid` 才是真正可用的路线，因为它把读码限制在 3 个高价值文件内。

### S2：带运行时线索的行为题

问题：

> 为什么 `verify_pre_dispatch` 失败会升级成 turn failure？

代表结果：

| 路线 | 触达文件数 | 源码读取数 | 结果质量 |
|------|------|------|------|
| `Native` | `6` | `4` | 找到真实链路：`process_turn -> apply_turn_failure -> fail_turn_context` |
| `Low-Level` | `5` | `0` | 原始版本严重 wandering；止损版只能诚实说“静态图不足，需要读源码” |
| `TaskContext-First` | `1` | `0` | 正确停在“需要读上层实现” |
| `Hybrid` | `6` | `6` | 补齐完整链路，且没有明显误判 |

判断：

1. 这是当前版本最能说明产品边界的一组实验。
2. `TaskContext-First` 的价值不是直接回答，而是尽快把 Claude 推到 `needs_code_read`。
3. `Low-Level` 在这类题上最不稳定，因为静态图缺边时会诱导 Claude 追很多“看起来相关”的失败路径。

### S3：大文件边界理解

问题：

> 我需要理解 `PlatformServer::health_report` 的模块边界，先看哪几个局部函数和入口？

代表结果：

| 路线 | 触达文件数 | 源码读取数 | 结果质量 |
|------|------|------|------|
| `Native` | `2` | `2` | 能答，但还是额外读了 `worker_registry.rs` |
| `Low-Level` | `1` | `0` | 已经能靠文件接口和局部依赖回答 |
| `TaskContext-First` | `1` | `0` | 直接给出 `health_report + worker_health_projection + PlatformHealthStatus::Ok` |
| `Hybrid` | `0` | `0` | 直接基于 `task_context` 收口，无需读码 |

判断：

1. 这是当前版本对 QuickDep 最友好的场景。
2. `TaskContext-First` 和 `Hybrid` 基本达到了“先给 agent 一个正确的局部阅读顺序”的目标。

### S4：显式调用链

问题：

> 从 `RuntimeCore::next_conflict_queue_head` 到 `Scheduler::dispatchable_head` 的静态调用链是什么？

代表结果：

| 路线 | 触达文件数 | 源码读取数 | 结果质量 |
|------|------|------|------|
| `Native` | `0` | `3` | 找到真实 3 跳链路 |
| `Low-Level` | `2` | `0` | 给出“静态图中不存在调用链”的假阴性 |
| `TaskContext-First` | `0` | `0` | 正确返回 `insufficient_graph` |
| `Hybrid` | `0` | `3` | 读很少源码后补出真实 3 跳链路 |

真实链路：

1. `RuntimeCore::next_conflict_queue_head`
2. `ExecutionService::next_conflict_queue_head`
3. `Scheduler::dispatchable_head`

判断：

1. 这组说明当前图谱仍有真实缺边。
2. `TaskContext-First` 在这里的价值是“错而不知”变成“知道自己图不够”。
3. `Low-Level` 反而更危险，因为它会把“图里没有”误表述成“静态上不存在”。

### S5：用户不说符号，但编辑器有上下文

问题：

> 这个改起来风险大吗？

附加上下文：

- `active_file=crates/ark-verification/src/lib.rs`
- `selection_symbol=verify_pre_dispatch`
- `selection_line=70`

代表结果：

| 路线 | 触达文件数 | 源码读取数 | 结果质量 |
|------|------|------|------|
| `Native` | `2` | `2` | 止损版能给出“中等偏低”判断 |
| `Low-Level` | `0` | `0` | 倾向低风险，但明显受图缺边影响 |
| `TaskContext-First` | `1` | `0` | 直接给出 `impact/low-risk` 结论 |
| `Hybrid` | `0` | `0` | 与 `task_context` 几乎一致，直接收口 |

判断：

1. 这组很接近真实 IDE 场景。
2. `workspace` 线索可以显著降低用户必须“问对问题”的负担。
3. 这也是最适合对外演示的场景之一。

### S6：真实无锚点提问

问题：

> 这个失败是哪里传上来的？

代表结果：

| 路线 | 触达文件数 | 源码读取数 | 结果质量 |
|------|------|------|------|
| `Native` | `0` | `0` | 止损成功，但只是在 prompt 约束下做到 |
| `Low-Level` | `0` | `0` | 止损成功，但回答比较啰嗦 |
| `TaskContext-First` | `0` | `0` | 最干净，直接 `needs_anchor` |
| `Hybrid` | `0` | `0` | 与 `task_context` 一致，直接要求补锚点 |

判断：

1. 这组直接支撑了 `needs_anchor` 的产品价值。
2. 当前最自然的 UX 应该是：直接提示用户补“测试名 / 函数名 / 文件 / 堆栈”中的任一项。

---

## 5. 失败样例与风险

这轮实验里，最值得认真看的失败样例有三类。

### 5.1 Low-Level 的图上 wandering

在 `S2` 和 `S5` 中，`Low-Level` 路线多次表现出：

1. 先查目标符号
2. 发现 incoming 为空
3. 转而追更多“可能相关”的失败函数、状态、crate
4. 最后不是停在“图不够”，而是继续扩图

这说明：

1. 低层工具不适合作为 Claude 的默认入口。
2. 如果不先给出场景和止损语义，Claude 会把“图不够”误当成“还没查够”。

### 5.2 Call-chain 的图缺边

`S4` 中：

1. `Native` 和 `Hybrid` 找到了真实 3 跳路径。
2. `TaskContext-First` 返回 `insufficient_graph`。
3. `Low-Level` 甚至给出“静态图中不存在调用链”的结论。

这说明：

1. 当前图谱还不能把 `call_chain` 当作高可靠结论面。
2. `insufficient_graph` 不是失败，而是正确行为。

### 5.3 Claude CLI 自身的不稳定性

本轮也遇到了外部噪声：

1. 并发时出现过 `API Error: terminated`
2. 某些路线会先写出答案，再没有优雅结束
3. `--json-schema` 能改善收尾一致性，但也会引入 `StructuredOutput` 工具调用

所以这轮报告更适合用于：

1. 比较路线趋势
2. 分析典型成功/失败模式

而不适合把所有数字当成最终基准值。

---

## 6. 对 QuickDep 的产品判断

### 当前已经成立的价值

1. 在大型仓库里缩小 Claude 的首轮搜索空间。
2. 在 `impact`、`large-file boundary`、`workspace-context` 场景下，把模糊问题变成可执行问题。
3. 在没有足够上下文时，用 `needs_anchor` 尽早止损。
4. 在 `behavior` / `call_chain` 场景里，用 `needs_code_read` 或 `insufficient_graph` 避免错误自信。

### 当前还不成立的价值

1. “纯图就能稳定回答行为题”
2. “纯图就能稳定回答跨模块调用链”
3. “Low-Level 工具组合足以替代 `get_task_context`”

### 最适合的对外表述

建议把故事收敛成：

> QuickDep 先帮 agent 进入正确代码区域，再决定是否需要读源码；它不是替代源码阅读，而是减少无效搜索和错误怀疑对象。

不建议主打：

> 一次性打包所有依赖，让 agent 不用读代码

因为这和当前实验结果并不一致。

---

## 7. 下一步建议

按优先级排序：

1. 强化 `behavior` 场景的 `suggested_reads`
   - 不追求直接解释行为
   - 重点是把上层 caller、失败分支文件、状态落点排对顺序
2. 修 `call_chain` 缺边
   - `S4` 已经证明这会直接影响可信度
3. 给 `Low-Level` 增加更强的止损语义
   - 或干脆在产品层不鼓励 agent 直接走 low-level 入口
4. 继续做重复实验
   - 每场景至少 3 轮
   - 统计中位数，而不是只看单轮样例

---

## 8. 原始文件

主要日志目录：

- `/tmp/quickdep-experiments-v2/ark-runtime/s1-impact/`
- `/tmp/quickdep-experiments-v2/ark-runtime/s2-behavior/`
- `/tmp/quickdep-experiments-v2/ark-runtime/s3-large-file/`
- `/tmp/quickdep-experiments-v2/ark-runtime/s4-call-chain/`
- `/tmp/quickdep-experiments-v2/ark-runtime/s5-editor-context/`
- `/tmp/quickdep-experiments-v2/ark-runtime/s6-no-anchor/`

其中：

1. `*-terminated.jsonl` 是被 API 噪声污染的失败样例。
2. `*run1b*` 或 `*run2*` 是为保证可比性做的受控重跑。
