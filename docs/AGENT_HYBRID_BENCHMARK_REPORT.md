# QuickDep Agent 混合基准测试报告

> 状态说明：这是当前用于 `S1-S5` 共同场景平均值和 `S6` watcher 结果的广义基线报告。
> 如果你要看 Rust 调用链修复后的最新定向复跑，请看 [ARK_RUNTIME_AGENT_COMPARISON_V3.md](ARK_RUNTIME_AGENT_COMPARISON_V3.md)。
> 不要把 `V3` 的定向结果直接混进本报告的 `S1-S5` 平均表。

## 1. 结论先行

这轮实验的核心结论不是“QuickDep 全面胜出”，而是：

1. `QuickDep-only` 的**结构收敛能力最强**，但**上下文成本不稳定**，在复杂问题上容易因为多次 MCP 往返把总 token 做高。
2. `Hybrid` 是当前最有实际价值的路线。
   - 在共同场景 `S1-S5` 上，`Hybrid` 的平均得分与 `Native-only`、`QuickDep-only` 相同，都是 `3.2/5`
   - 但 `Hybrid` 的平均 `file fan-out` 只有 `7.6`，远低于 `Native-only` 的 `35.8`
   - `Hybrid` 的平均原始源码读取量约 `22.7k chars`，明显低于 `Native-only` 的 `42.1k chars`
   - `Hybrid` 的平均上下文代理 token 约 `272k`，和 `Native-only` 的 `278k` 基本打平
3. `Native-only` 在**需要行为解释和业务语义**的题目上仍然更稳，尤其是 `S2`
4. QuickDep 的当前真实价值已经可以明确定位为：
   - 缩小候选文件范围
   - 压低盲目源码阅读
   - 支撑增量更新 / watcher 场景

换句话说：

> QuickDep 已经能稳定做“结构收敛器”，但还没有稳定做成“低 token 的独立解题器”。

---

## 2. 实验设置

### 2.1 仓库与脚本

- 目标仓库：`/Users/luozx/work/ark-runtime`
- 基准脚本：[scripts/agent_benchmark.py](../scripts/agent_benchmark.py)
- 原始输出目录：`/tmp/quickdep-benchmarks-v2/`
- 自动生成原始报告：`/tmp/quickdep-benchmarks-v2/REPORT.md`

### 2.2 路线

- `Q`: QuickDep-first / QuickDep-only 风格
- `N`: Native-only
- `H`: Hybrid

说明：

- `S6` 是 watcher / 增量刷新场景，只跑 `Q` 和 `H`
- `N` 在 `S6` 中标记为 `skipped`

### 2.3 运行约束

- 场景串行执行
- 场景内并行 `Q / N / H`
- 最大并发：`3`
- 单路线硬超时：`150s`

### 2.4 环境说明

这轮是通过 Claude Code headless 跑的，但当前本机 Claude Code 在 `stream-json init` 中报告的实际模型是 `glm-5`。因此本报告反映的是：

- Claude Code 工作流
- 当前本机提供商配置下的真实表现

而不是官方 Sonnet/Opus 的绝对成绩。

---

## 3. 总体结果

## 3.1 共同场景平均值（`S1-S5`）

| 路线 | 平均得分 | 平均耗时 ms | 平均 ctx tokens | 平均 file fan-out | 平均 raw source chars | 平均 MCP payload chars |
|------|---------:|------------:|----------------:|------------------:|----------------------:|------------------------:|
| `Q` | `3.2` | `89,825` | `379,111` | `5.0` | `8,811` | `24,700` |
| `N` | `3.2` | `70,365` | `278,350` | `35.8` | `42,056` | `0` |
| `H` | `3.2` | `72,045` | `272,461` | `7.6` | `22,748` | `7,781` |

### 3.2 读法

- `Q`：
  - 文件面最小
  - 原始源码读取最少
  - 但总上下文最高
  - 原因不是文件多，而是 MCP 查询链条偏长，且 payload 还不够瘦
- `N`：
  - 速度不慢
  - 行为解释类问题仍然可靠
  - 但 fan-out 和源码读取量明显更大
- `H`：
  - 保住了 `N` 的正确性底盘
  - 也拿到了接近 `Q` 的结构收敛收益
  - 在当前版本里是最均衡的路线

---

## 4. 分场景观察

## S1 队列 / 时序问题

- `Q`: `2/5`
- `N`: `2/5`
- `H`: `3/5`

观察：

- `Q` 把文件面压到 `3` 个文件，但结论被 `PlatformWorker` 分支带偏了，没把审批后再次 admission 检查和 scheduler 主路径讲完整
- `N` 文件面炸到 `157`，还一路读进了 `node_modules` 等噪音区域
- `H` 是这题最平衡的路线，保留了调度主线，也避免了 `N` 那种大面积扩散

结论：

- 这题证明了 QuickDep 的“结构收敛”价值
- 也暴露出 QuickDep 在**行为语义组织**上还不够，容易让 agent 过拟合到一条局部分支

## S2 失败传播问题

- `Q`: `3/5`
- `N`: `4/5`
- `H`: `2/5`

观察：

- `N` 是这题最好的路线
- `Q/H` 都能很快摸到正确文件，但在“验证引擎给出 decision”与“runtime caller 升级成 turn failure”这层职责边界上，解释不够完整

结论：

