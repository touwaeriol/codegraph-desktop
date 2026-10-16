import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { EngineIcon } from "@/components/EngineIcon";
import { call, desktop, message } from "@/lib/ipc";
import { t } from "@/lib/i18n";
import type {
  Activation,
  Client,
  ConfigStatus,
  Engine,
  Project,
  RuntimeSnapshot,
  SerenaSnapshot,
} from "@/lib/types";
import { ENGINES, engineLabel } from "./McpList";
import { EngineStatus } from "./EngineOverview";

type State = RuntimeSnapshot["state"];
type ConfigState = ConfigStatus["state"];
export const clientName = (client: Client) =>
  client === "codex" ? "Codex" : "Claude Code";
export const engineDescription = (engine: Engine) =>
  ({
    codegraph: t("代码图谱与关系检索"),
    serena: t("LSP 查询与诊断"),
    warpgrep: t("自然语言代码检索"),
    rea: t("应用与二进制逆向分析"),
  })[engine];

/**
 * One-click activation shared by the sidebar markers and the engine list:
 * enabling starts the engine and registers it with the active client,
 * disabling stops it and removes that registration. REA only toggles its
 * registration because clients launch it.
 */
export function useActivation({
  projects,
  client,
  codegraph,
  serena,
  warpgrep,
  refresh,
  openSettings,
  confirm,
}: {
  projects: Project[];
  client: Client;
  codegraph: Record<string, RuntimeSnapshot>;
  serena: Record<string, SerenaSnapshot>;
  warpgrep: Record<string, SerenaSnapshot>;
  refresh: () => Promise<void>;
  openSettings: () => void;
  confirm: (request: {
    title: string;
    detail: string;
    action: () => Promise<void>;
  }) => void;
}) {
  const [configs, setConfigs] = useState<
    Record<string, Partial<Record<Engine, ConfigState>>>
  >({});
  const [pending, setPending] = useState<Record<string, "on" | "off">>({});
  const [version, setVersion] = useState(0);
  const requests = useRef(new Map<string, number>());
  const loadConfig = useCallback(
    async (projectId: string) => {
      if (!desktop) return;
      const request = (requests.current.get(projectId) ?? 0) + 1;
      requests.current.set(projectId, request);
      const entries = await Promise.all(
        ENGINES.map(async (engine) => {
          const statuses = await call<ConfigStatus[]>(
            "get_client_config_status",
            { projectId, engine },
          ).catch(() => []);
          return [
            engine,
            statuses.find((status) => status.client === client)?.state,
          ] as const;
        }),
      );
      if (requests.current.get(projectId) === request)
        setConfigs((old) => ({
          ...old,
          [projectId]: Object.fromEntries(entries),
        }));
    },
    [client],
  );
  const projectIds = projects.map((project) => project.id).join("\n");
  useEffect(() => {
    projectIds
      .split("\n")
      .filter(Boolean)
      .forEach((id) => void loadConfig(id));
  }, [projectIds, loadConfig]);
  const stateOf = (projectId: string, engine: Engine): State | undefined => {
    const transition = pending[`${projectId}:${engine}`];
    if (transition) return transition === "on" ? "starting" : "stopping";
    if (engine === "rea")
      return configs[projectId]?.rea === "configured" ? "running" : "stopped";
    return {
      codegraph: codegraph[projectId]?.state,
      serena: serena[projectId]?.state ?? "stopped",
      warpgrep: warpgrep[projectId]?.state ?? "stopped",
    }[engine];
  };
  const isOn = (projectId: string, engine: Engine) =>
    stateOf(projectId, engine) === "running";
  async function activate(project: Project, engine: Engine, enabled: boolean) {
    const key = `${project.id}:${engine}`;
    setPending((old) => ({ ...old, [key]: enabled ? "on" : "off" }));
    const name = engineLabel(engine);
    try {
      const result = await call<Activation>("set_engine_enabled", {
        projectId: project.id,
        engine,
        enabled,
      });
      const target = clientName(result.client);
      toast.success(
        enabled
          ? t("{0} 已启用，并写入 {1} 项目配置；请在客户端重新加载 MCP", {
              0: name,
              1: target,
            })
          : t("{0} 已停用，并移除 {1} 项目配置", { 0: name, 1: target }),
      );
      result.warnings.forEach((warning) =>
        toast.warning(warning, { duration: 10000 }),
      );
    } catch (e) {
      const code =
        e && typeof e === "object" && "code" in e ? String(e.code) : "";
      if (code === "ENVIRONMENT_REQUIRED")
        toast.error(message(e), {
          duration: 10000,
          action: { label: t("前往设置"), onClick: openSettings },
        });
      else
        toast.error(
          t("{0}：{1}", {
            0: enabled
              ? t("启用 {0} 失败", { 0: name })
              : t("停用 {0} 失败", { 0: name }),
            1: message(e),
          }),
          { duration: 10000 },
        );
    } finally {
      setPending((old) => {
        const next = { ...old };
        delete next[key];
        return next;
      });
      setVersion((value) => value + 1);
      await Promise.all([refresh(), loadConfig(project.id)]).catch(() => {});
    }
  }
  function toggle(project: Project, engine: Engine) {
    const state = stateOf(project.id, engine);
    if (state === "starting" || state === "stopping") return;
    const enabled = state !== "running";
    const sessions = codegraph[project.id]?.sessions ?? 0;
    if (!enabled && engine === "codegraph" && sessions > 0)
      confirm({
        title: t("停用 {0}", { 0: "CodeGraph" }),
        detail: t("当前 {0} 个会话将断开，客户端需要重新连接。", {
          0: sessions,
        }),
        action: () => activate(project, engine, false),
      });
    else void activate(project, engine, enabled);
  }
  return {
    stateOf,
    isOn,
    toggle,
    configured: (projectId: string, engine: Engine) =>
      configs[projectId]?.[engine],
    reloadConfig: loadConfig,
    /** Bumps after every activation so open config views can reload. */
    version,
  };
}
export type ActivationRuntime = ReturnType<typeof useActivation>;

