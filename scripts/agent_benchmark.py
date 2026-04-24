#!/usr/bin/env python3
"""Run QuickDep Claude experiment scenarios against local repositories."""

from __future__ import annotations

import argparse
import concurrent.futures
import datetime as dt
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import textwrap
import threading
import time
from dataclasses import dataclass
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_TARGET_REPO = pathlib.Path("/Users/luozx/work/ark-runtime")
DEFAULT_BENCH_REPOS_ROOT = pathlib.Path(
    os.environ.get("QUICKDEP_BENCH_REPOS_ROOT", "/Users/luozx/work/bench-repos")
)
DEFAULT_OUTPUT_DIR = pathlib.Path("/tmp/quickdep-experiments")
DEFAULT_QUICKDEP_BIN = REPO_ROOT / "target" / "debug" / "quickdep"
DEFAULT_CLAUDE_BIN = pathlib.Path(shutil.which("claude") or "/opt/homebrew/bin/claude")
BENCH_REPO_DIRS = {
    "tokio": "tokio-master",
    "nest": "nest-master",
    "gin": "gin-master",
    "requests": "requests-main",
    "fmt": "fmt-main",
    "redis": "redis-unstable",
}
SOURCE_EXTENSIONS = {
    ".rs",
    ".py",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".java",
    ".kt",
    ".php",
    ".go",
    ".c",
    ".cc",
    ".cpp",
    ".h",
    ".hpp",
    ".cs",
    ".json",
    ".toml",
    ".md",
}
SEARCH_TOOL_NAMES = {"Grep", "Glob"}
READ_TOOL_NAMES = {"Read"}
SOURCE_PAYLOAD_TOOL_NAMES = {"LSP"}
SHELL_SEARCH_PATTERN = re.compile(r"\b(rg|grep|ag|fd|find)\b")
SHELL_READ_PATTERN = re.compile(r"\b(cat|sed|awk|head|tail|less|more)\b")
RELATIVE_FILE_PATTERN = re.compile(
    r"(?<![A-Za-z0-9_./+\-])([A-Za-z0-9_+\-]+(?:/[A-Za-z0-9_./+\-]+)*\.[A-Za-z0-9_+\-]+)"
)

ROUTE_DISPLAY_NAMES = {
    "claude-default": "Claude 默认行为",
    "claude-native-only": "Claude 原生工具 Only",
    "claude-quickdep-first": "Claude QuickDep First",
    "claude-quickdep-plus-native-tools": "Claude QuickDep Plus Native Tools",
}
ROUTE_ALIASES = {
    "q": "claude-quickdep-first",
    "n": "claude-native-only",
    "h": "claude-quickdep-plus-native-tools",
    "default": "claude-default",
    "native": "claude-native-only",
    "native-only": "claude-native-only",
    "quickdep-first": "claude-quickdep-first",
    "quickdep-plus-native-tools": "claude-quickdep-plus-native-tools",
}
BENCHMARK_ROUTE_IDS = (
    "claude-native-only",
    "claude-quickdep-first",
    "claude-quickdep-plus-native-tools",
)
ALL_ROUTE_IDS = ("claude-default",) + BENCHMARK_ROUTE_IDS
INCREMENTAL_ROUTE_IDS = (
    "claude-quickdep-first",
    "claude-quickdep-plus-native-tools",
)

ENTRY_SELECTION_EXPECTATIONS = {
    "s1": {
        "label": "Workflow 入口",
        "expected_tools": (
            "mcp__quickdep__analyze_workflow_context",
            "mcp__quickdep__get_task_context",
        ),
    },
    "s2": {
        "label": "Behavior 入口",
        "expected_tools": (
            "mcp__quickdep__analyze_behavior_context",
            "mcp__quickdep__get_task_context",
        ),
    },
    "s4": {
        "label": "Locate 入口",
        "expected_tools": (
            "mcp__quickdep__locate_relevant_code",
            "mcp__quickdep__get_task_context",
        ),
    },
    "s5": {
        "label": "Impact 入口",
        "expected_tools": (
            "mcp__quickdep__analyze_change_impact",
            "mcp__quickdep__get_task_context",
        ),
    },
}
CORE_BENCHMARK_SCENARIO_IDS = ("s1", "s2", "s3", "s5")
DEVELOPER_FLOW_SCENARIO_IDS = ("s6", "s7", "s8")
CROSS_LANGUAGE_SCENARIO_IDS = ("s9", "s10", "s11", "s12", "s13")
CROSS_LANGUAGE_ROUTE_IDS = (
    "claude-quickdep-plus-native-tools",
    "claude-native-only",
)
DEVELOPER_FLOW_ROUTE_IDS = (
    "claude-default",
    "claude-quickdep-plus-native-tools",
)


@dataclass(frozen=True)
class Scenario:
    sid: str
    title: str
    question: str
    gold_files: tuple[str, ...]
    gold_symbols: tuple[str, ...]
    allowed_routes: tuple[str, ...] = ALL_ROUTE_IDS
    incremental_target: str | None = None
    prompt_context: str | None = None
    repo_name: str | None = None
    language: str | None = None


