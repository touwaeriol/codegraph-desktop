import { useEffect, useRef, useState } from "react";
import { ArrowUpCircle, Download, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ExternalLink } from "@/components/ExternalLink";
import { call, desktop, message, subscribe } from "@/lib/ipc";
import { t, useLanguage } from "@/lib/i18n";
import packageInfo from "../../package.json";

interface UpdateSnapshot {
  sequence: number;
  status:
    | "idle"
    | "checking"
    | "available"
    | "upToDate"
    | "downloading"
    | "preparing"
    | "installing"
    | "error";
  currentVersion: string;
  latestVersion: string | null;
  notes: string | null;
  checkedAt: string | null;
  downloaded: number;
  total: number | null;
  autoCheck: boolean;
  canInstall: boolean;
  error: { code: string; message: string } | null;
}
const busyStatuses = ["checking", "downloading", "preparing", "installing"];
export function useUpdates() {
  const [snapshot, setSnapshot] = useState<UpdateSnapshot>({
    sequence: 0,
    status: "idle",
    currentVersion: packageInfo.version,
    latestVersion: null,
    notes: null,
    checkedAt: null,
    downloaded: 0,
    total: null,
    autoCheck: true,
    canInstall: false,
    error: null,
  });
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const current = useRef(0);
  function accept(next: UpdateSnapshot) {
    if (next.sequence < current.current) return;
    current.current = next.sequence;
    setSnapshot(next);
  }
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    let dispose: (() => void) | undefined;
    void subscribe<UpdateSnapshot>("app-update", (next) => {
      if (active) accept(next);
    })
      .then(async (unlisten) => {
        if (!active) {
          unlisten();
          return;
        }
        dispose = unlisten;
        const next = await call<UpdateSnapshot>("get_update_snapshot");
        if (active) accept(next);
      })
      .catch((e) => {
        if (active) setError(message(e));
      });
    return () => {
      active = false;
      dispose?.();
    };
  }, []);
  async function perform(command: string, args?: Record<string, unknown>) {
    setWorking(true);
    setError("");
    try {
      accept(await call<UpdateSnapshot>(command, args));
    } catch (e) {
      setError(message(e));
    } finally {
      setWorking(false);
    }
  }
  return {
    snapshot,
    error,
    busy: working || busyStatuses.includes(snapshot.status),
    check: () => perform("check_for_updates"),
    install: () => perform("install_update"),
    setAuto: (enabled: boolean) =>
      perform("set_auto_update_check", { enabled }),
    cancel: async () => {
      try {
        await call("cancel_update_download");
      } catch (e) {
        setError(message(e));
      }
    },
  };
}
type Updates = ReturnType<typeof useUpdates>;
function statusText(status: UpdateSnapshot["status"]) {
  return {
    idle: t("尚未检查更新"),
    checking: t("正在检查更新…"),
    available: t("发现新版本"),
    upToDate: t("已是最新版本"),
    downloading: t("正在下载并校验更新…"),
    preparing: t("正在停止受管实例…"),
    installing: t("正在安装，完成后将自动重启…"),
    error: t("更新未完成"),
  }[status];
}

