import { useCallback, useEffect, useState, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Code2,
  Copy,
  Loader2,
  Play,
  RefreshCw,
  Square,
  ArrowRight,
  CheckCircle2,
} from "lucide-react";
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
  useEffect(() => {
    let active = true;
    setError("");
    setOperation(null);
    setPending(false);
    if (!projectId || !desktop) return;
    const refresh = async () => {
      try {
        const data = await call<Record<string, SerenaSnapshot>>(
          "list_serena_snapshots",
        );
        if (active) {
          setSnapshots(data);
          setError("");
        }
      } catch (e) {
        if (active) setError(message(e));
      }
    };
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [projectId]);
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
  const language = useLanguage();
  const { snapshot: s, busy, error, run } = runtime;
  const running = s.state === "running";
  const labels = {
    running: t("运行中"),
    stopped: t("已停止"),
    starting: t("启动中"),
    stopping: t("停止中"),
    error: t("运行错误"),
  };
  return (
    <div className="space-y-5" data-testid="serena-panel">
      {(error || s.error) && (
        <Alert variant="destructive">
          <AlertDescription>{error || s.error?.message}</AlertDescription>
        </Alert>
      )}
      <Card className="engine-detail">
        <CardHeader className="flex flex-row items-start justify-between gap-4 flex-wrap">
          <div className="flex gap-3">
            <span className="engine-symbol serena">
              <Code2 size={23} />
            </span>
            <div>
              <CardTitle>Serena</CardTitle>
              <CardDescription className="mt-2">
                {t("符号搜索、引用分析与语义编辑")}
              </CardDescription>
            </div>
          </div>
          <Badge variant="secondary">
            {running && <CheckCircle2 size={12} />} {labels[s.state]}
          </Badge>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="toolbar">
            {busy ? (
              <Button
                variant="outline"
                onClick={() =>
                  void runtime.cancel().catch((e) => toast.error(message(e)))
                }
              >
                <Loader2 className="animate-spin" />
                {t("取消任务")}
              </Button>
            ) : (
              <>
                <Button
                  disabled={!desktop}
                  onClick={() => void run(running ? "stop" : "start")}
                >
                  {running ? <Square /> : <Play />}
                  {running ? t("停止 Serena") : t("启动 Serena")}
                </Button>
                {running && (
                  <Button variant="outline" onClick={() => void run("restart")}>
                    <RefreshCw />
                    {t("重启")}
                  </Button>
                )}
              </>
            )}
            <Button variant="ghost" onClick={settings}>
              {t("配置运行环境")}
            </Button>
          </div>
          <div className="serena-metrics">
            <div>
              <div className="metric-label">{t("进程 PID")}</div>
              <strong>{s.pid ?? "—"}</strong>
            </div>
            <div>
              <div className="metric-label">{t("可用工具")}</div>
              <strong>{running ? s.tools.length : "—"}</strong>
            </div>
            <div>
              <div className="metric-label">{t("最近启动")}</div>
              <span>
                {s.startedAt
                  ? new Date(s.startedAt).toLocaleString(language)
                  : "—"}
              </span>
            </div>
          </div>
          {s.endpoint && (
            <div className="endpoint-line">
              <code>{s.endpoint}</code>
              <Button
                variant="ghost"
                size="icon"
                aria-label={t("复制服务地址")}
                onClick={() =>
                  void navigator.clipboard
                    .writeText(s.endpoint!)
                    .then(() => toast.success(t("已复制")))
                    .catch((e) => toast.error(message(e)))
                }
              >
                <Copy size={14} />
              </Button>
            </div>
          )}
          <p className="text-sm text-muted-foreground leading-6">
            {t(
              "Serena 使用独立的原生 HTTP 服务与单项目模式。首次启动可能需要准备语言服务器，可在运行日志中查看进度。",
            )}
          </p>
          {running && (
            <details>
              <summary className="cursor-pointer text-sm text-primary">
                {t("查看可用工具")}
              </summary>
              <div className="flex flex-wrap gap-2 mt-3">
                {s.tools.map((tool) => (
                  <Badge key={tool} variant="secondary" className="mono">
                    {tool}
                  </Badge>
                ))}
              </div>
            </details>
          )}
        </CardContent>
      </Card>
      <div className="panel-grid">
        <Card>
          <CardHeader>
            <CardTitle>{t("语义代码工具")}</CardTitle>
            <CardDescription>
              {t("按符号理解和修改代码，与 CodeGraph 的图谱检索互补。")}
            </CardDescription>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground leading-6">
            {t(
              "无需先初始化 CodeGraph 索引。Serena 按项目语言启动语言服务器，并维护自己的 .serena 项目配置。",
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>{t("客户端接入")}</CardTitle>
            <CardDescription>
              {t("为当前项目添加独立的 serena MCP 条目。")}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Button variant="outline" onClick={configure}>
              {t("配置客户端")}
              <ArrowRight />
            </Button>
          </CardContent>
        </Card>
      </div>
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
    <Field className="settings-engine" data-testid="serena-settings">
      <FieldLabel htmlFor="serena-entry">{t("Serena 入口")}</FieldLabel>
      <p className="text-xs text-muted-foreground">
        {t("可选引擎 · 符号搜索与语义编辑")}
      </p>
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
          onClick={() => void detect()}
        >
          {busy ? <Loader2 className="animate-spin" /> : null}
          {t("检测 Serena 并使用")}
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-destructive text-sm break-all">
          {error}
        </p>
      )}
      {environment?.available && (
        <p className="text-primary text-sm">{environment.version}</p>
      )}
      <p className="text-xs text-muted-foreground">
        {t("安装 uv 后执行以下命令，然后检测入口。更换入口后需重启 Serena。")}
      </p>
      <code className="install-command">
        uv tool install -p 3.13 serena-agent
      </code>
      <a
        href="https://oraios.github.io/serena/02-usage/010_installation.html"
        target="_blank"
        rel="noreferrer"
        className="text-primary text-sm underline"
      >
        {t("Serena 官方安装说明 ↗")}
      </a>
    </Field>
  );
}