SCENARIOS: dict[str, Scenario] = {
    "s1": Scenario(
        sid="s1",
        title="Queued After Approval",
        question=(
            "一个 execution 在审批通过后，为什么仍然可能继续停留在 `Queued`，"
            "而不是直接进入 `Running`？请解释真正的状态流转和调度原因。"
        ),
        gold_files=(
            "crates/ark-store/src/write.rs",
            "crates/ark-runtime/src/lib.rs",
            "crates/ark-runtime/src/core_flow_service.rs",
            "crates/ark-runtime/src/flow.rs",
            "crates/ark-execution/src/lib.rs",
            "crates/ark-scheduler/src/lib.rs",
        ),
        gold_symbols=(
            "Store::approve_pending_approval",
            "Runtime::approval_resolve",
            "CoreFlowService::resume_approved_execution",
            "RuntimeCore::dispatch_execution",
            "RuntimeCore::prepare_execution_dispatch",
            "ExecutionService::next_conflict_queue_head",
            "Scheduler::admit",
            "Scheduler::dispatchable_head",
        ),
    ),
    "s2": Scenario(
        sid="s2",
        title="Pre-Dispatch Failure Propagation",
        question=(
            "为什么 `verify_pre_dispatch` 失败后，turn 会直接失败，而不是只把当前 "
            "execution 跳过？请区分验证层和 runtime 消费层的职责。"
        ),
        gold_files=(
            "crates/ark-verification/src/lib.rs",
            "crates/ark-runtime-verification/src/lib.rs",
            "crates/ark-runtime/src/core_flow_service.rs",
            "crates/ark-runtime/src/flow.rs",
        ),
        gold_symbols=(
            "VerificationEngine::verify_pre_dispatch",
            "RuntimeVerification::verify_pre_dispatch",
            "VerificationDecision::is_passed",
            "RuntimeCore::apply_turn_failure",
        ),
    ),
    "s3": Scenario(
        sid="s3",
        title="Conflict Queue Call Chain",
        question=(
            "从 `RuntimeCore::next_conflict_queue_head` 到 "
            "`Scheduler::dispatchable_head` 的真实调用链是什么？请给出中间委托层。"
        ),
        gold_files=(
            "crates/ark-runtime/src/flow.rs",
            "crates/ark-execution/src/lib.rs",
            "crates/ark-store/src/read.rs",
            "crates/ark-scheduler/src/lib.rs",
        ),
        gold_symbols=(
            "RuntimeCore::next_conflict_queue_head",
            "ExecutionService::next_conflict_queue_head",
            "Store::list_concurrency_window",
            "Scheduler::dispatchable_head",
        ),
    ),
    "s4": Scenario(
        sid="s4",
        title="health_report Boundary",
        question=(
            "如果我要理解 `PlatformServer::health_report` 的逻辑，最值得先看的 "
            "3 到 5 个局部点是什么，为什么？目标是尽量少读无关代码。"
        ),
        gold_files=("crates/ark-platform-server/src/lib.rs",),
        gold_symbols=(
            "PlatformServer::health_report",
            "PlatformServer::reconcile_expired_worker_leases",
            "worker_health_projection",
            "PlatformServer::metrics_snapshot",
            "DeploymentPreset::requires_workers",
        ),
    ),
    "s5": Scenario(
        sid="s5",
        title="next_conflict_queue_head Risk Surface",
        question=(
            "如果我要修改 `next_conflict_queue_head` 的选头逻辑，哪些调用路径和行为"
            "最容易被改坏？请按风险排序，并指出最重要的回归点。"
        ),
        gold_files=(
            "crates/ark-runtime/src/flow.rs",
            "crates/ark-runtime/src/lib.rs",
            "crates/ark-execution/src/lib.rs",
            "crates/ark-store/src/read.rs",
            "crates/ark-scheduler/src/lib.rs",
        ),
        gold_symbols=(
            "RuntimeCore::next_conflict_queue_head",
            "ExecutionService::next_conflict_queue_head",
            "Store::list_concurrency_window",
            "Scheduler::dispatchable_head",
            "RuntimeCore::commit_or_reject_execution",
            "RuntimeCore::apply_turn_failure",
            "Runtime::approval_resolve",
            "Runtime::runtime_cancel",
        ),
    ),
    "s6": Scenario(
        sid="s6",
        title="Incremental Watcher Refresh",
        question=(
            "在一个 disposable worktree 里，对 `PlatformServer::health_report` 做最小改动："
            "新增 `push_issue` helper，并把若干 `issues.push(...)` 改成 helper 调用。"
            "不要使用 `rebuild_database` 或强制全量重扫。请验证 QuickDep 是否能通过 "
            "watcher / 增量更新反映出 `health_report -> push_issue` 新依赖，并报告观察到的延迟。"
        ),
        gold_files=("crates/ark-platform-server/src/lib.rs",),
        gold_symbols=("PlatformServer::health_report", "push_issue"),
        allowed_routes=INCREMENTAL_ROUTE_IDS,
        incremental_target="crates/ark-platform-server/src/lib.rs",
    ),
    "s7": Scenario(
        sid="s7",
        title="No-Anchor Workflow Triage",
        question=(
            "审批已经点过通过了，但这个 execution 还是没跑起来，一直卡在排队。"
            "先别全仓库乱搜，告诉我最应该先看的链路和关键位置。"
        ),
        gold_files=(
            "crates/ark-store/src/write.rs",
            "crates/ark-runtime/src/lib.rs",
            "crates/ark-runtime/src/core_flow_service.rs",
            "crates/ark-runtime/src/flow.rs",
            "crates/ark-scheduler/src/lib.rs",
        ),
        gold_symbols=(
            "approval_resolve",
            "approve_pending_approval",
            "resume_approved_execution",
            "prepare_execution_dispatch",
            "Scheduler::admit",
        ),
        allowed_routes=DEVELOPER_FLOW_ROUTE_IDS,
    ),
    "s8": Scenario(
        sid="s8",
        title="Editor Context Risk Triage",
        question=(
            "基于当前编辑器上下文，帮我判断如果我要改这里，最应该先看的局部点和回归风险是什么。"
        ),
        gold_files=(
            "crates/ark-runtime/src/flow.rs",
            "crates/ark-runtime/src/lib.rs",
            "crates/ark-execution/src/lib.rs",
            "crates/ark-store/src/read.rs",
            "crates/ark-scheduler/src/lib.rs",
        ),
        gold_symbols=(
            "RuntimeCore::next_conflict_queue_head",
            "ExecutionService::next_conflict_queue_head",
            "Store::list_concurrency_window",
            "Scheduler::dispatchable_head",
            "RuntimeCore::apply_turn_failure",
            "Runtime::runtime_cancel",
        ),
        allowed_routes=DEVELOPER_FLOW_ROUTE_IDS,
        prompt_context=textwrap.dedent(
            """\
            编辑器上下文：
            - active_file: crates/ark-runtime/src/flow.rs
            - selection_symbol: RuntimeCore::next_conflict_queue_head
            - 目标：先判断改动风险和最值得看的局部点
            """
        ).strip(),
    ),
    "s9": Scenario(
        sid="s9",
        title="tokio Builder Build Boundary",
        question=(
            "如果我要理解 `Builder::build` 是如何把配置分流到不同 runtime 实现的，"
            "最值得先看的局部点是什么？目标是尽量少读无关代码。"
        ),
        gold_files=("tokio/src/runtime/builder.rs",),
        gold_symbols=(
            "Builder::build",
            "Builder::build_current_thread_runtime",
            "Builder::build_threaded_runtime",
        ),
        allowed_routes=CROSS_LANGUAGE_ROUTE_IDS,
        repo_name="tokio",
        language="Rust",
    ),
    "s10": Scenario(
        sid="s10",
        title="nest Factory Create Boundary",
        question=(
            "如果我要理解 `NestFactoryStatic.create` 是如何把应用初始化起来的，"
            "最值得先看的局部点是什么？目标是尽量少读无关代码。"
        ),
        gold_files=("packages/core/nest-factory.ts",),
        gold_symbols=(
            "NestFactoryStatic.create",
            "NestFactoryStatic.initialize",
            "NestFactoryStatic.createNestInstance",
            "NestFactoryStatic.createAdapterProxy",
        ),
        allowed_routes=CROSS_LANGUAGE_ROUTE_IDS,
        repo_name="nest",
        language="TypeScript",
    ),
    "s11": Scenario(
        sid="s11",
        title="gin HTTP Dispatch Boundary",
        question=(
            "如果我要理解 `Engine.handleHTTPRequest` 的请求分发逻辑，"
            "最值得先看的局部点是什么？目标是尽量少读无关代码。"
        ),
        gold_files=("gin.go", "tree.go"),
        gold_symbols=(
            "Engine.handleHTTPRequest",
            "getValue",
            "serveError",
            "redirectTrailingSlash",
            "redirectFixedPath",
        ),
        allowed_routes=CROSS_LANGUAGE_ROUTE_IDS,
        repo_name="gin",
        language="Go",
    ),
    "s12": Scenario(
        sid="s12",
        title="requests Session Request Boundary",
        question=(
            "如果我要理解 `Session.request` 的主流程，最值得先看的局部点是什么？"
            "目标是尽量少读无关代码。"
        ),
        gold_files=("src/requests/sessions.py",),
        gold_symbols=(
            "Session.request",
            "Session.prepare_request",
            "Session.send",
            "Session.merge_environment_settings",
        ),
        allowed_routes=CROSS_LANGUAGE_ROUTE_IDS,
        repo_name="requests",
        language="Python",
    ),
    "s13": Scenario(
        sid="s13",
        title="fmt fdopen Boundary",
        question=(
            "如果我要理解 `file::fdopen` 的局部逻辑，最值得先看的局部点是什么？"
            "目标是尽量少读无关代码。"
        ),
        gold_files=("src/os.cc", "include/fmt/os.h"),
        gold_symbols=(
            "file::fdopen",
            "buffered_file",
            "buffered_file::close",
            "buffered_file::descriptor",
        ),
        allowed_routes=CROSS_LANGUAGE_ROUTE_IDS,
        repo_name="fmt",
        language="C++",
    ),
}


