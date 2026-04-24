# QuickDep Agent 混合基准测试计划

## 1. 目标

这轮基准测试不再尝试证明“QuickDep 单独就能回答所有问题”，而是回答三个更实际的问题：

1. QuickDep 能否让 agent 更快收敛到正确的 3 到 5 个关键文件
2. `QuickDep + 原生工具` 的混合路线，是否优于只用原生工具
3. QuickDep 的真实价值，是否主要体现在“结构收敛”和“减少盲搜”而不是“完全替代源码阅读”

本计划以 `ark-runtime` 为主测试仓库，问题全部设计成真实工程问法，而不是抽象符号查询。

---

## 2. 总体原则

### 2.1 路线矩阵

每个场景固定跑 3 条隔离路线：

| 路线 | 名称 | 目标 | 允许手段 |
|------|------|------|----------|
| `Q` | QuickDep-first | 测试 QuickDep 是否足以快速收敛上下文 | 以 QuickDep MCP 为主；若必须读源码，只允许少量定点确认 |
| `N` | Native-only | 作为当前主流 agent 工作流基线 | 禁用 QuickDep，只用 `rg` / `sed` / `cat` / 代码搜索 |
| `H` | Hybrid | 测试 QuickDep 和原生工具结合后的真实上限 | 先用 QuickDep 缩小范围，再读少量源码完成解释 |

说明：

- `Q` 不是“绝对禁止源码”，而是“强约束 QuickDep 主导”
- `H` 是本轮最重要路线，因为它最接近未来真实使用方式
- 若某题 `Q` 明显不适合，结果也有价值，因为这恰好说明 QuickDep 当前边界

### 2.2 并发规则

严格遵守：

- 默认并发数：`3`
- 最大并发数：`4`
- 推荐做法：每个场景只并行启动 `Q / N / H` 三个独立会话
- 第四个槽位只保留给：
  - rerun
  - judge
  - transcript 解析

禁止做法：

- 同一场景同时开超过 3 条主路线
- 多个场景一起跑导致总并发超过 4

### 2.3 评估重点

本轮不只看“最后答对没”，还要看 agent 是怎么到达答案的：

- 是否更快触达 gold files
- 是否减少文件 fan-out
- 是否减少盲目原始源码读取
- 是否减少总上下文消耗
- 是否让答案更完整、更可操作

### 2.4 软预算

为避免同一路线在不同场景下跑出完全不同风格，建议增加软预算，只做记录，不做硬中断：

| 路线 | 软预算 |
|------|--------|
| `Q` | 前 3 次工具调用应以 QuickDep 为主；原始源码定点读取不超过 2 个片段 |
| `N` | 不允许 QuickDep；优先把关键文件控制在 5 个以内 |
| `H` | 前 2 次工具调用里至少 1 次使用 QuickDep；原始源码读取尽量控制在 5 个文件以内 |

若超出软预算：

- 不判失败
- 但必须在 `metrics.json` 和最终总结里显式记录
- 视为“工具没有帮助 agent 自然收敛”的反向信号

---

## 3. 执行环境

### 3.1 仓库

- QuickDep 仓库：`/path/to/quickdep`
- 目标仓库：`/path/to/target-repo`

### 3.2 QuickDep 前置状态

每轮正式实验前先做一次独立预热，不计入场景统计：

1. `scan_project(path=/Users/luozx/work/ark-runtime, rebuild=false)`
2. 轮询 `get_scan_status`
3. 确认状态进入 `Loaded`
4. 如需要增量实验，再确认 watcher 处于开启状态

### 3.3 产物目录

统一输出到：

```text
/tmp/quickdep-benchmarks-v2/
```

建议目录结构：

```text
/tmp/quickdep-benchmarks-v2/
  metadata.json
  scenario_s1/
    q/
      prompt.txt
      transcript.jsonl
      answer.md
      metrics.json
    n/
      prompt.txt
      transcript.jsonl
      answer.md
      metrics.json
    h/
      prompt.txt
      transcript.jsonl
      answer.md
      metrics.json
    judge/
      score.json
      notes.md
```

