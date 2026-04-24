# QuickDep Claude 实验报告

> 状态：本轮已完成
> 最近更新：2026-04-24
> 说明：本报告只写入本轮实际重跑结果，不继承旧实验数字。

## 1. 本轮范围

本轮只接受以下实验结果写入：

1. 第一波入口选择实验
2. 第二波 `ark-runtime` 核心 benchmark
3. 第三波真实开发流专项
4. 第四波跨语言 sanity

## 2. 本次执行配置

- 执行日期：`2026-04-24`
- 主目标仓库：`ark-runtime`
- Claude 并发上限：`3`
- 第一波入口选择：`claude-default`
- 第二波核心 benchmark：`claude-native-only`、`claude-quickdep-first`、`claude-quickdep-plus-native-tools`
- 第三波真实开发流：`s6 Incremental Watcher Refresh`、`s7 No-Anchor Workflow Triage`、`s8 Editor Context Risk Triage`
- 第四波跨语言 sanity：`tokio`、`nest`、`gin`、`requests`、`fmt`
- 本轮原始产物保存在执行机本地 `/tmp/quickdep-experiments/`，不提交到仓库

## 3. 当前状态

| 波次 | 状态 | 备注 |
| --- | --- | --- |
| 第一波入口选择实验 | `Completed` | 4 个场景全部跑完 |
| 第二波核心 benchmark | `Completed` | `ark-runtime` 4 个核心场景全部跑完 |
| 第三波真实开发流专项 | `Completed` | 无锚点、编辑器上下文、增量更新 3 个场景全部跑完 |
| 第四波跨语言 sanity | `Completed` | `tokio`、`nest`、`gin`、`requests`、`fmt` 5 个仓库全部跑完 |

## 4. 第一波入口选择实验

### 结果总览

| 入口 | 状态 | 问题 | 期望入口 | Claude 第一跳 | 是否命中正确入口 | 首次命中前是否搜索扩散 | 首次命中前触达文件数 | 首次命中前源码读取字符数 | 首次命中时间 ms | 备注 |
| --- | --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | --- |
| Workflow 入口 | `Completed` | 为什么审批通过后仍可能停留在 `Queued` | `analyze_workflow_context` 或 `get_task_context` | `mcp__quickdep__analyze_workflow_context` | 是 | 否 | 0 | 0 | 15815.75 | 第一跳正确，后续收敛到 6 个关键文件 |
| Behavior 入口 | `Completed` | 为什么 `verify_pre_dispatch` 失败会升级成 turn failure | `analyze_behavior_context` 或 `get_task_context` | `mcp__quickdep__analyze_behavior_context` | 是 | 否 | 0 | 0 | 17045.98 | 第一跳正确，答案质量较高 |
| Impact 入口 | `Completed` | 如果修改 `next_conflict_queue_head`，风险面在哪里 | `analyze_change_impact` 或 `get_task_context` | `mcp__quickdep__analyze_change_impact` | 是 | 否 | 0 | 0 | 12983.72 | 第一跳正确，后续补了少量定点源码确认 |
| Locate 入口 | `Completed` | 如果要先理解 `PlatformServer::health_report`，最值得先看哪些局部点 | `locate_relevant_code` 或 `get_task_context` | `mcp__quickdep__analyze_behavior_context` | 否 | 否 | 0 | 0 | 6386.28 | 入口误判，但仍较快命中目标文件，后续出现 `Grep` |

### 第一波结论

- 4 个入口里命中 3 个，当前第一跳命中率是 `75%`
- `Locate` 场景是当前最明确的路由问题，Claude 会把“先看哪里”误判成行为分析
- 即使入口误判，这 4 个场景在首次命中前都没有发生大范围搜索扩散

## 5. 第二波核心 benchmark

### 工作流问题

问题：为什么审批通过后，execution 仍然可能停留在 `Queued`？

