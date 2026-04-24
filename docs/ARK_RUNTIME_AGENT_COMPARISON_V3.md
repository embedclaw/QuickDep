# QuickDep on `ark-runtime`: Claude 第三轮对比实验 V3

## 1. 这轮实验要回答什么

这轮不是重新证明一个空泛结论，而是收敛两个具体问题：

1. V2 里暴露的 Rust 调用链缺边，修完后是否真的改变了 agent 的答案质量。
2. QuickDep 现在到底适合讲什么故事，是“普遍省 token”，还是“在大仓库里更快锁定正确代码区域”。

结论先放前面：

> V3 已经能支撑一个更准确的产品故事：QuickDep 在大型 Rust 仓库里，能明显帮助 agent 更快进入正确的调用链、失败传播链和风险面；但它还不能稳定回答跨阶段工作流题，也不能宣称会在所有场景下自动省 token。

---

## 2. 本轮前置修复

第三轮实验之前，先落了两次代码修复。

### 2.1 Rust 调用链解析修复

- Commit: `66f7d3f`
- 目的：修掉 Rust 委托调用和带类型接收者的缺边问题

这次修复覆盖了：

1. `self.<field>.<method>()`
2. 带类型参数的接收者调用，如 `core.verifications.verify_pre_dispatch(...)`
3. 带字段链的委托调用，如 `scheduler.dispatchable_head(...)`

实现方式：

1. 在 `src/parser/rust.rs` 中把 Rust 结构体字段抽取为 `Property` 符号并保留类型签名。
2. 在 `src/resolver/symbol.rs` 中新增 Rust 接收者链解析：
   - 从 `self` 或参数签名推导根类型
   - 通过字段链逐步解析 owner type
   - 对 Rust method suffix 做唯一化匹配

### 2.2 编辑器锚点优先级修复

- Commit: `891aa98`
- 目的：当 `workspace.active_file` 和 `workspace.selection_symbol` 同时存在时，优先在当前文件内锚定，而不是先做全局模糊匹配

这次修复主要落在 `src/mcp/mod.rs`。它不是本轮基准的主角，但对真实 IDE 场景是必要修补。

### 2.3 代码验证

修复完成后已通过：

1. `cargo fmt`
2. `cargo test`

测试结果：

- `309` 个 lib tests 通过
- integration tests 通过
- doctests 通过

---

## 3. 实验设置

- QuickDep 仓库分支：`dev/mcp-context-envelope-refactor`
- `ark-runtime` commit：`ef4377c`
- 生成时间：`2026-04-24`
- 原始报告：`/tmp/quickdep-benchmarks-v3/REPORT.md`
- 元数据：`/tmp/quickdep-benchmarks-v3/metadata.json`

路线缩写：

1. `q` = QuickDep low-level
2. `n` = Native only
3. `h` = Hybrid

实验场景：

1. `S1` 审批通过后为什么仍停留在 `Queued`
2. `S2` `verify_pre_dispatch` 为什么会升级成 turn failure
3. `S3` `RuntimeCore::next_conflict_queue_head -> Scheduler::dispatchable_head` 真实调用链
4. `S5` 修改 `next_conflict_queue_head` 的风险面分析

本轮 `ark-runtime` 索引状态：

- files: `97`
- symbols: `7772`
- dependencies: `14554`
- imports: `2970`

数据来源：

- `./target/debug/quickdep debug --stats /Users/luozx/work/ark-runtime`

---

## 4. V2 到 V3，哪些问题真的闭环了

### 4.1 Rust 调用链假阴性已经被修掉

V2 的核心失败点之一是：

> `RuntimeCore::next_conflict_queue_head -> Scheduler::dispatchable_head` 这条真实 3 跳链路，图里给不出来。

V3 的结果已经变了：

| 场景 | 路线 | 分数 | 工具数 | 触达文件 | 源码读取字符 |
|------|------|------|------:|------:|------:|
| `S3 Conflict Queue Call Chain` | `q` | `4/5` | `7` | `5` | `0` |
| `S3 Conflict Queue Call Chain` | `h` | `4/5` | `8` | `3` | `3641` |
| `S3 Conflict Queue Call Chain` | `n` | `4/5` | `13` | `6` | `9732` |

其中 `q` 路线已经能直接回答：

1. `RuntimeCore::next_conflict_queue_head`
2. `ExecutionService::next_conflict_queue_head`
3. `Scheduler::dispatchable_head`

手工点查也能直接看到边已经存在：

