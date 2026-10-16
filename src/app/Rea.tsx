import { ScanSearch } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ExternalLink } from "@/components/ExternalLink";
import { t, useLanguage } from "@/lib/i18n";
import {
  EngineConnectionCard,
  EngineSettingsCard,
  Metric,
} from "./EngineOverview";

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
          "JavaScript / Electron 和 .NET 静态分析无需原生反编译器；深度原生分析需自行配置 Hopper、Ghidra 或 IDA 等工具。",
        )}
      </p>
      <p>
        {t(
          "首次启动或测试会通过 npx 下载固定版本的 rea-agents，需要访问 npm registry。分析工具及其许可证由用户自行准备。",
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
              "REA 由 Codex / Claude Code 按需启动，无需在桌面启动实例或建立 CodeGraph 索引。退出桌面应用不影响客户端的 REA 进程。",
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
  return (
    <EngineSettingsCard
      name="REA"
      description={t("可选引擎 · 应用与二进制逆向分析")}
      testId="rea-settings"
    >
      <ReaRequirements />
      <code className="block mt-4 text-xs break-all">{REA_COMMAND}</code>
      <p className="mt-3 text-xs text-muted-foreground leading-6">
        {t(
          "Windows 配置使用 cmd.exe /d /c 启动 npx。项目配置只注册 MCP；配套技能可按 REA 官方说明另行安装。",
        )}
      </p>
    </EngineSettingsCard>
  );
}