| 路线 | 状态 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude 原生工具 Only | `Completed` | 26637.42 | 131774 | 162 | 46041 | 0 | 434936 | 3 | 分数略高，但明显发生大范围搜索和文件扩散 |
| Claude QuickDep First | `Completed` | 11976.65 | 77504 | 9 | 9880 | 19145 | 265239 | 2 | 更快也更收敛，但金标准符号覆盖不足 |
| Claude QuickDep Plus Native Tools | `Completed` | 12135.93 | 75059 | 7 | 18315 | 14620 | 310253 | 2 | 同样更快更收敛，但关键符号覆盖仍不足 |

### 失败传播问题

问题：为什么 `verify_pre_dispatch` 失败会升级成 turn failure？

| 路线 | 状态 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude 原生工具 Only | `Completed` | 5294.30 | 56247 | 7 | 12836 | 0 | 236958 | 2 | 最快，但答案覆盖偏浅 |
| Claude QuickDep First | `Completed` | 14817.72 | 112803 | 22 | 21058 | 39830 | 511289 | 4 | 本场景质量最好，但代价最高 |
| Claude QuickDep Plus Native Tools | `Completed` | 14283.67 | 84284 | 8 | 31213 | 8903 | 305564 | 3 | 质量和代价都介于两者之间 |

### 调用链问题

问题：`RuntimeCore::next_conflict_queue_head` 到 `Scheduler::dispatchable_head` 的真实调用链是什么？

| 路线 | 状态 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude 原生工具 Only | `Completed` | 5547.79 | 51691 | 6 | 7919 | 0 | 199570 | 4 | 结果可用，但仍依赖 grep/read 手工串链 |
| Claude QuickDep First | `Completed` | 7175.52 | 51069 | 5 | 0 | 8174 | 135170 | 4 | 金标准文件和符号都命中，几乎不读源码 |
| Claude QuickDep Plus Native Tools | `Completed` | 6157.60 | 50719 | 3 | 2180 | 3470 | 165730 | 4 | 分数相同，但阅读范围最小 |

### 风险面问题

问题：如果修改 `next_conflict_queue_head`，哪些调用路径和回归点最容易被改坏？

| 路线 | 状态 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude 原生工具 Only | `Completed` | 5454.60 | 67804 | 5 | 23429 | 0 | 330783 | 3 | 漏掉关键恢复路径 |
| Claude QuickDep First | `Completed` | 15094.49 | 73211 | 7 | 0 | 28790 | 187806 | 4 | 本场景质量最好，风险面覆盖更完整 |
| Claude QuickDep Plus Native Tools | `Completed` | 12869.73 | 103061 | 8 | 17292 | 11577 | 505421 | 3 | 结果可用，但代价偏高且符号覆盖不稳 |

### 第二波汇总

| 路线 | 4 个场景得分 | 平均得分 | 平均总耗时 ms | 平均触达文件数 |
| --- | --- | ---: | ---: | ---: |
| Claude 原生工具 Only | `3, 2, 4, 3` | 3.00 | 76879.00 | 45.00 |
| Claude QuickDep First | `2, 4, 4, 4` | 3.50 | 78646.75 | 10.75 |
| Claude QuickDep Plus Native Tools | `2, 3, 4, 3` | 3.00 | 78280.75 | 6.50 |

### 第二波结论

- 如果只看平均分，当前最好的是 `Claude QuickDep First`
- 如果只看平均触达文件数，当前最好的是 `Claude QuickDep Plus Native Tools`
- 如果只看平均总耗时，三条路线差距不大，当前数据不支持“QuickDep 在所有问题上都更快”
- 当前更稳的结论是：QuickDep 明显改善了收敛能力，但速度收益依赖问题类型

## 6. 第三波真实开发流专项

### 无锚点问题

问题：审批已经点过通过了，但这个 execution 还是没跑起来，一直卡在排队。先别全仓库乱搜，告诉我最应该先看的链路和关键位置。

| 路线 | 状态 | 第一跳 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude 默认行为 | `Completed` | `mcp__quickdep__analyze_workflow_context` | 16207.46 | 133913 | 6 | 17515 | 14217 | 363554 | 4 | Claude 主动选中了正确的 workflow 入口 |
| Claude QuickDep Plus Native Tools | `Completed` | `mcp__quickdep__analyze_behavior_context` | 24410.03 | 96187 | 12 | 18289 | 12427 | 414429 | 3 | 入口偏到了 behavior，漏掉审批恢复主链路 |

