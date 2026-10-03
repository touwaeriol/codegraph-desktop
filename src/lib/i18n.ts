import { useSyncExternalStore } from "react";
import { english, type MessageKey } from "./messages";
export type Language = "en" | "zh-CN";
export function resolveLanguage(preferred: string | undefined): Language {
  return /^zh(?:-|_|$)/i.test(preferred ?? "") ? "zh-CN" : "en";
}
let language: Language = resolveLanguage(
  typeof navigator === "undefined"
    ? undefined
    : (navigator.languages?.[0] ?? navigator.language),
);
const listeners = new Set<() => void>();
export function setLanguage(next: Language) {
  if (typeof document !== "undefined") document.documentElement.lang = next;
  if (language === next) return;
  language = next;
  listeners.forEach((listener) => listener());
}
export function useLanguage() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => language,
    () => "en" as Language,
  );
}
export function t(
  key: MessageKey,
  values: Record<string, string | number | undefined> = {},
): string {
  const template = language === "en" ? english[key] : key;
  return template.replace(/\{(\d+)\}/g, (match, name: string) =>
    values[name] === undefined ? match : String(values[name]),
  );
}
setLanguage(language);
