import { useCallback, useEffect, useRef, useState } from "react";
import { KeyRound, Loader2, Search } from "lucide-react";
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
import type { Settings, WarpGrepSnapshot } from "@/lib/types";

const stopped: WarpGrepSnapshot = {
  state: "stopped",
  pid: null,
  endpoint: null,
  startedAt: null,
  tools: [],
  error: null,
};
export function useWarpGrep(projectId: string) {
  const [snapshots, setSnapshots] = useState<Record<string, WarpGrepSnapshot>>(
    {},
  );
  const snapshot = snapshots[projectId] ?? stopped;
  const current = useRef(projectId);
  current.current = projectId;
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(async () => {
    try {
      setSnapshots(
        await call<Record<string, WarpGrepSnapshot>>("list_warpgrep_snapshots"),
      );
    } catch (e) {
      setError(message(e));
    }
  }, []);
  useEffect(() => {
    setError("");
    setBusy(false);
    if (!projectId || !desktop) return;
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => clearInterval(timer);
  }, [projectId, refresh]);
  const run = useCallback(
    async (action: "start" | "stop" | "restart", id = projectId) => {
      setBusy(true);
      setError("");
      try {
        await call<WarpGrepSnapshot>("warpgrep_operation", {
          projectId: id,
          action,
        });
      } catch (e) {
        if (current.current === id) setError(message(e));
        else throw e;
      } finally {
        setBusy(false);
        await refresh();
      }
    },
    [projectId, refresh],
  );
  return { snapshot, snapshots, refresh, error, busy, run };
}

export function WarpGrepPanel({
  runtime,
  keyHint,
  settings,
  configure,
}: {
  runtime: ReturnType<typeof useWarpGrep>;
  keyHint?: string | null;
  settings: () => void;
  configure: () => void;
}) {
  useLanguage();
  const { snapshot: s, error } = runtime;
  return (
    <div className="panel-grid engine-overview" data-testid="warpgrep-panel">
      {(error || s.error) && (
        <Alert variant="destructive" className="col-span-full">
          <AlertDescription>{error || s.error?.message}</AlertDescription>
        </Alert>
      )}
      {!keyHint && (
        <Alert className="col-span-full">
          <KeyRound />
          <AlertDescription className="flex items-center justify-between gap-3 flex-wrap">
            {t("需先在设置中配置并验证 Morph API Key，才能启动 WarpGrep。")}
            <Button variant="outline" size="sm" onClick={settings}>
              {t("前往设置")}
            </Button>
          </AlertDescription>
        </Alert>
      )}
      <EngineRuntimeCard
        name="WarpGrep"
        state={s.state}
        pid={null}
        port={s.endpoint ? new URL(s.endpoint).port : null}
        startedAt={s.startedAt}
        detail={{ name: t("Morph API Key"), value: keyHint ?? "—" }}
        onRefresh={() => void runtime.refresh()}
        busy={runtime.busy}
      />
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Search size={17} />
            {t("自然语言代码检索")}
          </CardTitle>
          <CardDescription>Morph WarpGrep</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="metrics">
            <Metric name={t("运行模式")} value={t("共享网关路由")} />
            <Metric name={t("连接协议")} value="Streamable HTTP" />
            <Metric
              name={t("可用工具")}
              value={s.state === "running" ? s.tools.length : "—"}
            />
            <Metric
              name={t("检索模型")}
              value={<span className="mono">morph-warp-grep-v2.1</span>}
            />
          </div>
          {s.tools.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {s.tools.map((tool) => (
                <Badge variant="secondary" className="mono" key={tool}>
                  {tool}
                </Badge>
              ))}
            </div>
          )}
          <p className="text-xs text-muted-foreground leading-6">
            {t(
              "仅提供只读工具 codebase_search：模型规划 grep、列目录与读取，桌面应用只在当前项目目录内本地执行，不访问项目外文件。",
            )}
          </p>
          <p className="text-xs text-muted-foreground leading-6">
            {t(
              "检索时读取的项目代码片段会发送至 Morph API 处理，请确认符合项目的代码外发要求。",
            )}
          </p>
          <Button variant="outline" onClick={settings}>
            {t("配置运行环境")}
          </Button>
        </CardContent>
      </Card>
      <EngineConnectionCard
        name="WarpGrep"
        configure={configure}
        endpoint={s.endpoint}
      />
    </div>
  );
}

export function WarpGrepSettings({
  settings,
  onChange,
}: {
  settings: Settings | null;
  onChange: () => Promise<void>;
}) {
  const language = useLanguage();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const hint = settings?.morphKeyHint;
  async function perform(action: () => Promise<unknown>, success: string) {
    setBusy(true);
    setError("");
    try {
      await action();
      setKey("");
      await onChange();
      toast.success(success);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <EngineSettingsCard
      name="WarpGrep"
      description={t("自然语言代码检索")}
      testId="warpgrep-settings"
    >
      <Field>
        <FieldLabel htmlFor="morph-api-key">{t("Morph API Key")}</FieldLabel>
        <Input
          id="morph-api-key"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={t("粘贴 Morph API Key（sk-…）")}
        />
        <div className="toolbar">
          <Button
            variant="outline"
            disabled={busy || !desktop || !key.trim()}
            onClick={() =>
              void perform(
                () => call<Settings>("save_morph_api_key", { apiKey: key }),
                t("API Key 验证通过并已保存"),
              )
            }
          >
            {busy ? <Loader2 className="animate-spin" /> : null}
            {t("验证并保存")}
          </Button>
          {hint && (
            <Button
              variant="ghost"
              disabled={busy || !desktop}
              onClick={() =>
                void perform(
                  () => call<Settings>("remove_morph_api_key"),
                  t("API Key 已移除，运行中的 WarpGrep 已停止"),
                )
              }
            >
              {t("移除")}
            </Button>
          )}
        </div>
        {error && (
          <p role="alert" className="text-destructive text-sm break-all">
            {error}
          </p>
        )}
        <p className="text-xs text-muted-foreground">
          {hint ? t("已验证：{0}", { 0: hint }) : t("尚未配置 API Key")}
        </p>
        {settings?.morphKeyValidatedAt && (
          <p className="text-xs text-muted-foreground">
            {t("验证时间：{0}", {
              0: new Date(settings.morphKeyValidatedAt).toLocaleString(language),
            })}
          </p>
        )}
        <p className="text-xs text-muted-foreground leading-5">
          {t(
            "保存前会用此 Key 真实调用一次 WarpGrep 模型（约 1K tokens），验证通过才保存。Key 仅保存在本机应用数据目录，不会写入项目配置。",
          )}
        </p>
        <details className="engine-install">
          <summary>{t("安装说明")}</summary>
          <p className="text-xs text-muted-foreground">
            {t(
              "检索时读取的项目代码片段会发送至 Morph API 处理，请确认符合项目的代码外发要求。",
            )}
          </p>
          <ExternalLink
            href="https://morphllm.com/dashboard/api-keys"
            className="text-primary text-sm underline"
          >
            {t("获取 Morph API Key ↗")}
          </ExternalLink>
          <ExternalLink
            href="https://docs.morphllm.com/sdk/components/warp-grep"
            className="text-primary text-sm underline"
          >
            {t("WarpGrep 文档 ↗")}
          </ExternalLink>
        </details>
      </Field>
    </EngineSettingsCard>
  );
}
