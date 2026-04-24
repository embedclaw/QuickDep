# QuickDep 安装 Prompt

把下面的提示词整段贴给你的 Claude Code、Codex 或 OpenCode。它会先检查公开分发渠道是否真的可用，再自动回退到源码安装，并把 QuickDep 接入当前 Agent 客户端。

截至 `2026-04-24`，我们已经实测通过的是：

- `cargo install --path .`
- `quickdep install-mcp claude`
- `quickdep install-mcp codex`
- `quickdep install-mcp opencode`

GitHub Release、Homebrew、npm 的发布链路已经准备好，但当前还没有公开上线。所以这个 prompt 会先检查它们，再决定是否回退到源码安装。

## Claude Code

```md
你在我的本机终端里工作。请帮我安装 QuickDep，并把它接入 Claude Code。

按下面步骤执行，不要跳步：

1. 先记录我当前的项目目录为 `ORIGINAL_PROJECT_DIR="$(pwd)"`，后面扫描项目时必须回到这个目录。
2. 识别当前操作系统和 CPU 架构。
3. 先检查公开分发方式是否真实可用，按这个顺序尝试：
   - GitHub Releases: https://github.com/Northcipher/QuickDep/releases
   - Homebrew: brew install northcipher/tap/quickdep
   - npm: npm i -g @northcipher/quickdep
4. 如果这三种方式都不可用，回退到源码安装：
   - 如果本机已经有 QuickDep 仓库，就进入仓库根目录执行 `cargo install --path .`
   - 如果本机还没有 QuickDep 仓库，就先 clone `https://github.com/Northcipher/QuickDep.git`，再执行 `cargo install --path .`
5. 安装完成后必须验证：
   - `quickdep --version`
   - `quickdep install-mcp claude`
   - `claude mcp list`
6. 然后切回 `ORIGINAL_PROJECT_DIR`，把这个目录作为第一个项目跑起来：
   - 先执行 `cd "$ORIGINAL_PROJECT_DIR"`
   - 再执行 `quickdep scan "$ORIGINAL_PROJECT_DIR"`
   - 如果我要 HTTP 接口，再执行 `quickdep --http 8080 --http-only`
   - 再检查 `curl http://127.0.0.1:8080/health`
7. 最后告诉我：
   - 最终使用的是哪种安装方式
   - 哪些公开渠道不可用，失败信息是什么
   - QuickDep 是否已经出现在 Claude Code 的 MCP 列表里
   - 第一个项目是否扫描成功
```

## Codex

```md
你在我的本机终端里工作。请帮我安装 QuickDep，并把它接入 Codex。

按下面步骤执行，不要跳步：

1. 先记录我当前的项目目录为 `ORIGINAL_PROJECT_DIR="$(pwd)"`，后面扫描项目时必须回到这个目录。
2. 识别当前操作系统和 CPU 架构。
3. 先检查公开分发方式是否真实可用，按这个顺序尝试：
   - GitHub Releases: https://github.com/Northcipher/QuickDep/releases
   - Homebrew: brew install northcipher/tap/quickdep
   - npm: npm i -g @northcipher/quickdep
4. 如果这三种方式都不可用，回退到源码安装：
   - 如果本机已经有 QuickDep 仓库，就进入仓库根目录执行 `cargo install --path .`
   - 如果本机还没有 QuickDep 仓库，就先 clone `https://github.com/Northcipher/QuickDep.git`，再执行 `cargo install --path .`
5. 安装完成后必须验证：
   - `quickdep --version`
   - `quickdep install-mcp codex`
   - `codex mcp list`
6. 然后切回 `ORIGINAL_PROJECT_DIR`，把这个目录作为第一个项目跑起来：
   - 先执行 `cd "$ORIGINAL_PROJECT_DIR"`
   - 再执行 `quickdep scan "$ORIGINAL_PROJECT_DIR"`
   - 如果我要 HTTP 接口，再执行 `quickdep --http 8080 --http-only`
   - 再检查 `curl http://127.0.0.1:8080/health`
7. 最后告诉我：
   - 最终使用的是哪种安装方式
   - 哪些公开渠道不可用，失败信息是什么
   - QuickDep 是否已经出现在 Codex 的 MCP 列表里
   - 第一个项目是否扫描成功
```

## OpenCode

```md
你在我的本机终端里工作。请帮我安装 QuickDep，并把它接入 OpenCode。

按下面步骤执行，不要跳步：

1. 先记录我当前的项目目录为 `ORIGINAL_PROJECT_DIR="$(pwd)"`，后面扫描项目时必须回到这个目录。
2. 识别当前操作系统和 CPU 架构。
3. 先检查公开分发方式是否真实可用，按这个顺序尝试：
   - GitHub Releases: https://github.com/Northcipher/QuickDep/releases
   - Homebrew: brew install northcipher/tap/quickdep
   - npm: npm i -g @northcipher/quickdep
4. 如果这三种方式都不可用，回退到源码安装：
   - 如果本机已经有 QuickDep 仓库，就进入仓库根目录执行 `cargo install --path .`
   - 如果本机还没有 QuickDep 仓库，就先 clone `https://github.com/Northcipher/QuickDep.git`，再执行 `cargo install --path .`
5. 安装完成后必须验证：
   - `quickdep --version`
   - `quickdep install-mcp opencode`
   - `opencode mcp list`
6. 然后切回 `ORIGINAL_PROJECT_DIR`，把这个目录作为第一个项目跑起来：
   - 先执行 `cd "$ORIGINAL_PROJECT_DIR"`
   - 再执行 `quickdep scan "$ORIGINAL_PROJECT_DIR"`
   - 如果我要 HTTP 接口，再执行 `quickdep --http 8080 --http-only`
   - 再检查 `curl http://127.0.0.1:8080/health`
7. 最后告诉我：
   - 最终使用的是哪种安装方式
   - 哪些公开渠道不可用，失败信息是什么
   - QuickDep 是否已经出现在 OpenCode 的 MCP 列表里
   - 第一个项目是否扫描成功
```

## 一句话版本

如果你只想给工程师一个最短提示，可以直接发这一句：

```md
请先检查 QuickDep 的 GitHub Release / Homebrew / npm 是否真实可用；如果都不可用就从源码 `cargo install --path .` 安装，然后执行 `quickdep install-mcp <claude|codex|opencode>`，用对应客户端的 `mcp list` 验证，最后扫描当前项目并告诉我结果。
```
