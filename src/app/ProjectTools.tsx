import { t, useLanguage } from "@/lib/i18n";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  CheckCircle2,
  Copy,
  Download,
  Loader2,
  Pause,
  Play,
  RefreshCw,
  Terminal,
  TriangleAlert,
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
import { TabsContent } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Field, FieldLabel } from "@/components/ui/field";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { LineDiff } from "@/components/LineDiff";
import { Elapsed } from "@/components/Elapsed";
import { call, message, subscribe } from "@/lib/ipc";
import type {
  Client,
  ConfigPreview,
  ConfigResult,
  ConfigStatus,
  LogEntry,
  Project,
  RuntimeSnapshot,
  TaskProgress,
} from "@/lib/types";

type PreviewSource =
  | { kind: "client"; action: "install" | "remove"; clients: Client[] }
  | { kind: "restore"; operationId: string }
  | { kind: "previous"; root: string };
type FileDraft = { mode: "merge" | "overwrite" | "edit"; content?: string };
function statusLabel(value: string) {
  const labels: Record<string, string> = {
    success: t("成功"),
    unchanged: t("未修改"),
    failed: t("失败"),
    rolledBack: t("已回滚"),
    rollbackFailed: t("回滚失败"),
    skipped: t("已跳过"),
    pending: t("待完成"),
    partial: t("部分完成"),
    completed: t("已完成"),
    interrupted: t("已中断"),
  };
  return labels[value] ?? value;
}