ROUTE_PROMPTS = {
    "claude-default": textwrap.dedent(
        """\
        你正在分析仓库：{repo_path}

        请回答下面这个工程问题：
        {question}

        工作要求：
        - 自主选择最合适的第一步，不要为了凑工具而盲目搜索
        - 优先回答“先看哪里最有解释力”，而不是先做大范围搜索
        - 在最终答案里说明你第一步为什么这样做

        输出要求：
        1. 结论
        2. 第一跳工具和原因
        3. 关键文件（最多 5 个）
        4. 关键符号 / 调用链
        5. 不确定点
        """
    ),
    "claude-quickdep-first": textwrap.dedent(
        """\
        你正在分析仓库：{repo_path}

        请回答下面这个工程问题：
        {question}

        约束：
        - 优先使用 QuickDep MCP 工具
        - 不要先做大范围 grep 或整文件通读
        - 只有在 QuickDep 不足以支撑判断时，才允许做少量定点源码确认
        - 如果你读取源码，必须说明是被哪一个 QuickDep 结果引导过去的

        输出要求：
        1. 结论
        2. 关键文件（最多 5 个）
        3. 关键符号 / 调用链
        4. 不确定点
        """
    ),
    "claude-native-only": textwrap.dedent(
        """\
        你正在分析仓库：{repo_path}

        请回答下面这个工程问题：
        {question}

        约束：
        - 禁用 QuickDep 和任何外部索引
        - 只允许使用原生搜索和源码阅读手段
        - 尽量减少无关文件扩散

        输出要求：
        1. 结论
        2. 关键文件（最多 5 个）
        3. 关键符号 / 调用链
        4. 不确定点
        """
    ),
    "claude-quickdep-plus-native-tools": textwrap.dedent(
        """\
        你正在分析仓库：{repo_path}

        请回答下面这个工程问题：
        {question}

        工作方式：
        - 先用 QuickDep 缩小到候选文件 / 候选符号
        - 再用少量原生源码阅读确认行为细节
        - 目标是在正确性和上下文成本之间取得最优平衡

        输出要求：
        1. 结论
        2. 关键文件（最多 5 个）
        3. 关键符号 / 调用链
        4. 不确定点
        """
    ),
}


def log(message: str) -> None:
    now = dt.datetime.now().strftime("%H:%M:%S")
    print(f"[{now}] {message}", flush=True)


def normalize_route_id(value: str) -> str:
    key = value.strip().lower()
    return ROUTE_ALIASES.get(key, key)


def route_display_name(route: str) -> str:
    return ROUTE_DISPLAY_NAMES.get(route, route)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("run", help="Run benchmark scenarios")
    run.add_argument(
        "--repo",
        type=pathlib.Path,
        default=DEFAULT_TARGET_REPO,
        help="Target repository path",
    )
    run.add_argument(
        "--output-dir",
        type=pathlib.Path,
        default=DEFAULT_OUTPUT_DIR,
        help="Output directory",
    )
    run.add_argument(
        "--scenarios",
        nargs="+",
        default=["all"],
        help="Scenario ids to run (default: all)",
    )
    run.add_argument(
        "--routes",
        nargs="+",
        default=list(BENCHMARK_ROUTE_IDS),
        help=f"Routes to run ({', '.join(ALL_ROUTE_IDS)})",
    )
    run.add_argument(
        "--max-workers",
        type=int,
        default=3,
        help="Maximum parallel routes per scenario",
    )
    run.add_argument(
        "--claude-bin",
        type=pathlib.Path,
        default=DEFAULT_CLAUDE_BIN,
        help="Claude CLI path",
    )
    run.add_argument(
        "--quickdep-bin",
        type=pathlib.Path,
        default=DEFAULT_QUICKDEP_BIN,
        help="QuickDep binary path",
    )
    run.add_argument("--model", default=None, help="Optional fixed model")
    run.add_argument(
        "--bare",
        action="store_true",
        help="Run Claude in --bare mode for lower startup noise",
    )
    run.add_argument(
        "--skip-prewarm",
        action="store_true",
        help="Skip quickdep scan prewarm for the base repo",
    )
    run.add_argument(
        "--route-timeout-seconds",
        type=int,
        default=180,
        help="Per-route hard timeout in seconds",
    )

    report = subparsers.add_parser("report", help="Generate markdown report")
    report.add_argument(
        "--output-dir",
        type=pathlib.Path,
        default=DEFAULT_OUTPUT_DIR,
        help="Output directory",
    )
    report.add_argument(
        "--markdown",
        type=pathlib.Path,
        default=None,
        help="Markdown output path (default: <output-dir>/REPORT.md)",
    )

    return parser.parse_args()


def select_scenarios(values: list[str]) -> list[Scenario]:
    if values == ["all"]:
        ordered = (
            "s1",
            "s2",
            "s3",
            "s4",
            "s5",
            "s6",
            "s7",
            "s8",
            "s9",
            "s10",
            "s11",
            "s12",
            "s13",
        )
        return [SCENARIOS[key] for key in ordered]
    selected: list[Scenario] = []
    for value in values:
        key = value.lower()
        if key not in SCENARIOS:
            raise SystemExit(f"unknown scenario: {value}")
        selected.append(SCENARIOS[key])
    return selected


