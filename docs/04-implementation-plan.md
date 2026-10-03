# 实施与验收计划

状态：开发与验收计划；日期：2026-10-03。首版源码已开始实现；下列里程碑以退出条件是否实际验证为准，不能因文件已写出就标记完成。最新进展见 [验证记录](05-validation.md)。

## 1. 开发顺序

### M0：验证独立实例与 MCP 复用

先建立两个临时样例项目，包含名称相同但实现不同的函数，验证实际返回来源。

必须完成：

- 确认 CodeGraph 安装入口、运行时参数、版本和 serve 能力。
- 分别初始化项目 A、B，启动各自 stdio 服务并读取工具 schema。
- 验证项目路径绑定、roots 请求、取消、异常退出及 daemon/watch 行为。
- 编写最小 Rust 桥接原型，完成一个上游会话、两个下游会话。
- 用两个模拟客户端发送相同 JSON-RPC ID 的请求，验证结果不串线。
- 尝试通过 `projectPath`、文件绝对路径和符号链接越界，确认拦截策略。
- Windows 上验证完整进程树停止，不影响其他 CodeGraph 实例。
- 在实际 Codex 和 Claude Code 中分别完成真实工具调用，再同时连接同一项目。

退出条件：证明核心架构可行并记录兼容版本。若失败，先修正桥接或上游适配，不进入精细 UI 开发；不能将双客户端各启一个 CodeGraph 当作通过。

### M1：Tauri 应用骨架和项目登记

- Tauri 2 + React + TypeScript + Vite 脚手架。
- 中文侧栏、项目空状态、添加/编辑/移除、设置页。
- SQLite 项目登记与迁移，真实目录选择及规范化去重。
- IPC 参数验证、结构化错误、单实例应用与托盘。

退出条件：重启后项目列表可靠恢复；坏数据库不被覆盖；Web 前端无法执行任意命令。

### M2：实例与索引任务

- 实现项目状态机、锁、generation、进程树清理。
- 初始化、同步、重建、取消与日志推送。
- 独立端口、运行记录、鉴权及实例启动探测。
- 多项目并发限制、批量操作和关闭行为。

退出条件：两个项目同时运行，停止其中一个不影响另一个；失败不遗留监听端口或孤儿进程。

### M3：连接器与配置管理

- 打包 `cg-mcp-connector`，无需用户另装 Node 来运行连接器。
- 实现项目级 TOML/JSON 合并、diff、哈希检查、备份与恢复。
- 0.1.3 起生成固定 HTTP 地址和鉴权头，以固定服务名 `codegraph` 配置项目专属网关；旧连接器保留兼容。
- 显示真实会话、测试结果及客户端授权说明。

退出条件：两个客户端共用同一实例；已有无关设置与 MCP 配置不丢失；部分写入可诊断、可恢复。

### M4：UI 完整性与分发

- 按 UI 设计完善加载、错误、空状态、键盘和高 DPI。
- 提供 Windows 安装包，连接器放在稳定安装位置。
- 验证升级、移动安装位置后的检测与修复提示。
- 完成使用说明、限制、日志收集步骤及发布检查。

退出条件：在干净 Windows 用户环境中完成添加项目到双客户端查询的全流程。未签名构建明确标注，不规避系统签名提示。

## 2. 预期源码组织

以下为目标结构，不表示这些文件已经存在：

```text
codegraph-desktop/
  docs/
  src/
    app/
    features/projects/
    features/runtime/
    features/indexing/
    features/mcp-config/
    features/logs/
    features/settings/
    components/ui/
    lib/ipc.ts
    lib/types.ts
  src-tauri/
    capabilities/
    src/
      commands/
      projects/
      runtime/
      gateway/
      configuration/
      persistence/
      diagnostics/
    tauri.conf.json
  crates/
    project-protocol/          网关与连接器共用 DTO、错误及运行记录定义
    cg-mcp-connector/          独立 stdio 连接器
  tests/
    fixtures/
    integration/
    e2e/
```

不为单个页面引入过度抽象。共享类型生成与 schema 校验应使 Rust 和 TypeScript 的状态枚举保持一致。

## 3. 必须覆盖的自动测试

### 3.1 项目与路径

- 同目录不同大小写或 junction 不重复登记。
- 中文、空格、`&`、括号等路径不触发 Shell 解释。
- 两个 Git worktree 分别登记。
- 丢失目录、只读目录、移除运行项目、重新定位项目的状态正确。

### 3.2 生命周期

