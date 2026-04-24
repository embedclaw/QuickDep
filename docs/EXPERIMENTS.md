# QuickDep 实验文档导航

这份文档用来回答两个问题：

1. 仓库里这么多实验文档，哪份才是当前该引用的。
2. 哪些数字是当前可以对外说的，哪些只是某一轮迭代记录。

## 当前文档分工

| 文档 | 状态 | 用途 | 当前是否适合对外直接引用 |
| --- | --- | --- | --- |
| [AGENT_HYBRID_BENCHMARK_REPORT.md](AGENT_HYBRID_BENCHMARK_REPORT.md) | 当前广义基线 | `ark-runtime` 共同场景 `S1-S5` 平均值，以及 `S6` watcher 结果 | 是 |
| [ARK_RUNTIME_AGENT_COMPARISON_V3.md](ARK_RUNTIME_AGENT_COMPARISON_V3.md) | 当前最新定向复跑 | Rust 调用链修复后，对 `S1 / S2 / S3 / S5` 的定向验证 | 是，但只适合引用它覆盖到的场景 |
| [ARK_RUNTIME_AGENT_COMPARISON_V2.md](ARK_RUNTIME_AGENT_COMPARISON_V2.md) | 历史记录 | 第二轮实验过程和当时的问题暴露面 | 否 |
| [AGENT_HYBRID_BENCHMARK_PLAN.md](AGENT_HYBRID_BENCHMARK_PLAN.md) | 当前方法论计划 | 指标定义、评分规则、执行协议 | 是 |
| [AGENT_CONTEXT_EXPERIMENT_PLAN.md](AGENT_CONTEXT_EXPERIMENT_PLAN.md) | 早期场景化计划 | `task_context` 设计阶段的实验想法和问题拆分 | 仅作设计参考 |

## 当前应统一使用的实验口径

### 1. 对外平均值

如果要说“在大仓库共同场景上的总体趋势”，当前统一引用：

- [AGENT_HYBRID_BENCHMARK_REPORT.md](AGENT_HYBRID_BENCHMARK_REPORT.md)

当前可对外使用的聚合数据是：

| 路线 | 平均得分 | 平均耗时 ms | 平均 ctx tokens | 平均 file fan-out | 平均 raw source chars | 平均 MCP payload chars |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `Q` QuickDep-only | `3.2` | `89,825` | `379,111` | `5.0` | `8,811` | `24,700` |
| `N` Native-only | `3.2` | `70,365` | `278,350` | `35.8` | `42,056` | `0` |
| `H` Hybrid | `3.2` | `72,045` | `272,461` | `7.6` | `22,748` | `7,781` |

这组数适合支撑的故事是：

- QuickDep 已经能显著压低文件 fan-out
- Hybrid 是当前最稳的真实工作流
- QuickDep-only 还不能稳定宣称“更省总 token”

### 2. 对外最新修复收益

如果要说“最新一轮修复后，哪些具体能力变好了”，当前统一引用：

- [ARK_RUNTIME_AGENT_COMPARISON_V3.md](ARK_RUNTIME_AGENT_COMPARISON_V3.md)

当前最新定向结果适合支撑的点是：

- `S3` 调用链题：`q / h / n` 都达到 `4/5`，Rust 调用链假阴性已明显改善
- `S5` 风险面题：`q = 4/5`、`h = 4/5`、`n = 2/5`
- `S2` 失败传播题：`h = 4/5`、`q = 3/5`、`n = 2/5`
- `S1` 工作流题仍然是短板，三条路线都是 `2/5`

### 3. 不该混着说的地方

以下两类数字不要混在同一张表里：

1. `AGENT_HYBRID_BENCHMARK_REPORT.md` 的 `S1-S5` 聚合平均值
2. `ARK_RUNTIME_AGENT_COMPARISON_V3.md` 的定向复跑结果

原因：

- 两轮实验覆盖场景不同
- 运行时索引状态和前置修复不同
- `V3` 不是完整重跑 `S1-S6`

所以：

- 要讲“总体趋势”，用广义基线
- 要讲“最近修复产生了什么收益”，用 `V3`

## 当前建议引用顺序

### 面向 README / 官网 / 对外介绍

1. 先引用 [AGENT_HYBRID_BENCHMARK_REPORT.md](AGENT_HYBRID_BENCHMARK_REPORT.md)
2. 再补一句“最新定向复跑见 [ARK_RUNTIME_AGENT_COMPARISON_V3.md](ARK_RUNTIME_AGENT_COMPARISON_V3.md)”

### 面向内部开发决策

1. 先看 [ARK_RUNTIME_AGENT_COMPARISON_V3.md](ARK_RUNTIME_AGENT_COMPARISON_V3.md)
2. 再回看 [AGENT_HYBRID_BENCHMARK_REPORT.md](AGENT_HYBRID_BENCHMARK_REPORT.md)
3. 最后用 [AGENT_HYBRID_BENCHMARK_PLAN.md](AGENT_HYBRID_BENCHMARK_PLAN.md) 校验方法论是否还成立

## 当前实验结论的一句话版本

QuickDep 现在最站得住的价值，不是“它总能替代源码阅读”，而是：

> 它已经能帮助 agent 在大型仓库里更快缩小范围、进入更可能正确的代码区域；其中 Hybrid 是当前最稳的落地方式。
