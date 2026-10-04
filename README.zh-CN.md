# CodeGraph Desktop

[English](README.md) | **简体中文**

基于 **Tauri 2 + React + TypeScript + Vite** 的 CodeGraph 与 Serena 项目管理桌面应用。

每个项目独立启动 CodeGraph，使用独立索引、运行状态、日志和鉴权令牌。一个本地 HTTP 服务通过 `/mcp/{项目ID}` 路由到对应实例，Codex 与 Claude Code 仍通过项目级 MCP 配置连接。

## 当前阶段

源码包含 React 界面、Tauri/Rust 后端、项目网关与旧配置兼容连接器。构建、协议及安装回归的范围见 [验证记录](docs/05-validation.md)。首次客户端信任、原生窗口交互与干净机器安装仍需单独验收。

[GitHub 公共仓库](https://github.com/touwaeriol/codegraph-desktop) · [下载 Release](https://github.com/touwaeriol/codegraph-desktop/releases) · [Actions 构建](https://github.com/touwaeriol/codegraph-desktop/actions)

| 系统 | 架构 | 发布格式 |
| --- | --- | --- |
| Windows | AMD64、ARM64 | NSIS `.exe` |
| macOS | ARM64（Apple Silicon） | `.dmg`、`.pkg` |
| Linux | AMD64、ARM64 | `.deb`、`.AppImage` |

Release 只有在五个原生构建与检查全部成功后才发布，每个平台提供 SHA256 校验文件。当前未配置 Windows 代码签名或 Apple 签名/公证，产物为未签名构建。

Windows 安装器检测到已有安装后直接覆盖原目录，不显示“先卸载旧版本”的维护选项。主程序和连接器一起替换，Windows 设置中的独立卸载入口仍保留。已通过独立测试身份的静默覆盖升级及卸载回归，未操作用户的正式安装。

更新前会先请求应用停止受管 CodeGraph 和索引任务，再退出并释放程序文件。Windows 对不支持新退出协议的旧版本保留精确路径进程关闭兼容；Linux DEB 和 macOS PKG 会等待标准安装路径的应用退出，超时则停止安装。DMG 拖拽与 AppImage 文件替换没有安装前钩子，使用这两种格式时应先从托盘退出应用。不会按名称全局终止其他项目的 Node/CodeGraph。

## 界面语言

首次启动时，应用使用系统首选显示语言：中文环境使用简体中文，其余语言回退英文。在 **设置 → 界面语言** 中选择 **简体中文** 或 **English**，点击保存偏好即可立即更新界面和托盘菜单；手动选择会在重启后保留。

Windows 安装器自动使用中文或英文，不弹出语言选择窗口。macOS/Linux 的系统安装界面遵循操作系统自身的语言设置。项目名称、路径、配置文件内容和 CodeGraph 原始输出保持原文。

## 0.1.3 HTTP 直连与配置编辑

- 项目 MCP 服务统一命名为 `codegraph`，Codex 和 Claude Code 直接通过 Streamable HTTP 连接本机项目网关，不再启动客户端 stdio 连接器。网关到 CodeGraph 仍使用原生 stdio。
- 应用持久保存一个共享回环端口，各项目保留独立的稳定 ID、鉴权令牌和 MCP 会话。地址为 `http://127.0.0.1:<端口>/mcp/<项目ID>`。端口占用时明确报错，不自动改端口；使用前需在桌面软件启动该项目。停止单个项目不会停止共享服务或其他项目。
- 启动时会备份并迁移归属指纹仍匹配的旧受管 HTTP 配置。手动修改过的条目会保留，可通过“预览并配置”检查更新；不会修改客户端全局配置。
- 点击侧栏“设置”后，在右侧主区域显示独立设置页；点击项目即可返回项目工作台。
- 已配置旧版的项目需重新“预览并配置”，确认从 command/args 迁移为 URL 与鉴权头。保留连接器二进制用于旧配置兼容，新生成配置不再引用它。
- “预览并配置”可按文件选择合并配置、覆盖整个文件或编辑完整内容。覆盖会删除该文件其他设置，必须核对差异后应用。
- 编辑后点击“重新读取 / 刷新预览”，语法校验通过才能应用；外部编辑器修改文件后也可刷新，窗口内草稿会保留。
- 原配置无法解析时，可从错误提示进入“以覆盖模式预览”，再切换编辑修复；这一步不会写入文件。
- 应用前备份原文件并检查外部修改。手动更改或删除 `codegraph` 的 URL/鉴权字段后，该条目可能不再连接当前项目，也不再登记为本项目受管条目。项目配置含本机鉴权信息，不应上传或分享这些令牌。

## Serena（可选引擎）

CodeGraph 与 Serena 使用相同的工作台布局和页头操作。设置页将两个引擎作为同级卡片：宽窗口并排、窄窗口纵向排列，通用偏好单独放置。窗口支持拖动调整大小，最小尺寸为 800 × 560。

项目工作台增加 CodeGraph / Serena 引擎切换。Serena 提供符号搜索、引用分析和语义编辑，无需先建立 CodeGraph 索引；侧栏分别显示两个引擎的运行状态。

1. 安装 [uv](https://docs.astral.sh/uv/getting-started/installation/)，执行 `uv tool install -p 3.13 serena-agent`，详见 [Serena 官方安装说明](https://oraios.github.io/serena/02-usage/010_installation.html)。
2. 在“设置 → Serena 入口”中检测或手动选择可执行文件；检测通过后才保存入口。
3. 选择项目和 Serena，点击“启动实例”。首次使用可能下载语言服务器依赖，失败时查看运行日志；各编程语言所需依赖由 Serena 管理。
4. 在“MCP 配置”中默认同时勾选 CodeGraph 与 Serena，也可只选一个。选择 Codex / Claude Code 后，一次预览并应用两个 MCP；每个项目文件只写入一次，统一备份、失败回滚。默认合并保留其他设置，也支持覆盖和手动编辑。只修改项目的 `.codex/config.toml` / `.mcp.json`。

Serena 通过 `--context ide --project <绝对路径>` 单项目模式启动原生 HTTP，每个项目使用固定独立端口 `http://127.0.0.1:<端口>/mcp`，客户端直接连接。启动检查要求存在符号工具且没有切换项目工具。端口冲突报错，不自动切换。原生 Serena 不使用 CodeGraph 的令牌，仅供本机可信客户端使用；单项目模式不是操作系统沙箱。

停止 Serena 不影响 CodeGraph 或其他项目。退出应用、安装器请求退出、删除/迁移项目和“全部停止”都会回收受管 Serena 进程树。原有项目自动启动设置仍用于 CodeGraph，Serena 需主动启动。安装包不内置 Serena，需要自行安装其入口。已在 Windows 使用 Serena 1.7.0 验证原生 HTTP 和符号查询；macOS/Linux 的 Serena 运行验收与跨平台编译分别进行。

## 本地开发

通用依赖：Node.js 22+、Rust stable，以及本机安装的 CodeGraph。按目标系统安装 [Tauri 原生构建依赖](https://v2.tauri.app/start/prerequisites/)：Windows 使用 MSVC/Windows SDK/WebView2，macOS 使用 Xcode 命令行工具，Linux 使用 WebKitGTK 4.1、GTK/AppIndicator 等依赖。每种架构在对应原生环境构建。

```powershell
npm ci
# 前端浏览器预览：明确显示桌面环境未连接，不生成假项目
npm run dev

# 跨平台检查与打包（五种目标系统/架构）
node scripts/build-desktop.mjs Check
node scripts/build-desktop.mjs Bundle

# 生成连接器、检查前端构建、执行 Rust 测试和 Clippy
./scripts/build-desktop.ps1 -Mode Check

# 使用真实 CodeGraph 样例运行双项目 / 双客户端 MCP 测试
./scripts/build-desktop.ps1 -Mode Mcp

# 启动实际桌面应用；先生成随应用使用的连接器
./scripts/build-desktop.ps1 -Mode Dev

# 构建 Windows NSIS 安装包，自动包含连接器
./scripts/build-desktop.ps1 -Mode Bundle
```

原始包位于 `target/release/bundle/`；标准化安装包与校验文件位于 `target/release-assets/<系统>-<架构>/`。CI 配置位于 `.github/workflows/desktop.yml`。

`Mcp` 模式默认使用当前用户 npm 安装的 CodeGraph；其他位置可通过 `-CodeGraphBundle` 和 `-CodeGraphEntry` 指定。样例项目创建在临时目录，测试不修改用户项目。这个模式使用模拟 MCP 客户端，不替代 Codex / Claude Code 的真实客户端联调。

真实客户端联调脚本及模型服务限制见 [客户端测试说明](tests/clients/README.md)。

直接调用 `cargo test --workspace` 或 `npm run tauri` 前，需先生成 Tauri 所需的连接器 sidecar；统一脚本会处理其目标平台命名。构建脚本仅检查开发依赖，不自动安装系统工具。

## 源码结构

- `src/`：中文工作台、项目管理、配置预览、日志与设置。
- `src-tauri/`：Tauri IPC、SQLite、配置文件管理、索引任务与应用生命周期。
- `crates/project-protocol/`：私有运行记录及共享协议数据。
- `crates/project-gateway/`：CodeGraph 入口适配、进程管理和多会话 MCP 网关。
- `crates/cg-mcp-connector/`：客户端 stdio 到项目网关的连接器。
- `tests/m0/`：真实上游技术验证脚本及证据。
- `scripts/`：Windows 检查、开发与打包入口。

## 设计文档

- [产品需求与范围](docs/01-product-requirements.md)：用户流程、项目隔离约束、首版范围与验收目标。
- [系统设计](docs/02-system-design.md)：进程架构、MCP 桥接、数据模型、配置写入和运行生命周期。
- [UI 设计](docs/03-ui-design.md)：导航、页面线框、视觉规范、交互及异常状态。
- [实施与验收计划](docs/04-implementation-plan.md)：技术验证、开发顺序、测试场景及发布条件。

## 核心约定

1. 一个已启动项目对应一个受管 CodeGraph 服务实例；一个实例可包含上游自身的工作进程。
2. 不同项目不共享运行实例、索引数据库或请求队列。
3. 同一项目的 Codex 与 Claude Code 共用该项目实例，不因新增客户端连接而启动第二个 CodeGraph。
4. CodeGraph 可共用一份安装；“实例独立”不等于“每个项目重复安装”。
5. MCP 配置分别写入项目的 `.codex/config.toml` 和 `.mcp.json`，不修改全局客户端配置。
6. 默认仅本机访问。程序退出时停止受管服务；关闭窗口默认保留托盘运行。

## 阅读说明

文档更新：2026-10-04。发布矩阵包含上述五种原生目标，具体构建结果以 Actions 为准。首版验证基线为当前机器上的 CodeGraph 1.6.2，不将此版本视为永久最新版本。

图中的项目名称、UUID、端口和路径均为示例，不代表已经创建或启动的实例。
