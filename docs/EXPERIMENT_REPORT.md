# QuickDep Claude 实验报告

> 状态：待重跑  
> 说明：这份报告从零开始重建，不继承任何旧实验数字。

## 1. 本轮范围

本轮只接受以下实验结果写入：

1. 第一波入口选择实验
2. 第二波 `ark-runtime` 核心 benchmark
3. 第三波真实开发流专项
4. 第四波跨语言 sanity

## 2. 当前状态

| 波次 | 状态 | 备注 |
| --- | --- | --- |
| 第一波入口选择实验 | `Pending` | 尚未开始 |
| 第二波核心 benchmark | `Pending` | 尚未开始 |
| 第三波真实开发流专项 | `Pending` | 尚未开始 |
| 第四波跨语言 sanity | `Pending` | 尚未开始 |

## 3. 第一波入口选择实验

### Workflow 入口

- 状态：`Pending`
- 问题：为什么审批通过后仍可能停留在 `Queued`
- 期望入口：`analyze_workflow_context` 或 `get_task_context`
- Claude 第一跳：
- 是否命中正确入口：
- 首次命中前是否搜索扩散：
- 首次命中前触达文件数：
- 首次命中前源码读取字符数：
- 首次命中时间：
- 备注：

### Behavior 入口

- 状态：`Pending`
- 问题：为什么 `verify_pre_dispatch` 失败会升级成 turn failure
- 期望入口：`analyze_behavior_context` 或 `get_task_context`
- Claude 第一跳：
- 是否命中正确入口：
- 首次命中前是否搜索扩散：
- 首次命中前触达文件数：
- 首次命中前源码读取字符数：
- 首次命中时间：
- 备注：

### Impact 入口

- 状态：`Pending`
- 问题：如果修改 `next_conflict_queue_head`，风险面在哪里
- 期望入口：`analyze_change_impact` 或 `get_task_context`
- Claude 第一跳：
- 是否命中正确入口：
- 首次命中前是否搜索扩散：
- 首次命中前触达文件数：
- 首次命中前源码读取字符数：
- 首次命中时间：
- 备注：

### Locate 入口

- 状态：`Pending`
- 问题：如果要先理解 `PlatformServer::health_report`，最值得先看哪些局部点
- 期望入口：`locate_relevant_code` 或 `get_task_context`
- Claude 第一跳：
- 是否命中正确入口：
- 首次命中前是否搜索扩散：
- 首次命中前触达文件数：
- 首次命中前源码读取字符数：
- 首次命中时间：
- 备注：

## 4. 第二波核心 benchmark

### 工作流问题

| 路线 | 状态 | 首次命中时间 | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude 原生工具 Only | `Pending` |  |  |  |  |  |  |
| Claude QuickDep First | `Pending` |  |  |  |  |  |  |
| Claude QuickDep Plus Native Tools | `Pending` |  |  |  |  |  |  |

### 失败传播问题

| 路线 | 状态 | 首次命中时间 | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude 原生工具 Only | `Pending` |  |  |  |  |  |  |
| Claude QuickDep First | `Pending` |  |  |  |  |  |  |
| Claude QuickDep Plus Native Tools | `Pending` |  |  |  |  |  |  |

### 调用链问题

| 路线 | 状态 | 首次命中时间 | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude 原生工具 Only | `Pending` |  |  |  |  |  |  |
| Claude QuickDep First | `Pending` |  |  |  |  |  |  |
| Claude QuickDep Plus Native Tools | `Pending` |  |  |  |  |  |  |

### 风险面问题

| 路线 | 状态 | 首次命中时间 | 触达文件数 | 源码读取字符数 | QuickDep 返回字符数 | 总上下文 token | 最终评分 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude 原生工具 Only | `Pending` |  |  |  |  |  |  |
| Claude QuickDep First | `Pending` |  |  |  |  |  |  |
| Claude QuickDep Plus Native Tools | `Pending` |  |  |  |  |  |  |

## 5. 第三波真实开发流专项

### 无锚点问题

- 状态：`Pending`
- 结果：

### 编辑器上下文问题

- 状态：`Pending`
- 结果：

### 增量更新问题

- 状态：`Pending`
- 结果：

## 6. 第四波跨语言 sanity

| 仓库 | 语言 | 路线 | 状态 | 结果摘要 |
| --- | --- | --- | --- | --- |
| `tokio` | Rust | Claude QuickDep Plus Native Tools | `Pending` |  |
| `nest` | TypeScript | Claude QuickDep Plus Native Tools | `Pending` |  |
| `gin` | Go | Claude QuickDep Plus Native Tools | `Pending` |  |
| `requests` | Python | Claude QuickDep Plus Native Tools | `Pending` |  |
| `fmt` 或 `redis` | C 或 C++ | Claude QuickDep Plus Native Tools | `Pending` |  |

## 7. 当前结论

待填写。

## 8. 不允许写入这份报告的内容

- 旧实验数字
- 没有原始日志支撑的估算值
- 使用旧的单字母路线缩写的表格
