import { useEffect, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { call, desktop, message } from "@/lib/ipc";
import { t } from "@/lib/i18n";
import type { DisabledToolsSaved, Engine, EngineToolLists } from "@/lib/types";

const same = (a: string[], b: string[]) =>
  a.length === b.length && a.every((value, index) => value === b[index]);

/**
 * Lets the user hide an engine's tools from the model. Codex and Devin
 * receive the list in their entries, Cursor an allowlist of the remaining
 * tools, and Claude Code deny rules.
 */
export function EngineTools({ engine }: { engine: Engine }) {
  const [known, setKnown] = useState<string[]>([]);
  const [saved, setSaved] = useState<string[]>([]);
  const [disabled, setDisabled] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState<"refresh" | "save" | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!desktop) return;
    call<EngineToolLists>("get_engine_tools")
      .then((lists) => {
        setKnown(lists.known[engine] ?? []);
        setSaved(lists.disabled[engine] ?? []);
        setDisabled(lists.disabled[engine] ?? []);
      })
      .catch((e) => setError(message(e)));
  }, [engine]);
  const tools = [...new Set([...known, ...saved])].sort();
  const query = filter.trim().toLowerCase();
  const visible = query
    ? tools.filter((tool) => tool.toLowerCase().includes(query))
    : tools;
  const dirty = !same(disabled, saved);
  async function refresh() {
    setBusy("refresh");
    setError("");
    try {
      setKnown(
        await call<string[]>("refresh_engine_tools", { engine }).then((list) =>
          [...list].sort(),
        ),
      );
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  }
  async function save() {
    setBusy("save");
    setError("");
    try {
      const result = await call<DisabledToolsSaved>("save_disabled_tools", {
        engine,
        tools: disabled,
      });
      setSaved(result.disabled);
      setDisabled(result.disabled);
      toast.success(
        result.updated.length
          ? t(
              "工具设置已保存，并更新 {0} 个客户端配置；请在客户端重新加载 MCP",
              {
                0: result.updated.length,
              },
            )
          : t("工具设置已保存"),
      );
      if (result.failed.length)
        toast.warning(
          t("以下项目配置未更新，请在 MCP 配置中检查：{0}", {
            0: result.failed.join("；"),
          }),
          { duration: 10000 },
        );
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  }
  function toggle(tool: string, enabled: boolean) {
    setDisabled((old) =>
      enabled ? old.filter((value) => value !== tool) : [...old, tool].sort(),
    );
  }
  return (
    <details
      className="engine-install engine-tools"
      data-testid={`tools-${engine}`}
    >
      <summary>
        {tools.length
          ? t("工具（已停用 {0} / 共 {1}）", {
              0: saved.length,
              1: tools.length,
            })
          : t("工具")}
      </summary>
      <p className="text-xs text-muted-foreground leading-5">
        {t(
          "取消勾选的工具不会提供给模型：Codex 与 Devin 写入禁用列表，Cursor 写入其余工具的允许列表，Claude Code 写入项目 .claude/settings.json 的 deny 规则。保存后自动更新已由本应用注入的配置。",
        )}
      </p>
      <div className="toolbar">
        {tools.length > 8 && (
          <Input
            className="engine-tools-filter"
            aria-label={t("筛选工具")}
            placeholder={t("筛选工具")}
            spellCheck={false}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        )}
        <Button
          variant="outline"
          size="sm"
          disabled={!desktop || busy !== null}
          onClick={() => void refresh()}
        >
          {busy === "refresh" ? (
            <Loader2 className="animate-spin" />
          ) : (
            <RefreshCw />
          )}
          {t("获取工具列表")}
        </Button>
      </div>
      {tools.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          {engine === "rea"
            ? t("获取时会临时启动 REA 读取工具列表，完成后结束进程。")
            : t("先启动任一项目的该引擎实例，再获取工具列表。")}
        </p>
      ) : (
        <div className="engine-tools-list" role="group" aria-label={t("工具")}>
          {visible.map((tool) => (
            <label key={tool}>
              <input
                type="checkbox"
                checked={!disabled.includes(tool)}
                onChange={(e) => toggle(tool, e.target.checked)}
              />
              <span className="mono">{tool}</span>
            </label>
          ))}
        </div>
      )}
      {error && (
        <p role="alert" className="text-destructive text-sm break-all">
          {error}
        </p>
      )}
      <div className="toolbar">
        <Button
          size="sm"
          disabled={!desktop || !dirty || busy !== null}
          onClick={() => void save()}
        >
          {busy === "save" ? <Loader2 className="animate-spin" /> : null}
          {t("保存工具设置")}
        </Button>
        {disabled.length > 0 && (
          <Button
            variant="ghost"
            size="sm"
            disabled={busy !== null}
            onClick={() => setDisabled([])}
          >
            {t("全部启用")}
          </Button>
        )}
      </div>
    </details>
  );
}