### 编辑器上下文问题

问题：基于当前编辑器上下文，如果我要改 `RuntimeCore::next_conflict_queue_head`，最应该先看的局部点和回归风险是什么。

| 路线 | 状态 | 第一跳 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude 默认行为 | `Completed` | `mcp__quickdep__analyze_change_impact` | 6588.39 | 106645 | 4 | 28814 | 1746 | 268207 | 4 | 入口正确，风险面覆盖可用 |
| Claude QuickDep Plus Native Tools | `Completed` | `mcp__quickdep__analyze_change_impact` | 5477.61 | 73172 | 3 | 11810 | 1228 | 205084 | 4 | 同样正确，而且更快也更收敛 |

### 增量更新问题

- 状态：`Completed`
- 结论：两条 QuickDep 路线都成功在不做 `rebuild_database` 的前提下观测到新增符号和新增依赖

| 路线 | 状态 | 总耗时 ms | 刷新延迟 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude QuickDep First | `Completed` | 71501 | 19725.35 | 4 | 4628 | 8257 | 347698 | 5 | 新符号和 3 条新依赖都被正确观测到 |
| Claude QuickDep Plus Native Tools | `Completed` | 73936 | 13895.64 | 4 | 5918 | 8099 | 350198 | 5 | 同样成功，脚本测得刷新延迟更短 |

### 第三波结论

- Claude 默认行为已经能在无锚点 workflow 问题上主动选中 `analyze_workflow_context`，说明自然语言问题路由开始具备实用性
- 但 `Claude QuickDep Plus Native Tools` 在 `s7` 仍然会把“审批后还在排队”误判成 behavior 问题，这说明 prompt 和路由提示还不够稳
- 编辑器上下文场景是本轮最健康的真实开发流案例：两条路线都走对入口，`QuickDep Plus Native Tools` 还把首次命中时间压到 `5.48s`、触达文件数压到 `3`
- QuickDep watcher / 增量更新在本轮实验中可用，但正式口径必须写成“脚本测得刷新延迟约 `13.9s` 到 `19.7s`”，不能写成“近乎即时”

## 7. 第四波跨语言 sanity

这波只测“局部边界问题”而不是跨模块工作流问题，所以结论只能覆盖“理解一个局部主流程时，QuickDep 和原生工具各自表现如何”。

| 仓库 | 语言 | 路线 | 状态 | 首次命中时间 ms | 总耗时 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 最终评分 | 备注 |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `tokio` | Rust | Claude QuickDep Plus Native Tools | `Completed` | 8214.40 | 78041 | 9 | 8451 | 9135 | 3 | 降低了源码读取，但边界包仍偏宽 |
| `tokio` | Rust | Claude 原生工具 Only | `Completed` | 6520.21 | 61761 | 40 | 103778 | 0 | 3 | 更快，但文件扩散很大 |
| `nest` | TypeScript | Claude QuickDep Plus Native Tools | `Completed` | 6995.77 | 66883 | 17 | 4849 | 22198 | 3 | 第一跳用了 `scan_project`，包面过宽 |
| `nest` | TypeScript | Claude 原生工具 Only | `Completed` | 6464.27 | 58467 | 5 | 53978 | 0 | 4 | 质量更高，定位更直接 |
| `gin` | Go | Claude QuickDep Plus Native Tools | `Completed` | 9596.12 | 63053 | 12 | 6357 | 26338 | 3 | 符号覆盖高，但文件面偏大 |
| `gin` | Go | Claude 原生工具 Only | `Completed` | 7006.71 | 65102 | 3 | 3601 | 0 | 4 | 关键局部点更集中 |
| `requests` | Python | Claude QuickDep Plus Native Tools | `Completed` | 12336.24 | 61700 | 9 | 8855 | 40072 | 3 | 读码更少，但收敛还不够紧 |
| `requests` | Python | Claude 原生工具 Only | `Completed` | 11966.21 | 75455 | 2 | 11711 | 0 | 4 | 质量更高，但仍要靠原生读码 |
| `fmt` | C++ | Claude QuickDep Plus Native Tools | `Completed` | 10439.79 | 55336 | 6 | 10336 | 3266 | 3 | 头文件和实现文件都能命中，但仍偏宽 |
| `fmt` | C++ | Claude 原生工具 Only | `Completed` | 5999.08 | 49099 | 11 | 4532 | 0 | 3 | 更快，但没有更完整 |