---

## 4. 场景总览

| 场景 | 类型 | 核心问题 | 主要考点 |
|------|------|----------|----------|
| `S1` | 队列 / 时序 | 为什么 execution 会停留在 `Queued` | 状态流转、审批恢复、调度头选择 |
| `S2` | 失败传播 | 为什么 `verify_pre_dispatch` 失败会让 turn 失败 | 验证包装层、失败处理链 |
| `S3` | 调用链 | `RuntimeCore::next_conflict_queue_head` 如何到 `Scheduler::dispatchable_head` | 跨 crate 委托链 |
| `S4` | 大文件边界 | `PlatformServer::health_report` 需要读哪些局部实现 | 大文件裁剪、模块边界 |
| `S5` | 修改风险 | 如果修改 `next_conflict_queue_head`，风险面在哪里 | 影响范围、恢复路径、回归点 |
| `S6` | 增量更新 | 改完一个调用点后，索引能否及时反映变化 | watcher、增量刷新、结果正确性 |

---

## 5. 场景细化与金标准

## S1 队列 / 时序问题

### 问题定义

问题模板：

> 一个 execution 在审批通过后，为什么仍然可能继续停留在 `Queued`，而不是直接进入 `Running`？

### 目标考点

- 审批通过并不等于立即运行
- `Queued` 是显式状态，不是中间噪音
- 真正能否运行，取决于恢复路径和冲突窗口调度

### 锚点文件

- `/Users/luozx/work/ark-runtime/crates/ark-store/src/write.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/core_flow_service.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-execution/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-scheduler/src/lib.rs`

### Gold files

- `/Users/luozx/work/ark-runtime/crates/ark-store/src/write.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/core_flow_service.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-execution/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-scheduler/src/lib.rs`

### Gold symbols

- `Store::approve_pending_approval`
- `Runtime::approval_resolve`
- `CoreFlowService::resume_approved_execution`
- `RuntimeCore::dispatch_execution`
- `RuntimeCore::prepare_execution_dispatch`
- `ExecutionService::next_conflict_queue_head`
- `Scheduler::admit`
- `Scheduler::dispatchable_head`

### Gold answer

正确答案至少应覆盖以下 4 点：

1. `approve_pending_approval` 会把状态从 `WaitingApproval` 更新到 `Queued`，并不会直接改成 `Running`
2. 审批通过后还要经过 `approval_resolve -> resume_approved_execution -> dispatch_execution`
3. `dispatch_execution` 内部会再次走 `prepare_execution_dispatch -> check_admission`
4. 如果冲突窗口里存在更早的 `Created / Queued / WaitingApproval / Running / Blocked` 项，调度器可能继续让当前 execution 保持 `Queued`

### 判分重点

- 是否明确指出 `Queued` 是审批通过后的正常状态
- 是否指出“再次 admission 检查”这一层
- 是否识别到 `Scheduler` 是决定能否真正转为 `Running` 的关键点

---

## S2 失败传播问题

### 问题定义

问题模板：

> 为什么 `verify_pre_dispatch` 失败后，turn 会直接失败，而不是只把当前 execution 跳过？

### 锚点文件

- `/Users/luozx/work/ark-runtime/crates/ark-verification/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime-verification/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/core_flow_service.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`

### Gold files

- `/Users/luozx/work/ark-runtime/crates/ark-verification/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime-verification/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/core_flow_service.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`

### Gold symbols

- `VerificationEngine::verify_pre_dispatch`
- `RuntimeVerification::verify_pre_dispatch`
- `VerificationDecision::is_passed`
- `RuntimeCore::apply_turn_failure`

### Gold answer

正确答案至少应覆盖以下 4 点：

