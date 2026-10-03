import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  ArrowRight,
  CheckCircle2,
  Circle,
  Copy,
  Folder,
  FolderOpen,
  GitBranch,
  Loader2,
  MoreHorizontal,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  Square,
  Terminal,
  TriangleAlert,
} from "lucide-react";
import { Toaster, toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import {
  Empty,
  EmptyHeader,
  EmptyTitle,
  EmptyDescription,
  EmptyContent,
  EmptyMedia,
} from "@/components/ui/empty";
import { call, desktop, message, subscribe } from "@/lib/ipc";
import type {
  Environment,
  Project,
  RuntimeSnapshot,
  Settings,
  TaskProgress,
} from "@/lib/types";
import { ProjectTools } from "./ProjectTools";
import { Elapsed } from "@/components/Elapsed";

const stateNames = {
  stopped: "已停止",
  starting: "启动中",
  running: "运行中",
  stopping: "停止中",
  error: "运行错误",
};
const indexNames = {
  unknown: "尚未检测",
  missing: "未初始化",
  ready: "索引可用",
  indexing: "索引处理中",
  error: "索引异常",
};
export function Status({ snapshot }: { snapshot?: RuntimeSnapshot }) {
  return (
    <Badge
      variant="secondary"
      className={snapshot?.state === "running" ? "text-primary bg-accent" : ""}
    >
      <span className="status">
        {snapshot?.state === "running" ? (
          <CheckCircle2 size={12} />
        ) : snapshot?.state === "error" ? (
          <TriangleAlert size={12} />
        ) : (
          <Circle size={10} />
        )}{" "}
        {snapshot ? stateNames[snapshot.state] : "尚未获取"}
      </span>
    </Badge>
  );
}
export function ErrorNotice({ error }: { error: string }) {
  return error ? (
    <Alert variant="destructive">
      <TriangleAlert />
      <AlertTitle>操作未完成</AlertTitle>
      <AlertDescription className="break-all">{error}</AlertDescription>
    </Alert>
  ) : null;
}
export default function App() {
  const [projects, setProjects] = useState<Project[]>([]),
    [snapshots, setSnapshots] = useState<Record<string, RuntimeSnapshot>>({}),
    [selected, setSelected] = useState(""),
    [environment, setEnvironment] = useState<Environment | null>(null),
    [settings, setSettings] = useState<Settings | null>(null),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(desktop),
    [busy, setBusy] = useState(false),
    [query, setQuery] = useState(""),
    [filter, setFilter] = useState("all"),
    [sort, setSort] = useState("name"),
    [tab, setTab] = useState("overview"),
    [modal, setModal] = useState<
      "add" | "edit" | "relocate" | "remove" | "settings" | null
    >(null),
    [form, setForm] = useState({
      path: "",
      name: "",
      notes: "",
      autoStart: false,
    }),
    [modalError, setModalError] = useState(""),
    [batch, setBatch] = useState<
      {
        projectId: string;
        action: "start" | "stop";
        name: string;
        ok: boolean;
        detail: string;
      }[]
    >([]),
    [confirm, setConfirm] = useState<{
      title: string;
      detail: string;
      action: () => Promise<void>;
    } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const scrollPositions = useRef(new Map<string, number>());
  const [duplicateProjectId, setDuplicateProjectId] = useState<string | null>(
    null,
  );
  function changeTab(next: string) {
    scrollPositions.current.set(
      `${selected}:${tab}`,
      mainRef.current?.scrollTop ?? 0,
    );
    setTab(next);
  }
  useLayoutEffect(() => {
    if (mainRef.current)
      mainRef.current.scrollTop =
        scrollPositions.current.get(`${selected}:${tab}`) ?? 0;
  }, [selected, tab]);
  const environmentLoaded = useRef(false);
  const refreshRequest = useRef(0);
  const project = projects.find((p) => p.id === selected),
    snapshot = snapshots[selected];
  const refresh = useCallback(async () => {
    if (!desktop) return;
    const request = ++refreshRequest.current;
    try {
      const [items, prefs] = await Promise.all([
        call<Project[]>("list_projects"),
        call<Settings>("get_settings"),
      ]);
      if (request !== refreshRequest.current) return;
      setProjects(items);
      setSettings(prefs);
      if (!environmentLoaded.current) {
        environmentLoaded.current = true;
        try {
          setEnvironment(await call<Environment>("detect_codegraph"));
        } catch (e) {
          environmentLoaded.current = false;
          throw e;
        }
      }
      setSelected((old) =>
        items.some((p) => p.id === old) ? old : (items[0]?.id ?? ""),
      );
      const states = await Promise.all(
        items.map((p) =>
          call<RuntimeSnapshot>("get_project_snapshot", { projectId: p.id }),
        ),
      );
      if (request !== refreshRequest.current) return;
      setSnapshots((old) =>
        Object.fromEntries(
          states.map((s) => {
            const previous = old[s.projectId];
            return [
              s.projectId,
              previous && previous.sequence > s.sequence ? previous : s,
            ];
          }),
        ),
      );
    } catch (e) {
      if (request === refreshRequest.current) setError(message(e));
    } finally {
      if (request === refreshRequest.current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribe<RuntimeSnapshot>("project-state-changed", (s) =>
      setSnapshots((old) => {
        const previous = old[s.projectId];
        if (previous && s.sequence < previous.sequence) return old;
        return { ...old, [s.projectId]: s };
      }),
    )
      .then(async (fn) => {
        if (disposed) fn();
        else {
          unlisten = fn;
          await refresh();
        }
      })
      .catch((e) => {
        if (!disposed) {
          setError(message(e));
          setLoading(false);
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);
  function showModal(type: typeof modal) {
    setModalError("");
    setDuplicateProjectId(null);
    setForm(
      type === "add"
        ? { path: "", name: "", notes: "", autoStart: false }
        : {
            path: project?.rootPath ?? "",
            name: project?.name ?? "",
            notes: project?.notes ?? "",
            autoStart: project?.autoStart ?? false,
          },
    );
    setModal(type);
  }
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchRef.current?.focus();
      }
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        showModal("add");
      }
      if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        showModal("settings");
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [project]);
  async function run(action: () => Promise<unknown>, success?: string) {
    setBusy(true);
    setError("");
    try {
      await action();
      if (success) toast.success(success, { duration: 3000 });
      await refresh();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  async function chooseDirectory() {
    try {
      const path = await open({
        directory: true,
        multiple: false,
        title: "选择项目目录",
      });
      if (typeof path === "string")
        setForm((f) => ({
          ...f,
          path,
          name: f.name || path.split(/[\\/]/).filter(Boolean).pop() || "",
        }));
    } catch (e) {
      setModalError(message(e));
    }
  }
  async function submitProject() {
    setBusy(true);
    setModalError("");
    setDuplicateProjectId(null);
    try {
      if (modal === "add") {
        const item = await call<Project>("add_project", form);
        setSelected(item.id);
      } else if (modal === "edit")
        await call("update_project", {
          projectId: selected,
          name: form.name,
          notes: form.notes,
          autoStart: form.autoStart,
        });
      else if (modal === "relocate")
        await call("relocate_project", {
          projectId: selected,
          selectedPath: form.path,
        });
      else if (modal === "remove")
        await call("remove_project", { projectId: selected });
      setModal(null);
      await refresh();
      toast.success("项目管理记录已更新");
    } catch (e) {
      setModalError(message(e));
      if (
        e &&
        typeof e === "object" &&
        "existingProjectId" in e &&
        typeof e.existingProjectId === "string"
      )
        setDuplicateProjectId(e.existingProjectId);
    } finally {
      setBusy(false);
    }
  }
  async function runtime(action: "start" | "stop" | "restart") {
    const perform = async () => {
      await run(
        () => call(`${action}_project`, { projectId: selected }),
        "操作已提交；以实际状态为准",
      );
    };
    if (action !== "start" && (snapshot?.sessions ?? 0) > 0)
      setConfirm({
        title: action === "stop" ? "停止项目实例" : "重启项目实例",
        detail: `当前 ${snapshot?.sessions} 个会话将断开，客户端需要重新连接。`,
        action: perform,
      });
    else await perform();
  }
  async function cancelStartup() {
    await run(async () => {
      const tasks = await call<TaskProgress[]>("get_project_tasks", {
        projectId: selected,
      });
      const task = tasks.find(
        (t) =>
          t.state === "running" && (t.kind === "start" || t.kind === "restart"),
      );
      if (!task) throw new Error("启动任务状态已变化，请刷新后重试。");
      await call("cancel_task", { operationId: task.operationId });
    }, "已请求取消启动");
  }
  async function indexing(kind: "init" | "sync" | "rebuild") {
    const perform = async () => {
      await run(
        () => call("run_index_task", { projectId: selected, kind }),
        "索引任务已提交",
      );
    };
    if (snapshot?.state === "running" || kind === "rebuild")
      setConfirm({
        title: kind === "rebuild" ? "重建项目索引" : "进入索引维护",
        detail:
          "操作会暂时停止当前实例；完成后恢复原先运行状态。重建不会删除项目源码。",
        action: perform,
      });
    else await perform();
  }
  async function batchRuntime(action: "start" | "stop") {
    setBatch([]);
    setBusy(true);
    const results = await Promise.all(
      projects.map(async (p) => {
        try {
          await call(`${action}_project`, { projectId: p.id });
          return {
            projectId: p.id,
            action,
            name: p.name,
            ok: true,
            detail: "操作已提交",
          };
        } catch (e) {
          return {
            projectId: p.id,
            action,
            name: p.name,
            ok: false,
            detail: message(e),
          };
        }
      }),
    );
    setBatch(results);
    setBusy(false);
    await refresh();
  }
  const displayed = projects
    .filter(
      (p) =>
        `${p.name} ${p.rootPath}`.toLowerCase().includes(query.toLowerCase()) &&
        (filter !== "running" || snapshots[p.id]?.state === "running"),
    )
    .sort((a, b) =>
      sort === "recent"
        ? b.updatedAt.localeCompare(a.updatedAt)
        : a.name.localeCompare(b.name, "zh-CN"),
    );
  return (
    <>
      <div className="workspace">
        <aside className="sidebar">
          <div className="brand">
            <div className="brand-mark">
              <GitBranch size={23} />
            </div>
            <div>
              <div className="font-semibold text-base">CodeGraph</div>
              <div className="text-xs text-muted-foreground mt-1">
                DESKTOP · 本地项目管理
              </div>
            </div>
          </div>
          <div className="px-4">
            <Button className="w-full" onClick={() => showModal("add")}>
              <Plus />
              添加项目
            </Button>
            <div className="relative mt-4">
              <Search
                className="absolute left-2.5 top-2.5 text-muted-foreground"
                size={16}
              />
              <Input
                ref={searchRef}
                aria-label="搜索项目"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="搜索项目…"
                className="pl-8 bg-white"
              />
            </div>
            <div className="flex items-center justify-between text-xs text-muted-foreground my-4">
              <span>
                全部 {projects.length} · 运行中{" "}
                {
                  Object.values(snapshots).filter((s) => s.state === "running")
                    .length
                }
              </span>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon" aria-label="批量项目操作">
                    <MoreHorizontal size={16} />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent>
                  <DropdownMenuGroup>
                    <DropdownMenuItem
                      disabled={busy || !projects.length}
                      onClick={() => void batchRuntime("start")}
                    >
                      启动全部项目
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      disabled={busy || !projects.length}
                      onClick={() =>
                        setConfirm({
                          title: "停止全部项目",
                          detail: "全部受管实例及其客户端连接将停止。",
                          action: () => batchRuntime("stop"),
                        })
                      }
                    >
                      停止全部项目
                    </DropdownMenuItem>
                  </DropdownMenuGroup>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            <div className="flex gap-2 mb-2">
              <select
                aria-label="项目筛选"
                className="text-xs bg-transparent w-1/2"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              >
                <option value="all">全部状态</option>
                <option value="running">仅运行中</option>
              </select>
              <select
                aria-label="项目排序"
                className="text-xs bg-transparent w-1/2"
                value={sort}
                onChange={(e) => setSort(e.target.value)}
              >
                <option value="name">按名称</option>
                <option value="recent">最近更新</option>
              </select>
            </div>
          </div>
          <nav className="project-list" aria-label="项目列表">
            {displayed.map((p) => (
              <button
                key={p.id}
                className={`project-row ${p.id === selected ? "selected" : ""}`}
                onClick={() => {
                  setSelected(p.id);
                  setError("");
                }}
              >
                <div className="flex gap-2 items-center font-medium mb-2">
                  <Folder size={16} />
                  <span className="truncate">{p.name}</span>
                </div>
                <Status snapshot={snapshots[p.id]} />
                <div
                  className="mono text-[11px] text-muted-foreground truncate mt-2"
                  title={p.rootPath}
                >
                  {p.rootPath}
                </div>
              </button>
            ))}
            {!loading && !displayed.length && (
              <p className="text-xs text-muted-foreground px-3 py-6">
                {query ? "没有匹配的项目" : "项目会显示在这里"}
              </p>
            )}
          </nav>
          <div className="p-4 border-t">
            <div className="text-xs text-muted-foreground mb-3 flex items-center gap-2">
              <span
                className={`h-1.5 w-1.5 rounded-full ${environment?.available ? "bg-primary" : "bg-amber-600"}`}
              />
              {environment?.available
                ? `CodeGraph ${environment.version ?? "已检测"}`
                : desktop
                  ? "CodeGraph 尚未就绪"
                  : "桌面环境未连接"}
            </div>
            <Button
              variant="ghost"
              className="w-full justify-start"
              onClick={() => showModal("settings")}
            >
              <Settings2 />
              设置
              <span className="ml-auto text-xs text-muted-foreground">
                Ctrl+,
              </span>
            </Button>
          </div>
        </aside>
        <main className="main" ref={mainRef}>
          {!desktop && (
            <div className="bg-amber-50 border-b border-amber-200 text-amber-900 px-7 py-3 text-xs flex items-center gap-2">
              <TriangleAlert size={15} />
              浏览器预览 · 桌面环境未连接。请启动桌面应用以管理真实项目。
            </div>
          )}
          {project ? (
            <>
              <header className="project-header">
                <div className="flex justify-between items-center gap-2 mb-4">
                  <div className="text-xs text-muted-foreground flex items-center gap-2">
                    项目工作台 <span>/</span> {project.name}
                  </div>
                  <div className="toolbar">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() =>
                        void run(() =>
                          call("open_project_directory", {
                            projectId: selected,
                          }),
                        )
                      }
                    >
                      <FolderOpen />
                      打开目录
                    </Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="项目菜单"
                        >
                          <MoreHorizontal />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent>
                        <DropdownMenuGroup>
                          <DropdownMenuItem onClick={() => showModal("edit")}>
                            编辑名称与备注
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => showModal("relocate")}
                          >
                            重新定位项目
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            className="text-destructive"
                            onClick={() => showModal("remove")}
                          >
                            移除管理记录
                          </DropdownMenuItem>
                        </DropdownMenuGroup>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </div>
                <div className="flex justify-between items-start gap-4 flex-wrap">
                  <div className="min-w-0">
                    <h1 className="text-2xl font-semibold tracking-tight">
                      {project.name}
                    </h1>
                    <div className="flex gap-2 items-center mt-2 text-muted-foreground">
                      <span
                        className="mono text-xs truncate max-w-[600px]"
                        title={project.rootPath}
                      >
                        {project.rootPath}
                      </span>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0"
                        aria-label="复制项目路径"
                        onClick={() =>
                          void navigator.clipboard
                            .writeText(project.rootPath)
                            .then(() => toast.success("路径已复制"))
                        }
                      >
                        <Copy size={12} />
                      </Button>
                    </div>
                  </div>
                  <div className="toolbar">
                    {snapshot?.state === "running" ? (
                      <>
                        <Button
                          variant="outline"
                          disabled={busy}
                          onClick={() => void runtime("restart")}
                        >
                          <RefreshCw />
                          重启
                        </Button>
                        <Button
                          disabled={busy}
                          onClick={() => void runtime("stop")}
                        >
                          <Square />
                          停止实例
                        </Button>
                      </>
                    ) : snapshot?.state === "starting" ? (
                      <Button
                        disabled={busy}
                        onClick={() => void cancelStartup()}
                      >
                        <Loader2 className="animate-spin" />
                        取消启动
                      </Button>
                    ) : (
                      <Button
                        disabled={
                          busy ||
                          !environment?.available ||
                          snapshot?.state === "stopping"
                        }
                        onClick={() =>
                          snapshot?.indexState === "missing"
                            ? void indexing("init")
                            : void runtime("start")
                        }
                      >
                        <Play />
                        {snapshot?.indexState === "missing"
                          ? "初始化索引"
                          : snapshot?.state === "stopping"
                            ? "停止中"
                            : "启动实例"}
                      </Button>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-3 mt-3 text-xs text-muted-foreground">
                  <Status snapshot={snapshot} />
                  <span>
                    {snapshot
                      ? indexNames[snapshot.indexState]
                      : "索引尚未获取"}
                  </span>
                  <span>{snapshot?.sessions ?? "—"} 个会话</span>
                  {project.autoStart && <span>应用启动时自动启动</span>}
                </div>
              </header>
              <div className="content space-y-4">
                <ErrorNotice error={error || snapshot?.error?.message || ""} />
                {environment &&
                  (!environment.available || environment.error) && (
                    <Alert>
                      <TriangleAlert />
                      <AlertTitle>
                        {environment.available
                          ? "CodeGraph 兼容性提示"
                          : "未找到可用的 CodeGraph"}
                      </AlertTitle>
                      <AlertDescription>
                        {environment.error ??
                          "请在设置中选择安装入口，再进行索引与实例操作。"}
                        <Button
                          variant="link"
                          onClick={() => showModal("settings")}
                        >
                          配置运行环境
                        </Button>
                      </AlertDescription>
                    </Alert>
                  )}
                <Tabs value={tab} onValueChange={changeTab}>
                  <TabsList className="mb-5">
                    <TabsTrigger value="overview">概览</TabsTrigger>
                    <TabsTrigger value="config">MCP 配置</TabsTrigger>
                    <TabsTrigger value="logs">运行日志</TabsTrigger>
                  </TabsList>
                  <TabsContent value="overview">
                    <div className="panel-grid">
                      <Card>
                        <CardHeader className="flex-row justify-between">
                          <div>
                            <CardTitle className="flex items-center gap-2">
                              <Activity size={17} />
                              实例
                            </CardTitle>
                            <CardDescription className="mt-2">
                              当前项目独立的进程与网关
                            </CardDescription>
                          </div>
                          <Button
                            variant="ghost"
                            size="icon"
                            aria-label="刷新状态"
                            disabled={busy}
                            onClick={() =>
                              void run(() =>
                                call("refresh_index_status", {
                                  projectId: selected,
                                }),
                              )
                            }
                          >
                            <RefreshCw size={16} />
                          </Button>
                        </CardHeader>
                        <CardContent>
                          <div className="metrics">
                            <Metric
                              name="运行状态"
                              value={
                                snapshot
                                  ? stateNames[snapshot.state]
                                  : "尚未获取"
                              }
                            />
                            <Metric
                              name="本机端口"
                              value={snapshot?.port ?? "尚未获取固定端口"}
                            />
                            <Metric
                              name="进程 PID"
                              value={snapshot?.pid ?? "—"}
                            />
                            <Metric
                              name="最近启动"
                              value={
                                snapshot?.startedAt
                                  ? new Date(snapshot.startedAt).toLocaleString(
                                      "zh-CN",
                                    )
                                  : "—"
                              }
                            />
                            <Metric
                              name="客户端会话"
                              value={snapshot?.sessions ?? "尚未获取"}
                            />
                            {snapshot?.startedAt &&
                              snapshot.state === "running" && (
                                <div>
                                  <div className="metric-label">运行时长</div>
                                  <Elapsed since={snapshot.startedAt} />
                                </div>
                              )}
                            <Metric
                              name="入口版本"
                              value={environment?.version ?? "尚未获取"}
                            />
                          </div>
                        </CardContent>
                      </Card>
                      <Card>
                        <CardHeader>
                          <CardTitle className="flex items-center gap-2">
                            <GitBranch size={17} />
                            代码索引
                          </CardTitle>
                          <CardDescription>
                            统计与可用性以 CodeGraph 的实际结果为准
                          </CardDescription>
                        </CardHeader>
                        <CardContent>
                          <div className="metrics">
                            <Metric
                              name="索引状态"
                              value={
                                snapshot
                                  ? indexNames[snapshot.indexState]
                                  : "尚未获取"
                              }
                            />
                            <Metric
                              name="已索引文件"
                              value={
                                snapshot?.indexStats?.fileCount ?? "尚未获取"
                              }
                            />
                            <Metric
                              name="图谱节点"
                              value={
                                snapshot?.indexStats?.nodeCount ?? "尚未获取"
                              }
                            />
                            <Metric
                              name="图谱关系"
                              value={
                                snapshot?.indexStats?.edgeCount ?? "尚未获取"
                              }
                            />
                            {snapshot?.indexStats?.checkedAt && (
                              <Metric
                                name="统计获取时间"
                                value={new Date(
                                  snapshot.indexStats.checkedAt,
                                ).toLocaleString("zh-CN")}
                              />
                            )}
                          </div>
                          <p className="text-xs text-muted-foreground my-6">
                            运行期间同步或重建会短暂断开客户端，完成后恢复实例。
                          </p>
                          <div className="toolbar">
                            <Button
                              variant="outline"
                              disabled={busy || !environment?.available}
                              onClick={() =>
                                void indexing(
                                  snapshot?.indexState === "missing"
                                    ? "init"
                                    : "sync",
                                )
                              }
                            >
                              <RefreshCw />
                              {snapshot?.indexState === "missing"
                                ? "初始化索引"
                                : "增量同步"}
                            </Button>
                            <Button
                              variant="ghost"
                              disabled={busy || !environment?.available}
                              onClick={() => void indexing("rebuild")}
                            >
                              重建索引
                            </Button>
                          </div>
                        </CardContent>
                      </Card>
                      <Card className="col-span-full">
                        <CardHeader>
                          <CardTitle>客户端接入</CardTitle>
                          <CardDescription>
                            Codex 与 Claude Code 共用当前项目的一个实例
                          </CardDescription>
                        </CardHeader>
                        <CardContent className="flex justify-between items-center gap-4 flex-wrap">
                          <p className="text-sm text-muted-foreground">
                            配置写入、独立连接测试与真实客户端会话分别验证。
                          </p>
                          <Button
                            variant="outline"
                            onClick={() => changeTab("config")}
                          >
                            配置客户端
                            <ArrowRight />
                          </Button>
                        </CardContent>
                      </Card>
                      {project.notes && (
                        <Card className="col-span-full">
                          <CardHeader>
                            <CardTitle>项目备注</CardTitle>
                          </CardHeader>
                          <CardContent className="whitespace-pre-wrap text-muted-foreground">
                            {project.notes}
                          </CardContent>
                        </Card>
                      )}
                    </div>
                  </TabsContent>
                  <ProjectTools
                    key={selected}
                    project={project}
                    snapshot={snapshot}
                    tab={tab}
                    refresh={refresh}
                  />
                </Tabs>
                {batch.length > 0 && (
                  <Card>
                    <CardHeader>
                      <CardTitle>批量操作结果</CardTitle>
                      <CardDescription>
                        {batch.filter((b) => b.ok).length} 项已提交 ·{" "}
                        {batch.filter((b) => !b.ok).length} 项失败
                      </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-2">
                      {batch.map((b) => (
                        <div
                          key={b.projectId}
                          className={`text-sm ${b.ok ? "" : "text-destructive"}`}
                        >
                          {b.name} · {b.detail}
                          {!b.ok && (
                            <Button
                              variant="link"
                              size="sm"
                              disabled={busy}
                              onClick={() =>
                                void run(async () => {
                                  await call(`${b.action}_project`, {
                                    projectId: b.projectId,
                                  });
                                  setBatch((old) =>
                                    old.map((item) =>
                                      item.projectId === b.projectId
                                        ? {
                                            ...item,
                                            ok: true,
                                            detail: "重试已提交",
                                          }
                                        : item,
                                    ),
                                  );
                                })
                              }
                            >
                              重试此项目
                            </Button>
                          )}
                        </div>
                      ))}
                    </CardContent>
                  </Card>
                )}
              </div>
            </>
          ) : (
            <div className="content">
              <ErrorNotice error={error} />
              {loading ? (
                <div className="empty-hero">
                  <Loader2 className="animate-spin text-primary" />
                  <p className="mt-4 text-muted-foreground">正在读取项目…</p>
                </div>
              ) : (
                <Empty className="empty-hero border-0">
                  <EmptyHeader>
                    <EmptyMedia
                      variant="icon"
                      className="bg-accent text-primary size-16 mb-4"
                    >
                      <GitBranch className="size-8" />
                    </EmptyMedia>
                    <Badge variant="outline" className="mb-3">
                      你的代码，井然有序
                    </Badge>
                    <EmptyTitle className="text-2xl">
                      添加一个项目，开始管理它的 CodeGraph
                    </EmptyTitle>
                    <EmptyDescription className="max-w-lg mt-3">
                      为每个项目维护独立索引与运行实例，让 Codex 和 Claude Code
                      连接到正确的代码图谱。
                    </EmptyDescription>
                  </EmptyHeader>
                  <EmptyContent>
                    <Button onClick={() => showModal("add")}>
                      <FolderOpen />
                      选择项目目录
                    </Button>
                    <span className="text-xs text-muted-foreground">
                      或按 Ctrl+N 添加项目
                    </span>
                  </EmptyContent>
                  <div className="steps">
                    {[
                      ["01", "选择本地项目", "项目与 Git worktree 分别管理"],
                      ["02", "建立代码索引", "使用本机已安装的 CodeGraph"],
                      ["03", "接入 AI 客户端", "同一项目，共用一个受管实例"],
                    ].map(([n, t, d]) => (
                      <div key={n}>
                        <span className="mono text-primary text-xs">{n}</span>
                        <h3 className="font-medium my-2">{t}</h3>
                        <p className="text-xs text-muted-foreground">{d}</p>
                      </div>
                    ))}
                  </div>
                </Empty>
              )}
            </div>
          )}
        </main>
      </div>
      <Dialog
        open={modal !== null}
        onOpenChange={(value) => {
          if (!value && !busy) setModal(null);
        }}
      >
        <DialogContent className="sm:max-w-[580px] max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>
              {modal === "add"
                ? "添加项目"
                : modal === "edit"
                  ? "编辑项目"
                  : modal === "relocate"
                    ? "重新定位项目"
                    : modal === "remove"
                      ? "移除管理记录"
                      : "设置"}
            </DialogTitle>
            <DialogDescription>
              {modal === "settings"
                ? "管理本机运行环境与应用偏好"
                : modal === "remove"
                  ? "运行中的实例将先停止。项目源码、索引与客户端配置会保留。"
                  : modal === "relocate"
                    ? "项目 ID 保持不变。先停止当前实例，再检测新目录；客户端配置需重新预览。"
                    : "每个项目拥有独立索引、网关和运行实例。"}
            </DialogDescription>
          </DialogHeader>
          <ErrorNotice error={modalError} />
          {duplicateProjectId && (
            <Button
              variant="outline"
              onClick={() => {
                setSelected(duplicateProjectId);
                setQuery("");
                setFilter("all");
                setModal(null);
              }}
            >
              定位已有项目
            </Button>
          )}
          {modal === "settings" ? (
            <SettingsForm
              settings={settings}
              environment={environment}
              busy={busy}
              save={async (action) => {
                setBusy(true);
                setModalError("");
                try {
                  await action();
                  environmentLoaded.current = false;
                  await refresh();
                  toast.success("设置已更新");
                } catch (e) {
                  setModalError(message(e));
                } finally {
                  setBusy(false);
                }
              }}
            />
          ) : modal !== "remove" ? (
            <form
              id="project-form"
              onSubmit={(e) => {
                e.preventDefault();
                void submitProject();
              }}
            >
              <FieldGroup>
                {modal !== "edit" && (
                  <Field>
                    <FieldLabel htmlFor="project-path">项目目录</FieldLabel>
                    <Input
                      id="project-path"
                      value={form.path}
                      onChange={(e) =>
                        setForm({ ...form, path: e.target.value })
                      }
                      required
                      placeholder="输入完整目录路径"
                    />
                    <Button
                      type="button"
                      variant="outline"
                      disabled={!desktop}
                      onClick={() => void chooseDirectory()}
                    >
                      <FolderOpen />
                      选择目录
                    </Button>
                  </Field>
                )}
                {modal !== "relocate" && (
                  <>
                    <Field>
                      <FieldLabel htmlFor="project-name">显示名称</FieldLabel>
                      <Input
                        id="project-name"
                        value={form.name}
                        onChange={(e) =>
                          setForm({ ...form, name: e.target.value })
                        }
                        required
                        maxLength={120}
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="project-notes">
                        备注（可选）
                      </FieldLabel>
                      <Textarea
                        id="project-notes"
                        value={form.notes}
                        onChange={(e) =>
                          setForm({ ...form, notes: e.target.value })
                        }
                      />
                    </Field>
                    <Field orientation="horizontal">
                      <input
                        id="project-auto"
                        type="checkbox"
                        checked={form.autoStart}
                        onChange={(e) =>
                          setForm({ ...form, autoStart: e.target.checked })
                        }
                      />
                      <FieldLabel htmlFor="project-auto">
                        应用启动时自动启动此项目
                      </FieldLabel>
                    </Field>
                  </>
                )}
              </FieldGroup>
            </form>
          ) : (
            <Alert>
              <Folder />
              <AlertTitle>{project?.name}</AlertTitle>
              <AlertDescription className="break-all">
                {project?.rootPath}
              </AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => setModal(null)}
            >
              {modal === "settings" ? "完成" : "取消"}
            </Button>
            {modal !== "settings" && (
              <Button
                variant={modal === "remove" ? "destructive" : "default"}
                type={modal === "remove" ? "button" : "submit"}
                form="project-form"
                disabled={busy || !desktop}
                onClick={
                  modal === "remove" ? () => void submitProject() : undefined
                }
              >
                {busy ? <Loader2 className="animate-spin" /> : null}
                {modal === "remove"
                  ? "移除管理记录"
                  : modal === "add"
                    ? "添加项目"
                    : "保存"}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={!!confirm}
        onOpenChange={(value) => {
          if (!value && !busy) setConfirm(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{confirm?.title}</DialogTitle>
            <DialogDescription>{confirm?.detail}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => setConfirm(null)}
            >
              取消
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                const action = confirm?.action;
                setConfirm(null);
                void action?.();
              }}
            >
              继续
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Toaster position="bottom-right" richColors />
    </>
  );
}
function Metric({ name, value }: { name: string; value: string | number }) {
  return (
    <div>
      <div className="metric-label">{name}</div>
      <div className="text-sm font-medium break-all">{value}</div>
    </div>
  );
}
function SettingsForm({
  settings,
  environment,
  busy,
  save,
}: {
  settings: Settings | null;
  environment: Environment | null;
  busy: boolean;
  save: (action: () => Promise<unknown>) => Promise<void>;
}) {
  const [entry, setEntry] = useState(settings?.codegraphEntry ?? ""),
    [concurrency, setConcurrency] = useState(settings?.indexConcurrency ?? 2),
    [behavior, setBehavior] = useState(settings?.closeBehavior ?? "tray");
  return (
    <FieldGroup>
      <Field>
        <FieldLabel htmlFor="entry">CodeGraph 入口</FieldLabel>
        <Input
          id="entry"
          value={entry}
          onChange={(e) => setEntry(e.target.value)}
          placeholder="自动检测或选择可执行入口"
        />
        <div className="toolbar">
          <Button
            variant="outline"
            disabled={busy || !desktop}
            onClick={() =>
              void open({ multiple: false, directory: false }).then((path) => {
                if (typeof path === "string") setEntry(path);
              })
            }
          >
            选择文件
          </Button>
          <Button
            variant="outline"
            disabled={busy || !desktop}
            onClick={() =>
              void save(() =>
                entry
                  ? call("set_codegraph_entry", { selectedPath: entry })
                  : call("detect_codegraph"),
              )
            }
          >
            检测并使用
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {environment?.available
            ? `已检测：${environment.version ?? "版本未提供"}`
            : (environment?.error ?? "尚未检测")}
          。更换入口后，运行实例需重启生效。
        </p>
        {environment?.available && environment.error && (
          <Alert>
            <TriangleAlert />
            <AlertTitle>兼容性提示</AlertTitle>
            <AlertDescription>{environment.error}</AlertDescription>
          </Alert>
        )}
      </Field>
      <Field>
        <FieldLabel htmlFor="concurrency">索引任务并发数</FieldLabel>
        <Input
          id="concurrency"
          type="number"
          min={1}
          max={4}
          value={concurrency}
          onChange={(e) => setConcurrency(Number(e.target.value))}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="close-behavior">关闭窗口行为</FieldLabel>
        <select
          className="border rounded-md p-2"
          id="close-behavior"
          value={behavior}
          onChange={(e) => setBehavior(e.target.value as "tray" | "exit")}
        >
          <option value="tray">隐藏到托盘</option>
          <option value="exit">退出并停止全部实例</option>
        </select>
      </Field>
      <Button
        disabled={busy || !desktop || concurrency < 1 || concurrency > 4}
        onClick={() =>
          void save(() =>
            call("save_settings", {
              indexConcurrency: concurrency,
              closeBehavior: behavior,
            }),
          )
        }
      >
        保存偏好
      </Button>
      <Field>
        <FieldLabel>应用数据目录</FieldLabel>
        <p className="mono text-xs break-all text-muted-foreground">
          {settings?.appDataDir ?? "桌面启动后可用"}
        </p>
        <Button
          variant="outline"
          disabled={!desktop}
          onClick={() => void save(() => call("open_app_data_directory"))}
        >
          打开数据目录
        </Button>
      </Field>
      <p className="text-xs text-muted-foreground">
        CodeGraph Desktop 0.1.4 · 开发构建
        <br />
        完全退出应用会停止受管实例。
      </p>
      <a
        className="text-primary text-sm underline"
        href="https://github.com/colbymchenry/codegraph"
        target="_blank"
        rel="noreferrer"
      >
        CodeGraph 官方项目与安装说明 ↗
      </a>
    </FieldGroup>
  );
}