def resolve_scenario_repo_path(scenario: Scenario, default_repo: pathlib.Path) -> pathlib.Path:
    if scenario.repo_name is None:
        return default_repo

    repo_dir = BENCH_REPO_DIRS.get(scenario.repo_name)
    if repo_dir is None:
        raise SystemExit(f"missing benchmark repo mapping for scenario {scenario.sid}: {scenario.repo_name}")

    repo_path = DEFAULT_BENCH_REPOS_ROOT / repo_dir
    if not repo_path.exists():
        raise SystemExit(
            f"benchmark repo for scenario {scenario.sid} not found: {repo_path}"
        )
    return repo_path


def ensure_quickdep_scan(quickdep_bin: pathlib.Path, repo: pathlib.Path) -> None:
    log(f"Prewarm quickdep scan for {repo}")
    subprocess.run(
        [str(quickdep_bin), "scan", str(repo)],
        check=True,
        cwd=str(REPO_ROOT),
    )


def run_command(
    argv: list[str],
    cwd: pathlib.Path,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        cwd=str(cwd),
        env=env,
        check=True,
        text=True,
        capture_output=True,
    )


def build_mcp_config(route: str, quickdep_bin: pathlib.Path) -> dict[str, Any]:
    if route == "claude-native-only":
        return {"mcpServers": {}}
    return {
        "mcpServers": {
            "quickdep": {
                "command": str(quickdep_bin),
                "args": ["serve"],
            }
        }
    }


def iter_strings(value: Any) -> list[str]:
    strings: list[str] = []
    if isinstance(value, str):
        strings.append(value)
    elif isinstance(value, dict):
        for item in value.values():
            strings.extend(iter_strings(item))
    elif isinstance(value, list):
        for item in value:
            strings.extend(iter_strings(item))
    return strings


def normalize_content(value: Any) -> str:
    if isinstance(value, str):
        return value
    if value is None:
        return ""
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


def is_search_tool(tool_name: str, input_text: str) -> bool:
    if tool_name in SEARCH_TOOL_NAMES:
        return True
    return tool_name == "Bash" and bool(SHELL_SEARCH_PATTERN.search(input_text))


def is_read_tool(tool_name: str, input_text: str) -> bool:
    if tool_name in READ_TOOL_NAMES:
        return True
    return tool_name == "Bash" and bool(SHELL_READ_PATTERN.search(input_text))


def counts_as_source_read(tool_name: str, input_text: str) -> bool:
    return tool_name in SOURCE_PAYLOAD_TOOL_NAMES or is_read_tool(tool_name, input_text)


def detect_files(text: str, repo_path: pathlib.Path) -> set[str]:
    found: set[str] = set()
    absolute_pattern = re.compile(re.escape(str(repo_path)) + r"/[A-Za-z0-9_./+\-]+")

    for match in absolute_pattern.findall(text):
        path = pathlib.Path(match)
        if path.suffix in SOURCE_EXTENSIONS and path.exists():
            found.add(str(path.resolve()))
    for match in RELATIVE_FILE_PATTERN.findall(text):
        candidate = match.rstrip(".,:;!?)]}\"'`")
        path = repo_path / candidate
        if path.suffix in SOURCE_EXTENSIONS and path.exists():
            found.add(str(path.resolve()))
    return found


def first_hit_text(lines: list[str], scenario: Scenario) -> bool:
    haystack = "\n".join(lines)
    for gold_file in scenario.gold_files:
        if gold_file in haystack:
            return True
    for gold_symbol in scenario.gold_symbols:
        if gold_symbol in haystack:
            return True
    return False


