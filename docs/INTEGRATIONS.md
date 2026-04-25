# QuickDep Agent 集成与分发

## 1. 目标

QuickDep 的推荐运行形态是本地 `stdio MCP` 服务：

```bash
quickdep serve
```

这样最适合：

- Codex
- Claude Code
- OpenCode

原因：

- QuickDep 需要直接访问本机仓库
- 需要在本地创建 `.quickdep/` 数据和 watcher
- 不需要额外部署远程服务

---

## 2. 安装 QuickDep

截至 `2026-04-24`，当前实际验证结果如下：

| 路径 | 状态 | 验证方式 |
| --- | --- | --- |
| `cargo install --path .` | 已验证可用 | 实测安装成功，`quickdep --version` 返回 `0.1.0` |
| `quickdep install-mcp claude` | 已验证可用 | `claude mcp list` 显示 QuickDep 已连接 |
| `quickdep install-mcp codex` | 已验证可用 | `codex mcp list` 可见 QuickDep |
| `quickdep install-mcp opencode` | 已验证可用 | `opencode mcp list` 显示 QuickDep 已连接 |
| GitHub Releases | 尚未发布 | 最新下载地址当前返回 `404` |
| Homebrew | 尚未发布 | `Formula/quickdep.rb` 当前返回 `404` |
| npm 包装器 | 尚未发布 | `npm view @northcipher/quickdep` 当前返回 `E404` |

所以今天如果要真正装起来，应该直接用源码安装：

```bash
cargo install --path .
```

安装后确认：

```bash
quickdep --version
```

---

## 3. 一键安装到 Agent 工具

QuickDep 提供 `install-mcp` 子命令来写入或调用目标客户端配置。

### 3.1 Claude Code

```bash
quickdep install-mcp claude
```

可选：

```bash
quickdep install-mcp claude --scope user
quickdep install-mcp claude --scope project
quickdep install-mcp claude --name quickdep-local
```

底层等价于：

```bash
claude mcp add --scope local quickdep -- /absolute/path/to/quickdep serve
```

### 3.2 Codex

```bash
quickdep install-mcp codex
```

底层等价于：

```bash
codex mcp add quickdep -- /absolute/path/to/quickdep serve
```

Codex 会把配置写入其 MCP 配置中，通常由 `~/.codex/config.toml` 管理。

### 3.3 OpenCode

```bash
quickdep install-mcp opencode
```

默认写入：

```text
~/.config/opencode/opencode.json
```

也可以指定路径：

```bash
quickdep install-mcp opencode --opencode-config /path/to/opencode.json
```

生成的配置片段类似：

```json
{
  "mcp": {
    "quickdep": {
      "type": "local",
      "command": ["/absolute/path/to/quickdep", "serve"]
    }
  }
}
```

### 3.4 Dry Run

查看将要执行的动作而不实际写入：

```bash
quickdep install-mcp claude --dry-run
quickdep install-mcp codex --dry-run
quickdep install-mcp opencode --dry-run
```

---

## 4. 手动配置示例

### 4.1 Claude Code

```bash
claude mcp add --scope local quickdep -- /absolute/path/to/quickdep serve
```

### 4.2 Codex

```bash
codex mcp add quickdep -- /absolute/path/to/quickdep serve
```

### 4.3 OpenCode

在 `~/.config/opencode/opencode.json` 中加入：

```json
{
  "mcp": {
    "quickdep": {
      "type": "local",
      "command": ["/absolute/path/to/quickdep", "serve"]
    }
  }
}
```

---

## 5. 推荐的仓库内提示

如果希望 agent 更稳定地优先使用 QuickDep，可在仓库的 `AGENTS.md` 或 `CLAUDE.md` 中加入类似说明：

```md
Use the `quickdep` MCP server for symbol lookup, dependency tracing, and cross-file interface queries before falling back to raw text search.
```

---

## 6. 分发策略

推荐分发优先级：

1. GitHub Releases
2. Homebrew
3. npm
4. cargo install

### 6.1 GitHub Releases

发布工作流已经配置好；在仓库打 `v*` tag 后，会构建以下产物并准备上传。当前仓库还没有公开 release，所以这些 URL 还不能直接下载。

Release 产物统一命名：

- `quickdep-darwin-aarch64.tar.gz`
- `quickdep-linux-x86_64.tar.gz`
- `quickdep-windows-x86_64.zip`
- `checksums.txt`

这些产物会由 [`.github/workflows/release.yml`](../.github/workflows/release.yml) 在发布时自动构建并上传。

### 6.2 Homebrew

Homebrew 计划使用独立 tap，例如：

```bash
brew install northcipher/tap/quickdep
```

公式脚本已经准备好，但 tap / formula 当前还没有公开发布。发布后，公式会直接引用 GitHub Release 产物和 SHA256。

### 6.3 npm

npm 只作为二进制包装器，目标命令如下：

```bash
npm i -g @northcipher/quickdep
```

包装脚本已经在仓库中准备好，但 npm registry 当前还没有公开发布。发布后，安装脚本会根据平台下载对应的 GitHub Release 产物，并把 `quickdep` 放到包内 `bin` 目录下。

---

## 7. 当前边界

- QuickDep 当前推荐的是本地 `stdio MCP`
- 远程 MCP / 托管 SaaS 不在本轮交付范围内

---

## 8. 后续建议

后续可继续补充：

- 自动生成 `AGENTS.md` 提示片段
- `quickdep doctor` 检查 agent 集成状态
- 发布后自动更新 Homebrew tap
- npm 自动发布流程
