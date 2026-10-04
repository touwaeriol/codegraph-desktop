import type { ReactNode } from "react";
import {
  Activity,
  ArrowRight,
  CheckCircle2,
  Circle,
  Copy,
  Code2,
  GitBranch,
  Loader2,
  Play,
  RefreshCw,
  Square,
  TriangleAlert,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Elapsed } from "@/components/Elapsed";
import { t, useLanguage } from "@/lib/i18n";
import type { RuntimeSnapshot } from "@/lib/types";
import { toast } from "sonner";
import { message } from "@/lib/ipc";

type State = RuntimeSnapshot["state"];
export function EngineStatus({ snapshot }: { snapshot?: { state: State } }) {
  const labels = {
    running: t("运行中"),
    stopped: t("已停止"),
    starting: t("启动中"),
    stopping: t("停止中"),
    error: t("运行错误"),
  };
  const state = snapshot?.state;
  return (
    <Badge
      variant="secondary"
      className={
        state === "running"
          ? "text-primary bg-accent"
          : state === "error"
            ? "text-destructive"
            : ""
      }
    >
      <span className="status">
        {state === "running" ? (
          <CheckCircle2 size={12} />
        ) : state === "error" ? (
          <TriangleAlert size={12} />
        ) : state === "starting" || state === "stopping" ? (
          <Loader2 size={12} className="animate-spin" />
        ) : (
          <Circle size={10} />
        )}{" "}
        {state ? labels[state] : t("尚未获取")}
      </span>
    </Badge>
  );
}
export function EngineActions({
  state,
  busy,
  disabled,
  startLabel,
  onStart,
  onStop,
  onRestart,
  onCancel,
}: {
  state?: State;
  busy: boolean;
  disabled?: boolean;
  startLabel?: string;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="toolbar" data-testid="engine-actions">
      {state === "starting" ? (
        <Button disabled={busy} onClick={onCancel}>
          <Loader2 className="animate-spin" />
          {t("取消启动")}
        </Button>
      ) : state === "running" ? (
        <>
          <Button variant="outline" disabled={busy} onClick={onRestart}>
            <RefreshCw />
            {t("重启")}
          </Button>
          <Button disabled={busy} onClick={onStop}>
            <Square />
            {t("停止实例")}
          </Button>
        </>
      ) : (
        <Button
          disabled={busy || disabled || state === "stopping"}
          onClick={onStart}
        >
          {busy || state === "stopping" ? (
            <Loader2 className="animate-spin" />
          ) : (
            <Play />
          )}
          {state === "stopping" ? t("停止中") : (startLabel ?? t("启动实例"))}
        </Button>
      )}
    </div>
  );
}
export function Metric({ name, value }: { name: string; value: ReactNode }) {
  return (
    <div>
      <div className="metric-label">{name}</div>
      <div className="text-sm font-medium break-words">{value}</div>
    </div>
  );
}
export function EngineRuntimeCard({
  name,
  state,
  pid,
  port,
  startedAt,
  detail,
  onRefresh,
  busy,
}: {
  name: string;
  state?: State;
  pid?: number | null;
  port?: string | number | null;
  startedAt?: string | null;
  detail: { name: string; value: ReactNode };
  onRefresh?: () => void;
  busy?: boolean;
}) {
  const language = useLanguage();
  return (
    <Card className="engine-runtime-card" data-testid="engine-runtime-card">
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div>
          <CardTitle className="flex items-center gap-2">
            <Activity size={17} />
            {t("运行实例")}
          </CardTitle>
          <CardDescription className="mt-2">
            {name} · {t("当前项目")}
          </CardDescription>
        </div>
        {onRefresh && (
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("刷新状态")}
            disabled={busy}
            onClick={onRefresh}
          >
            <RefreshCw size={16} />
          </Button>
        )}
      </CardHeader>
      <CardContent>
        <div className="metrics">
          <Metric
            name={t("运行状态")}
            value={<EngineStatus snapshot={state ? { state } : undefined} />}
          />
          <Metric name={t("本机端口")} value={port ?? "—"} />
          <Metric name={t("进程 PID")} value={pid ?? "—"} />
          <Metric
            name={t("最近启动")}
            value={
              startedAt ? new Date(startedAt).toLocaleString(language) : "—"
            }
          />
          <Metric
            name={t("运行时长")}
            value={
              startedAt && state === "running" ? (
                <Elapsed since={startedAt} />
              ) : (
                "—"
              )
            }
          />
          <Metric name={detail.name} value={detail.value} />
        </div>
      </CardContent>
    </Card>
  );
}
export function EngineConnectionCard({
  name,
  configure,
  endpoint,
}: {
  name: string;
  configure: () => void;
  endpoint?: string | null;
}) {
  return (
    <Card
      className="col-span-full engine-connection-card"
      data-testid="engine-connection-card"
    >
      <CardHeader>
        <CardTitle>{t("客户端接入")}</CardTitle>
        <CardDescription>
          {t("为当前项目配置 {0} 的 MCP 连接。", { 0: name })}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex items-center justify-between gap-4 flex-wrap">
        <div className="min-w-0">
          <p className="text-sm text-muted-foreground">Codex · Claude Code</p>
          {endpoint && (
            <div className="flex items-center gap-2 mt-2">
              <code className="text-xs break-all">{endpoint}</code>
              <Button
                variant="ghost"
                size="icon"
                aria-label={t("复制服务地址")}
                onClick={() =>
                  void navigator.clipboard
                    .writeText(endpoint)
                    .then(() => toast.success(t("已复制")))
                    .catch((e) => toast.error(message(e)))
                }
              >
                <Copy size={14} />
              </Button>
            </div>
          )}
        </div>
        <Button variant="outline" onClick={configure}>
          {t("配置客户端")}
          <ArrowRight />
        </Button>
      </CardContent>
    </Card>
  );
}

export function EngineSettingsCard({
  name,
  description,
  children,
  testId,
}: {
  name: string;
  description: string;
  children: ReactNode;
  testId: string;
}) {
  return (
    <Card className="engine-settings-card" data-testid={testId}>
      <CardHeader>
        <div className="flex items-center gap-3">
          <span className="engine-symbol">
            {name === "Serena" ? <Code2 size={20} /> : <GitBranch size={20} />}
          </span>
          <div className="min-w-0">
            <CardTitle>{name}</CardTitle>
            <CardDescription className="mt-1">{description}</CardDescription>
          </div>
        </div>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}