1. `VerificationEngine::verify_pre_dispatch` 只负责生成 `VerificationDecision`
2. `RuntimeVerification::verify_pre_dispatch` 会记录 decision，但不会自己决定 turn 命运
3. 真正把失败升级成 turn 失败的是 `core_flow_service` 中对 `!pre_dispatch.is_passed()` 的判断
4. `apply_turn_failure` 会连带处理 turn 上下文和后续恢复逻辑，因此这是业务流程决策，不只是单次 execution 的局部失败

### 判分重点

- 是否区分“验证引擎给出结论”和“runtime 消费结论”
- 是否指出 turn failure 发生在 caller 侧，而不是 verification crate 内部

---

## S3 调用链定位问题

### 问题定义

问题模板：

> 从 `RuntimeCore::next_conflict_queue_head` 到 `Scheduler::dispatchable_head` 的真实调用链是什么？

### 锚点文件

- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-execution/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-store/src/read.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-scheduler/src/lib.rs`

### Gold files

- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-execution/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-store/src/read.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-scheduler/src/lib.rs`

### Gold symbols

- `RuntimeCore::next_conflict_queue_head`
- `ExecutionService::next_conflict_queue_head`
- `Store::list_concurrency_window`
- `Scheduler::dispatchable_head`

### Gold path

最小正确路径：

1. `RuntimeCore::next_conflict_queue_head`
2. `ExecutionService::next_conflict_queue_head`
3. `Store::list_concurrency_window`
4. `Scheduler::dispatchable_head`

说明：

- 若回答里把 `Store::list_concurrency_window` 省略，可视为部分正确
- 若断言“没有路径”或漏掉 `ExecutionService` 委托层，判为错误

### 判分重点

- 是否能穿过 Rust 委托调用
- 是否能识别出中间的 `ExecutionService`
- 是否能把存储读取和调度决策分开说清

---

## S4 大文件边界问题

### 问题定义

问题模板：

> 如果我要理解 `PlatformServer::health_report` 的逻辑，最值得先看的 3 到 5 个局部点是什么，为什么？

### 锚点文件

- `/Users/luozx/work/ark-runtime/crates/ark-platform-server/src/lib.rs`

### Gold files

- `/Users/luozx/work/ark-runtime/crates/ark-platform-server/src/lib.rs`

### Gold symbols

- `PlatformServer::health_report`
- `PlatformServer::reconcile_expired_worker_leases`
- `worker_health_projection`
- `PlatformServer::metrics_snapshot`
- `DeploymentPreset::requires_workers`

### Gold answer

正确答案至少应覆盖以下 4 点：

1. 入口本身是 `PlatformServer::health_report`
2. 需要先看 `reconcile_expired_worker_leases`，因为它在逻辑一开始就改变 worker 视图
3. 需要看 `worker_health_projection`，因为 worker 列表展示由它投影
4. 需要看 `metrics_snapshot` 和 `DeploymentPreset::requires_workers`，因为最终 `status / ready / issues` 依赖这两块

### 判分重点

- 是否把注意力压缩在单文件或极少文件
- 是否给出“为什么先看这几个点”
- 是否避免无关地扩散到大量 HTTP / RPC 入口

---

## S5 修改风险分析问题

### 问题定义

问题模板：

> 如果我要修改 `next_conflict_queue_head` 的选头逻辑，哪些调用路径和行为最容易被改坏？请按风险排序。

### 锚点文件

- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-execution/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-store/src/read.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-scheduler/src/lib.rs`

### Gold files

- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/flow.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-runtime/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-execution/src/lib.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-store/src/read.rs`
- `/Users/luozx/work/ark-runtime/crates/ark-scheduler/src/lib.rs`

### Gold symbols

- `RuntimeCore::next_conflict_queue_head`
- `ExecutionService::next_conflict_queue_head`
- `Store::list_concurrency_window`
- `Scheduler::dispatchable_head`
- `RuntimeCore::commit_or_reject_execution`
- `RuntimeCore::apply_turn_failure`
- `Runtime::approval_resolve`
- `Runtime::runtime_cancel`

### Gold risks

正确答案至少应提到以下高风险面：

