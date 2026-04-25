import type {
  DependenciesResponse,
  DependencyNode,
  ProjectState,
} from "./api/types";
import type { Locale } from "./i18n";

export type Tone = "neutral" | "good" | "info" | "warn" | "danger";

function localeTag(locale: Locale): string {
  return locale === "zh-CN" ? "zh-CN" : "en-US";
}

function normalizeLabelKey(value: string): string {
  return value
    .trim()
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toLowerCase();
}

function translateLabel(
  value: string,
  locale: Locale,
  labels: Record<string, { "zh-CN": string; en: string }>,
): string {
  const normalized = normalizeLabelKey(value);
  return labels[normalized]?.[locale] ?? value;
}

export function summarizeProjectState(state: ProjectState, locale: Locale): {
  label: string;
  tone: Tone;
  detail: string;
  stats?: {
    files?: number;
    symbols?: number;
    dependencies?: number;
  };
} {
  if (state === "NotLoaded") {
    return {
      label: locale === "zh-CN" ? "未加载" : "Not Loaded",
      tone: "neutral",
      detail:
        locale === "zh-CN"
          ? "项目已注册，但还没有执行扫描。"
          : "Project is registered but has not been scanned yet.",
    };
  }

  if ("Loading" in state) {
    const payload = state.Loading;
    return {
      label: locale === "zh-CN" ? "加载中" : "Loading",
      tone: "info",
      detail:
        locale === "zh-CN"
          ? `${payload.scanned_files}/${payload.total_files} 个文件`
          : `${payload.scanned_files}/${payload.total_files} files`,
    };
  }

  if ("Loaded" in state) {
    const payload = state.Loaded;
    return {
      label: payload.watching
        ? locale === "zh-CN"
          ? "实时监控"
          : "Live"
        : locale === "zh-CN"
          ? "已加载"
          : "Loaded",
      tone: "good",
      detail:
        locale === "zh-CN"
          ? `已加载于 ${formatTimestamp(payload.loaded_at, locale)}`
          : `Loaded ${formatTimestamp(payload.loaded_at, locale)}`,
      stats: {
        files: payload.file_count,
        symbols: payload.symbol_count,
        dependencies: payload.dependency_count,
      },
    };
  }

  if ("WatchPaused" in state) {
    const payload = state.WatchPaused;
    return {
      label: locale === "zh-CN" ? "监控已暂停" : "Watch Paused",
      tone: "warn",
      detail: payload.reason,
      stats: {
        files: payload.file_count,
        symbols: payload.symbol_count,
        dependencies: payload.dependency_count,
      },
    };
  }

  const payload = state.Failed;
  return {
    label: locale === "zh-CN" ? "失败" : "Failed",
    tone: "danger",
    detail: payload.error,
  };
}

export function formatTimestamp(unixSeconds: number | undefined, locale: Locale): string {
  if (!unixSeconds) {
    return locale === "zh-CN" ? "无" : "n/a";
  }

  const date = new Date(unixSeconds * 1000);
  return new Intl.DateTimeFormat(localeTag(locale), {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function compactPath(path: string, keep = 3): string {
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= keep) {
    return path;
  }

  return `.../${parts.slice(-keep).join("/")}`;
}

export function dependencyBuckets(data?: DependenciesResponse): {
  incoming: DependencyNode[];
  outgoing: DependencyNode[];
} {
  if (!data) {
    return { incoming: [], outgoing: [] };
  }

  if (data.direction === "incoming") {
    return { incoming: data.dependencies ?? [], outgoing: [] };
  }

  if (data.direction === "outgoing") {
    return { incoming: [], outgoing: data.dependencies ?? [] };
  }

  return {
    incoming: data.incoming ?? [],
    outgoing: data.outgoing ?? [],
  };
}

export const splitDependencies = dependencyBuckets;

export function presentSymbolKind(kind: string, locale: Locale): string {
  return translateLabel(kind, locale, {
    function: { "zh-CN": "函数", en: "Function" },
    method: { "zh-CN": "方法", en: "Method" },
    class: { "zh-CN": "类", en: "Class" },
    struct: { "zh-CN": "结构体", en: "Struct" },
    enum: { "zh-CN": "枚举", en: "Enum" },
    enum_variant: { "zh-CN": "枚举成员", en: "Enum Variant" },
    interface: { "zh-CN": "接口", en: "Interface" },
    trait: { "zh-CN": "特征", en: "Trait" },
    type_alias: { "zh-CN": "类型别名", en: "Type Alias" },
    module: { "zh-CN": "模块", en: "Module" },
    constant: { "zh-CN": "常量", en: "Constant" },
    variable: { "zh-CN": "变量", en: "Variable" },
    property: { "zh-CN": "属性", en: "Property" },
    macro: { "zh-CN": "宏", en: "Macro" },
  });
}

export function presentDependencyKind(kind: string, locale: Locale): string {
  return translateLabel(kind, locale, {
    call: { "zh-CN": "调用", en: "Call" },
    inherit: { "zh-CN": "继承", en: "Inherit" },
    implement: { "zh-CN": "实现", en: "Implement" },
    type_use: { "zh-CN": "类型使用", en: "Type Use" },
    import: { "zh-CN": "导入", en: "Import" },
  });
}

export function formatCount(value?: number): string {
  if (value === undefined || Number.isNaN(value)) {
    return "0";
  }

  return new Intl.NumberFormat().format(value);
}
