# QuickDep Claude 实验执行手册

## 1. 执行前检查

开始实验前，先确认：

1. QuickDep 可以正常启动
2. Claude CLI 可以正常工作
3. `ark-runtime` 已经在本机，并且 QuickDep 可以扫描
4. QuickDep MCP 已经接到 Claude
5. 当前总并发不超过 `4`

推荐先做一次预热：

```bash
quickdep scan /Users/luozx/work/ark-runtime
quickdep status /Users/luozx/work/ark-runtime
```

## 2. 产物目录

建议统一放到：

```text
/tmp/quickdep-experiments/
```

建议结构：

```text
/tmp/quickdep-experiments/
  wave-1-entry-selection/
  wave-2-core-benchmark/
  wave-3-developer-flow/
  wave-4-cross-language/
```

每个实验至少保存：

- `prompt.md`
- `transcript.jsonl` 或原始日志
- `answer.md`
- `metrics.json`
- `judge.md`

脚本默认输出目录也应保持一致：

```bash
python3 scripts/agent_benchmark.py run --output-dir /tmp/quickdep-experiments/wave-2-core-benchmark
```

## 3. 自动化执行命令

统一使用：

- 路线 ID：`claude-default`
- 路线 ID：`claude-native-only`
- 路线 ID：`claude-quickdep-first`
- 路线 ID：`claude-quickdep-plus-native-tools`

报告里一律显示全称路线名，不再写单字母缩写。

第一波入口选择实验：

```bash
python3 scripts/agent_benchmark.py run \
  --repo /Users/luozx/work/ark-runtime \
  --output-dir /tmp/quickdep-experiments/wave-1-entry-selection \
  --scenarios s1 s2 s4 s5 \
  --routes claude-default \
  --max-workers 1
```

第二波核心 benchmark：

```bash
python3 scripts/agent_benchmark.py run \
  --repo /Users/luozx/work/ark-runtime \
  --output-dir /tmp/quickdep-experiments/wave-2-core-benchmark \
  --scenarios s1 s2 s3 s5 \
  --routes claude-native-only claude-quickdep-first claude-quickdep-plus-native-tools \
  --max-workers 3
```

第三波增量更新专项：

```bash
python3 scripts/agent_benchmark.py run \
  --repo /Users/luozx/work/ark-runtime \
  --output-dir /tmp/quickdep-experiments/wave-3-developer-flow \
  --scenarios s6 \
  --routes claude-quickdep-first claude-quickdep-plus-native-tools \
  --max-workers 2
```

生成 Markdown 汇总：

```bash
python3 scripts/agent_benchmark.py report \
  --output-dir /tmp/quickdep-experiments/wave-2-core-benchmark
```

## 4. 路线 prompt 模板

### Claude 默认行为

```md
你在一个真实代码仓库里回答工程问题。请优先选择最合适的工具，而不是先做大范围搜索。

问题：
{QUESTION}

要求：
1. 先给结论
2. 列出你第一步为什么选择这个工具
3. 说明你最终阅读了哪些文件
4. 说明还有哪些不确定点
```

### Claude 原生工具 Only

```md
你在一个真实代码仓库里回答工程问题。

问题：
{QUESTION}

约束：
1. 不允许使用 QuickDep
2. 只能使用原生搜索、读文件、代码导航
3. 尽量减少无关文件扩散

输出：
1. 结论
2. 关键文件
3. 关键链路
4. 不确定点
```

### Claude QuickDep First

```md
你在一个真实代码仓库里回答工程问题。

问题：
{QUESTION}

约束：
1. QuickDep 必须主导前几步分析
2. 不要先做大范围搜索
3. 只有 QuickDep 无法支撑判断时，才允许少量定点源码确认
4. 如果你读取源码，必须说明是被哪个 QuickDep 结果引导过去的

输出：
1. 结论
2. 关键文件
3. 关键符号或调用链
4. 不确定点
```

### Claude QuickDep Plus Native Tools

```md
你在一个真实代码仓库里回答工程问题。

问题：
{QUESTION}

工作方式：
1. 先用 QuickDep 缩小到最值得读的文件和符号
2. 再用少量源码阅读确认行为细节
3. 不要在没有 QuickDep 收敛证据前做大范围搜索

输出：
1. 结论
2. QuickDep 帮你缩小范围的证据
3. 最终确认时阅读的源码点
4. 不确定点
```

## 5. 第一波实验清单

### 实验 1：Workflow 入口选择

问题：

```text
为什么审批通过后，execution 仍然可能停留在 Queued，而不是直接进入 Running？
```

优先观察：

- Claude 第一跳是否使用 `analyze_workflow_context`
- 如果没有使用，是否至少使用 `get_task_context`

### 实验 2：Behavior 入口选择

问题：

```text
为什么 verify_pre_dispatch 失败后，turn 会直接失败，而不是只跳过当前 execution？
```

优先观察：

- Claude 第一跳是否使用 `analyze_behavior_context`
- 如果没有使用，是否至少使用 `get_task_context`

### 实验 3：Impact 入口选择

问题：

```text
如果我要修改 next_conflict_queue_head，哪些地方最容易被我改坏？
```

优先观察：

- Claude 第一跳是否使用 `analyze_change_impact`
- 如果没有使用，是否至少使用 `get_task_context`

### 实验 4：Locate 入口选择

问题：

```text
如果我要先理解 PlatformServer::health_report，最值得先看哪些局部点？
```

优先观察：

- Claude 第一跳是否使用 `locate_relevant_code`
- 如果没有使用，是否至少使用 `get_task_context`

## 6. 第二波实验清单

### 实验 5：工作流问题

问题：

```text
为什么审批通过后，execution 仍然可能停留在 Queued？
```

路线：

1. Claude 原生工具 Only
2. Claude QuickDep First
3. Claude QuickDep Plus Native Tools

### 实验 6：失败传播问题

问题：

```text
为什么 verify_pre_dispatch 失败会升级成 turn failure？
```

路线：

1. Claude 原生工具 Only
2. Claude QuickDep First
3. Claude QuickDep Plus Native Tools

### 实验 7：调用链问题

问题：

```text
RuntimeCore::next_conflict_queue_head 到 Scheduler::dispatchable_head 的真实调用链是什么？
```

路线：

1. Claude 原生工具 Only
2. Claude QuickDep First
3. Claude QuickDep Plus Native Tools

### 实验 8：风险面问题

问题：

```text
如果修改 next_conflict_queue_head，哪些调用路径和回归点最容易被改坏？
```

路线：

1. Claude 原生工具 Only
2. Claude QuickDep First
3. Claude QuickDep Plus Native Tools

## 7. Judge 规则

每个实验都要单独判断：

1. Claude 有没有先走对入口
2. Claude 有没有明显过度扩散
3. Claude 命中的关键文件是否正确
4. 最终结论有没有覆盖关键链路
5. 有没有出现“答错但非常自信”的情况

## 8. 禁止事项

- 不要多个主场景同时堆满并发
- 不要把旧实验数字直接拷进新报告
- 不要发明临时路线 ID，统一使用 `claude-default`、`claude-native-only`、`claude-quickdep-first`、`claude-quickdep-plus-native-tools`
- 不要在报告里重新引入旧的单字母路线缩写