### 第四波汇总

| 路线 | 5 个场景得分 | 平均得分 | 平均总耗时 ms | 平均触达文件数 | 平均源码读取字符数 |
| --- | --- | ---: | ---: | ---: | ---: |
| Claude QuickDep Plus Native Tools | `3, 3, 3, 3, 3` | 3.00 | 65002.60 | 10.60 | 7769.60 |
| Claude 原生工具 Only | `3, 4, 4, 4, 3` | 3.60 | 61976.80 | 12.20 | 35520.00 |

### 第四波结论

- 第四波证明了 QuickDep 的跨语言链路是通的：5 个不同语言仓库都能正常扫描、提问、返回可用答案
- 但这波数据**不支持**“QuickDep 在跨语言局部边界问题上已经优于原生工具”的结论；当前 `Claude 原生工具 Only` 平均分是 `3.6`，高于 `QuickDep Plus Native Tools` 的 `3.0`
- QuickDep 仍然有明确价值：它把平均源码读取字符数从 `35520` 压到了 `7769.6`，显著减少了盲读源码的量
- 当前最大问题不是“不能跨语言”，而是“局部边界包太宽”，导致 QuickDep 路线虽然少读源码，却经常把候选文件铺得太开，甚至让质量不如原生路线
- 因为这波只覆盖局部边界问题，不能反推到所有跨语言场景；跨模块工作流和影响分析还需要单独设计第二轮跨语言实验

## 8. 当前结论

- 本轮数据支持的主故事不是“QuickDep 让所有问题都更快”，而是“QuickDep 更适合做结构化收敛层，帮助 Agent 先缩到正确代码区域”
- 在 `ark-runtime` 的跨文件 workflow、调用链、风险分析问题里，QuickDep 明显减少了文件扩散；`Claude QuickDep First` 平均分最高，`Claude QuickDep Plus Native Tools` 平均触达文件数最低
- 在真实开发流里，QuickDep 已经能支撑无锚点问题、编辑器上下文问题和增量更新，但路由还不够稳，尤其是 `s7` 暴露了 workflow/behavior 入口还会混淆
- 在跨语言局部边界问题里，QuickDep 已经证明“能用”，但还没证明“更好用”；当前它更像一个减少盲读源码的辅助手段，而不是原生搜索的普适替代
- 当前最明确的产品问题有两个：`Locate`/workflow 类入口路由稳定性不够，以及跨语言边界包过宽
- 当前最明确的工程能力亮点仍然是增量更新，它已经能在真实改动后反映新符号和新依赖

## 9. 下一步

1. 修正 `Locate` 和无锚点 workflow 场景的入口路由，让 Claude 更稳定地区分 `workflow`、`behavior`、`locate`
2. 把跨语言“局部边界包”收紧到更少的候选文件和候选符号，优先解决 `nest`、`gin`、`requests` 里包面过宽的问题
3. 避免在已索引项目上把 `scan_project` 当成第一跳，降低无效 MCP 步骤
4. 为跨语言第二轮实验补充“跨模块工作流 / 影响分析”场景，验证 QuickDep 真正擅长的问题类型在非 Rust 仓库里的表现
5. 继续优化 `s1` 工作流包和 `s5` 风险包，让关键符号覆盖更完整，同时压低额外上下文

## 10. 不允许写入这份报告的内容

- 旧实验数字
- 没有原始日志支撑的估算值
- 使用旧的单字母路线缩写的表格
