import type { Locale } from "./i18n";

export function errorText(error: unknown, locale: Locale): string {
  if (error instanceof Error) {
    return error.message;
  }
  return locale === "zh-CN" ? "发生了未预期错误" : "Unexpected error";
}