def write_json(path: pathlib.Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")


def write_text(path: pathlib.Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def report_cell(value: Any) -> str:
    return "-" if value is None else str(value)


def scenario_repo_path(
    scenario: Scenario,
    base_repo: pathlib.Path,
    scenario_dir: pathlib.Path,
    route: str,
) -> tuple[pathlib.Path, pathlib.Path | None]:
    if scenario.sid != "s6":
        return base_repo, None

    worktree_root = scenario_dir / "worktrees"
    worktree_path = worktree_root / route
    if worktree_path.exists():
        subprocess.run(
            ["git", "-C", str(base_repo), "worktree", "remove", "--force", str(worktree_path)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    worktree_root.mkdir(parents=True, exist_ok=True)
    run_command(
        [
            "git",
            "-C",
            str(base_repo),
            "worktree",
            "add",
            "--detach",
            str(worktree_path),
            "HEAD",
        ],
        cwd=base_repo,
    )
    return worktree_path, worktree_path


def cleanup_worktree(base_repo: pathlib.Path, worktree_path: pathlib.Path | None) -> None:
    if not worktree_path:
        return
    subprocess.run(
        ["git", "-C", str(base_repo), "worktree", "remove", "--force", str(worktree_path)],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def build_prompt(route: str, repo_path: pathlib.Path, scenario: Scenario) -> str:
    prompt = ROUTE_PROMPTS[route].format(repo_path=repo_path, question=scenario.question)
    if scenario.prompt_context:
        prompt += textwrap.dedent(
            f"""

            补充上下文：
            {scenario.prompt_context}
            """
        )
    max_tools = 12 if scenario.sid == "s6" else 8
    prompt += textwrap.dedent(
        f"""

        额外约束：
        - 最多使用 {max_tools} 次工具调用
        - 一旦定位到能支撑结论的 3 到 5 个关键文件，就停止继续扩散
        - 即使仍有不确定点，也必须基于当前证据给出最终答案
        """
    )
    if scenario.sid == "s6":
        prompt += textwrap.dedent(
            f"""

            额外要求：
            - 你运行在 disposable worktree：{repo_path}
            - 先确保 QuickDep 已对这个 worktree 建立索引并进入 Loaded 状态
            - 然后在 `{scenario.incremental_target}` 做如下精确修改：
              1. 在 `pub async fn health_report` 之前新增：
                 `fn push_issue(issues: &mut Vec<String>, issue: &str) {{ issues.push(issue.to_string()); }}`
              2. 把以下三处调用改成 helper：
                 - `issues.push("database_unavailable".to_string());`
                 - `issues.push("no_active_workers".to_string());`
                 - `issues.push("worker_lease_expired".to_string());`
            - 不要调用 `rebuild_database`，也不要做全量重扫
            - 修改完成后，持续查询直到 QuickDep 反映出：
              - 新符号 `push_issue`
              - `PlatformServer::health_report` 对 `push_issue` 的新依赖
            - 如果 20 秒内没有看到更新，明确说明失败和你看到的现象

            输出时额外补一节：
            5. 增量刷新观察（包含你观察到的刷新延迟）
            """
        )
    return prompt


def collect_route_metrics(
    transcript_path: pathlib.Path,
    scenario: Scenario,
    repo_path: pathlib.Path,
) -> dict[str, Any]:
    tool_inputs: dict[str, dict[str, Any]] = {}
    metrics: dict[str, Any] = {
        "tool_count": 0,
        "tool_names": [],
        "mcp_tool_count": 0,
        "first_tool_name": None,
        "file_fanout": 0,
        "files_touched": [],
        "file_fanout_before_first_hit": 0,
        "files_touched_before_first_hit": [],
        "search_tool_count_before_first_hit": 0,
        "search_tool_names_before_first_hit": [],
        "read_tool_count_before_first_hit": 0,
        "raw_source_chars": 0,
        "raw_source_chars_before_first_hit": 0,
        "mcp_payload_chars": 0,
        "time_to_first_hit_ms": None,
        "refresh_after_edit_ms": None,
        "duration_ms": None,
        "result_text": "",
        "last_assistant_text": "",
        "usage": {},
        "status": "unknown",
    }
    files_touched: set[str] = set()
    files_touched_before_first_hit: set[str] = set()
    first_hit_ms: float | None = None
    edit_ms: float | None = None
    first_refresh_ms: float | None = None

    for raw_line in transcript_path.read_text().splitlines():
        if not raw_line.strip():
            continue
        event = json.loads(raw_line)
        elapsed_ms = event.get("_elapsed_ms")
        event_copy = dict(event)
        event_copy.pop("_elapsed_ms", None)
        event_text = normalize_content(event_copy)
        event_has_hit = first_hit_text([event_text], scenario)
        before_first_hit = first_hit_ms is None and not event_has_hit

        detected = detect_files(event_text, repo_path)
        files_touched.update(detected)
        if before_first_hit:
            files_touched_before_first_hit.update(detected)

        if event.get("type") == "assistant":
            message = event.get("message", {})
            for item in message.get("content", []):
                if item.get("type") == "text":
                    metrics["last_assistant_text"] = item.get("text", "")
                if item.get("type") != "tool_use":
                    continue
                tool_name = item.get("name", "")
                tool_id = item.get("id", "")
                metrics["tool_count"] += 1
                metrics["tool_names"].append(tool_name)
                if metrics["first_tool_name"] is None:
                    metrics["first_tool_name"] = tool_name
                if tool_name.startswith("mcp__quickdep__"):
                    metrics["mcp_tool_count"] += 1
                tool_input = item.get("input", {})
                tool_inputs[tool_id] = {
                    "name": tool_name,
                    "input": tool_input,
                    "elapsed_ms": elapsed_ms,
                }
                input_text = normalize_content(tool_input)
                files_touched.update(detect_files(input_text, repo_path))
                if before_first_hit:
                    files_touched_before_first_hit.update(detect_files(input_text, repo_path))
                    if is_search_tool(tool_name, input_text):
                        metrics["search_tool_count_before_first_hit"] += 1
                        metrics["search_tool_names_before_first_hit"].append(tool_name)
                    if is_read_tool(tool_name, input_text):
                        metrics["read_tool_count_before_first_hit"] += 1
                if scenario.sid == "s6" and edit_ms is None:
                    if tool_name in {"Edit", "Write", "NotebookEdit"} and scenario.incremental_target in input_text:
                        edit_ms = elapsed_ms
                    if tool_name == "Bash" and scenario.incremental_target in input_text:
                        edit_ms = elapsed_ms

        elif event.get("type") == "user":
            message = event.get("message", {})
            tool_use_id = ""
            for item in message.get("content", []):
                if item.get("type") == "tool_result":
                    tool_use_id = item.get("tool_use_id", "")
                    break
            tool_info = tool_inputs.get(tool_use_id, {})
            tool_name = tool_info.get("name", "")
            tool_input_text = normalize_content(tool_info.get("input", {}))
            payload = message.get("tool_use_result", {}).get("content")
            payload_text = normalize_content(payload if payload is not None else message.get("content"))
            if tool_name.startswith("mcp__quickdep__"):
                metrics["mcp_payload_chars"] += len(payload_text)
            elif counts_as_source_read(tool_name, tool_input_text):
                metrics["raw_source_chars"] += len(payload_text)
            files_touched.update(detect_files(payload_text, repo_path))
            if before_first_hit:
                files_touched_before_first_hit.update(detect_files(payload_text, repo_path))
                if counts_as_source_read(tool_name, tool_input_text):
                    metrics["raw_source_chars_before_first_hit"] += len(payload_text)
            if scenario.sid == "s6" and edit_ms is not None and first_refresh_ms is None:
                if "push_issue" in payload_text:
                    first_refresh_ms = elapsed_ms

        elif event.get("type") == "result":
            metrics["duration_ms"] = event.get("duration_ms")
            metrics["result_text"] = event.get("result", "")
            metrics["usage"] = event.get("usage", {})
            metrics["status"] = "success" if not event.get("is_error") else "error"

        if first_hit_ms is None and event_has_hit:
            first_hit_ms = elapsed_ms

    if not metrics["result_text"] and metrics["last_assistant_text"]:
        metrics["result_text"] = metrics["last_assistant_text"]
    if metrics["status"] == "unknown":
        metrics["status"] = "incomplete"

    metrics["files_touched"] = sorted(files_touched)
    metrics["file_fanout"] = len(files_touched)
    metrics["files_touched_before_first_hit"] = sorted(files_touched_before_first_hit)
    metrics["file_fanout_before_first_hit"] = len(files_touched_before_first_hit)
    metrics["time_to_first_hit_ms"] = first_hit_ms
    if scenario.sid == "s6" and edit_ms is not None and first_refresh_ms is not None:
        metrics["refresh_after_edit_ms"] = max(0, first_refresh_ms - edit_ms)
    return metrics


def run_route(
    *,
    route: str,
    scenario: Scenario,
    base_repo: pathlib.Path,
    scenario_dir: pathlib.Path,
    claude_bin: pathlib.Path,
    quickdep_bin: pathlib.Path,
    model: str | None,
    bare: bool,
    route_timeout_seconds: int,
) -> dict[str, Any]:
    route_dir = scenario_dir / route
    route_dir.mkdir(parents=True, exist_ok=True)

    repo_path, worktree_path = scenario_repo_path(scenario, base_repo, scenario_dir, route)
    try:
        if scenario.sid == "s6":
            ensure_quickdep_scan(quickdep_bin, repo_path)

        mcp_config = build_mcp_config(route, quickdep_bin)
        mcp_config_path = route_dir / "mcp-config.json"
        write_json(mcp_config_path, mcp_config)

        prompt = build_prompt(route, repo_path, scenario)
        write_text(route_dir / "prompt.txt", prompt + "\n")

        argv = [
            str(claude_bin),
            "-p",
            prompt,
            "--verbose",
            "--output-format",
            "stream-json",
            "--no-session-persistence",
            "--dangerously-skip-permissions",
            "--strict-mcp-config",
            "--mcp-config",
            str(mcp_config_path),
        ]
        if bare:
            argv.append("--bare")
        if model:
            argv.extend(["--model", model])

        transcript_path = route_dir / "transcript.jsonl"
        stderr_path = route_dir / "stderr.txt"
        process = subprocess.Popen(
            argv,
            cwd=str(repo_path),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

        start = time.monotonic()
        stdout_lock = threading.Lock()
        stderr_chunks: list[str] = []
        timed_out = False

        def drain_stderr() -> None:
            assert process.stderr is not None
            for chunk in process.stderr:
                with stdout_lock:
                    stderr_chunks.append(chunk)

        stderr_thread = threading.Thread(target=drain_stderr, daemon=True)
        stderr_thread.start()

        assert process.stdout is not None
        with transcript_path.open("w") as handle:
            for line in process.stdout:
                elapsed_ms = round((time.monotonic() - start) * 1000, 2)
                if elapsed_ms >= route_timeout_seconds * 1000:
                    timed_out = True
                    process.terminate()
                    break
                stripped = line.strip()
                if not stripped:
                    continue
                try:
                    event = json.loads(stripped)
                except json.JSONDecodeError:
                    event = {"type": "raw", "line": stripped}
                event["_elapsed_ms"] = elapsed_ms
                handle.write(json.dumps(event, ensure_ascii=False) + "\n")

        if timed_out:
            try:
                return_code = process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                return_code = process.wait(timeout=10)
        else:
            return_code = process.wait()
        stderr_thread.join(timeout=2)
        write_text(stderr_path, "".join(stderr_chunks))
        if return_code != 0 and not timed_out:
            raise RuntimeError(f"route {route} failed with exit code {return_code}")

        metrics = collect_route_metrics(transcript_path, scenario, repo_path)
        if timed_out:
            metrics["status"] = "timeout"
            metrics["timeout_seconds"] = route_timeout_seconds
        metrics["route"] = route
        metrics["route_display_name"] = route_display_name(route)
        metrics["scenario"] = scenario.sid
        metrics["repo_path"] = str(repo_path)
        write_json(route_dir / "metrics.json", metrics)
        write_text(route_dir / "answer.md", metrics.get("result_text", "") + "\n")
        return metrics
    finally:
        cleanup_worktree(base_repo, worktree_path)


def score_route(scenario: Scenario, route: str, metrics: dict[str, Any]) -> dict[str, Any]:
    answer = metrics.get("result_text", "")
    files_touched = set(metrics.get("files_touched", []))
    gold_abs = {str((pathlib.Path(metrics["repo_path"]) / file).resolve()) for file in scenario.gold_files}
    file_hits = len(files_touched & gold_abs)
    symbol_hits = sum(1 for symbol in scenario.gold_symbols if symbol in answer)
    score = 0
    notes: list[str] = []

    if metrics.get("status") == "success":
        score += 1
    if file_hits:
        score += 1
    if symbol_hits >= max(1, len(scenario.gold_symbols) // 3):
        score += 1
    if scenario.sid == "s3":
        if "ExecutionService::next_conflict_queue_head" in answer and "Scheduler::dispatchable_head" in answer:
            score += 1
        else:
            notes.append("缺少完整调用链")
    elif scenario.sid in {"s5", "s8"}:
        for keyword in ("approval_resolve", "apply_turn_failure", "runtime_cancel"):
            if keyword in answer:
                score += 1
                break
        else:
            notes.append("缺少关键恢复路径")
    elif scenario.sid == "s7":
        keyword_hits = sum(
            1
            for keyword in ("approval_resolve", "resume_approved_execution", "Scheduler::admit")
            if keyword in answer
        )
        if keyword_hits >= 2:
            score += 1
        else:
            notes.append("未明确收敛到审批恢复主链路")
    elif scenario.sid == "s6":
        refresh = metrics.get("refresh_after_edit_ms")
        if refresh is not None:
            score += 2
        elif "push_issue" in answer:
            score += 1
            notes.append("看到了新符号，但未能量化刷新延迟")
        else:
            notes.append("未观察到增量刷新")
    elif scenario.sid in {"s4", *CROSS_LANGUAGE_SCENARIO_IDS}:
        fanout = metrics.get("file_fanout", 0)
        if fanout <= max(5, len(scenario.gold_files) + 2):
            score += 1
        else:
            notes.append("局部边界问题的阅读范围仍偏大")
    else:
        if symbol_hits >= max(2, len(scenario.gold_symbols) // 2):
            score += 1
        else:
            notes.append("金标准符号覆盖不足")

    score = min(score, 5)
    return {
        "route": route,
        "scenario": scenario.sid,
        "score_0_to_5": score,
        "gold_file_recall": round(file_hits / max(1, len(scenario.gold_files)), 3),
        "gold_symbol_recall": round(symbol_hits / max(1, len(scenario.gold_symbols)), 3),
        "notes": notes,
    }


def run_scenario(
    scenario: Scenario,
    *,
    base_repo: pathlib.Path,
    output_dir: pathlib.Path,
    routes: list[str],
    max_workers: int,
    claude_bin: pathlib.Path,
    quickdep_bin: pathlib.Path,
    model: str | None,
    bare: bool,
    route_timeout_seconds: int,
) -> dict[str, Any]:
    scenario_dir = output_dir / f"scenario_{scenario.sid}"
    scenario_dir.mkdir(parents=True, exist_ok=True)
    scenario_repo = resolve_scenario_repo_path(scenario, base_repo)
    selected_routes = [route for route in routes if route in scenario.allowed_routes]
    skipped_routes = [route for route in routes if route not in scenario.allowed_routes]
    for route in skipped_routes:
        skipped_dir = scenario_dir / route
        skipped_dir.mkdir(parents=True, exist_ok=True)
        write_json(
            skipped_dir / "metrics.json",
            {
                "route": route,
                "scenario": scenario.sid,
                "status": "skipped",
                "reason": "route not applicable for this scenario",
            },
        )

    log(f"Run {scenario.sid}: routes={','.join(selected_routes)}")
    results: dict[str, Any] = {"scenario": scenario.sid, "routes": {}, "skipped_routes": skipped_routes}
    with concurrent.futures.ThreadPoolExecutor(max_workers=min(max_workers, len(selected_routes) or 1)) as executor:
        future_map = {
            executor.submit(
                run_route,
                route=route,
                scenario=scenario,
                base_repo=scenario_repo,
                scenario_dir=scenario_dir,
                claude_bin=claude_bin,
                quickdep_bin=quickdep_bin,
                model=model,
                bare=bare,
                route_timeout_seconds=route_timeout_seconds,
            ): route
            for route in selected_routes
        }
        for future in concurrent.futures.as_completed(future_map):
            route = future_map[future]
            results["routes"][route] = future.result()

    judge_dir = scenario_dir / "judge"
    judge_dir.mkdir(parents=True, exist_ok=True)
    scores = {
        route: score_route(scenario, route, metrics)
        for route, metrics in results["routes"].items()
    }
    write_json(judge_dir / "score.json", scores)
    notes_lines = []
    for route in sorted(scores):
        notes_lines.append(
            f"{route_display_name(route)}: score={scores[route]['score_0_to_5']} notes={scores[route]['notes']}"
        )
    write_text(judge_dir / "notes.md", "\n".join(notes_lines) + ("\n" if notes_lines else ""))
    write_json(scenario_dir / "summary.json", results)
    return results


def summarize_search_expansion(metrics: dict[str, Any]) -> str:
    count = metrics.get("search_tool_count_before_first_hit", 0)
    if not count:
        return "否"
    names = metrics.get("search_tool_names_before_first_hit", [])
    if not names:
        return "是"
    return f"是 ({', '.join(names[:3])})"


def entry_selection_row(
    scenario_id: str,
    scenario_dir: pathlib.Path,
) -> list[str] | None:
    if scenario_id not in ENTRY_SELECTION_EXPECTATIONS:
        return None
    route = "claude-default"
    metrics_path = scenario_dir / route / "metrics.json"
    if not metrics_path.exists():
        return None
    metrics = json.loads(metrics_path.read_text())
    expectation = ENTRY_SELECTION_EXPECTATIONS[scenario_id]
    first_tool = metrics.get("first_tool_name") or "-"
    matched = first_tool in expectation["expected_tools"]
    return [
        expectation["label"],
        route_display_name(route),
        first_tool,
        "是" if matched else "否",
        summarize_search_expansion(metrics),
        str(metrics.get("file_fanout_before_first_hit", "-")),
        str(metrics.get("raw_source_chars_before_first_hit", "-")),
        str(metrics.get("time_to_first_hit_ms", "-")),
        metrics.get("status", "-"),
    ]


def generate_report(output_dir: pathlib.Path, markdown_path: pathlib.Path) -> pathlib.Path:
    scenario_dirs = sorted(output_dir.glob("scenario_*"))
    lines = [
        "# QuickDep Claude Experiment Report",
        "",
        f"- Generated: {dt.datetime.now().isoformat()}",
        f"- Output dir: `{output_dir}`",
        "",
        "## Wave 1 Entry Selection",
        "",
        "| 场景 | 路线 | Claude 第一跳 | 是否命中正确入口 | 首次命中前是否搜索扩散 | 首次命中前触达文件数 | 首次命中前源码读取字符数 | 首次命中时间 ms | 状态 |",
        "|---|---|---|---|---|---:|---:|---:|---|",
    ]

    for scenario_dir in scenario_dirs:
        scenario_id = scenario_dir.name.removeprefix("scenario_")
        row = entry_selection_row(scenario_id, scenario_dir)
        if row is not None:
            lines.append("| " + " | ".join(row) + " |")

    lines.extend(
        [
            "",
            "## Wave 2 Core Benchmark",
            "",
            "| Scenario | Route | Score | Duration ms | Tool count | File fan-out | Raw source chars | MCP payload chars | Total ctx tokens | Notes |",
            "|---|---|---:|---:|---:|---:|---:|---:|---:|---|",
        ]
    )

    for scenario_dir in scenario_dirs:
        scenario_id = scenario_dir.name.removeprefix("scenario_")
        if scenario_id not in CORE_BENCHMARK_SCENARIO_IDS:
            continue
        score_path = scenario_dir / "judge" / "score.json"
        scores = json.loads(score_path.read_text()) if score_path.exists() else {}
        for route in BENCHMARK_ROUTE_IDS:
            route_dir = scenario_dir / route
            if not route_dir.exists():
                continue
            metrics_path = route_dir / "metrics.json"
            if not metrics_path.exists():
                continue
            metrics = json.loads(metrics_path.read_text())
            status = metrics.get("status")
            score = scores.get(route, {}).get("score_0_to_5", "-")
            notes = ", ".join(scores.get(route, {}).get("notes", []))
            if status == "skipped":
                lines.append(
                    f"| {scenario_id} | {route_display_name(route)} | - | - | - | - | - | - | - | skipped |"
                )
                continue
            usage = metrics.get("usage", {})
            total_ctx_tokens = usage.get("input_tokens", 0) + usage.get("cache_read_input_tokens", 0)
            lines.append(
                "| {scenario} | {route} | {score} | {duration} | {tools} | {fanout} | {raw_chars} | {mcp_chars} | {ctx} | {notes} |".format(
                    scenario=scenario_id,
                    route=route_display_name(route),
                    score=score,
                    duration=metrics.get("duration_ms", "-"),
                    tools=metrics.get("tool_count", "-"),
                    fanout=metrics.get("file_fanout", "-"),
                    raw_chars=metrics.get("raw_source_chars", "-"),
                    mcp_chars=metrics.get("mcp_payload_chars", "-"),
                    ctx=total_ctx_tokens,
                    notes=notes or "-",
                )
            )

    if any((output_dir / f"scenario_{scenario_id}").exists() for scenario_id in DEVELOPER_FLOW_SCENARIO_IDS):
        lines.extend(
            [
                "",
                "## Wave 3 Developer Flow",
                "",
                "| Scenario | Route | Score | First tool | First hit ms | Refresh after edit ms | File fan-out | Notes |",
                "|---|---|---:|---|---:|---:|---:|---|",
            ]
        )
        for scenario_id in DEVELOPER_FLOW_SCENARIO_IDS:
            scenario_dir = output_dir / f"scenario_{scenario_id}"
            if not scenario_dir.exists():
                continue
            scenario = SCENARIOS[scenario_id]
            score_path = scenario_dir / "judge" / "score.json"
            scores = json.loads(score_path.read_text()) if score_path.exists() else {}
            for route in scenario.allowed_routes:
                metrics_path = scenario_dir / route / "metrics.json"
                if not metrics_path.exists():
                    continue
                metrics = json.loads(metrics_path.read_text())
                notes = ", ".join(scores.get(route, {}).get("notes", [])) or "-"
                lines.append(
                    "| {scenario} | {route} | {score} | {first_tool} | {first_hit} | {refresh} | {fanout} | {notes} |".format(
                        scenario=scenario_id,
                        route=route_display_name(route),
                        score=scores.get(route, {}).get("score_0_to_5", "-"),
                        first_tool=metrics.get("first_tool_name") or "-",
                        first_hit=report_cell(metrics.get("time_to_first_hit_ms")),
                        refresh=report_cell(metrics.get("refresh_after_edit_ms")),
                        fanout=report_cell(metrics.get("file_fanout")),
                        notes=notes,
                    )
                )

    if any((output_dir / f"scenario_{scenario_id}").exists() for scenario_id in CROSS_LANGUAGE_SCENARIO_IDS):
        lines.extend(
            [
                "",
                "## Wave 4 Cross-Language Sanity",
                "",
                "| Repo | Language | Scenario | Route | Score | First hit ms | Duration ms | File fan-out | Raw source chars | MCP payload chars | Notes |",
                "|---|---|---|---|---:|---:|---:|---:|---:|---:|---|",
            ]
        )
        for scenario_id in CROSS_LANGUAGE_SCENARIO_IDS:
            scenario_dir = output_dir / f"scenario_{scenario_id}"
            if not scenario_dir.exists():
                continue
            scenario = SCENARIOS[scenario_id]
            score_path = scenario_dir / "judge" / "score.json"
            scores = json.loads(score_path.read_text()) if score_path.exists() else {}
            for route in scenario.allowed_routes:
                metrics_path = scenario_dir / route / "metrics.json"
                if not metrics_path.exists():
                    continue
                metrics = json.loads(metrics_path.read_text())
                notes = ", ".join(scores.get(route, {}).get("notes", [])) or "-"
                lines.append(
                    "| {repo} | {language} | {scenario} | {route} | {score} | {first_hit} | {duration} | {fanout} | {raw_chars} | {mcp_chars} | {notes} |".format(
                        repo=scenario.repo_name or "-",
                        language=scenario.language or "-",
                        scenario=scenario_id,
                        route=route_display_name(route),
                        score=scores.get(route, {}).get("score_0_to_5", "-"),
                        first_hit=report_cell(metrics.get("time_to_first_hit_ms")),
                        duration=report_cell(metrics.get("duration_ms")),
                        fanout=report_cell(metrics.get("file_fanout")),
                        raw_chars=report_cell(metrics.get("raw_source_chars")),
                        mcp_chars=report_cell(metrics.get("mcp_payload_chars")),
                        notes=notes,
                    )
                )

    lines.extend(["", "## Per Scenario", ""])
    for scenario_dir in scenario_dirs:
        scenario_id = scenario_dir.name.removeprefix("scenario_")
        scenario = SCENARIOS[scenario_id]
        score_path = scenario_dir / "judge" / "score.json"
        scores = json.loads(score_path.read_text()) if score_path.exists() else {}
        lines.append(f"### {scenario_id.upper()} {scenario.title}")
        lines.append("")
        lines.append(f"- Question: {scenario.question}")
        lines.append(f"- Gold files: {', '.join(scenario.gold_files)}")
        lines.append(f"- Gold symbols: {', '.join(scenario.gold_symbols)}")
        lines.append("")
        if scenario_id in ENTRY_SELECTION_EXPECTATIONS:
            expectation = ENTRY_SELECTION_EXPECTATIONS[scenario_id]
            lines.append(
                f"- Expected first entry: {', '.join(expectation['expected_tools'])}"
            )
            lines.append("")
        for route in ALL_ROUTE_IDS:
            metrics_path = scenario_dir / route / "metrics.json"
            if not metrics_path.exists():
                continue
            metrics = json.loads(metrics_path.read_text())
            answer_path = scenario_dir / route / "answer.md"
            answer = answer_path.read_text().strip() if answer_path.exists() else ""
            lines.append(f"#### {route_display_name(route)}")
            if metrics.get("status") == "skipped":
                lines.append("")
                lines.append("- Skipped")
                lines.append("")
                continue
            usage = metrics.get("usage", {})
            total_ctx_tokens = usage.get("input_tokens", 0) + usage.get("cache_read_input_tokens", 0)
            lines.append("")
            lines.append(f"- Score: {scores.get(route, {}).get('score_0_to_5', '-')}/5")
            lines.append(f"- First tool: {metrics.get('first_tool_name') or '-'}")
            lines.append(f"- Duration ms: {metrics.get('duration_ms')}")
            lines.append(f"- Tool count: {metrics.get('tool_count')}")
            lines.append(f"- File fan-out: {metrics.get('file_fanout')}")
            lines.append(f"- File fan-out before first hit: {metrics.get('file_fanout_before_first_hit')}")
            lines.append(
                f"- Search expansion before first hit: {summarize_search_expansion(metrics)}"
            )
            lines.append(f"- Raw source chars: {metrics.get('raw_source_chars')}")
            lines.append(
                f"- Raw source chars before first hit: {metrics.get('raw_source_chars_before_first_hit')}"
            )
            lines.append(f"- MCP payload chars: {metrics.get('mcp_payload_chars')}")
            lines.append(f"- Total ctx tokens: {total_ctx_tokens}")
            if metrics.get("refresh_after_edit_ms") is not None:
                lines.append(f"- Refresh after edit ms: {metrics['refresh_after_edit_ms']}")
            notes = scores.get(route, {}).get("notes", [])
            if notes:
                lines.append(f"- Judge notes: {', '.join(notes)}")
            lines.append("")
            lines.append("```text")
            lines.append(answer[:4000])
            lines.append("```")
            lines.append("")

    markdown_path.parent.mkdir(parents=True, exist_ok=True)
    markdown_path.write_text("\n".join(lines) + "\n")
    return markdown_path


def run_benchmark(args: argparse.Namespace) -> None:
    scenarios = select_scenarios(args.scenarios)
    routes = list(dict.fromkeys(normalize_route_id(route) for route in args.routes))
    invalid_routes = [route for route in routes if route not in ALL_ROUTE_IDS]
    if invalid_routes:
        supported = ", ".join(ALL_ROUTE_IDS)
        raise SystemExit(f"routes must be chosen from: {supported}")
    if args.max_workers > 4:
        raise SystemExit("--max-workers must be <= 4")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    metadata = {
        "generated_at": dt.datetime.now().isoformat(),
        "repo": str(args.repo.resolve()),
        "quickdep_bin": str(args.quickdep_bin.resolve()),
        "claude_bin": str(args.claude_bin.resolve()),
        "model": args.model,
        "bare": args.bare,
        "routes": routes,
        "scenarios": [scenario.sid for scenario in scenarios],
        "max_workers": args.max_workers,
    }
    write_json(args.output_dir / "metadata.json", metadata)

    if not args.skip_prewarm:
        prewarmed_repos: set[str] = set()
        for scenario in scenarios:
            applicable_routes = [route for route in routes if route in scenario.allowed_routes]
            if scenario.sid == "s6" or not any(route != "claude-native-only" for route in applicable_routes):
                continue
            repo_path = resolve_scenario_repo_path(scenario, args.repo).resolve()
            repo_key = str(repo_path)
            if repo_key in prewarmed_repos:
                continue
            ensure_quickdep_scan(args.quickdep_bin, repo_path)
            prewarmed_repos.add(repo_key)

    for scenario in scenarios:
        run_scenario(
            scenario,
            base_repo=args.repo,
            output_dir=args.output_dir,
            routes=routes,
            max_workers=args.max_workers,
            claude_bin=args.claude_bin,
            quickdep_bin=args.quickdep_bin,
            model=args.model,
            bare=args.bare,
            route_timeout_seconds=args.route_timeout_seconds,
        )

    markdown_path = generate_report(args.output_dir, args.output_dir / "REPORT.md")
    log(f"Report written to {markdown_path}")


def main() -> None:
    args = parse_args()
    if args.command == "run":
        run_benchmark(args)
        return

    markdown_path = args.markdown or (args.output_dir / "REPORT.md")
    generated = generate_report(args.output_dir, markdown_path)
    log(f"Report written to {generated}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