- `RuntimeCore::next_conflict_queue_head` 的 outgoing 现在包含：
  - `ExecutionService::next_conflict_queue_head`
  - `Store::list_concurrency_window`
  - `Scheduler::dispatchable_head`

这条证据来自：

- `./target/debug/quickdep debug /Users/luozx/work/ark-runtime -d 'crates/ark-runtime/src/flow.rs::RuntimeCore::next_conflict_queue_head'`

这说明本轮修复的不是“文案问题”，而是底层图边真实补上了。

### 4.2 `verify_pre_dispatch` 的上游调用者可见性已经回来了

V2 另一个关键缺口是 `verify_pre_dispatch` 的 incoming caller 不完整，导致 impact / behavior 题很容易越查越散。

现在手工点查 `VerificationService::verify_pre_dispatch`，其 incoming 已经能看到：

1. `RuntimeFlowService::process_turn`
2. `RuntimeCore::process_turn`
3. `RuntimeCore::retry_intervention_turn`
4. 更上游的 `turn_submit` / `interaction_resolve`

这条证据来自：

- `./target/debug/quickdep debug /Users/luozx/work/ark-runtime -d 'crates/ark-runtime-verification/src/lib.rs::VerificationService::verify_pre_dispatch'`

这和 V3 的 `S2` 结果是吻合的：

| 场景 | 路线 | 分数 | 结论 |
|------|------|------|------|
| `S2 Pre-Dispatch Failure Propagation` | `h` | `4/5` | 可以较完整地区分验证层和 runtime 消费层职责 |
| `S2 Pre-Dispatch Failure Propagation` | `q` | `3/5` | 已能给出主链路，但金标准覆盖还不够完整 |
| `S2 Pre-Dispatch Failure Propagation` | `n` | `2/5` | 能读源码回答，但金标准覆盖仍不足 |

这里最重要的变化不是 `q` 变成满分，而是：

> QuickDep 不再把 agent 卡死在“上游 caller 不见了”的错误前提上。

### 4.3 编辑器锚点修复已落地，但不是本轮主 benchmark

`891aa98` 修了 `active_file + selection_symbol` 的锚点优先级问题，这对 IDE 场景是必要修补。

但要保持严谨：本轮 V3 的四个 benchmark 场景没有专门复跑一个纯编辑器锚点题，所以这里暂时只把它记为“已修代码，未作为本轮主要量化结论”。

---

## 5. 第三轮实验结果怎么看

### 5.1 总表

| 场景 | 路线 | 分数 | 耗时 ms | 工具数 | 文件 fan-out | 源码字符 | MCP 字符 | 总上下文 token |
|------|------|------|------:|------:|------:|------:|------:|------:|
| `S1` | `h` | `2/5` | `260338` | `34` | `24` | `42733` | `32125` | `755623` |
| `S1` | `n` | `2/5` | `124644` | `18` | `19` | `34304` | `0` | `344770` |
| `S1` | `q` | `2/5` | `245627` | `38` | `19` | `25737` | `19458` | `843918` |
| `S2` | `h` | `4/5` | `71185` | `11` | `11` | `27986` | `10660` | `232207` |
| `S2` | `n` | `2/5` | `89805` | `13` | `6` | `30635` | `0` | `233947` |
| `S2` | `q` | `3/5` | `123003` | `16` | `6` | `25862` | `11060` | `399694` |
| `S3` | `h` | `4/5` | `47099` | `8` | `3` | `3641` | `3660` | `147144` |
| `S3` | `n` | `4/5` | `89246` | `13` | `6` | `9732` | `0` | `237938` |
| `S3` | `q` | `4/5` | `65100` | `7` | `5` | `0` | `7121` | `150047` |
| `S5` | `h` | `4/5` | `100756` | `12` | `7` | `8714` | `21356` | `243860` |
| `S5` | `n` | `2/5` | `110295` | `11` | `6` | `28004` | `0` | `293719` |
| `S5` | `q` | `4/5` | `111442` | `11` | `4` | `0` | `28511` | `320959` |

说明：

1. `total_ctx_tokens` 是 benchmark 脚本记录的上下文代理指标，不是账单值。
2. 这组数字更适合比较路线趋势，不适合当成精确成本结论。

### 5.2 最强证据场景：`S3` 真实调用链

`S3` 是本轮最关键的正向证据。

原因很简单：

1. 它正对 V2 的已知缺边。
2. 修复后 `q/h/n` 三条路线都能答对。
3. `q` 路线做到 `0` 源码读取字符，且只用 `7` 个工具。

这组结果足以支撑：

