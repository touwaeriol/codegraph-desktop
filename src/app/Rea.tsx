import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Loader2, Plus, ScanSearch, X } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ExternalLink } from "@/components/ExternalLink";
import { call, desktop, message } from "@/lib/ipc";
import { t, useLanguage } from "@/lib/i18n";
import type { MessageKey } from "@/lib/messages";
import type {
  ReaCheck,
  ReaEnvironment,
  ReaEnvironmentSaved,
} from "@/lib/types";
import {
  EngineConnectionCard,
  EngineSettingsCard,
  Metric,
} from "./EngineOverview";
import { EngineTools } from "./EngineTools";
import { ReaTools } from "./ReaTools";

export const REA_COMMAND = "npx -y rea-agents@6.5.0 mcp";

function ReaRequirements() {
  return (
    <div className="space-y-3 text-sm text-muted-foreground leading-6">
      <p>
        {t(
          "REA 需要 Node.js 22.19+（22.x）、24.11+（24.x）或 26+，以及 PATH 中的 npm / npx；不支持 Node.js 23、25 和预发布版本。",
        )}
      </p>
      <p>
        {t(
          "JavaScript / Electron 和 .NET 静态分析无需原生反编译器；深度原生分析可在设置中一键安装 Ghidra，或自行配置 Hopper、IDA。",
        )}
      </p>
      <p>
        {t(
          "首次启动或测试会通过 npx 下载固定版本的 rea-agents，需要访问 npm registry。Ghidra、JADX 等可在设置中一键安装，商业工具及其许可证由用户自行准备。",
        )}
      </p>
      <ExternalLink
        href="https://github.com/morluto/rea"
        className="underline underline-offset-4"
      >
        {t("REA 官方项目与安装说明 ↗")}
      </ExternalLink>
    </div>
  );
}

