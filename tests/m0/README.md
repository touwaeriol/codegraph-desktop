# M0 实测与实现边界

当前已实测上游 CodeGraph 1.6.2：两个带中文、空格及 `&` 的临时项目初始化成功；stdio MCP 握手、工具列表、真实 `uniqueMarker` 查询分别返回 A_ONLY_739 和 B_ONLY_739；关闭 stdin 后两个服务均退出，退出码为 0。完整结果见 upstream-evidence.json。该文件记录一次本机运行证据，临时路径不保证一直存在。

本机上游源码确认 `CODEGRAPH_NO_DAEMON=1` 进入 direct 模式。网关强制设置此环境变量，不附着全局 daemon；使用 Job Object 管理受管进程树。只开放本轮实际发现的 `codegraph_explore`，限制工具参数并固定 projectPath。未知工具不开放。

## 可重复验证

1. `node tests/m0/probe.cjs [CodeGraph bundle目录]`
2. `cargo build -p cg-mcp-connector`
3. `cargo build -p project-gateway --example host`
4. `node tests/m0/gateway-smoke.cjs [CodeGraph npm shim路径]`

smoke 启动两个真实网关、三个连接器，验证两个客户端使用相同 JSON-RPC ID 的响应归属、跨项目 projectPath 拒绝、停止 A 后 B 仍能查询，以及停止后的端口释放。只有脚本成功生成 gateway-evidence.json 才代表该组验证通过。

## 协议取舍

采用固定版本 rmcp 0.8.5 的官方 stdio、Streamable HTTP 和会话实现。SDK 将下游请求与上游请求分别编号。暂不转发进度通知，不透传下游 progressToken；不声明 sampling/elicitation/roots 等未实现能力，CodeGraph 使用固定 --path。

rmcp 0.8.5 在发送取消通知时立即本地结束 await_response，并不确认上游任务已经停止。因此排队取消立即生效；执行中取消保留串行锁，等待当前只读查询自然完成并丢弃结果。120 秒仍无法确认结束时拒绝新的工具调用并要求重启，避免不确定调用与下一条并行；不自动重试。

## 未验证时不得声称通过

真实 Codex 和 Claude Code 授权与工具调用、完整 Windows 安装升级、watcher 变更、长时间压力与恶意路径覆盖需单独记录实测。模拟客户端和独立 MCP 探测不能替代真实客户端验收。

## 2026-10-03 编译与桥接实测

已完成 `cargo test -p project-gateway -p project-protocol`：3 项测试通过，其中网关测试覆盖运行中取消后保持串行、另一客户端继续工作及队列饱和拒绝；协议测试覆盖运行地址与身份字段校验。

已构建 cg-mcp-connector 与 host 示例，并实际运行 gateway-smoke.cjs 成功。gateway-evidence.json 记录同 ID 不同查询响应归属、双连接器复用、跨项目拒绝、停止隔离、端口释放、token/Host/Origin 与旧实例会话拒绝。

本轮发现并修复 Windows Node 启动问题：标准 canonicalize 会把脚本入口转换为 `\\?\C:\...`，Node 24 解析该入口时报 EISDIR。入口与项目路径使用 dunce 规范化，保留目录身份解析但输出兼容 Node 的普通路径。

模拟客户端桥接通过仍不代表真正 Codex/Claude Code 的授权与调用已经通过，真实客户端验收另行记录。

补充实测已通过：在运行项目中新建包含随机函数的新源码文件，确认 watcher 完成索引后工具查询返回该随机函数标记；不是仅验证旧索引符号读取磁盘的新返回值。另强制终止 host，检查终止前枚举的整个 CodeGraph 进程树均消失且端口关闭，另一项目继续查询成功。测试二进制先复制至临时目录，不占用编译输出。

## HTTP 直连与持久地址

`GatewayOptions.persistent_token=Some(...)` 启用稳定连接：令牌必须为 64 位十六进制，且 preferred_port 必须为非零固定端口。端口占用返回 PORT_BIND_FAILED，不改用随机端口。None 保留旧临时连接测试行为。host 示例通过 CODEGRAPH_TEST_HTTP_TOKEN / CODEGRAPH_TEST_HTTP_PORT 读取测试参数，令牌不打印到控制台，运行记录仍走私有目录发布。

验证命令：`cargo build -p project-gateway --example http_direct_smoke`，随后 `node tests/m0/http-direct-smoke.cjs [CodeGraph shim路径]`。此测试使用官方 rmcp Streamable HTTP 客户端直接握手与调用，不启动 cg-mcp-connector；覆盖双客户端不同查询、跨项目参数和 token 拒绝、固定端口与 token 重启后再次连接、端口占用不可漂移。原 gateway-smoke.cjs 保留作为旧连接器兼容性和生命周期回归。
