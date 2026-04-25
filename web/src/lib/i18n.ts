export type Locale = "zh-CN" | "en";

export function resolveDefaultLocale(): Locale {
  if (typeof window === "undefined") {
    return "en";
  }

  return window.navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}