export function ReaPanel({
  configure,
  settings,
}: {
  configure: () => void;
  settings: () => void;
}) {
  useLanguage();
  return (
    <div className="panel-grid engine-overview" data-testid="rea-panel">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <ScanSearch size={17} />
            {t("应用与二进制逆向分析")}
          </CardTitle>
          <CardDescription>REA · Reverse Engineer Anything</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="metrics">
            <Metric name={t("运行模式")} value={t("由客户端启动")} />
            <Metric name={t("连接协议")} value="stdio" />
          </div>
          <code className="block text-xs break-all">{REA_COMMAND}</code>
          <p className="text-sm text-muted-foreground leading-6">
            {t(
              "REA 由 Codex、Claude Code、Cursor 或 Devin 按需启动，无需在桌面启动实例或建立 CodeGraph 索引。退出桌面应用不影响客户端的 REA 进程。",
            )}
          </p>
          <p className="text-sm text-muted-foreground leading-6">
            {t(
              "配置后重新加载客户端并批准 MCP。测试连接会临时启动 REA，仅验证握手和工具列表，完成后回收测试进程。",
            )}
          </p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>{t("运行环境")}</CardTitle>
          <CardDescription>{t("本地分析工具")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <ReaRequirements />
          <Button variant="outline" onClick={settings}>
            {t("配置运行环境")}
          </Button>
        </CardContent>
      </Card>
      <EngineConnectionCard name="REA" configure={configure} />
    </div>
  );
}

export function ReaSettings() {
  useLanguage();
  const [saved, setSaved] = useState<ReaEnvironment>({});
  const [values, setValues] = useState<ReaEnvironment>({});
  const [custom, setCustom] = useState<[string, string][]>([]);
  const [busy, setBusy] = useState<"save" | "check" | null>(null);
  const [error, setError] = useState("");
  const [checks, setChecks] = useState<ReaCheck[] | null>(null);
  const [toolsVersion, setToolsVersion] = useState(0);
  function load(environment: ReaEnvironment) {
    setSaved(environment);
    setValues(
      Object.fromEntries(
        Object.entries(environment).filter(([key]) => KNOWN.has(key)),
      ),
    );
    setCustom(Object.entries(environment).filter(([key]) => !KNOWN.has(key)));
  }
  useEffect(() => {
    if (!desktop) return;
    let live = true;
    // Installed tools fill unset paths before the form shows them.
    call<ReaEnvironmentSaved | null>("adopt_rea_tools")
      .then((adopted) => {
        // Not gated on `live`: the save already happened either way.
        if (!adopted) return;
        setToolsVersion((v) => v + 1);
        toast.success(
          adopted.updated.length
            ? t(
                "已识别已安装的分析工具并写入 REA 环境，已更新 {0} 个项目配置；请在客户端重新加载 MCP",
                { 0: adopted.updated.length },
              )
            : t("已识别已安装的分析工具并写入 REA 环境"),
        );
      })
      .catch((e) => live && setError(message(e)))
      .then(() => call<ReaEnvironment>("get_rea_environment"))
      .then((environment) => live && load(environment))
      .catch((e) => live && setError(message(e)));
    return () => {
      live = false;
    };
  }, []);
  const draft: ReaEnvironment = { ...values };
  for (const [key, value] of custom)
    if (key.trim() || value.trim()) draft[key] = value;
  const dirty = canonical(draft) !== canonical(saved);
  async function save() {
    setBusy("save");
    setError("");
    try {
      const result = await call<ReaEnvironmentSaved>("save_rea_environment", {
        environment: draft,
      });
      load(result.environment);
      setChecks(null);
      toast.success(
        result.updated.length
          ? t("REA 环境已保存，并更新 {0} 个项目配置；请在客户端重新加载 MCP", {
              0: result.updated.length,
            })
          : t("REA 环境已保存"),
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
    } finally {
      setBusy(null);
    }
  }
  async function check() {
    setBusy("check");
    setError("");
    try {
      setChecks(await call<ReaCheck[]>("check_rea_environment"));
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  }
  function browse(key: string, directory: boolean) {
    void open({ multiple: false, directory })
      .then((path) => {
        if (typeof path === "string")
          setValues((old) => ({ ...old, [key]: path }));
      })
      .catch((e) => setError(message(e)));
  }
  const set = (key: string, value: string) =>
    setValues((old) => ({ ...old, [key]: value }));
  return (
    <EngineSettingsCard
      name="REA"
      description={t("可选引擎 · 应用与二进制逆向分析")}
      testId="rea-settings"
    >
      <div className="rea-settings-fields">
        {FIELDS.map((field) => {
          const id = `rea-${field.key}`;
          const value = values[field.key] ?? "";
          return (
            <Field key={field.key}>
              <FieldLabel htmlFor={id}>{t(field.label)}</FieldLabel>
              {field.kind === "provider" ? (
                <select
                  id={id}
                  className="border rounded-md p-2"
                  value={value}
                  onChange={(e) => set(field.key, e.target.value)}
                >
                  <option value="">{t("自动选择")}</option>
                  <option value="ghidra">Ghidra</option>
                  <option value="ida">IDA</option>
                  <option value="hopper">Hopper</option>
                </select>
              ) : field.kind === "profile" ? (
                <select
                  id={id}
                  className="border rounded-md p-2"
                  value={value}
                  onChange={(e) => set(field.key, e.target.value)}
                >
                  <option value="">{t("完整（默认）")}</option>
                  <option value="compact">{t("精简")}</option>
                </select>
              ) : (
                <div className="rea-path-input">
                  <Input
                    id={id}
                    value={value}
                    spellCheck={false}
                    onChange={(e) => set(field.key, e.target.value)}
                    placeholder={t("未设置")}
                  />
                  <Button
                    variant="outline"
                    disabled={!desktop}
                    onClick={() =>
                      browse(field.key, field.kind === "directory")
                    }
                  >
                    {field.kind === "directory" ? t("选择目录") : t("选择文件")}
                  </Button>
                </div>
              )}
              <p className="mono text-xs text-muted-foreground">{field.key}</p>
            </Field>
          );
        })}
      </div>
      <ReaTools onInstalled={load} version={toolsVersion} />
      <details className="engine-install mt-4">
        <summary>{t("其他环境变量（{0}）", { 0: custom.length })}</summary>
        <p className="text-xs text-muted-foreground">
          {t(
            "可填写 REA 支持的其他变量，例如 REA_GHIDRA_STARTUP_TIMEOUT_MS、REA_APKTOOL_COMMAND、REA_ADB_PATH。",
          )}
        </p>
        {custom.map(([key, value], index) => (
          <div className="rea-env-row" key={index}>
            <Input
              className="mono"
              aria-label={t("变量名")}
              placeholder={t("变量名")}
              spellCheck={false}
              value={key}
              onChange={(e) =>
                setCustom((old) =>
                  old.map((row, i) =>
                    i === index ? [e.target.value, row[1]] : row,
                  ),
                )
              }
            />
            <Input
              aria-label={t("值")}
              placeholder={t("值")}
              spellCheck={false}
              value={value}
              onChange={(e) =>
                setCustom((old) =>
                  old.map((row, i) =>
                    i === index ? [row[0], e.target.value] : row,
                  ),
                )
              }
            />
            <Button
              variant="ghost"
              size="icon"
              aria-label={t("删除变量")}
              onClick={() =>
                setCustom((old) => old.filter((_, i) => i !== index))
              }
            >
              <X />
            </Button>
          </div>
        ))}
        <Button
          variant="outline"
          size="sm"
          onClick={() => setCustom((old) => [...old, ["", ""]])}
        >
          <Plus />
          {t("添加变量")}
        </Button>
      </details>
      <div className="toolbar mt-4">
        <Button
          disabled={!desktop || !dirty || busy !== null}
          onClick={() => void save()}
        >
          {busy === "save" ? <Loader2 className="animate-spin" /> : null}
          {t("保存")}
        </Button>
        <Button
          variant="outline"
          disabled={!desktop || dirty || busy !== null}
          title={dirty ? t("请先保存，检查使用已保存的配置") : undefined}
          onClick={() => void check()}
        >
          {busy === "check" ? <Loader2 className="animate-spin" /> : null}
          {t("检查分析引擎")}
        </Button>
      </div>
      {error && (
        <p role="alert" className="mt-3 text-destructive text-sm break-all">
          {error}
        </p>
      )}
      {checks && (
        <ul className="rea-checks" data-testid="rea-checks">
          {checks.map((item) => (
            <li key={item.name} data-ok={item.ok}>
              <Badge variant={item.ok ? "secondary" : "outline"}>
                {item.ok ? t("就绪") : t("未就绪")}
              </Badge>
              <div className="min-w-0">
                <strong className="mono">{item.name}</strong>
                {item.detail && <p>{item.detail}</p>}
                {!item.ok &&
                  item.remediation &&
                  item.remediation !== item.detail && <p>{item.remediation}</p>}
              </div>
            </li>
          ))}
        </ul>
      )}
      <p className="mt-3 text-xs text-muted-foreground leading-6">
        {t(
          "变量会写入各客户端的项目 MCP 配置；保存后自动更新已由本应用注入 REA 的项目配置，重新加载客户端后生效。",
        )}
      </p>
      <EngineTools engine="rea" />
      <details className="engine-install mt-3">
        <summary>{t("安装说明")}</summary>
        <ReaRequirements />
        <code className="block text-xs break-all">{REA_COMMAND}</code>
        <p className="text-xs text-muted-foreground leading-6">
          {t(
            "Windows 配置使用 cmd.exe /d /c 启动 npx。项目配置只注册 MCP；配套技能可按 REA 官方说明另行安装。",
          )}
        </p>
      </details>
    </EngineSettingsCard>
  );
}

const FIELDS: {
  key: string;
  label: MessageKey;
  kind: "provider" | "profile" | "directory" | "file";
}[] = [
  { key: "REA_ANALYSIS_PROVIDER", label: "默认分析引擎", kind: "provider" },
  { key: "GHIDRA_INSTALL_DIR", label: "Ghidra 安装目录", kind: "directory" },
  { key: "JAVA_HOME", label: "Ghidra 使用的 JDK 目录", kind: "directory" },
  { key: "REA_IDA_MCP_CONFIG", label: "IDA MCP 注册文件", kind: "file" },
  { key: "HOPPER_LAUNCHER_PATH", label: "Hopper 启动器", kind: "file" },
  { key: "REA_JADX_MCP_JAR", label: "jadx-headless-mcp JAR", kind: "file" },
  { key: "REA_ILSPY_CMD_PATH", label: "ILSpy 命令行", kind: "file" },
  {
    key: "REA_MCP_INPUT_SCHEMA_PROFILE",
    label: "工具参数描述",
    kind: "profile",
  },
];
const KNOWN = new Set(FIELDS.map((field) => field.key));
/** Compares environments the way the backend stores them. */
const canonical = (environment: ReaEnvironment) =>
  JSON.stringify(
    Object.entries(environment)
      .map(([key, value]) => [key.trim(), value.trim()])
      .filter(([, value]) => value)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
  );