export function ClientSwitch({
  client,
  onChange,
  disabled,
  label,
}: {
  client: Client;
  onChange: (client: Client) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <div className="mcp-client-picker" role="group" aria-label={label}>
      {(["codex", "claude"] as const).map((value) => (
        <button
          key={value}
          type="button"
          aria-pressed={client === value}
          disabled={disabled}
          onClick={() => onChange(value)}
        >
          {clientName(value)}
        </button>
      ))}
    </div>
  );
}

export function EngineList({
  project,
  engine,
  client,
  activation,
  select,
}: {
  project: Project;
  engine: Engine;
  client: Client;
  activation: ActivationRuntime;
  select: (engine: Engine) => void;
}) {
  return (
    <div className="engine-list" role="group" aria-label={t("项目引擎")}>
      {ENGINES.map((value) => {
        const name = engineLabel(value);
        const state = activation.stateOf(project.id, value);
        const configured =
          activation.configured(project.id, value) === "configured";
        const transitional = state === "starting" || state === "stopping";
        return (
          <div
            key={value}
            className="engine-list-row"
            data-selected={engine === value}
            data-testid={`engine-row-${value}`}
          >
            <button
              type="button"
              className="engine-list-select"
              aria-pressed={engine === value}
              onClick={() => select(value)}
            >
              <span className="engine-symbol">
                <EngineIcon engine={value} />
              </span>
              <span className="min-w-0">
                <strong>{name}</strong>
                <small>{engineDescription(value)}</small>
              </span>
            </button>
            {value === "rea" ? (
              <Badge variant="secondary">{t("由客户端启动")}</Badge>
            ) : (
              <EngineStatus snapshot={state ? { state } : undefined} />
            )}
            <span
              className="engine-list-config"
              data-configured={configured}
              title={t("当前客户端：{0}", { 0: clientName(client) })}
            >
              {configured
                ? t("已注入 {0}", { 0: clientName(client) })
                : t("未注入 {0}", { 0: clientName(client) })}
            </span>
            <label className="mcp-switch">
              <input
                type="checkbox"
                role="switch"
                aria-label={t("启用 {0}", { 0: name })}
                checked={state === "running" || state === "starting"}
                disabled={!desktop || transitional}
                onChange={() => activation.toggle(project, value)}
              />
              <span className="mcp-switch-track" aria-hidden="true">
                <span />
              </span>
            </label>
          </div>
        );
      })}
    </div>
  );
}
