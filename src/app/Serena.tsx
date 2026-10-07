import { useCallback, useEffect, useState, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Code2, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Field, FieldLabel } from "@/components/ui/field";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { call, desktop, message } from "@/lib/ipc";
import { ExternalLink } from "@/components/ExternalLink";
import {
  EngineRuntimeCard,
  EngineConnectionCard,
  EngineSettingsCard,
  Metric,
} from "./EngineOverview";
import { t, useLanguage } from "@/lib/i18n";
import type { Environment, SerenaSnapshot } from "@/lib/types";

const stopped: SerenaSnapshot = {
  state: "stopped",
  pid: null,
  endpoint: null,
  startedAt: null,
  tools: [],
  error: null,
};
export function useSerena(projectId: string) {
  const [snapshots, setSnapshots] = useState<Record<string, SerenaSnapshot>>(
    {},
  );
  const snapshot = snapshots[projectId] ?? stopped;
  const current = useRef(projectId);
  current.current = projectId;
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [operation, setOperation] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      const data = await call<Record<string, SerenaSnapshot>>(
        "list_serena_snapshots",
      );
      setSnapshots(data);
    } catch (e) {
      setError(message(e));
    }
  }, []);
  useEffect(() => {
    setError("");
    setOperation(null);
    setPending(false);
    if (!projectId || !desktop) return;
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => {
      clearInterval(timer);
    };
  }, [projectId, refresh]);
  useEffect(() => {
    if (!operation) return;
    let active = true;
    const timer = setInterval(
      () =>
        void call<{ operationId: string; state: string }[]>(
          "get_project_tasks",
          { projectId },
        )
          .then((tasks) => {
            if (
              active &&
              tasks.some(
                (task) =>
                  task.operationId === operation && task.state !== "running",
              )
            )
              setOperation(null);
          })
          .catch((e) => {
            if (active) {
              setError(message(e));
              setOperation(null);
            }
          }),
      1000,
    );
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [operation, projectId]);
  const run = useCallback(
    async (action: string) => {
      setPending(true);
      setError("");
      try {
        const op = await call<string>("serena_operation", {
          projectId,
          action,
        });
        if (current.current === projectId) setOperation(op);
      } catch (e) {
        setError(message(e));
      } finally {
        setPending(false);
      }
    },
    [projectId],
  );
  return {
    snapshot,
    snapshots,
    refresh,
    error,
    busy: pending || !!operation,
    run,
    cancel: async () => {
      if (operation) await call("cancel_task", { operationId: operation });
    },
  };
}

