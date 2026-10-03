import { t, useLanguage } from "@/lib/i18n";
import { useMemo } from "react";
import { diffLines } from "diff";

/** Exact line comparison: whitespace and missing final newlines remain significant. */
export function LineDiff({ before, after }: { before: string; after: string }) {
  useLanguage();
  const changes = useMemo(() => diffLines(before, after), [before, after]);
  let oldLine = 0;
  let newLine = 0;
  if (before === after)
    return (
      <p className="text-xs text-muted-foreground p-4 border rounded-md">
        {t("文件内容未变化。")}
      </p>
    );
  return (
    <div
      className="diff !p-0"
      role="region"
      aria-label={t("逐行配置差异")}
      tabIndex={0}
    >
      <div className="min-w-max">
        {changes.flatMap((change, block) => {
          const lines = change.value.split("\n");
          const hasFinalNewline = lines.at(-1) === "";
          if (hasFinalNewline) lines.pop();
          return lines.map((line, index) => {
            const oldNumber = change.added ? "" : ++oldLine;
            const newNumber = change.removed ? "" : ++newLine;
            return (
              <div
                key={`${block}-${index}`}
                className={`flex leading-6 ${change.added ? "bg-green-50 text-green-900" : change.removed ? "bg-red-50 text-red-900" : "text-muted-foreground"}`}
              >
                <span
                  aria-label={t("原行号")}
                  className="select-none w-10 px-1 shrink-0 text-right opacity-60"
                >
                  {oldNumber}
                </span>
                <span
                  aria-label={t("新行号")}
                  className="select-none w-10 px-1 shrink-0 text-right opacity-60"
                >
                  {newNumber}
                </span>
                <span
                  aria-label={
                    change.added
                      ? t("新增")
                      : change.removed
                        ? t("删除")
                        : t("未变")
                  }
                  className="select-none w-7 text-center shrink-0"
                >
                  {change.added ? "+" : change.removed ? "−" : " "}
                </span>
                <span className="pr-4">
                  {line}
                  {!hasFinalNewline && index === lines.length - 1 ? (
                    <span className="italic opacity-60">
                      {t("⏎ 无末尾换行")}
                    </span>
                  ) : null}
                </span>
              </div>
            );
          });
        })}
      </div>
    </div>
  );
}