export function UpdateNotice({
  updates,
  open,
}: {
  updates: Updates;
  open: () => void;
}) {
  const s = updates.snapshot;
  return (
    <>
      {s.latestVersion && ["available", "downloading"].includes(s.status) && (
        <Alert className="m-5 w-auto" data-testid="update-notice">
          <ArrowUpCircle />
          <AlertTitle>
            {t("CodeGraph Desktop {0} 可用", { 0: s.latestVersion })}
          </AlertTitle>
          <AlertDescription className="flex justify-between items-center gap-3">
            <span>{statusText(s.status)}</span>
            <Button variant="outline" size="sm" onClick={open}>
              {t("查看更新")}
            </Button>
          </AlertDescription>
        </Alert>
      )}
      <Dialog open={["preparing", "installing"].includes(s.status)}>
        <DialogContent
          className="[&>button]:hidden"
          onEscapeKeyDown={(e) => e.preventDefault()}
          onInteractOutside={(e) => e.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle>{t("正在更新应用")}</DialogTitle>
            <DialogDescription>{statusText(s.status)}</DialogDescription>
          </DialogHeader>
          <Loader2 className="animate-spin mx-auto" />
        </DialogContent>
      </Dialog>
    </>
  );
}

export function UpdateSettings({ updates }: { updates: Updates }) {
  const language = useLanguage();
  const [confirm, setConfirm] = useState(false);
  const s = updates.snapshot;
  const progress = s.total
    ? Math.min(100, Math.round((s.downloaded / s.total) * 100))
    : null;
  const error = updates.error || s.error?.message;
  return (
    <Card data-testid="update-settings">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ArrowUpCircle size={18} />
          {t("软件更新")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div>
            <p className="text-sm">
              {t("当前版本：{0}", { 0: s.currentVersion })}
            </p>
            <p className="text-sm mt-1" aria-live="polite">
              {statusText(s.status)}
              {s.latestVersion && ` · ${s.latestVersion}`}
            </p>
            {s.checkedAt && (
              <p className="text-xs text-muted-foreground mt-1">
                {t("上次检查：{0}", {
                  0: new Date(s.checkedAt).toLocaleString(language),
                })}
              </p>
            )}
          </div>
          <div className="toolbar">
            <Button
              variant="outline"
              disabled={!desktop || updates.busy}
              onClick={() => void updates.check()}
            >
              {s.status === "checking" ? (
                <Loader2 className="animate-spin" />
              ) : (
                <RefreshCw />
              )}
              {t("检查更新")}
            </Button>
            {s.latestVersion && s.canInstall && s.status !== "downloading" && (
              <Button disabled={updates.busy} onClick={() => setConfirm(true)}>
                <Download />
                {t("下载并安装")}
              </Button>
            )}
            {s.status === "downloading" && (
              <Button variant="outline" onClick={() => void updates.cancel()}>
                {t("取消下载")}
              </Button>
            )}
          </div>
        </div>
        {s.status === "downloading" && (
          <div className="space-y-2" role="status">
            <progress
              className="w-full h-2 accent-primary"
              aria-label={t("下载进度")}
              value={progress ?? undefined}
              max={100}
            />
            <p className="text-xs text-muted-foreground">
              {(s.downloaded / 1024 / 1024).toFixed(1)} MB
              {s.total
                ? ` / ${(s.total / 1024 / 1024).toFixed(1)} MB · ${progress}%`
                : ""}
            </p>
          </div>
        )}
        {error && (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}
        {s.latestVersion && !s.canInstall && (
          <p className="text-sm text-muted-foreground">
            {t(
              "当前运行方式不支持应用内安装，请前往发布页下载安装包。Linux DEB 请使用系统包管理器更新。",
            )}
          </p>
        )}
        {s.notes && (
          <details className="text-sm">
            <summary className="cursor-pointer">{t("更新说明")}</summary>
            <p className="mt-2 whitespace-pre-wrap text-muted-foreground max-h-48 overflow-auto">
              {s.notes}
            </p>
          </details>
        )}
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={s.autoCheck}
            disabled={!desktop || updates.busy}
            onChange={(e) => void updates.setAuto(e.target.checked)}
          />
          {t("自动检查 GitHub Release 更新")}
        </label>
        <p className="text-xs text-muted-foreground">
          {t(
            "启动后及每 6 小时检查一次；下载安装需要你确认，项目与设置会保留。",
          )}
        </p>
        <ExternalLink
          className="text-sm text-primary underline"
          href="https://github.com/touwaeriol/codegraph-desktop/releases"
        >
          {t("打开 GitHub 发布页")}
        </ExternalLink>
        <Dialog open={confirm} onOpenChange={setConfirm}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{t("安装更新并重启")}</DialogTitle>
              <DialogDescription>
                {t(
                  "请先保存正在进行的工作。下载和签名校验完成后，将停止受管实例、安装更新并重新打开应用。",
                )}
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={() => setConfirm(false)}>
                {t("取消")}
              </Button>
              <Button
                onClick={() => {
                  setConfirm(false);
                  void updates.install();
                }}
              >
                {t("安装更新并重启")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </CardContent>
    </Card>
  );
}
