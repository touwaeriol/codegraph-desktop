import { useCallback, useEffect, useState } from "react";
import { Download, FolderOpen, Loader2, X } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { call, desktop, message, subscribe } from "@/lib/ipc";
import { t } from "@/lib/i18n";
import type { MessageKey } from "@/lib/messages";
import type {
  ReaEnvironment,
  ReaEnvironmentSaved,
  ReaToolProgress,
  ReaTools as ReaToolList,
} from "@/lib/types";

const TOOLS: { id: string; name: string; description: MessageKey }[] = [
  {
    id: "ghidra",
    name: "Ghidra 12.1.4",
    description: "原生二进制分析，约 570 MB，自动附带 JDK 21",
  },
  {
    id: "jadx",
    name: "jadx-headless-mcp 0.7.1",
    description: "Android APK 分析，约 41 MB，自动附带 JDK 21",
  },
  {
    id: "jdk",
    name: "Temurin JDK 21",
    description: "Ghidra 与 JADX 使用的 Java 运行时，约 200 MB",
  },
  {
    id: "ilspy",
    name: "ilspycmd",
    description: ".NET 反编译命令行，需已安装 .NET SDK",
  },
];
const ITEMS: Record<string, string> = {
  jdk: "JDK 21",
  ghidra: "Ghidra",
  jadx: "JADX",
  ilspy: "ilspycmd",
};
const megabytes = (bytes: number) => (bytes / 1024 / 1024).toFixed(0);

/**
 * Installs the open-source tools REA uses next to the app (or into the app
 * data directory) and points REA's environment at them.
 */
export function ReaTools({
  onInstalled,
  version,
}: {
  onInstalled: (environment: ReaEnvironment) => void;
  /** Changes when REA's environment was updated elsewhere. */
  version: number;
}) {
  const [list, setList] = useState<ReaToolList | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [progress, setProgress] = useState<ReaToolProgress | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(() => {
    call<ReaToolList>("get_rea_tools")
      .then(setList)
      .catch((e) => setError(message(e)));
  }, []);
  useEffect(() => {
    if (desktop) load();
  }, [load, version]);
  useEffect(() => {
    if (!desktop) return;
    let live = true;
    let dispose: (() => void) | undefined;
    void subscribe<ReaToolProgress>("rea-tool-install", (next) => {
      if (live) setProgress(next);
    }).then((unlisten) => {
      if (live) dispose = unlisten;
      else unlisten();
    });
    return () => {
      live = false;
      dispose?.();
    };
  }, []);
  async function install(tool: string) {
    setActive(tool);
    setProgress(null);
    setError("");
    try {
      const result = await call<ReaEnvironmentSaved>("install_rea_tool", {
        tool,
      });
      onInstalled(result.environment);
      toast.success(
        result.updated.length
          ? t(
              "{0} 已安装并写入 REA 环境，已更新 {1} 个项目配置；请在客户端重新加载 MCP",
              {
                0: ITEMS[tool],
                1: result.updated.length,
              },
            )
          : t("{0} 已安装并写入 REA 环境", { 0: ITEMS[tool] }),
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
      // Dependencies that finished, such as the JDK, are already saved.
      void call<ReaEnvironment>("get_rea_environment")
        .then(onInstalled)
        .catch(() => {});
    } finally {
      setActive(null);
      setProgress(null);
      load();
    }
  }
  const busy = active !== null || list?.installing === true;
  const percent =
    progress?.total && progress.phase === "download"
      ? Math.round((progress.downloaded / progress.total) * 100)
      : null;
  return (
    <div className="rea-tools" data-testid="rea-tools">
      <h3>{t("一键安装分析工具")}</h3>
      <p className="text-xs text-muted-foreground leading-5">
        {t(
          "从官方发布地址下载并校验 SHA-256，安装到软件所在目录（该目录不可写或非 Windows 时使用应用数据目录），并自动写入 REA 的 MCP 配置；客户端重新加载 MCP 后，REA 即可直接使用。IDA 与 Hopper 为商业软件，请自行安装后填写路径。",
        )}
      </p>
      {list && (
        <div className="endpoint-line" data-testid="rea-tools-directory">
          <span className="min-w-0 text-xs">
            <span className="text-muted-foreground">{t("安装位置")} </span>
            <code>{list.directory}</code>
          </span>
          <Button
            variant="ghost"
            size="sm"
            onClick={() =>
              void call("open_rea_tools_directory").catch((e) =>
                setError(message(e)),
              )
            }
          >
            <FolderOpen />
            {t("打开目录")}
          </Button>
        </div>
      )}
      <ul>
        {TOOLS.map((tool) => {
          const state = list?.tools.find((item) => item.id === tool.id);
          return (
            <li key={tool.id} data-testid={`rea-tool-${tool.id}`}>
              <div className="min-w-0">
                <strong>{tool.name}</strong>
                <small>{t(tool.description)}</small>
              </div>
              {state?.inUse ? (
                <Badge variant="secondary">{t("使用中")}</Badge>
              ) : state?.installed ? (
                <Badge variant="outline">{t("已安装")}</Badge>
              ) : null}
              <Button
                variant="outline"
                size="sm"
                disabled={!desktop || busy || state?.inUse}
                onClick={() => void install(tool.id)}
              >
                {active === tool.id ? (
                  <Loader2 className="animate-spin" />
                ) : (
                  <Download />
                )}
                {state?.installed ? t("使用") : t("安装")}
              </Button>
            </li>
          );
        })}
      </ul>
      {active && (
        <div className="space-y-2" role="status">
          <progress
            className="w-full h-2 accent-primary"
            aria-label={t("安装进度")}
            value={percent ?? undefined}
            max={100}
          />
          <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
            <span>
              {!progress
                ? t("正在准备…")
                : progress.phase === "download"
                  ? t("正在下载 {0}：{1} MB{2}", {
                      0: ITEMS[progress.item] ?? progress.item,
                      1: megabytes(progress.downloaded),
                      2: progress.total
                        ? ` / ${megabytes(progress.total)} MB · ${percent}%`
                        : "",
                    })
                  : progress.phase === "verify"
                    ? t("正在校验 {0}…", {
                        0: ITEMS[progress.item] ?? progress.item,
                      })
                    : progress.phase === "extract"
                      ? t("正在解压 {0}…", {
                          0: ITEMS[progress.item] ?? progress.item,
                        })
                      : t("正在写入 REA 环境…")}
            </span>
            <Button
              variant="ghost"
              size="sm"
              onClick={() =>
                void call("cancel_rea_tool_install").catch((e) =>
                  setError(message(e)),
                )
              }
            >
              <X />
              {t("取消")}
            </Button>
          </div>
        </div>
      )}
      {error && (
        <p role="alert" className="text-destructive text-sm break-all">
          {error}
        </p>
      )}
    </div>
  );
}