1. 执行成功后释放并恢复下一个 queued execution 的路径
2. turn failure 后释放并恢复下一个 queued execution 的路径
3. approval deny 后释放并恢复下一个 queued execution 的路径
4. session cancel 后按 `released_keys` 恢复 queued execution 的路径
5. 调度公平性和顺序语义，尤其是 `created_at` 排序与 `WaitingApproval / Running / Blocked` 对 head 的影响

### 判分重点

- 是否看到了多个 caller，而不是只盯着一个方法
- 是否给出“行为风险”而不只是“文件列表”
- 是否明确把“恢复点”作为主要回归面

---

## S6 增量刷新 / watcher 问题

### 问题定义

这是一个控制变量场景，不直接用现有代码提问，而是在 disposable worktree 上做一个最小改动：

1. 在 `/Users/luozx/work/ark-runtime/crates/ark-platform-server/src/lib.rs` 新增：
   - `fn push_issue(issues: &mut Vec<String>, issue: &str)`
2. 把 `health_report` 中若干 `issues.push(\"...\")` 改成 `push_issue(&mut issues, \"...\")`
3. 不做全量 rebuild，只依赖 watcher / 增量更新

执行要求：

- 在 disposable worktree 中完成修改
- 记录修改时间戳
- 场景结束后回滚该 worktree

问题模板：

> 代码改完后，多快能从索引和工具结果里观察到 `health_report -> push_issue` 这条新依赖？需要做全量重扫吗？

### Gold files

- `/Users/luozx/work/ark-runtime/crates/ark-platform-server/src/lib.rs`

### Gold symbols

- `PlatformServer::health_report`
- `push_issue`

### Gold answer

正确答案至少应覆盖以下 4 点：

1. 新符号 `push_issue` 应出现在 `find_interfaces` / `get_file_interfaces` 结果中
2. `health_report` 的 outgoing dependencies 应出现 `push_issue`
3. 这次变化应由 watcher / 增量更新吸收，而不是强制全量 rebuild
4. 若结果长时间不刷新，应明确记录 watcher 延迟或增量失效问题

### 判分重点

- 首次观察到变化的时间
- 是否真的反映出新依赖
- 是否出现旧缓存残留

---

## 6. 路线 Prompt 模板

所有路线都要求最终输出固定结构：

```text
1. 结论
2. 关键文件（最多 5 个）
3. 关键符号 / 调用链
4. 不确定点
```

### 6.1 `Q` 路线模板

```text
你正在分析仓库 {{repo_path}}。

请回答下面这个工程问题：
{{scenario_question}}

约束：
- 优先使用 QuickDep MCP 工具
- 不要先做大范围 grep 或整文件通读
- 只有在 QuickDep 无法支撑判断时，才允许做少量定点源码确认
- 如果你读取源码，必须说明是被哪一个 QuickDep 结果引导过去的

输出要求：
- 先给结论
- 明确列出你依赖的关键符号和关键文件
- 如果 QuickDep 当前能力不足以回答，要直接指出缺口
```

### 6.2 `N` 路线模板

```text
你正在分析仓库 {{repo_path}}。

请回答下面这个工程问题：
{{scenario_question}}

约束：
- 禁用 QuickDep 和任何外部索引
- 只允许使用原生搜索和源码阅读手段
- 尽量减少无关文件扩散

输出要求：
- 先给结论
- 明确列出你实际依赖的关键文件和关键符号
- 如果存在多个可能解释，要说明哪一个最可信
```

### 6.3 `H` 路线模板

```text
你正在分析仓库 {{repo_path}}。

请回答下面这个工程问题：
{{scenario_question}}

工作方式：
- 先用 QuickDep 缩小到候选文件 / 候选符号
- 再用少量原生源码阅读确认行为细节
- 目标是在正确性和上下文成本之间取得最优平衡

输出要求：
- 先给结论
- 列出 QuickDep 帮你缩小范围的证据
- 列出你最终阅读确认的源码点
- 如果 QuickDep 和源码证据不一致，要显式记录
```

---

## 7. 指标定义

