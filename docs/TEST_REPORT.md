# QuickDep 测试报告

## 1. 目的

本文档记录当前 QuickDep 的实测覆盖范围、测试结果、已确认边界，以及后续仍需补强的方向。

测试目标不是只验证：

- 能否成功扫描仓库

而是同时验证：

- 是否能提取真实符号
- 是否能建立合理依赖边
- watcher 和增量更新是否生效
- 并发查询时服务是否稳定
- 在真实仓库规模下是否仍能工作

---

## 2. 测试环境

- 项目路径：当前仓库根目录
- 执行方式：
  - `quickdep scan`
  - `quickdep debug --file`
  - `quickdep debug --deps`
  - HTTP `serve --http-only`
  - HTTP `/api/interfaces/search`
  - HTTP `/api/dependencies`
  - HTTP `/api/query/batch`
- 数据存储：项目目录下 `.quickdep/`

---

## 3. 测试范围

本轮覆盖了以下类别：

- 最小复现工程语义测试
- 多语言真实仓库扫描测试
- 文件级接口提取测试
- 接口依赖查询测试
- watcher 实时更新测试
- 文件 rename / 依赖迁移测试
- 并发 HTTP 查询压力测试
- 中文文件名 / 中文符号测试
- `batch_query` 组合查询测试

---

## 4. 最小工程语义测试

以下最小工程场景已手工构造并验证：

| 场景 | 结果 | 结论 |
|------|------|------|
| TypeScript `tsconfig paths` | 通过 | `@/utils/math` 可解析到真实目标符号 |
| TypeScript re-export | 通过 | `export { foo } from './core'` 可追到 `core.ts::foo` |
| Rust `pub use` | 通过 | `main_call -> inner::foo` 可解析 |
| Rust `derive` 宏边界 | 通过（边界明确） | 手写结构体/函数可见，宏展开代码不可见 |
| Go 泛型函数 | 通过 | `Use -> Map[T any]` 可建立依赖边 |
| Go goroutine/channel | 通过 | `Use -> worker` 可识别；并发语义本身不做动态分析 |
| C 头/源映射 | 通过 | `main.c -> math.c::add` 可解析 |
| C++ namespace/template/include | 通过 | `run -> Box::get` 与 `helper` 可解析 |

结论：

- 之前部分 review 中关于 `TS paths`、`TS re-export`、`Rust pub use`、`Go 泛型` 不支持的判断，已经不符合当前代码状态。

---

## 5. 真实仓库测试

### 5.1 已测试仓库

| 语言 | 仓库 | 结果 |
|------|------|------|
| TypeScript | `nest` | 通过 |
| Python | `flask` | 通过 |
| Python | `requests` | 通过 |
| Go | `cobra` | 通过 |
| Go | `gin` | 通过 |
| Rust | `ripgrep` | 通过 |
| Rust | `tokio` | 通过 |
| C++ | `fmt` | 通过 |
| C | `redis` | 通过 |
| JavaScript | `axios` | 扫描成功；历史上的“默认不支持”结论已过期 |

### 5.2 扫描结果汇总

| 仓库 | 文件数 | 符号数 | 依赖数 |
|------|--------|--------|--------|
| `nest` | 1279 | 5800 | 4002 |
| `flask` | 35 | 499 | 569 |
| `requests` | 21 | 312 | 437 |
| `cobra` | 19 | 370 | 968 |
| `gin` | 已验证可扫描和查询 | 未单独记录统计值 |
| `ripgrep` | 85 | 3679 | 7321 |
| `tokio` | 499 | 5608 | 4631 |
| `fmt` | 76 | 2029 | 3246 |
| `redis` | 719 | 12023 | 41136 |

### 5.3 文件级接口抽样

以下文件经 `quickdep debug <repo> --file <path>` 实测可提取接口：

- `flask`: `src/flask/app.py`
- `requests`: `src/requests/api.py`
- `cobra`: `command.go`
- `gin`: `gin.go`
- `nest`: `packages/core/nest-factory.ts`
- `ripgrep`: `crates/core/main.rs`
- `tokio`: `tokio/src/lib.rs`
- `fmt`: `include/fmt/format.h`
- `redis`: `src/server.c`

说明：

- 这一步验证的不是扫描统计，而是目标文件中确实可以提取出结构化符号。

### 5.4 依赖抽样

以下接口经 `quickdep debug <repo> --deps <qualified_name>` 实测能返回合理依赖：

- `flask`: `src/flask/app.py::Flask::__init__`
- `requests`: `src/requests/api.py::get`
- `cobra`: `command.go::Command::Context`
- `nest`: `packages/core/nest-factory.ts::NestFactoryStatic::create`
- 最小工程：
  - `src/index.ts::main -> src/utils/math.ts::add`
  - `src/index.ts::main -> src/core.ts::foo`
  - `src/lib.rs::main_call -> src/inner.rs::foo`
  - `main.go::Use -> main.go::Map`
  - `src/main.c::run -> src/math.c::add`
  - `src/main.cpp::app::run -> src/box.h::app::Box::get`
  - `src/main.cpp::app::run -> src/box.cpp::app::helper`

