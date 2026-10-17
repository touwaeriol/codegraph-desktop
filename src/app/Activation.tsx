import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { EngineIcon } from "@/components/EngineIcon";
import { CLIENTS, clientName } from "@/lib/clients";
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
type ClientStates = Partial<Record<Client, ConfigState>>;
/** An activation in flight: the client it targets, or every client. */
type Pending = { client: Client | null; engine?: "on" | "off" };
export { clientName };
export const engineDescription = (engine: Engine) =>
  ({
    codegraph: t("代码图谱与关系检索"),
    serena: t("LSP 查询与诊断"),
    warpgrep: t("自然语言代码检索"),
    rea: t("应用与二进制逆向分析"),
  })[engine];

/**
 * One-click activation shared by the sidebar markers and the engine list.
 * Each client switch registers the engine with that client, starting it
 * first; turning off its last client stops it. Sidebar markers enable with
 * the default client and disable everywhere. REA only toggles registrations
 * because clients launch it.
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
  /** Default client for the sidebar markers. */
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
    Record<string, Partial<Record<Engine, ClientStates>>>
  >({});
  const [pending, setPending] = useState<Record<string, Pending>>({});
  const [version, setVersion] = useState(0);
  const requests = useRef(new Map<string, number>());
  const loadConfig = useCallback(async (projectId: string) => {
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
          Object.fromEntries(
            statuses.map((status) => [status.client, status.state]),
          ) as ClientStates,
        ] as const;
      }),
    );
    if (requests.current.get(projectId) === request)
      setConfigs((old) => ({
        ...old,
        [projectId]: Object.fromEntries(entries),
      }));
  }, []);
  const projectIds = projects.map((project) => project.id).join("\n");
  useEffect(() => {
    projectIds
      .split("\n")
      .filter(Boolean)
      .forEach((id) => void loadConfig(id));
  }, [projectIds, loadConfig]);
  const configuredClients = (projectId: string, engine: Engine) =>
    CLIENTS.filter(
      (value) => configs[projectId]?.[engine]?.[value] === "configured",
    );
  const stateOf = (projectId: string, engine: Engine): State | undefined => {
    const transition = pending[`${projectId}:${engine}`]?.engine;
    if (transition) return transition === "on" ? "starting" : "stopping";
    if (engine === "rea")
      return configuredClients(projectId, "rea").length ? "running" : "stopped";
    return {
      codegraph: codegraph[projectId]?.state,
      serena: serena[projectId]?.state ?? "stopped",
      warpgrep: warpgrep[projectId]?.state ?? "stopped",
    }[engine];
  };
  const isOn = (projectId: string, engine: Engine) =>
    stateOf(projectId, engine) === "running";
  async function activate(
    project: Project,
    engine: Engine,
    enabled: boolean,
    target: Client | null,
  ) {
    const key = `${project.id}:${engine}`;
    const others = configuredClients(project.id, engine).filter(
      (value) => value !== target,
    );
    const running = isOn(project.id, engine);
    setPending((old) => ({
      ...old,
      [key]: {
        client: target,
        engine: enabled
          ? running
            ? undefined
            : "on"
          : target && others.length
            ? undefined
            : "off",
      },
    }));
    const name = engineLabel(engine);
    try {
      const result = await call<Activation>("set_engine_enabled", {
        projectId: project.id,
        engine,
        enabled,
        client: target,
      });
      const targets = result.clients.map(clientName).join(" / ");
      toast.success(
        enabled
          ? t("{0} 已启用，并写入 {1} 项目配置；请在客户端重新加载 MCP", {
              0: name,
              1: targets,
            })
          : result.running
            ? t("{0} 已从 {1} 项目配置移除", { 0: name, 1: targets })
            : targets
              ? t("{0} 已停用，并移除 {1} 项目配置", { 0: name, 1: targets })
              : t("{0} 已停用", { 0: name }),
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
  /** Asks before a stop that would disconnect CodeGraph sessions. */
  function guard(
    project: Project,
    engine: Engine,
    stops: boolean,
    action: () => Promise<void>,
  ) {
    const sessions = codegraph[project.id]?.sessions ?? 0;
    if (stops && engine === "codegraph" && sessions > 0)
      confirm({
        title: t("停用 {0}", { 0: "CodeGraph" }),
        detail: t("当前 {0} 个会话将断开，客户端需要重新连接。", {
          0: sessions,
        }),
        action,
      });
    else void action();
  }
  function toggle(project: Project, engine: Engine) {
    if (pending[`${project.id}:${engine}`]) return;
    const state = stateOf(project.id, engine);
    if (state === "starting" || state === "stopping") return;
    const enabled = state !== "running";
    guard(project, engine, !enabled, () =>
      activate(project, engine, enabled, null),
    );
  }
  function toggleClient(project: Project, engine: Engine, target: Client) {
    if (pending[`${project.id}:${engine}`]) return;
    const state = stateOf(project.id, engine);
    if (state === "starting" || state === "stopping") return;
    const configured = configuredClients(project.id, engine);
    const enabled = !configured.includes(target);
    const stops = !enabled && configured.length === 1;
    guard(project, engine, stops, () =>
      activate(project, engine, enabled, target),
    );
  }
  return {
    stateOf,
    isOn,
    toggle,
    toggleClient,
    /** The client a pending activation targets; null means every client. */
    pendingClient: (projectId: string, engine: Engine) =>
      pending[`${projectId}:${engine}`]?.client,
    busy: (projectId: string, engine: Engine) =>
      !!pending[`${projectId}:${engine}`],
    clientState: (projectId: string, engine: Engine, value: Client) =>
      configs[projectId]?.[engine]?.[value],
    /** Whether any client registers the engine. */
    configured: (projectId: string, engine: Engine) =>
      configuredClients(projectId, engine).length > 0,
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
    <select
      className="client-select"
      aria-label={label}
      value={client}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value as Client)}
    >
      {CLIENTS.map((value) => (
        <option key={value} value={value}>
          {clientName(value)}
        </option>
      ))}
    </select>
  );
}

