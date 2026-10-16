# CodeGraph Desktop

[English](README.md) | **简体中文**

基于 **Tauri 2 + React + TypeScript + Vite** 的项目 MCP 管理桌面应用，支持 CodeGraph、Serena、WarpGrep 与 REA。

每个项目独立启动 CodeGraph，使用独立索引、运行状态、日志和鉴权令牌。一个本地 HTTP 服务通过 `/mcp/{项目ID}` 路由到对应实例，Codex 与 Claude Code 仍通过项目级 MCP 配置连接。

## 当前阶段

源码包含 React 界面、Tauri/Rust 后端、项目网关与旧配置兼容连接器。构建、协议及安装回归的范围见 [验证记录](docs/05-validation.md)。首次客户端信任、原生窗口交互与干净机器安装仍需单独验收。

[GitHub 公共仓库](https://github.com/touwaeriol/codegraph-desktop) · [下载 Release](https://github.com/touwaeriol/codegraph-desktop/releases) · [Actions 构建](https://github.com/touwaeriol/codegraph-desktop/actions)

| 系统 | 架构 | 发布格式 |
| --- | --- | --- |
| Windows | AMD64、ARM64 | NSIS `.exe` |
| macOS | ARM64（Apple Silicon） | `.dmg`、`.pkg` |
| Linux | AMD64、ARM64 | `.deb`、`.AppImage` |

Release 只有在五个原生构建与检查全部成功后才发布，每个平台提供 SHA256 校验文件。macOS `.app` 使用 ad-hoc 签名保证完整性，尚无 Apple Developer ID 签名或公证；Windows 安装包和 macOS PKG 安装器未配置开发者证书签名。

从 0.1.25 起，发布前会检查 `.app`、DMG / PKG 内的应用和更新安装后的应用签名，修复旧包缺少资源签名造成的“已损坏”提示。ad-hoc 签名不等同于 Apple 的发行认证，首次下载仍可能要求在 **系统设置 → 隐私与安全性 → 仍要打开** 中确认。请从官方 Release 获取安装包；不建议关闭系统的全局安全检查。

Windows 安装器检测到已有安装后直接覆盖原目录，不显示“先卸载旧版本”的维护选项。主程序和连接器一起替换，Windows 设置中的独立卸载入口仍保留。已通过独立测试身份的静默覆盖升级及卸载回归，未操作用户的正式安装。

更新前会先请求应用停止受管 CodeGraph 和索引任务，再退出并释放程序文件。Windows 对不支持新退出协议的旧版本保留精确路径进程关闭兼容；Linux DEB 和 macOS PKG 会等待标准安装路径的应用退出，超时则停止安装。DMG 拖拽与 AppImage 文件替换没有安装前钩子，使用这两种格式时应先从托盘退出应用。不会按名称全局终止其他项目的 Node/CodeGraph。

## 应用内更新

从 **0.1.24** 起，应用默认在启动 5 秒后、以及每 6 小时检查 GitHub Release 的正式版本。发现更新时在项目页提示；**设置 → 软件更新** 提供手动检查、更新说明、下载进度、取消下载和自动检查开关。

点击“下载并安装”并确认后，应用先下载并验证更新包签名和签名中的版本号，再停止受管实例、替换应用并重启。项目与设置保留，重启后恢复此前运行的引擎。网络故障或签名不匹配不会触发安装；安装失败会显示错误，可重试或手动安装。

- Windows AMD64 / ARM64 使用 NSIS 覆盖升级；macOS Apple Silicon 替换已安装的 `.app`；Linux AppImage 原位更新。
- Linux DEB 支持检测和提醒，安装需通过系统包管理器完成。开发模式禁用应用内安装，避免替换开发环境。
- **0.1.23 及更早版本需手动安装一次 0.1.24 或更新版本**，之后即可使用应用内更新。

发布流程生成签名包、`.sig` 和 `latest.json`，所有平台构建与校验完成后一起公开。更新包签名、macOS ad-hoc 完整性签名和 Apple Developer ID / 公证是不同机制，当前未配置发行证书与公证。GitHub Actions 使用仓库密钥 `TAURI_SIGNING_PRIVATE_KEY`；本地打包需设置同名环境变量为私钥文件路径。私钥不得提交到仓库，需妥善备份；已安装版本依赖内置公钥验证未来的更新。

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

## 项目 MCP 自动配置

在项目的 **MCP 配置** 中，先在列表顶部切换 **Codex** 或 **Claude Code**，再打开所需 MCP 的开关。每行只显示状态圆点、名称和开关，顶部显示已配置数量。开启后自动合并写入当前客户端的项目配置，关闭后自动移除对应受管条目，无需点击预览或应用按钮。悬停行可查看说明，行内更多菜单提供连接测试和配置检查。

列表从实际配置文件读取状态，切换引擎或重新打开项目后会保留；保存时显示“保存中”，失败后重新读取文件并显示实际状态。每次更改沿用备份、外部修改检测和写入失败回滚机制，保留其他 MCP 与设置。同名条目不受本应用管理或已被手动修改时，自动操作不会覆盖或删除它，可点击“检查配置”或展开 **高级配置** 处理冲突、手动编辑和预览。

勾选只负责客户端配置；CodeGraph、Serena 与 WarpGrep 仍需启动实例，REA 由客户端启动。配置更新后请重新加载客户端并按客户端要求批准 MCP。

左侧项目名称旁以单行彩色缩写标记 MCP：C（CodeGraph）、S（Serena）、W（WarpGrep）、R（REA）。受管引擎运行时标记填充颜色，停止时淡化，异常时显示红色边框；悬停可查看完整名称与状态。REA 标记表示由客户端管理，不代表已建立连接。

## Serena（可选引擎）

CodeGraph 与 Serena 使用相同的工作台布局和页头操作。设置页将两个引擎作为同级卡片：宽窗口并排、窄窗口纵向排列，通用偏好单独放置。窗口支持拖动调整大小，最小尺寸为 800 × 560。

项目工作台增加 CodeGraph / Serena 引擎切换。Serena 提供符号搜索、引用分析和语义编辑，无需先建立 CodeGraph 索引；侧栏分别显示两个引擎的运行状态。

1. 安装 [uv](https://docs.astral.sh/uv/getting-started/installation/)，执行 `uv tool install -p 3.13 serena-agent`，详见 [Serena 官方安装说明](https://oraios.github.io/serena/02-usage/010_installation.html)。
2. 在“设置 → Serena 入口”中检测或手动选择可执行文件；检测通过后才保存入口。
3. 选择项目和 Serena，点击“启动实例”。首次使用可能下载语言服务器依赖，失败时查看运行日志；各编程语言所需依赖由 Serena 管理。
4. 在“MCP 配置”顶部选择 Codex / Claude Code，打开 Serena 开关，项目配置自动保存；也可分别启用 CodeGraph 等其他 MCP。只修改项目的 `.codex/config.toml` / `.mcp.json`。

Serena 通过 `--context ide --project <绝对路径>` 单项目模式启动原生 HTTP，每个项目使用固定独立端口 `http://127.0.0.1:<端口>/mcp`，客户端直接连接。启动检查要求存在符号工具且没有切换项目工具。端口冲突报错，不自动切换。原生 Serena 不使用 CodeGraph 的令牌，仅供本机可信客户端使用；单项目模式不是操作系统沙箱。

停止 Serena 不影响 CodeGraph 或其他项目。退出应用、安装器请求退出、删除/迁移项目和“全部停止”都会回收受管 Serena 进程树。引擎运行状态会跨重启保持：桌面退出时（包括安装器更新，更新后会自动重新打开应用）正在运行的 CodeGraph、Serena 与 WarpGrep 实例，下次启动时自动恢复，记录保存在应用数据目录的 `engine-resume.json`。手动停止或启动失败的引擎会从记录中移除；项目“自动启动”设置仍用于 CodeGraph。安装包不内置 Serena，需要自行安装其入口。Serena 不会自行安装的语言服务器由桌面在启动前或检测到启动失败后自动安装（目前支持 Go 的 `gopls`），装入应用数据目录并置于 Serena 的 PATH 首位；未设置 `HTTPS_PROXY` 时使用系统代理。暂无安装方案的语言服务器会在 Serena 面板直接显示失败原因。已在 Windows 使用 Serena 1.7.0 验证原生 HTTP 和符号查询；macOS/Linux 的 Serena 运行验收与跨平台编译分别进行。

## WarpGrep（可选引擎）

[Morph WarpGrep](https://docs.morphllm.com/sdk/components/warp-grep) 是自然语言代码检索子代理：`morph-warp-grep-v2.1` 模型规划多轮 grep、列目录和读取，最终返回相关文件与行段。桌面应用在项目网关内原生实现 WarpGrep 协议，不依赖 Node 包或 `npx` 进程。

1. 在 [Morph 控制台](https://morphllm.com/dashboard/api-keys) 创建 API Key。
2. 在“设置 → WarpGrep”中粘贴 Key，点击“验证并保存”。保存前会真实调用一次 WarpGrep 模型（约 1K tokens），成功才保存；Key 无效、额度不足、限流或网络失败都会提示且不保存。设置页只显示脱敏提示。“移除”会删除 Key 并停止所有运行中的 WarpGrep 路由。
3. 选择项目和 WarpGrep，点击“启动实例”。未验证 Key 时拒绝启动；无需 CodeGraph 索引。
4. 在“MCP 配置”顶部选择客户端，打开 WarpGrep 开关（验证 Key 后可用），配置自动保存。客户端获得 `warpgrep` 条目，地址为 `http://127.0.0.1:<共享端口>/warpgrep/<项目ID>`，使用项目鉴权令牌。

路由只暴露一个只读工具 `codebase_search`（参数 `search_string`；可选 `includeSource`，默认 false，仅返回文件路径与行段，设为 true 时附带对应代码；可选 `maxChars`，默认 12000）。模型发起的 grep/读取/列目录/glob 均在本机、且只在绑定的项目根目录内执行：绝对路径、`..` 越界及指向项目外的符号链接会被拒绝，常见依赖/构建目录、隐藏文件、嵌套仓库与 git worktree、二进制和超过 2 MB 的文件会跳过。每次检索最多 6 轮、100 秒（低于 Codex 默认 120 秒的工具超时），每个项目最多 4 个并发检索；单次模型请求超过 20 秒或临时失败时自动重试一次。每轮的模型耗时和工具调用都写入项目运行日志。

Morph API Key 保存在仅当前用户可访问的应用数据目录中的应用数据库内，不写入项目配置文件。**检索过程中读取的代码片段会发送至 Morph API**，启用前请确认符合各项目的代码外发要求。停止 WarpGrep、删除/迁移项目、“全部停止”和退出应用都会注销路由。

## REA（可选引擎）

[REA（Reverse Engineer Anything）](https://github.com/morluto/rea) 提供应用、JavaScript / Electron、.NET 和原生二进制分析。本应用按官方 stdio 协议注册 REA，由 Codex / Claude Code 启动和管理进程。

1. 准备 Node.js 22.x（≥22.19）、24.x（≥24.11）或 26+，并确保客户端和桌面应用的 PATH 中有 npm / npx。不支持 Node.js 23、25 和预发布版本。
2. 选择项目和 **REA**，进入 **MCP 配置**。列表显示当前项目各客户端的实际配置状态。
3. 在列表顶部选择 Codex / Claude Code，打开 REA 开关，项目配置自动保存，然后重新加载客户端并批准 MCP。使用固定版本 `npx -y rea-agents@6.5.0 mcp`；Windows 使用 `cmd.exe /d /c npx …`。首次启动需要联网下载包，Codex 配置允许 120 秒启动和 600 秒工具调用。
4. 可直接点击 **测试连接**，无需启动 CodeGraph 或建立索引。测试临时启动 REA，执行 MCP 握手和工具发现，完成、失败、超时或退出桌面时均清理测试进程。测试通过仅证明 MCP 可连接，具体分析引擎需按任务另行验证。

REA 的客户端进程不受桌面的“全部停止”和退出操作影响。项目级配置决定哪些客户端项目发现 REA，不限制工具的文件访问范围。JavaScript / Electron 与 .NET 静态分析无需原生反编译器；深度原生分析需按 [REA 官方文档](https://github.com/morluto/rea) 配置 Hopper、Ghidra 或 IDA 等工具及许可证。配套技能可按官方说明另行安装；本应用只注册 MCP，不执行 REA 的全局 `setup`。

REA 配置使用已有的预览、合并、备份、回滚和受管条目移除流程，只修改项目的 `.codex/config.toml` / `.mcp.json`。手动更换 REA 的启动命令或参数后，该条目不再视为当前受管配置。

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
