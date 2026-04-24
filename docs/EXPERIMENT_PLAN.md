# QuickDep Claude 实验计划

## 1. 目标

这轮实验从零重建，不继承旧实验报告。

新的实验只回答 3 个问题：

1. Claude 会不会优先使用 QuickDep 提供的高层入口，而不是直接大范围搜索。
2. 在大型仓库里，QuickDep 是否真的能帮助 Claude 更快缩小到正确代码区域。
3. QuickDep 在真实开发流里，是否能和源码阅读、编辑、增量更新一起工作。

## 2. 总原则

- 只用 Claude 执行主实验。
- 默认最大并发数不超过 `4`。
- 每个主场景优先只并行跑 `3` 条路线。
- 不再使用旧的单字母路线缩写。
- 所有报告只写全称路线名。

## 3. 路线定义

### Claude 默认行为

不限制 Claude 的工具选择，只观察它会不会主动选对 QuickDep 的入口。

### Claude 原生工具 Only

- 禁止使用 QuickDep。
- 只允许 Claude 使用原生搜索、读文件、代码导航。

### Claude QuickDep First

- QuickDep 必须主导前几步收敛。
- 只有 QuickDep 无法支撑判断时，才允许少量定点源码确认。

### Claude QuickDep Plus Native Tools

- 先用 QuickDep 缩小范围。
- 再用少量源码阅读确认行为细节。
- 这是当前最接近真实使用方式的主路线。

## 4. 实验波次

### 第一波：入口选择实验

这是最先做的。

目的：

- 先验证 Claude 会不会正确使用 QuickDep 的高层入口。
- 如果这一步都做不到，后面的主 benchmark 就没有解释力。

场景：

1. `Workflow` 入口
问题：为什么审批通过后，execution 仍然可能停留在 `Queued`？
期望第一跳：`analyze_workflow_context` 或 `get_task_context`

2. `Behavior` 入口
问题：为什么 `verify_pre_dispatch` 失败后，turn 会直接失败？
期望第一跳：`analyze_behavior_context` 或 `get_task_context`

3. `Impact` 入口
问题：如果我要修改 `next_conflict_queue_head`，风险面在哪里？
期望第一跳：`analyze_change_impact` 或 `get_task_context`

4. `Locate` 入口
问题：如果我要先理解 `PlatformServer::health_report`，最值得先看的局部点是什么？
期望第一跳：`locate_relevant_code` 或 `get_task_context`

成功标准：

- Claude 第一跳命中正确场景入口。
- Claude 不先做大范围 `grep` / 整文件通读。
- Claude 给出的下一步阅读范围明显小于原生盲搜。

每次都必须记录：

- Claude 第一跳工具名
- 首次命中前是否出现搜索扩散
- 首次命中前触达文件数
- 首次命中前源码读取字符数
- 首次命中高价值文件或符号所需时间

### 第二波：主 benchmark

这是新的核心实验。

目标仓库：

- `ark-runtime`

每个场景跑 3 条路线：

1. Claude 原生工具 Only
2. Claude QuickDep First
3. Claude QuickDep Plus Native Tools

核心场景：

1. 工作流问题
问题：为什么审批通过后，execution 仍然可能停留在 `Queued`？
价值：验证 QuickDep 对跨阶段工作流问题是否真的有帮助。

2. 失败传播问题
问题：为什么 `verify_pre_dispatch` 失败会升级成 turn failure？
价值：验证“先收敛，再少量读码”是否优于纯原生路线。

3. 调用链问题
问题：`RuntimeCore::next_conflict_queue_head` 到 `Scheduler::dispatchable_head` 的真实调用链是什么？
价值：验证最近 Rust 调用链修复有没有真实收益。

4. 风险面问题
问题：如果修改 `next_conflict_queue_head`，哪些路径和回归点最容易被改坏？
价值：验证 QuickDep 对真实重构前分析是否有价值。

### 第三波：真实开发流专项

这波不用于主 headline，但必须做。

场景：

1. 无锚点自然语言问题
目标：看 Claude 会不会被 QuickDep 正确止损，而不是乱搜。

2. 编辑器上下文问题
目标：只给 `active_file` / `selection_symbol`，看 QuickDep 能不能把 Claude 拉到正确区域。

3. 增量更新问题
目标：Claude 改完代码后，不做 `rebuild_database`，验证 watcher / 增量更新是否能反映新依赖。

### 第四波：跨语言 sanity

这波只验证“不是只对 Rust 有效”，不做大矩阵。

建议仓库：

1. `tokio`
2. `nest`
3. `gin`
4. `requests`
5. `fmt` 或 `redis`

建议方式：

- 默认只跑 Claude QuickDep Plus Native Tools。
- 只在结果异常时补跑 Claude 原生工具 Only。

## 5. 指标

每个实验至少记录：

- 第一跳工具是否正确
- 首次命中高价值文件所需时间
- 触达文件数
- 原始源码读取字符数
- QuickDep 返回字符数
- 总上下文 token
- 最终答案评分

## 6. 评分规则

统一使用 `0-5`：

- `0`：核心结论错误
- `1`：只碰到表面信息
- `2`：部分命中，但遗漏关键链路
- `3`：基本正确，但仍有重要遗漏
- `4`：正确且覆盖较完整
- `5`：正确、完整、结构清晰、可直接指导工程行动

## 7. 当前建议执行顺序

1. 先做第一波 4 个入口选择实验
2. 再做第二波 4 个主 benchmark 场景
3. 再做第三波 3 个真实开发流专项
4. 最后做第四波跨语言 sanity

## 8. 当前输出物

这轮实验只保留 3 份文档：

1. [EXPERIMENT_PLAN.md](EXPERIMENT_PLAN.md)
2. [EXPERIMENT_RUNBOOK.md](EXPERIMENT_RUNBOOK.md)
3. [EXPERIMENT_REPORT.md](EXPERIMENT_REPORT.md)
