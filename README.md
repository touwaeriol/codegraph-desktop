# CodeGraph Desktop

**English** | [简体中文](README.zh-CN.md)

A desktop project manager for CodeGraph and Serena, built with **Tauri 2 + React + TypeScript + Vite**.

Each project runs its own CodeGraph instance with a separate index, runtime state, logs, and authentication token. One local HTTP service routes `/mcp/{project-id}` to the corresponding instance. Codex and Claude Code use project-level MCP configuration.

[GitHub repository](https://github.com/touwaeriol/codegraph-desktop) · [Download releases](https://github.com/touwaeriol/codegraph-desktop/releases) · [GitHub Actions](https://github.com/touwaeriol/codegraph-desktop/actions)

## Supported platforms

| Platform | Architecture | Packages |
| --- | --- | --- |
| Windows | AMD64, ARM64 | NSIS `.exe` |
| macOS | ARM64 (Apple Silicon) | `.dmg`, `.pkg` |
| Linux | AMD64, ARM64 | `.deb`, `.AppImage` |

A release is published only after all five native builds and checks succeed. Each platform includes a SHA-256 checksum file. Windows code signing and Apple signing/notarization are not currently configured.

The repository includes the React interface, Tauri/Rust backend, project gateway, and a compatibility connector for older configurations. See the [validation record (Chinese)](docs/05-validation.md) for completed checks and remaining acceptance work. Initial client trust, native window interactions, and installation on clean machines require separate acceptance testing.

## Installation and upgrades

Download the package for your system and architecture from [Releases](https://github.com/touwaeriol/codegraph-desktop/releases).

On Windows, an existing installation is updated in the same directory without uninstalling the previous version first. The application and connector are replaced together. A separate uninstall entry remains available in Windows Settings. Silent in-place upgrade and uninstall regression checks use an isolated test installation.

Before an update, the application is asked to stop managed CodeGraph processes and indexing tasks, then exit and release its executable files. Windows retains an exact-path process shutdown fallback for older versions. Linux DEB and macOS PKG installers wait for the application at the standard installation path to exit, and abort on timeout. DMG drag-and-drop installation and manual AppImage replacement have no pre-install hook: quit the application from its tray menu before replacing those files. Other projects' Node/CodeGraph processes are not globally terminated by name.

These are installer upgrade flows. In-app automatic update downloads are not yet implemented.

## Language

On first launch, the application uses the system's preferred display language: Chinese locales use Simplified Chinese; other languages fall back to English. Open **Settings → Display language**, choose **English** or **简体中文**, and save your preferences. The interface and tray menu update immediately, and your choice is retained across restarts.

The Windows installer automatically uses Simplified Chinese on Chinese-language systems and English otherwise, without a language-selection dialog. The macOS/Linux system installer interface follows the operating system's own language settings. Project names, paths, configuration contents, and raw CodeGraph output are preserved in their original language.

## MCP connections and configuration editing

- Project MCP services use the name `codegraph`. Codex and Claude Code connect directly to the local project gateway over Streamable HTTP. The gateway uses CodeGraph's native stdio transport upstream.
- The application persists one shared loopback port; each project keeps its own stable ID, authentication token, and MCP sessions. Endpoints use `http://127.0.0.1:<port>/mcp/<project-id>`. An occupied port causes an explicit error instead of silently changing ports. Start the project in the desktop application before connecting a client. Stopping one project leaves the shared service and other projects running.
- Existing application-managed HTTP configurations are backed up and migrated on startup when their ownership fingerprints still match. Manually modified entries are preserved and can be reviewed through **Preview and configure**. Global client configuration is not changed.
- **Settings** opens in the main workspace beside the project sidebar. Select a project to return to its workspace.
- To migrate an older configuration, open **Preview and configure** again and review the change from command/args to a URL and authentication header. The compatibility connector remains bundled, but new configurations do not reference it.
- For each file in the preview, choose to merge settings, overwrite the entire file, or edit its complete contents. Overwriting removes other settings in that file; review the diff before applying it.
- After editing, refresh the preview. Syntax validation must pass before applying changes. Refreshing after an external editor changes the file preserves your draft in the preview window.
- If the original configuration cannot be parsed, open a preview in overwrite mode, then switch to editing to repair it. Opening the preview does not write the file.
- Applying a configuration backs up the original files and checks for external changes. Manually changing or removing the `codegraph` URL or authentication fields may disconnect that entry from the project and remove its managed status. Configuration contains local authentication credentials: do not upload or share these tokens.

## Serena (optional)

Choose **Serena** in a project's engine switcher to manage semantic symbol search, references and editing separately from CodeGraph. It does not require a CodeGraph index. The sidebar reports both engines independently.

1. Install [uv](https://docs.astral.sh/uv/getting-started/installation/) and run `uv tool install -p 3.13 serena-agent`, following the [official Serena installation guide](https://oraios.github.io/serena/02-usage/010_installation.html).
2. In **Settings → Serena executable**, detect the executable or select it manually. Detection validates the CLI before saving it.
3. Select a project, choose **Serena**, and click **Start Serena**. First use may download language-server dependencies; inspect **Logs** if startup or tools fail. Language-specific dependencies remain Serena's responsibility.
4. Under **MCP configuration**, preview and apply the separate `serena` entry. Default merging preserves `codegraph` and unrelated settings. The same backup, edit, overwrite and external-change checks apply.

Serena runs its native Streamable HTTP server at `http://127.0.0.1:<project-port>/mcp` in `--context ide --project <absolute-path>` single-project mode. Each project has a stable independent port; an occupied port fails instead of switching. Clients connect directly, preserving Serena's tools and instructions. Startup checks for symbol tools and rejects servers that expose project switching. Native Serena endpoints do not use CodeGraph bearer tokens and are intended for trusted local clients. Single-project mode is not an OS sandbox.

Stopping Serena leaves CodeGraph and other projects running. Application exit, installer-requested shutdown, project removal/relocation, and **Stop all** also stop owned Serena process trees. Existing project autostart applies to CodeGraph; Serena is started explicitly. Binaries are not bundled: an installed Serena executable is required. Windows native HTTP and symbol queries were tested with Serena 1.7.0; macOS/Linux Serena runtime acceptance remains separate from cross-platform compilation.

## Local development

Prerequisites: Node.js 22+, Rust stable, and a local CodeGraph installation. Install the [Tauri native build prerequisites](https://v2.tauri.app/start/prerequisites/) for your system: MSVC, Windows SDK, and WebView2 on Windows; Xcode Command Line Tools on macOS; WebKitGTK 4.1, GTK, and AppIndicator dependencies on Linux. Each architecture is built on its native environment.

```sh
npm ci

# Browser preview shows that the desktop backend is disconnected;
# it does not generate fake projects.
npm run dev

# Check or package any of the five supported native targets.
node scripts/build-desktop.mjs Check
node scripts/build-desktop.mjs Bundle

# Launch the desktop application, preparing its connector first.
node scripts/build-desktop.mjs Dev
```

Additional Windows PowerShell entry points:

```powershell
# Build the connector, check the frontend, and run Rust tests and Clippy.
./scripts/build-desktop.ps1 -Mode Check

# Test two projects and two MCP clients against a real CodeGraph instance.
./scripts/build-desktop.ps1 -Mode Mcp

# Launch the desktop application.
./scripts/build-desktop.ps1 -Mode Dev

# Build the Windows NSIS installer, including the connector.
./scripts/build-desktop.ps1 -Mode Bundle
```

Raw packages are written to `target/release/bundle/`. Standardized packages and checksum files are written to `target/release-assets/<platform>-<architecture>/`. The CI workflow is `.github/workflows/desktop.yml`.

The `Mcp` mode uses the current user's npm-installed CodeGraph by default. Override its location with `-CodeGraphBundle` and `-CodeGraphEntry`. Sample projects are created in temporary directories; user projects are not modified. This mode uses simulated MCP clients and does not replace integration testing with actual Codex or Claude Code clients. See the [client testing guide (Chinese)](tests/clients/README.md) for real-client scripts and model-service limitations.

Before invoking `cargo test --workspace` or `npm run tauri` directly, generate the connector sidecar required by Tauri. The build scripts handle its platform-specific filename. They check development dependencies but do not install system tools automatically.

## Source layout

- `src/`: workspace UI, project management, configuration previews, logs, and settings.
- `src-tauri/`: Tauri IPC, SQLite persistence, configuration management, indexing tasks, and application lifecycle.
- `crates/project-protocol/`: private runtime records and shared protocol data.
- `crates/project-gateway/`: CodeGraph entry-point resolution, process management, and the multi-session MCP gateway.
- `crates/cg-mcp-connector/`: connector from client stdio to the project gateway.
- `tests/m0/`: technical validation scripts for the real upstream service.
- `scripts/`: native build and release scripts, plus Windows development and testing entry points.

## Design and release documentation

Detailed engineering documents are currently in Chinese:

- [Product requirements and scope](docs/01-product-requirements.md): workflows, project isolation, initial scope, and acceptance criteria.
- [System design](docs/02-system-design.md): process architecture, MCP bridging, data model, configuration writes, and runtime lifecycle.
- [UI design](docs/03-ui-design.md): navigation, page wireframes, visual rules, interactions, and error states.
- [Implementation and acceptance plan](docs/04-implementation-plan.md): technical validation, implementation order, test scenarios, and release requirements.
- [Validation record](docs/05-validation.md): completed checks and remaining acceptance work.
- [Release guide](docs/06-release.md): version tags, GitHub Actions, package upgrades, and files excluded from the public repository.

## Core conventions

1. Each started project has one managed CodeGraph service instance, which may include CodeGraph's own worker processes.
2. Projects do not share running instances, index databases, or request queues.
3. Codex and Claude Code for the same project share its instance. Adding a client does not start a second CodeGraph instance.
4. Projects can share one CodeGraph installation; separate instances do not require repeated installations.
5. MCP settings are written to the project's `.codex/config.toml` and `.mcp.json`. Global client configuration is not modified.
6. Access is local-only by default. Exiting stops managed services; closing the window leaves the application running in the tray by default.

## Reading notes

Documentation updated on October 4, 2026. GitHub Actions provides the actual build results for the five native targets above. The initial validation baseline used CodeGraph 1.6.2 on the development machine; this is not a claim that it will remain the latest version.

Project names, UUIDs, ports, and paths in diagrams are examples, not evidence that those instances have been created or started.
