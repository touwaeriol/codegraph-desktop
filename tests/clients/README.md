# 真实客户端 MCP 联调

`real-clients.cjs` 使用本机已安装、已配置认证的 Codex CLI 和 Claude Code，创建临时 CodeGraph 样例项目，通过两个真实客户端查询同一网关中的随机标记。提示词不包含预期标记，证据同时要求观察到 MCP 工具调用和正确最终结果。

默认测试 Streamable HTTP 直连，不复制或启动连接器。传入 `--stdio` 可单独验证旧连接器兼容路径。HTTP 令牌在诊断输出中替换为占位文本，临时客户端配置在退出时删除。

先完成 `scripts/build-desktop.ps1 -Mode Mcp`，生成连接器与 host 测试程序，再运行：

```powershell
node tests/clients/real-clients.cjs
# 仅检查 Codex
node tests/clients/real-clients.cjs --codex-only
# 为本次测试指定当前中转服务支持的 Claude 模型
node tests/clients/real-clients.cjs --claude-model claude-haiku-4-5-20251001
```

此测试会使用客户端已有的模型服务，可能消耗模型用量。Claude 单次测试设置 1 美元预算上限。脚本不记录或展示认证凭据；客户端原始诊断保存在被忽略的 `.tools/real-clients/`，发布证据仅保留版本/结果相关字段。

测试使用临时二进制副本，不锁住构建输出。运行记录在退出时清理，临时样例源码保留用于诊断。Codex MCP 设置通过本次 CLI 参数传入，Claude 通过 `--mcp-config` 加载样例配置，不持久修改全局配置。项目首次信任及 `.codex/config.toml` 自动加载流程仍需手工验证。

本轮 `real-client-evidence.json` 的两个客户端均通过。首次默认 Claude 模型因中转服务 503 失败的结果保存在 `default-model-evidence.json`，不将模型服务故障误记为网关通过或失败。