---

## 6. 动态行为测试

### 6.1 watcher 实时更新

已验证：

- 修改文件后，新增符号能够在短时间内被查询到
- 修改依赖调用后，依赖边会更新
- 高频覆盖写入后，最终状态能收敛到最新版本

### 6.2 文件 rename / 依赖迁移

构造场景：

- 初始：
  - `src/b.py::caller -> src/a.py::helper`
- rename 后：
  - `a.py -> a_new.py`
  - 更新 import 为 `from a_new import helper`

实测结果：

- rename 前查询结果：`src/b.py::caller -> src/a.py::helper`
- rename 后查询结果：`src/b.py::caller -> src/a_new.py::helper`

结论：

- 在 rename 后源码同步调整的前提下，watcher 和增量扫描可以把依赖边迁移到新文件。

### 6.3 并发 HTTP 查询压力

构造场景：

- HTTP 服务对外提供 `/api/interfaces/search`
- 8 并发线程持续查询
- 同时对同一个源文件进行 20 次高频改写

实测结果：

- 成功查询次数：`10846`
- 错误次数：`0`

结论：

- 本轮修复后，并发查询 + watcher 增量扫描的已知 `already loading` 500 问题未再复现。

### 6.4 batch_query

已验证：

- `find_interfaces`
- `get_file_interfaces`
- `get_dependencies`

通过 HTTP `/api/query/batch` 组合调用，3 个子查询全部成功返回。

---

## 7. 中文场景测试

### 7.1 中文文件名与中文符号

构造文件：

- `src/搜索.py`
- 函数：
  - `获取用户`
  - `获取订单`

实测结果：

- 文件查询可返回中文函数名
- 依赖查询可返回：
  - `src/搜索.py::获取订单 -> src/搜索.py::获取用户`

结论：

- 中文文件名、中文符号、中文依赖链在解析层面正常。

### 7.2 中文搜索边界

说明：

- 当前已验证的是中文符号解析和精确依赖查询
- 尚未证明中文全文搜索体验足够好
- FTS5 对 CJK 分词较弱这一点仍然成立

---

## 8. 已确认边界

以下内容属于当前产品边界，而不是这轮测试中的故障：

### 8.1 JavaScript 已默认支持，旧结论已过期

边界样例：`axios`

现状：

- 仓库可以扫描
- `.js` / `.jsx` / `.mjs` / `.cjs` 已在默认支持范围内
- `scan.languages` 默认包含 `javascript`
- 旧实验中的“JavaScript 默认不支持”说法已经与当前代码状态不一致

当前仍需注意：

- CommonJS、动态 `require`、运行时拼接路径这类模式，仍可能降低静态依赖完整性
- JavaScript 真实仓库仍值得继续补更多回归样例

结论：

- 对外文档不应再把“JavaScript 默认不支持”当成当前边界。
- JavaScript 当前更真实的边界是动态语义和生态复杂度，而不是语言开关未开启。

### 8.2 Rust 宏展开不可见

已确认：

- `#[derive(Debug)]` 这类宏不会展开成独立可查询符号
- 但手写的 `struct` / `fn` 本身仍可正常提取

结论：

- 宏生成代码不可见，仍属于当前设计边界。

### 8.3 动态语义不做深推断

包括但不限于：

- 动态分派的真实运行时类型
- 复杂宏展开后的真实语义
- Go channel 运行时通信行为
- 运行时反射或 metaprogramming 生成逻辑

结论：

- QuickDep 的定位仍然是静态符号和直接依赖分析，不是动态语义执行器。

---

## 9. 本轮结论

本轮测试表明：

- QuickDep 当前在 `Rust / TypeScript / JavaScript / Python / Go / C / C++` 七类语言上的主能力链路已被实测覆盖
- 真实仓库扫描在中大型规模下可工作
- 文件级接口提取和依赖查询在真实项目中可返回有效结果
- watcher、增量更新、rename 后依赖迁移和并发查询已做实测
- 修复后的并发扫描问题未再复现

同时也确认：

- JavaScript 已默认纳入当前支持矩阵，但复杂动态语义仍需继续补测
- Rust 宏展开依然不可见
- 中文全文搜索质量仍不是强项

---

## 10. 后续建议

建议继续推进以下方向：

1. 把本报告中的关键最小工程用例固化为自动化集成测试
2. 继续补 JavaScript 真实仓库回归，重点覆盖 CommonJS、mixed module 和动态导入模式
3. 若要提升中文搜索体验，可评估更适合 CJK 的分词方案
4. 若要扩大真实仓库覆盖，可继续加入：
   - 多模块 monorepo
   - 大型 C++ 工程
   - 更重度宏使用的 Rust 仓库
   - 包含大量 rename / 批量变更的 watcher 回归集