> 对“明确符号 + 明确调用链”问题，QuickDep 现在已经能把 agent 直接送到正确的链路上，而不需要先读源码试错。

### 5.3 第二强证据场景：`S5` 风险面分析

`S5` 问的是：

> 如果我要修改 `next_conflict_queue_head` 的选头逻辑，哪些路径最容易被改坏。

结果：

1. `q = 4/5`
2. `h = 4/5`
3. `n = 2/5`

这组数据说明 QuickDep 对“找高风险调用路径”已经有实际价值。尤其是 `q` 路线在 `0` 源码读取字符下，已经能把：

1. `commit_or_reject_execution`
2. `apply_turn_failure`
3. `approval_resolve`
4. `runtime_cancel`

这些关键调用者列进风险面。

这更接近真实重构前分析，而不是单纯的 demo。

### 5.4 `S2` 说明了正确使用方式仍然是 Hybrid

`S2` 不是纯图题，它要求区分：

1. 验证层只负责给出 `VerificationDecision`
2. runtime 消费层才决定 turn failure

这类题的 V3 结论是：

1. `h` 最稳，`4/5`
2. `q` 变好了，但还没有强到可以独立替代读码
3. `n` 能靠读码回答，但并没有明显更优

所以这里最准确的表述不是“QuickDep 直接回答了行为题”，而是：

> QuickDep 现在已经能把 agent 更快送到正确实现层，再由少量读码完成最后解释。

### 5.5 `S1` 仍然是主要短板

`S1` 问的是：

> 审批通过后，为什么 execution 还会停留在 `Queued`。

三条路线全部 `2/5`，这是本轮最该认真看的失败样例。

而且它的失败特征很稳定：

1. 问题横跨 `store -> runtime -> flow -> execution -> scheduler -> worker claim`
2. 这不是单纯的“某条 call edge 缺失”
3. 这是一个阶段性工作流问题，需要状态机和调度链一起打包

更直接地说：

> `S1` 暴露的已经不是“图找不到边”，而是“agent 缺少适合工作流题的 server-side 上下文包”。

这也是为什么 `q/h` 在这个场景里都明显变胖，工具数和上下文量都失控。

---

## 6. 这轮实验真正支撑的产品故事

V3 之后，可以讲的故事是：

> QuickDep 的核心价值，是帮助 agent 在大型仓库里更快缩小怀疑范围、进入正确代码区域、锁定关键调用链和风险面。

这个故事现在有数据支撑，尤其是：

1. `S3` 明确调用链
2. `S5` 风险面分析
3. `S2` 失败传播解释

但不该讲的故事也要明确写出来：

1. 不应该说“QuickDep 已经能稳定替代源码阅读”
2. 不应该说“QuickDep 在所有场景都自动更省 token”
3. 不应该说“low-level 路线已经适合作为默认 agent 入口”

如果把故事讲成“质量优先的快速定位层”，现在是站得住的。
如果讲成“普遍的一次性全量依赖打包器”，这轮数据还不支持。

---

## 7. 下一步该补哪里

优先级已经比较清楚：

### 7.1 第一优先级：补工作流题的上下文包

目标不是继续无上限扩图，而是给 `S1` 这类题做一个更像 agent 任务包的 server-side 输出，至少要包含：

1. 阶段链：审批、恢复、dispatch、claim、queue gating
2. 关键状态转换
3. 每阶段 1 到 2 个关键符号
4. 建议阅读顺序
5. 明确的“图足够 / 仍需读码 / 仍缺锚点”状态

### 7.2 第二优先级：继续把默认路线收敛到 Hybrid

这轮已经证明：

1. `q` 适合直接依赖图题和风险面题
2. `h` 更适合行为题
3. 对跨阶段工作流题，如果没有更好的任务包，`q` 只会越查越胖

所以产品层面更合理的方向不是鼓励 agent 自由组合 low-level，而是：

1. 先判场景
2. 再决定走 `q` 还是 `h`

### 7.3 第三优先级：继续做重复实验

这轮仍然是单轮对比，不是统计意义上的最终基线。

下一轮建议：

1. 对 `S2 / S3 / S5` 各重复 `3` 次
2. 对新增的工作流场景也做 `3` 次
3. 统计中位数，而不是只看单轮样本

---

## 8. 一句话总结

V3 最重要的变化不是“所有问题都解决了”，而是：

> 我们已经把问题从“底层 Rust 图不可信”收敛到了“工作流题还缺一个更适合 agent 的上下文包”。

这意味着下一轮优化应该优先补场景化打包和路由，而不是盲目继续扩低层图能力。