export function SerenaPanel({
  runtime,
  settings,
  configure,
}: {
  runtime: ReturnType<typeof useSerena>;
  settings: () => void;
  configure: () => void;
}) {
  useLanguage();
  const { snapshot: s, error } = runtime;
  return (
    <div className="panel-grid engine-overview" data-testid="serena-panel">
      {(error || s.error) && (
        <Alert variant="destructive" className="col-span-full">
          <AlertDescription>{error || s.error?.message}</AlertDescription>
        </Alert>
      )}
      <EngineRuntimeCard
        name="Serena"
        state={s.state}
        pid={s.pid}
        port={s.endpoint ? new URL(s.endpoint).port : null}
        startedAt={s.startedAt}
        detail={{
          name: t("可用工具"),
          value: s.state === "running" ? s.tools.length : "—",
        }}
        onRefresh={() => void runtime.refresh()}
        busy={runtime.busy}
      />
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Code2 size={17} />
            {t("语义代码工具")}
          </CardTitle>
          <CardDescription>{t("符号定位、引用分析与诊断")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="metrics">
            <Metric name={t("运行模式")} value={t("单项目模式")} />
            <Metric name={t("连接协议")} value="Streamable HTTP" />
            <Metric
              name={t("可用工具")}
              value={s.state === "running" ? s.tools.length : "—"}
            />
            <Metric name={t("项目配置")} value=".serena/project.yml" />
          </div>
          {s.tools.length > 0 && (
            <details>
              <summary className="text-primary text-sm cursor-pointer">
                {t("查看可用工具")}
              </summary>
              <div className="flex flex-wrap gap-2 mt-3">
                {s.tools.map((tool) => (
                  <Badge variant="secondary" className="mono" key={tool}>
                    {tool}
                  </Badge>
                ))}
              </div>
            </details>
          )}
          <p className="text-xs text-muted-foreground leading-6">
            {t("默认仅启用 LSP 查询与诊断。文件编辑、文本搜索及 memory 等辅助工具已禁用。")}
          </p>
          <p className="text-xs text-muted-foreground leading-6">
            {t("首次启动可能需要准备语言服务器，可在运行日志中查看进度。")}
          </p>
          <Button variant="outline" onClick={settings}>
            {t("配置运行环境")}
          </Button>
        </CardContent>
      </Card>
      <EngineConnectionCard
        name="Serena"
        configure={configure}
        endpoint={s.endpoint}
      />
    </div>
  );
}

export function SerenaSettings({ entry: initial }: { entry?: string | null }) {
  useLanguage();
  const [entry, setEntry] = useState(initial ?? "");
  const [environment, setEnvironment] = useState<Environment | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function detect() {
    setBusy(true);
    setError("");
    try {
      const result = await call<Environment>("detect_serena", {
        selectedPath: entry.trim() || null,
      });
      setEnvironment(result);
      if (!result.available || !result.entry)
        throw new Error(result.error || t("检测未返回可用入口，请重新检测。"));
      setEntry(result.entry);
      toast.success(t("Serena 检测成功"));
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <EngineSettingsCard
      name="Serena"
      description={t("LSP 查询与诊断")}
      testId="serena-settings"
    >
      <Field>
        <FieldLabel htmlFor="serena-entry">{t("Serena 入口")}</FieldLabel>
        <Input
          id="serena-entry"
          value={entry}
          onChange={(e) => setEntry(e.target.value)}
          placeholder={t("自动检测或选择可执行入口")}
        />
        <div className="toolbar">
          <Button
            variant="outline"
            disabled={busy || !desktop}
            onClick={() =>
              void open({ multiple: false, directory: false })
                .then((path) => {
                  if (typeof path === "string") setEntry(path);
                })
                .catch((e) => setError(message(e)))
            }
          >
            {t("选择文件")}
          </Button>
          <Button
            variant="outline"
            disabled={busy || !desktop}
            aria-label={t("检测 Serena 并使用")}
            onClick={() => void detect()}
          >
            {busy ? <Loader2 className="animate-spin" /> : null}
            {t("检测并使用")}
          </Button>
        </div>
        {error && (
          <p role="alert" className="text-destructive text-sm break-all">
            {error}
          </p>
        )}
        <p className="text-xs text-muted-foreground">
          {environment?.available
            ? t("已检测：{0}", { 0: environment.version ?? t("版本未提供") })
            : t("尚未检测")}
          {t("。更换入口后，运行实例需重启生效。")}
        </p>
        {environment?.available && environment.entry && (
          <p className="mono text-xs break-all">
            {t("实际入口：{0}", { 0: environment.entry })}
          </p>
        )}
        <details className="engine-install">
          <summary>{t("安装说明")}</summary>
          <p className="text-xs text-muted-foreground">
            {t(
              "安装 uv 后执行以下命令，然后检测入口。更换入口后需重启 Serena。",
            )}
          </p>
          <code className="install-command">
            uv tool install -p 3.13 serena-agent
          </code>
          <ExternalLink
            href="https://oraios.github.io/serena/02-usage/010_installation.html"
            className="text-primary text-sm underline"
          >
            {t("Serena 官方安装说明 ↗")}
          </ExternalLink>
        </details>
      </Field>
    </EngineSettingsCard>
  );
}
