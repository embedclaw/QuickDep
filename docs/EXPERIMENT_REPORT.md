# QuickDep Claude 实验报告

> 状态：进行中
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
- 第三波已执行子项：`s6 Incremental Watcher Refresh`
- 本轮原始产物保存在执行机本地 `/tmp/quickdep-experiments/`，不提交到仓库

## 3. 当前状态

| 波次 | 状态 | 备注 |
| --- | --- | --- |
| 第一波入口选择实验 | `Completed` | 4 个场景全部跑完 |
| 第二波核心 benchmark | `Completed` | `ark-runtime` 4 个核心场景全部跑完 |
| 第三波真实开发流专项 | `Partial` | 只完成了增量更新专项，另外 2 个场景待跑 |
| 第四波跨语言 sanity | `Pending` | 尚未开始 |

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

- 状态：`Pending`
- 结果：尚未开始

### 编辑器上下文问题

- 状态：`Pending`
- 结果：尚未开始

### 增量更新问题

- 状态：`Completed`
- 结论：两条 QuickDep 路线都成功在不做 `rebuild_database` 的前提下观测到新增符号和新增依赖

| 路线 | 状态 | 总耗时 ms | 刷新延迟 ms | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 | 备注 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Claude QuickDep First | `Completed` | 71501 | 19725.35 | 4 | 4628 | 8257 | 347698 | 5 | 新符号和 3 条新依赖都被正确观测到 |
| Claude QuickDep Plus Native Tools | `Completed` | 73936 | 13895.64 | 4 | 5918 | 8099 | 350198 | 5 | 同样成功，脚本测得刷新延迟更短 |

### 第三波结论

- QuickDep watcher / 增量更新在本轮实验中可用
- 但正式口径必须写成“脚本测得刷新延迟约 `13.9s` 到 `19.7s`”，不能写成“近乎即时”
- 这一轮只证明了增量更新专项可用，还没有覆盖无锚点问题和编辑器上下文问题

## 7. 第四波跨语言 sanity

| 仓库 | 语言 | 路线 | 状态 | 结果摘要 |
| --- | --- | --- | --- | --- |
| `tokio` | Rust | Claude QuickDep Plus Native Tools | `Pending` | 尚未开始 |
| `nest` | TypeScript | Claude QuickDep Plus Native Tools | `Pending` | 尚未开始 |
| `gin` | Go | Claude QuickDep Plus Native Tools | `Pending` | 尚未开始 |
| `requests` | Python | Claude QuickDep Plus Native Tools | `Pending` | 尚未开始 |
| `fmt` 或 `redis` | C 或 C++ | Claude QuickDep Plus Native Tools | `Pending` | 尚未开始 |

## 8. 当前结论

- 本轮数据已经支持一个更准确的故事：QuickDep 的主要价值是帮助 Claude 更快缩小到正确代码区域，而不是保证每个问题都更快
- 在 `s1` 和 `s3` 这类需要跨文件找工作流或调用链的问题里，QuickDep 明显减少了文件扩散
- 在 `s5` 这类重构前风险分析问题里，`Claude QuickDep First` 当前效果最好
- 在 `s2` 这类局部失败传播问题里，原生路线更快，但 QuickDep 路线答案更完整
- 当前最明确的产品问题是入口路由，尤其是 `Locate` 场景
- 当前最明确的工程能力亮点是增量更新，它已经能在真实改动后反映新符号和新依赖

## 9. 下一步

1. 继续补完第三波剩余两个场景：无锚点问题、编辑器上下文问题
2. 进入第四波跨语言 sanity，确认 Rust 之外的多语言表现
3. 修正 `Locate` 场景的入口路由，让 Claude 更稳定地选择 `locate_relevant_code`
4. 优化 `s1` 工作流包和 `s5` 风险包，让关键符号覆盖更完整
5. 控制 `s2` 和 `s5` 中 QuickDep 路线的额外展开，降低无效上下文

## 10. 不允许写入这份报告的内容

- 旧实验数字
- 没有原始日志支撑的估算值
- 使用旧的单字母路线缩写的表格