export function ProjectTools({
  project,
  snapshot,
  tab,
  refresh,
}: {
  project: Project;
  snapshot?: RuntimeSnapshot;
  tab: string;
  refresh: () => Promise<void>;
}) {
  const language = useLanguage();
  useEffect(() => {
    setError("");
    setProbe(null);
  }, [language]);
  const [clients, setClients] = useState<Client[]>(["codex", "claude"]),
    [statuses, setStatuses] = useState<ConfigStatus[]>([]),
    [preview, setPreview] = useState<ConfigPreview | null>(null),
    [result, setResult] = useState<ConfigResult | null>(null),
    [probe, setProbe] = useState<{
      success: boolean;
      tools: string[];
      message: string;
      checkedAt: string;
    } | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [logs, setLogs] = useState<LogEntry[]>([]),
    [tasks, setTasks] = useState<TaskProgress[]>([]),
    [level, setLevel] = useState("all"),
    [search, setSearch] = useState(""),
    [follow, setFollow] = useState(true);
  const scroller = useRef<HTMLDivElement>(null);
  const [previewSource, setPreviewSource] = useState<PreviewSource | null>(
    null,
  );
  const [drafts, setDrafts] = useState<Record<string, FileDraft>>({});
  const [previewDirty, setPreviewDirty] = useState(false);
  const [previewRecovery, setPreviewRecovery] = useState<{
    source: Extract<PreviewSource, { kind: "client" }>;
    reason: string;
  } | null>(null);
  const editablePreview =
    previewSource?.kind === "client" && previewSource.action === "install";
  const [previousRootPreview, setPreviousRootPreview] = useState<string | null>(
    null,
  );
  useEffect(() => {
    if (!preview) setPreviousRootPreview(null);
  }, [preview]);
  const logScrollPosition = useRef({ top: 0, left: 0 });
  useLayoutEffect(() => {
    if (tab === "logs" && scroller.current) {
      scroller.current.scrollTop = logScrollPosition.current.top;
      scroller.current.scrollLeft = logScrollPosition.current.left;
    }
  }, [tab]);
  const [backups, setBackups] = useState<
    { operationId: string; state: string; backupPath: string }[]
  >([]);
  const filtered = logs.filter(
    (l) =>
      (level === "all" || l.level.toLowerCase() === level) &&
      `${l.message} ${l.stage}`.toLowerCase().includes(search.toLowerCase()),
  );
  const virtualizer = useVirtualizer({
    count: filtered.length,
    enabled: tab === "logs",
    initialRect: { width: 800, height: 440 },
    getScrollElement: () => scroller.current,
    estimateSize: () => 30,
    overscan: 12,
  });
  async function loadStatus() {
    setStatuses(
      await call<ConfigStatus[]>("get_client_config_status", {
        projectId: project.id,
      }),
    );
    setBackups(await call("list_config_backups", { projectId: project.id }));
  }
  useEffect(() => {
    let active = true;
    const cleanups: (() => void)[] = [];
    const load = () =>
      Promise.all([
        call<LogEntry[]>("read_logs", { projectId: project.id, limit: 2000 }),
        call<ConfigStatus[]>("get_client_config_status", {
          projectId: project.id,
        }),
        call<TaskProgress[]>("get_project_tasks", { projectId: project.id }),
        call<{ operationId: string; state: string; backupPath: string }[]>(
          "list_config_backups",
          { projectId: project.id },
        ),
      ])
        .then(([entries, configs, currentTasks, savedBackups]) => {
          if (active) {
            setLogs((old) =>
              [
                ...entries,
                ...old.filter(
                  (l) =>
                    !entries.some(
                      (e) =>
                        e.sequence === l.sequence &&
                        e.generation === l.generation,
                    ),
                ),
              ].slice(-2000),
            );
            setStatuses(configs);
            setBackups(savedBackups);
            setTasks((old) => {
              const merged = new Map(
                old.map((task) => [task.operationId, task]),
              );
              currentTasks.forEach((task) => {
                if (
                  !merged.has(task.operationId) ||
                  merged.get(task.operationId)!.sequence <= task.sequence
                )
                  merged.set(task.operationId, task);
              });
              return [...merged.values()].slice(-20);
            });
          }
        })
        .catch((e) => {
          if (active) setError(message(e));
        });
    void Promise.all([
      subscribe<LogEntry>("project-log", (entry) => {
        if (entry.projectId === project.id)
          setLogs((old) => [...old, entry].slice(-2000));
      }).then((fn) => (active ? cleanups.push(fn) : fn())),
      subscribe<TaskProgress>("task-progress", (task) => {
        if (task.projectId !== project.id) return;
        setTasks((old) => {
          const existing = old.find(
            (task) => task.operationId === task.operationId,
          );
          if (existing && existing.sequence > task.sequence) return old;
          return [
            ...old.filter((task) => task.operationId !== task.operationId),
            task,
          ].slice(-20);
        });
        if (task.state !== "running") void refresh();
      }).then((fn) => (active ? cleanups.push(fn) : fn())),
    ])
      .then(() => {
        if (active) return load();
      })
      .catch((e) => {
        if (active) setError(message(e));
      });
    return () => {
      active = false;
      cleanups.forEach((fn) => fn());
    };
  }, [project.id, refresh, language]);
  useEffect(() => {
    if (follow && tab === "logs" && filtered.length)
      virtualizer.scrollToIndex(filtered.length - 1, { align: "end" });
  }, [filtered.length, follow, tab]);
  async function execute(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  async function configPreview(action: "install" | "remove") {
    await beginPreview({ kind: "client", action, clients: [...clients] });
  }
  async function requestPreview(
    source: PreviewSource,
    overrides: Record<string, FileDraft>,
  ) {
    if (source.kind === "restore")
      return call<ConfigPreview>("preview_restore_backup", {
        operationId: source.operationId,
      });
    if (source.kind === "previous")
      return call<ConfigPreview>("preview_previous_config", {
        projectId: project.id,
        previousRoot: source.root,
      });
    return call<ConfigPreview>("preview_client_config", {
      projectId: project.id,
      clients: source.clients,
      action: source.action,
      ...(source.action === "install"
        ? {
            overrides: source.clients.map((client) => {
              const draft = overrides[client] ?? { mode: "merge" as const };
              return {
                client,
                mode: draft.mode,
                ...(draft.mode === "edit"
                  ? { content: draft.content ?? "" }
                  : {}),
              };
            }),
          }
        : {}),
    });
  }
  async function beginPreview(source: PreviewSource) {
    setPreviewRecovery(null);
    await execute(async () => {
      let data: ConfigPreview;
      try {
        data = await requestPreview(source, {});
      } catch (e) {
        if (source.kind === "client" && source.action === "install")
          setPreviewRecovery({ source, reason: message(e) });
        throw e;
      }
      setPreviewSource(source);
      setDrafts({});
      setPreviewDirty(false);
      setPreviousRootPreview(source.kind === "previous" ? source.root : null);
      setPreview(data);
    });
  }
  async function recoverPreview() {
    if (!previewRecovery) return;
    const source = previewRecovery.source;
    const overrides = Object.fromEntries(
      source.clients.map((client) => [client, { mode: "overwrite" as const }]),
    );
    await execute(async () => {
      const data = await requestPreview(source, overrides);
      setPreviewSource(source);
      setDrafts(overrides);
      setPreviewDirty(false);
      setPreviousRootPreview(null);
      setPreview(data);
    });
  }
  async function refreshPreview() {
    if (!previewSource) return;
    setPreviewDirty(true);
    await execute(async () => {
      const data = await requestPreview(previewSource, drafts);
      setPreview(data);
      setPreviewDirty(false);
      toast.success(t("已根据磁盘最新文件重新预览，编辑草稿已保留"));
    });
  }
  function changeDraft(client: string, draft: FileDraft) {
    setDrafts((old) => ({ ...old, [client]: draft }));
    setPreviewDirty(true);
  }
  return (
    <>
      {tasks
        .filter((task) => task.state === "running")
        .map((task) => (
          <Alert key={task.operationId} className="mb-4">
            <Loader2 className="animate-spin" />
            <AlertTitle>{t("索引 / 实例任务进行中")}</AlertTitle>
            <AlertDescription>
              <div className="flex justify-between items-center gap-3">
                <span>
                  {task.message} ·{" "}
                  {new Date(task.timestamp).toLocaleTimeString(language)}
                  {task.startedAt && (
                    <>
                      {" "}
                      {t("· 已耗时")}
                      <Elapsed since={task.startedAt} />
                    </>
                  )}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  onClick={() =>
                    void execute(async () => {
                      await call("cancel_task", {
                        operationId: task.operationId,
                      });
                      toast(t("取消请求已发送"));
                    })
                  }
                >
                  {t("取消任务")}
                </Button>
              </div>
            </AlertDescription>
          </Alert>
        ))}
      {tasks
        .filter((task) => task.state === "failed")
        .slice(-1)
        .map((task) => (
          <Alert key={task.operationId} variant="destructive">
            <TriangleAlert />
            <AlertTitle>{t("任务失败")}</AlertTitle>
            <AlertDescription>
              {task.error?.message ?? task.message}
            </AlertDescription>
          </Alert>
        ))}
      {error && (
        <Alert variant="destructive" className="mb-4">
          <TriangleAlert />
          <AlertTitle>{t("操作未完成")}</AlertTitle>
          <AlertDescription className="break-all">{error}</AlertDescription>
        </Alert>
      )}
      {previewRecovery && !preview && (
        <Alert className="mb-4">
          <TriangleAlert />
          <AlertTitle>{t("合并预览未能生成")}</AlertTitle>
          <AlertDescription>
            <p className="break-all">{previewRecovery.reason}</p>
            <p>
              {t(
                "可以先生成覆盖预览，再切换为编辑内容进行修复。覆盖候选将删除所选文件中的其他设置，此操作仅预览，不会立即写入。",
              )}
            </p>
            <Button
              className="mt-2"
              variant="outline"
              disabled={busy}
              onClick={() => void recoverPreview()}
            >
              {t("以覆盖模式预览")}
            </Button>
          </AlertDescription>
        </Alert>
      )}
      <TabsContent value="overview">
        <Card className="mt-4">
          <CardHeader>
            <CardTitle>{t("最近活动")}</CardTitle>
            <CardDescription>{t("来自当前项目的真实运行日志")}</CardDescription>
          </CardHeader>
          <CardContent>
            {logs.length ? (
              logs
                .slice(-5)
                .reverse()
                .map((entry) => (
                  <div
                    key={`${entry.generation}-${entry.sequence}`}
                    className="text-xs flex gap-4 py-2"
                  >
                    <span className="text-muted-foreground shrink-0">
                      {new Date(entry.timestamp).toLocaleTimeString(language)}
                    </span>
                    <span className="break-all">{entry.message}</span>
                  </div>
                ))
            ) : (
              <p className="text-sm text-muted-foreground">
                {t("尚无活动记录")}
              </p>
            )}
          </CardContent>
        </Card>
      </TabsContent>
      <TabsContent value="config" className="space-y-4">
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <div>
            <h2 className="text-lg font-semibold">{t("MCP 配置")}</h2>
            <p className="text-sm text-muted-foreground mt-1">
              {t("客户端将共用")}
              {project.name}
              {t("的受管实例。")}
            </p>
          </div>
          <div className="toolbar">
            <Button
              variant="outline"
              disabled={busy || snapshot?.state !== "running"}
              title={
                snapshot?.state !== "running" ? t("需先启动实例") : undefined
              }
              onClick={() =>
                void execute(async () =>
                  setProbe(
                    await call("test_project_mcp", { projectId: project.id }),
                  ),
                )
              }
            >
              <ActivityIcon />
              {t("测试连接")}
            </Button>
            <Button
              disabled={busy || !clients.length}
              onClick={() => void configPreview("install")}
            >
              {t("预览并配置")}
            </Button>
          </div>
        </div>
        {(["codex", "claude"] as const).map((client) => {
          const status = statuses.find((s) => s.client === client);
          return (
            <Card key={client}>
              <CardHeader className="flex-row items-start justify-between gap-4">
                <div className="flex items-center gap-3">
                  <input
                    id={`client-${client}`}
                    type="checkbox"
                    checked={clients.includes(client)}
                    onChange={(e) =>
                      setClients((old) =>
                        e.target.checked
                          ? [...old, client]
                          : old.filter((c) => c !== client),
                      )
                    }
                  />
                  <div>
                    <CardTitle>
                      <label htmlFor={`client-${client}`}>
                        {client === "codex" ? "Codex" : "Claude Code"}
                      </label>
                    </CardTitle>
                    <CardDescription className="mt-2 mono">
                      {client === "codex" ? ".codex/config.toml" : ".mcp.json"}
                    </CardDescription>
                  </div>
                </div>
                <Badge variant="secondary">
                  {status
                    ? {
                        missing: t("未配置"),
                        configured: t("已配置"),
                        repair: t("需修复"),
                        parseError: t("解析错误"),
                      }[status.state]
                    : t("尚未获取")}
                </Badge>
              </CardHeader>
              <CardContent className="text-xs text-muted-foreground">
                {status?.message && (
                  <p className="text-destructive mb-2 break-all">
                    {status.message}
                  </p>
                )}
                {t("配置状态不代表实际连接。当前项目总会话：")}
                {snapshot?.sessions ?? t("尚未获取")}
                {t("；尚未获取按客户端分类的会话信息。")}
              </CardContent>
            </Card>
          );
        })}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">{t("连接路径")}</CardTitle>
            <CardDescription>
              {t("客户端 HTTP → 当前项目网关 → CodeGraph")}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {snapshot?.state === "running" && snapshot.port && (
              <p className="mono text-xs mb-3 break-all">
                {t("当前服务地址：http://127.0.0.1:")}
                {snapshot.port}/mcp
              </p>
            )}
            <p className="text-xs text-muted-foreground leading-6">
              {t(
                "请先在 CodeGraph Desktop 启动当前项目；实例停止或桌面应用退出后，HTTP 服务不可用。客户端可能要求信任项目或批准 MCP；写入后请重新加载客户端。独立测试通过不表示真实客户端已连接。",
              )}
            </p>
            <p className="text-xs text-muted-foreground leading-6 mt-2">
              {t(
                "项目使用固定本机端口，重启后地址保持不变；端口被占用时启动会报错，不会自动切换端口。配置中的地址与鉴权信息属于本机项目，请勿公开分享。",
              )}
            </p>
            <Alert className="mt-4">
              <RefreshCw />
              <AlertTitle>{t("迁移已有客户端配置")}</AlertTitle>
              <AlertDescription>
                {t(
                  "之前生成的配置需要重新点击“预览并配置”，核对后应用，才能迁移为直接 HTTP 连接。",
                )}
              </AlertDescription>
            </Alert>
            <div className="toolbar mt-4">
              <Button
                variant="ghost"
                size="sm"
                disabled={busy || !clients.length}
                onClick={() => void configPreview("remove")}
              >
                {t("预览移除受管配置")}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  void navigator.clipboard
                    .writeText(
                      t(
                        "在 CodeGraph Desktop 添加项目，初始化索引并启动实例；在 MCP 配置页预览并应用直接 HTTP 配置，然后在 Codex / Claude Code 信任项目并批准 MCP。已有配置需要重新预览应用以迁移。客户端 HTTP → 当前项目网关 → CodeGraph。项目使用固定本机端口，重启保持地址；端口冲突会报错，不会自动切换。实例停止或桌面应用退出后服务不可用。配置含本机项目鉴权信息，请勿公开分享。",
                      ),
                    )
                    .then(() => toast.success(t("接入说明已复制")))
                }
              >
                <Copy />
                {t("复制接入说明")}
              </Button>
            </div>
          </CardContent>
        </Card>
        {probe && (
          <Alert variant={probe.success ? "default" : "destructive"}>
            {probe.success ? <CheckCircle2 /> : <TriangleAlert />}
            <AlertTitle>
              {t("独立 MCP 测试")}
              {probe.success ? t("通过") : t("失败")}
            </AlertTitle>
            <AlertDescription>
              <p>{probe.message}</p>
              <p>
                {t("测试时间：")}
                {new Date(probe.checkedAt).toLocaleString(language)}
              </p>
              {probe.tools.length > 0 && (
                <p className="mono break-all">
                  {t("工具：")}
                  {probe.tools.join(language === "en" ? ", " : "、")}
                </p>
              )}
            </AlertDescription>
          </Alert>
        )}
        {result && (
          <Card>
            <CardHeader>
              <CardTitle>{t("配置操作结果")}</CardTitle>
              <CardDescription className="break-all">
                {t("备份：")}
                {result.backupPath}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {result.files.map((file) => (
                <div key={file.path} className="text-xs break-all">
                  <strong>{statusLabel(file.status)}</strong> · {file.path}
                  <p className="text-muted-foreground mt-1">{file.message}</p>
                </div>
              ))}
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void beginPreview({
                    kind: "restore",
                    operationId: result.operationId,
                  })
                }
              >
                {t("预览恢复此备份")}
              </Button>
            </CardContent>
          </Card>
        )}
        {(project.previousRoots ?? []).length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle>{t("旧目录配置清理")}</CardTitle>
              <CardDescription>
                {t(
                  "重新定位记录中的旧目录。仅预览并移除本应用受管条目，当前项目配置另行管理。",
                )}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {project.previousRoots.map((root) => (
                <div
                  key={root}
                  className="flex items-center justify-between gap-3"
                >
                  <code className="text-xs break-all">{root}</code>
                  <Button
                    variant="outline"
                    className="shrink-0"
                    disabled={busy}
                    onClick={() =>
                      void beginPreview({ kind: "previous", root })
                    }
                  >
                    {t("预览清理旧配置")}
                  </Button>
                </div>
              ))}
            </CardContent>
          </Card>
        )}
        {backups.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle>{t("配置备份")}</CardTitle>
              <CardDescription>
                {t("恢复前预览差异；不会覆盖备份之后的手动编辑。")}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {backups.map((backup) => (
                <div
                  key={backup.operationId}
                  className="flex gap-3 items-center justify-between"
                >
                  <div className="min-w-0 text-xs">
                    <p className="break-all mono">{backup.operationId}</p>
                    <p className="text-muted-foreground break-all mt-1">
                      {statusLabel(backup.state)} · {backup.backupPath}
                    </p>
                  </div>
                  <Button
                    className="shrink-0"
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() =>
                      void beginPreview({
                        kind: "restore",
                        operationId: backup.operationId,
                      })
                    }
                  >
                    {t("预览恢复")}
                  </Button>
                </div>
              ))}
            </CardContent>
          </Card>
        )}
      </TabsContent>
      <TabsContent value="logs">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Terminal size={17} />
              {t("运行日志")}
            </CardTitle>
            <CardDescription>
              {t(
                "仅当前项目 · 视图最多保留 2,000 行 · 清空视图不会删除磁盘日志",
              )}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="toolbar mb-4">
              <select
                aria-label={t("日志等级")}
                className="border rounded-md p-2"
                value={level}
                onChange={(e) => setLevel(e.target.value)}
              >
                <option value="all">{t("全部等级")}</option>
                <option value="info">{t("信息")}</option>
                <option value="warn">{t("警告")}</option>
                <option value="error">{t("错误")}</option>
                <option value="debug">{t("调试")}</option>
              </select>
              <Input
                aria-label={t("搜索日志")}
                placeholder={t("搜索日志…")}
                className="w-48"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
              <Button
                variant="outline"
                size="sm"
                onClick={() => setFollow(!follow)}
              >
                {follow ? <Pause /> : <Play />}
                {follow ? t("暂停跟随") : t("回到最新")}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  void navigator.clipboard
                    .writeText(
                      window.getSelection()?.toString() ||
                        filtered
                          .map(
                            (l) =>
                              `${l.timestamp} ${l.level} ${l.stage} ${l.message}`,
                          )
                          .join("\n"),
                    )
                    .then(() => toast.success(t("日志已复制")))
                }
              >
                <Copy />
                {t("复制")}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={busy}
                onClick={() =>
                  void execute(async () => {
                    const path = await call<string>("export_project_logs", {
                      projectId: project.id,
                    });
                    toast.success(t("日志已导出：{0}", { 0: path }));
                  })
                }
              >
                <Download />
                {t("导出")}
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setLogs([])}>
                {t("清空视图")}
              </Button>
            </div>
            <div
              ref={scroller}
              className="border rounded-md overflow-auto h-[440px] bg-white"
              onScroll={(e) => {
                const el = e.currentTarget;
                logScrollPosition.current = {
                  top: el.scrollTop,
                  left: el.scrollLeft,
                };
                if (el.scrollHeight - el.scrollTop - el.clientHeight > 60)
                  setFollow(false);
              }}
            >
              {filtered.length ? (
                <div
                  style={{
                    height: virtualizer.getTotalSize(),
                    position: "relative",
                    minWidth: "100%",
                  }}
                >
                  {virtualizer.getVirtualItems().map((row) => {
                    const item = filtered[row.index];
                    return (
                      <div
                        key={`${item.generation}-${item.sequence}-${row.index}`}
                        className="log-row"
                        style={{
                          position: "absolute",
                          top: 0,
                          left: 0,
                          width: "100%",
                          transform: `translateY(${row.start}px)`,
                        }}
                      >
                        <span className="text-muted-foreground">
                          {new Date(item.timestamp).toLocaleTimeString(
                            language,
                          )}
                        </span>
                        <span
                          className={`w-12 ${item.level === "error" ? "text-destructive" : "text-primary"}`}
                        >
                          {item.level.toUpperCase()}
                        </span>
                        <span className="text-muted-foreground">
                          {item.stage}
                        </span>
                        <span>{item.message}</span>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="h-full flex items-center justify-center text-muted-foreground text-sm">
                  {search
                    ? t("没有匹配的日志")
                    : t(
                        "尚无日志。启动实例或运行索引后，真实日志将显示在这里。",
                      )}
                </div>
              )}
            </div>
          </CardContent>
        </Card>
      </TabsContent>
      <Dialog
        open={!!preview}
        onOpenChange={(value) => {
          if (!value && !busy) setPreview(null);
        }}
      >
        <DialogContent className="sm:max-w-[920px] max-h-[85vh] overflow-auto">
          <DialogHeader>
            <DialogTitle>
              {previousRootPreview
                ? t("旧目录配置清理预览")
                : t("配置差异预览")}{" "}
              · {project.name}
            </DialogTitle>
            <DialogDescription>
              {t(
                "默认合并会保留其他 MCP 与设置；覆盖模式将替换整个文件。所有写入均先备份，并检查文件是否被外部修改。",
              )}
            </DialogDescription>
          </DialogHeader>
          {previousRootPreview && (
            <Alert>
              <TriangleAlert />
              <AlertTitle>{t("将修改旧目录中的受管配置")}</AlertTitle>
              <AlertDescription className="break-all">
                <code>{previousRootPreview}</code>
                <p>
                  {t(
                    "清理旧目录中的 Codex / Claude Code 条目。请核对下方每个文件路径和删除内容。",
                  )}
                </p>
              </AlertDescription>
            </Alert>
          )}
          {previewRecovery && (
            <Alert>
              <TriangleAlert />
              <AlertTitle>{t("由合并失败进入修复预览")}</AlertTitle>
              <AlertDescription className="break-all">
                {previewRecovery.reason}
              </AlertDescription>
            </Alert>
          )}
          <div className="text-xs mono break-all">
            {t("服务名：")}
            {preview?.serviceName}
          </div>
          <p className="text-xs text-muted-foreground">
            {t(
              "也可以在外部编辑器修改文件，然后点击“重新读取 / 刷新预览”。刷新以磁盘最新内容作对比，保留此处的编辑草稿。",
            )}
          </p>
          {error && (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          {preview?.files.map((file) => (
            <section key={file.path}>
              <div className="flex gap-2 items-center text-xs mb-2 flex-wrap">
                <strong className="break-all">{file.path}</strong>
                <Badge variant="outline">
                  {!file.existed
                    ? t("新建文件")
                    : file.conflict
                      ? t("替换已有项")
                      : t("更新受管配置")}
                </Badge>
              </div>
              {editablePreview && (
                <div className="space-y-3 mb-3">
                  <Field>
                    <FieldLabel htmlFor={`config-mode-${file.client}`}>
                      {t("写入方式")}
                    </FieldLabel>
                    <select
                      id={`config-mode-${file.client}`}
                      disabled={busy}
                      className="border rounded-md p-2 bg-white"
                      value={drafts[file.client]?.mode ?? "merge"}
                      onChange={(e) => {
                        const mode = e.target.value as FileDraft["mode"];
                        changeDraft(file.client, {
                          mode,
                          content:
                            drafts[file.client]?.content ??
                            (mode === "edit" ? file.after : undefined),
                        });
                      }}
                    >
                      <option value="merge">{t("合并配置（默认）")}</option>
                      <option value="overwrite">{t("覆盖整个文件")}</option>
                      <option value="edit">{t("编辑内容")}</option>
                    </select>
                  </Field>
                  {drafts[file.client]?.mode === "overwrite" && (
                    <Alert variant="destructive">
                      <TriangleAlert />
                      <AlertTitle>{t("覆盖整个文件")}</AlertTitle>
                      <AlertDescription>
                        {t(
                          "将仅保留本项目的 CodeGraph 配置；此文件中的其他 MCP、模型设置、注释等都会被移除。请重新预览并逐行核对删除内容。",
                        )}
                      </AlertDescription>
                    </Alert>
                  )}
                  {drafts[file.client]?.mode === "edit" && (
                    <Field>
                      <FieldLabel htmlFor={`config-content-${file.client}`}>
                        {t("完整文件内容（{0}）", {
                          0: file.client === "codex" ? "TOML" : "JSON",
                        })}
                      </FieldLabel>
                      <Textarea
                        id={`config-content-${file.client}`}
                        className="mono min-h-48 max-h-72 text-xs overflow-auto whitespace-pre"
                        wrap="off"
                        disabled={busy}
                        value={drafts[file.client]?.content ?? file.after}
                        onChange={(e) =>
                          changeDraft(file.client, {
                            mode: "edit",
                            content: e.target.value,
                          })
                        }
                      />
                      <p className="text-xs text-muted-foreground">
                        {t(
                          "编辑的是完整文件。重新预览时校验语法；失败会保留草稿。更改或删除 codegraph 的 HTTP URL 或鉴权字段后，可能不再连接当前项目，对应条目也不再登记为本项目受管配置。",
                        )}
                      </p>
                    </Field>
                  )}
                </div>
              )}
              {previewDirty && (
                <p className="text-xs text-amber-800 mb-2">
                  {t("以下仍为上一次通过校验的差异，请刷新预览后再应用。")}
                </p>
              )}
              <LineDiff before={file.before} after={file.after} />
            </section>
          ))}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => void refreshPreview()}
            >
              <RefreshCw />
              {t("重新读取 / 刷新预览")}
            </Button>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => setPreview(null)}
            >
              {t("取消")}
            </Button>
            <Button
              disabled={busy || previewDirty}
              onClick={() =>
                void execute(async () => {
                  if (!preview || previewDirty) return;
                  setPreviewDirty(true);
                  const applied = await call<ConfigResult>(
                    "apply_client_config",
                    {
                      previewId: preview.previewId,
                    },
                  );
                  setResult(applied);
                  setPreview(null);
                  setPreviewRecovery(null);
                  await loadStatus();
                  if (
                    applied.files.every((file) =>
                      ["success", "unchanged"].includes(file.status),
                    )
                  )
                    toast.success(t("配置操作已完成"));
                  else
                    setError(
                      t("部分配置未能完成，请查看逐文件结果和备份位置。"),
                    );
                })
              }
            >
              {busy && <Loader2 className="animate-spin" />}
              {t("应用配置")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
function ActivityIcon() {
  return <RefreshCw size={16} />;
}
