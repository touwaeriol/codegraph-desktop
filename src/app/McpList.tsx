import { Loader2, MoreHorizontal, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { t } from "@/lib/i18n";
import type { Client, ConfigStatus, Engine } from "@/lib/types";

export const ENGINES = ["codegraph", "serena", "warpgrep", "rea"] as const;
export const engineLabel = (value: Engine) =>
  ({
    codegraph: "CodeGraph",
    serena: "Serena",
    warpgrep: "WarpGrep",
    rea: "REA",
  })[value];
export type McpStatus = ConfigStatus & { engine: Engine };
export type McpChange = { engine: Engine; client: Client; enabled: boolean };

export function McpList({
  statuses,
  client,
  setClient,
  busy,
  pending,
  failed,
  morphReady,
  canProbe,
  toggle,
  probe,
  review,
}: {
  statuses: McpStatus[];
  client: Client;
  setClient: (client: Client) => void;
  busy: boolean;
  pending: McpChange | null;
  failed: McpChange | null;
  morphReady: boolean;
  canProbe: (engine: Engine) => boolean;
  toggle: (change: McpChange) => void;
  probe: (engine: Engine) => void;
  review: (engine: Engine, client: Client) => void;
}) {
  const clientName = client === "codex" ? "Codex" : "Claude Code";
  const clientStatuses = statuses.filter((status) => status.client === client);
  const configured = clientStatuses.filter(
    (status) => status.state === "configured",
  ).length;
  const descriptions: Record<Engine, string> = {
    codegraph: t("代码图谱与关系检索"),
    serena: t("LSP 查询与诊断"),
    warpgrep: t("自然语言代码检索"),
    rea: t("应用与二进制逆向分析"),
  };
  return (
    <Card data-testid="mcp-list" className="mcp-list-card">
      <CardHeader className="mcp-list-header">
        <CardTitle
          className="text-sm font-medium text-muted-foreground"
          aria-live="polite"
        >
          {clientStatuses.some((status) => status.state === "parseError")
            ? t("解析错误")
            : clientStatuses.length === ENGINES.length
              ? t("{0} 个 MCP 已配置", { 0: configured })
              : t("正在读取配置")}
        </CardTitle>
        <div
          className="mcp-client-picker"
          role="group"
          aria-label={t("MCP 客户端")}
        >
          {(["codex", "claude"] as const).map((value) => (
            <button
              key={value}
              type="button"
              aria-pressed={client === value}
              disabled={busy}
              onClick={() => setClient(value)}
            >
              {value === "codex" ? "Codex" : "Claude Code"}
            </button>
          ))}
        </div>
      </CardHeader>
      <CardContent className="mcp-list-content">
        <ul className="mcp-compact-list" aria-label={t("项目 MCP 列表")}>
          {ENGINES.map((engine) => {
            const status = clientStatuses.find(
              (status) => status.engine === engine,
            );
            const saving =
              pending?.engine === engine && pending.client === client;
            const needsReview =
              status?.state === "repair" ||
              status?.state === "parseError" ||
              (failed?.engine === engine && failed.client === client);
            const checked = saving
              ? pending.enabled
              : status?.state === "configured";
            const stateLabel = saving
              ? t("保存中")
              : status
                ? {
                    configured: t("已配置"),
                    missing: t("未配置"),
                    repair: t("需修复"),
                    parseError: t("解析错误"),
                  }[status.state]
                : t("正在读取配置");
            const hint =
              engine === "warpgrep" && !morphReady
                ? t("需先验证 Morph API Key")
                : status?.message || descriptions[engine];
            const stateId = `mcp-state-${engine}-${client}`;
            return (
              <li
                key={engine}
                data-testid={`mcp-row-${engine}`}
                className="mcp-compact-row"
                title={`${engineLabel(engine)} · ${stateLabel} · ${hint}`}
              >
                <span
                  className="mcp-state-dot"
                  data-state={
                    saving
                      ? "saving"
                      : needsReview
                        ? "error"
                        : checked
                          ? "configured"
                          : "missing"
                  }
                  aria-hidden="true"
                />
                <span className="mcp-row-name">{engineLabel(engine)}</span>
                <span id={stateId} className="sr-only" aria-live="polite">
                  {stateLabel} · {hint}
                </span>
                {saving ? (
                  <span
                    role="status"
                    className="mcp-row-action flex items-center justify-center"
                  >
                    <Loader2
                      size={15}
                      className="animate-spin text-muted-foreground"
                      aria-hidden="true"
                    />
                    <span className="sr-only">{t("保存中")}</span>
                  </span>
                ) : needsReview ? (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="mcp-row-action text-amber-600"
                    disabled={busy}
                    aria-label={`${t("检查配置")} · ${engineLabel(engine)} · ${clientName}`}
                    onClick={() => review(engine, client)}
                  >
                    <TriangleAlert size={15} />
                  </Button>
                ) : (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="mcp-row-action mcp-row-menu"
                        disabled={busy}
                        aria-label={t("{0} 操作", { 0: engineLabel(engine) })}
                      >
                        <MoreHorizontal size={16} />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        disabled={!canProbe(engine)}
                        onSelect={() => probe(engine)}
                      >
                        {t("测试连接")}
                      </DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => review(engine, client)}>
                        {t("检查配置")}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
                <label className="mcp-switch">
                  <input
                    type="checkbox"
                    role="switch"
                    aria-label={`${engineLabel(engine)} · ${clientName}`}
                    aria-describedby={stateId}
                    checked={!!checked}
                    disabled={
                      busy ||
                      !status ||
                      status.state === "parseError" ||
                      (engine === "warpgrep" && !morphReady && !checked)
                    }
                    onChange={(event) =>
                      toggle({ engine, client, enabled: event.target.checked })
                    }
                  />
                  <span className="mcp-switch-track" aria-hidden="true">
                    <span />
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
        <div className="mcp-list-footer">
          <span>{t("开关自动保存，客户端需重新加载 MCP。")}</span>
          <code title={clientStatuses[0]?.path}>
            {client === "codex" ? ".codex/config.toml" : ".mcp.json"}
          </code>
        </div>
      </CardContent>
    </Card>
  );
}