export function EngineList({
  project,
  engine,
  activation,
  select,
}: {
  project: Project;
  engine: Engine;
  activation: ActivationRuntime;
  select: (engine: Engine) => void;
}) {
  return (
    <div className="engine-list" role="group" aria-label={t("项目引擎")}>
      {ENGINES.map((value) => {
        const name = engineLabel(value);
        const state = activation.stateOf(project.id, value);
        const busy = activation.busy(project.id, value);
        const target = activation.pendingClient(project.id, value);
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
            <div
              className="engine-clients"
              role="group"
              aria-label={t("{0} 写入的客户端", { 0: name })}
            >
              {CLIENTS.map((client) => {
                const config = activation.clientState(
                  project.id,
                  value,
                  client,
                );
                const on = config === "configured";
                const saving = busy && (target === client || target === null);
                const label = clientName(client);
                return (
                  <label
                    key={client}
                    className="engine-client"
                    data-client={client}
                    data-state={config ?? "unknown"}
                    title={
                      config === "repair" || config === "parseError"
                        ? t("{0} 中的配置需要检查，请在 MCP 配置中处理", {
                            0: label,
                          })
                        : on
                          ? t("已注入 {0}", { 0: label })
                          : t("未注入 {0}", { 0: label })
                    }
                  >
                    <span>{label}</span>
                    <span className="mcp-switch">
                      <input
                        type="checkbox"
                        role="switch"
                        aria-label={t("在 {1} 中启用 {0}", {
                          0: name,
                          1: label,
                        })}
                        aria-busy={saving}
                        checked={on}
                        disabled={
                          !desktop ||
                          busy ||
                          config === undefined ||
                          config === "parseError"
                        }
                        onChange={() =>
                          activation.toggleClient(project, value, client)
                        }
                      />
                      <span className="mcp-switch-track" aria-hidden="true">
                        <span />
                      </span>
                    </span>
                  </label>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}