每条路线每个场景都记录以下指标：

| 指标 | 定义 |
|------|------|
| `time_to_first_hit` | 首次触达任一 gold file 或 gold symbol 的时间 |
| `total_latency` | 从发出 prompt 到最终答案结束的总耗时 |
| `tool_count` | 工具调用总数 |
| `file_fanout` | 显式触达的不同文件数量 |
| `raw_source_chars` | 原始源码读取字符数 |
| `mcp_payload_chars` | QuickDep / MCP 原始返回字符数 |
| `total_ctx_tokens` | CLI 暴露的总上下文代理 token |
| `gold_file_recall` | 触达的 gold files 占比 |
| `gold_symbol_recall` | 识别出的 gold symbols 占比 |
| `path_correctness` | 调用链题的正确性评分 |
| `risk_correctness` | 风险分析题的正确性评分 |
| `final_answer_score` | 最终答案质量综合分 |

### 7.1 评分建议

建议统一为 `0-5`：

- `0`：核心结论错误
- `1`：只碰到表面信息
- `2`：部分命中，但遗漏关键链路
- `3`：基本正确，有重要遗漏
- `4`：正确且覆盖较完整
- `5`：正确、完整、结构清晰、可直接指导工程行动

### 7.2 场景特定评分

- `S3` 重点看 `path_correctness`
- `S5` 重点看 `risk_correctness`
- `S6` 重点看 watcher 刷新延迟和结果正确性

---

## 8. 结果解释原则

为了避免再次出现“文件更少但 token 反而更高”这种误判，本轮按以下顺序解释结果：

1. 正确性是否足够
2. 是否更快命中 gold 区域
3. 是否减少无效源码读取
4. 是否降低总上下文成本

禁止只凭单一指标下结论：

- 不能只看 `file_fanout`
- 不能只看 `tool_count`
- 不能只看 `total_ctx_tokens`

更合理的判断方式是：

- 若 `H` 比 `N` 更快命中 gold files，且 `final_answer_score` 不低于 `N`，则 QuickDep 有实际 agent 价值
- 若 `Q` 经常失败而 `H` 稳定获益，说明 QuickDep 当前更适合作为“收敛器”，而不是“独立解题器”
- 若 `Q` / `H` 都不能明显改善 `N`，优先回头修 server-side 聚合和结果裁剪

---

## 9. 执行波次

### Wave 0：预热

- 构建或确认 QuickDep 索引
- 验证 watcher 正常
- 不记入正式成绩

### Wave 1：主分析题

- `S1`
- `S2`
- `S3`
- `S4`

执行策略：

- 每次只跑一个场景
- 场景内部并行 `Q / N / H`
- 总并发维持在 `3`

### Wave 2：高价值题

- `S5`
- `S6`

执行策略：

- 先跑 `S5`
- 再跑 disposable worktree 的 `S6`
- 如果某条路线明显异常，可使用第 4 槽位做一次 rerun 或 judge

---

## 10. 推荐产出

每个场景最终应产出：

1. 一份原始运行记录
2. 一份结构化指标 JSON
3. 一份 judge 打分
4. 一段结论摘要

总报告建议重点回答：

1. 哪些问题 `Q` 足够好
2. 哪些问题必须 `H`
3. 哪些问题 `QuickDep` 当前仍明显不如 `N`
4. 下一轮产品改进应该优先修什么

---

## 11. 预期结论形式

本轮最理想的结论不是“QuickDep 全面胜出”，而是更可执行的工程判断，例如：

- `QuickDep` 在调用链压缩和大文件边界缩小上有稳定收益
- `Hybrid` 在风险分析和时序问题上比 `Native-only` 更稳
- `QuickDep-only` 在涉及行为语义的问题上仍然不够
- 下一轮应优先投入：
  - 更瘦的场景化聚合结果
  - 更可靠的 Rust 委托链解析
  - 更可观测的 watcher / 增量刷新诊断

这类结论对产品路线更有价值，也更符合真实 agent 使用场景。