- 这类问题需要跨层职责解释
- QuickDep 现有查询能缩小范围，但不够支撑“为什么 caller 这么消费 decision”的完整叙述

## S3 调用链问题

- `Q`: `4/5`
- `N`: `4/5`
- `H`: `4/5`

观察：

- 这轮 `S3` 比上一轮健康得多
- 三条路线都成功命中主要调用链
- `Q/H` 的 fan-out 分别是 `4 / 4`，优于 `N` 的 `6`

结论：

- 对于“跨 crate 委托链，但语义不太复杂”的问题，QuickDep 已经开始进入可用区间

## S4 大文件边界问题

- `Q`: `4/5`
- `N`: `4/5`
- `H`: `4/5`

观察：

- 三条路线都能答对
- `Q` 把 fan-out 压到了 `2`
- `N` 也只触达 `2` 个文件，而且耗时和上下文都最低

结论：

- 这是 QuickDep 最自然的强项之一
- 但也说明：如果原生搜索本来就很容易命中单文件热点，QuickDep 的收益更多是“更整洁”，未必是“更快更省”

## S5 修改风险分析

- `Q`: `3/5`
- `N`: `2/5`
- `H`: `3/5`

观察：

- `Q/H` 都比 `N` 更容易收敛到核心文件
- 但三条路线都没把关键恢复点讲全
- 缺失最明显的是：
  - `approval_resolve`
  - `apply_turn_failure`
  - `runtime_cancel`
  这些路径和 `next_conflict_queue_head` 的关系没有被完整串起来

结论：

- 这题非常适合未来做一个“风险面打包”工具
- 单纯的符号搜索和局部依赖查询还不够

## S6 增量刷新 / watcher 问题

- `Q`: `5/5`
- `H`: `5/5`

观察：

- 两条路线都成功在 disposable worktree 内完成：
  - 新增 `push_issue`
  - `health_report` 改成调用 helper
  - 不使用 `rebuild_database`
  - 观察到新符号和新依赖边
- `Q` 的 `refresh_after_edit_ms` 约 `26.3s`
- `H` 的 `refresh_after_edit_ms` 约 `12.7s`

注意：

- 这里的 `refresh_after_edit_ms` 是**从第一次编辑动作到第一次脚本观察到正向结果**
- 它包含了 agent 连续编辑文件本身的时间，不等于纯 watcher 延迟
- agent 自己在答案里报告的“第一次正向查询几乎即时”与脚本结果并不矛盾

结论：

- QuickDep 在增量刷新场景上是实打实有产出能力的
- 这一点已经超过了“理论可行”，可以进入对外宣传的实证材料

---

## 5. 这轮实验真正说明了什么

## 5.1 已经被证明的价值

1. QuickDep 能显著压低文件 fan-out
2. QuickDep 能显著减少原始源码读取量
3. QuickDep 的 watcher / 增量更新能力在真实仓库上可用
4. `Hybrid` 路线已经能把这些结构收益转化成接近 `Native-only` 的总体效率

## 5.2 还没有被证明的价值

1. QuickDep-only 还没有稳定做到“更低 token”
2. QuickDep-only 还没有稳定做到“复杂语义题上比 Native-only 更对”
3. 风险分析 / 失败传播这类题目，依然需要 agent 自己读实现细节

## 5.3 对最初目标的修正

如果把最初目标定义为：

> “在大项目里，一次性找齐依赖给 agent，减少 token 和盲搜”

那么更准确的现状是：

- **减少盲搜**：已经做到
- **减少原始源码读取**：已经做到
- **稳定减少 agent 总上下文**：还没有稳定做到
- **一次性打包后直接回答复杂问题**：还没有做到

---

## 6. 产品方向建议

基于这轮结果，下一轮最值得做的不是继续堆底层查询，而是做更强的**场景化聚合**。

### 优先级 A

1. 更瘦的 server-side 场景打包
   - 审批 / 调度 / 恢复点
   - 失败传播
   - 风险面

2. 针对行为问题的“关键 caller / consumer”聚合
   - 不是只给符号定义
   - 要给“谁在消费这个 decision / status / result”

3. 控制 `QuickDep-only` 的 MCP 往返和 payload 膨胀
   - 减少 agent 连续追问
   - 把常见问题合成一次返回

### 优先级 B

1. 风险分析专用工具
   - 直接返回修改某符号最关键的恢复点、状态转移点、回归点

2. 更好的结果裁剪
   - 减少同文件长列表
   - 减少测试符号污染
   - 减少与当前问题弱相关的邻居

3. Benchmark harness 固化
   - 把 [scripts/agent_benchmark.py](../scripts/agent_benchmark.py) 继续做成可复跑基线
   - 后续每次关键改动都回跑 `S1-S6`

---

## 7. 对外表达建议

当前最适合对外讲的，不是“QuickDep 比原生工具更快更省 token”，而是：

1. QuickDep 能把 agent 的代码搜索从“盲读大仓库”变成“先收敛到关键文件”
2. QuickDep 和原生工具不是替代关系，而是组合关系
3. QuickDep 在增量更新和局部依赖追踪上已经具备实战价值

更保守也更准确的对外表述是：

> QuickDep 当前最擅长的是把 agent 带到正确的代码区域，而不是替 agent 完成所有复杂语义推理。

这比“纯 token 节省工具”的定位更真实，也更能指导下一轮产品演进。