- 同时两次启动只有一个上游服务实例。
- Starting 中取消、握手超时、端口占用、上游立即退出均能清理。
- 停止 A 后 B 的连续查询不受影响。
- GUI 正常退出和异常终止后无受管孤儿进程；上游 daemon 行为明确。
- 旧 generation 的延迟事件不会覆盖新实例状态。
- 手工外部实例存在时不会被按名字误杀。

### 3.3 协议与隔离

- 两个下游用相同请求 ID，结果准确归属。
- 排队取消、运行中取消、客户端断开、超时不波及其他会话。
- A、B 含同名函数，查询分别返回自己的实现。
- 越界 projectPath、绝对文件路径、`..`、符号链接被拒绝。
- token 错误、Origin 不合法、Host 异常、旧 session 被拒绝。
- 请求队列满时可预测地拒绝，不无限占用内存。
- stdout 不混入日志；未知方法和不支持能力返回规范错误。

### 3.4 配置与恢复

- 合并保留 Codex 的模型、其他 MCP、注释与无关 TOML 段落。
- Claude Code JSON 的无关键值完整保留。
- 原文件不存在、BOM、无效编码、语法错误分别有正确处理。
- 写入前文件被修改时拒绝旧预览。
- 模拟第二份文件写入失败，第一份按清单恢复；恢复失败展示部分完成。
- 崩溃中断操作可在重启后检测；恢复不覆盖用户后续编辑。
- 移除受管项保留其他项；受管项被手工编辑后不静默删除。
- HTTP 配置包含本机项目 token，避免日志和测试证据泄露；备份和运行记录使用用户私有目录权限，不自动提交项目配置。

## 4. 必须完成的手工联调

记录实际 Windows、CodeGraph、Codex、Claude Code、应用和连接器版本。

1. 在空目录添加项目并初始化代码索引。
2. 启动实例，写入两个客户端配置，分别完成客户端信任步骤。
3. 从 Codex 查询样例函数，再从 Claude Code 查询调用链。
4. 保持两者连接，确认只有一个该项目上游服务会话。
5. 修改源文件，验证 watcher 后的新查询返回新内容。
6. 启动第二项目，验证路由隔离。
7. 停止第一项目，确认其客户端失败可见、第二项目继续可用。
8. 重启第一项目，确认客户端能重新建立连接，配置无需端口修复。
9. 隐藏窗口、退出应用、重启应用分别验证托盘和进程行为。
10. 用最小窗口与 200% DPI 检查配置差异和日志界面。

独立 MCP 探测成功不替代第 3 步的真实客户端测试。无法运行真实客户端时必须在交付说明中标注未验证，不能称全流程通过。

## 5. 性能目标

以下是设计预算，尚未测量：

- 200 个登记项目时，搜索及选择项目交互目标小于 100 ms。
- 日志按批次推送，单批 100–250 ms，前端使用有界缓存。
- 索引吞吐由 CodeGraph 决定，应用展示实际耗时，不预先承诺速度。
- 长时间空闲不轮询全部源码，不额外重复 CodeGraph watcher。
- 记录应用自身与每项目 CodeGraph 的资源占用，区分二者再做优化。

## 6. 发布阻断项

- 发生跨项目查询或双客户端响应串线。
- 配置写入导致无关字段丢失且无法恢复。
- 停止实例遗留不可控进程，或误杀其他项目实例。
- UI 报告运行成功但 MCP 握手失败。
- 本地接口暴露到非回环地址或鉴权可绕过。
- 连接器实际启动了额外 CodeGraph，违背统一管理约定。
- Windows 安装包遗漏连接器或写入的路径无法执行。

## 7. 官方参考与证据记录

- [CodeGraph 仓库](https://github.com/colbymchenry/codegraph)：安装、索引与 MCP CLI。
- 本地实查：CodeGraph `1.6.2`；`codegraph help serve` 返回 `--mcp`、`--path`、`--no-watch`。
- [Tauri 2](https://v2.tauri.app/start/) 和 [Capabilities](https://v2.tauri.app/security/capabilities/)：Web/Rust 分层和权限模型。
- [Codex MCP](https://developers.openai.com/codex/mcp/)：项目配置与 stdio 服务定义。
- [Claude Code MCP](https://code.claude.com/docs/en/mcp)：项目范围及授权。
- [MCP 2025-06-18 传输规范](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)：桥接验证的协议基线；实现时按双方支持版本协商。

实现阶段记录官方依赖锁定版本、验证命令和实际结果，避免将设计假设误当作已验证能力。
