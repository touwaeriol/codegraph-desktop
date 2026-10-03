import { t, useLanguage } from "@/lib/i18n";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
export const desktop = isTauri();
export function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop)
    return Promise.reject(
      new Error(t("桌面环境未连接，请在 CodeGraph Desktop 中执行此操作。")),
    );
  return invoke<T>(command, args);
}
export function subscribe<T>(event: string, callback: (payload: T) => void) {
  return listen<T>(event, (e) => callback(e.payload));
}
export function message(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error)
    return String(error.message);
  return JSON.stringify(error);
}
