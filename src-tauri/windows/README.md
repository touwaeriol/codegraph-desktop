# Windows installer

`installer.nsi` is based on the Tauri CLI 2.12.1 template:
https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi

The upstream template is dual-licensed under Apache-2.0 or MIT; the original license texts are retained in `licenses/`.

Changes: remove the maintenance page and its old-uninstaller invocation; reuse the registered installation directory without prompting during upgrades; retain version comparison for downgrade policy, application-running checks, file replacement, uninstaller generation, and Windows uninstall registration.

This product ships NSIS only. MSI migration is not provided by this template. Review upstream changes when upgrading Tauri CLI. The independent uninstaller remains available from Windows Settings and the installation directory. Do not modify the generated `target/release/nsis/x64/installer.nsi`.

Before installation, temporarily use passive process-closing behavior for the main executable and `cg-mcp-connector.exe`, then restore the installer's original mode. Restart Manager targets their exact installed file paths, so other directories' same-name executables are not selected. Uninstall also checks the connector lock. Test with `tests/windows/in-place-upgrade.ps1 -FixtureManifest <isolated manifest> -TestLockedConnector`; it holds both installed executables open and keeps another directory's same-name connector running to verify scope.

更新前退出顺序（0.1.4）：安装与卸载均先仅向 `$INSTDIR` 内的主程序发送 `--shutdown-for-update`，异步请求不代表已退出。Restart Manager 按完整文件路径轮询最多 40 秒，确认原进程离开后才清理旧连接器和写入文件；旧版本或无响应程序超时后沿用精确路径 RM 回退，不按进程名称停止 Node/CodeGraph。

`tests/windows/graceful-upgrade.ps1` 会直接提取生产模板宏并编译独立 NSIS 回归。合作测试主程序启动工作进程，收到更新请求后依次停止工作进程、退出；测试验证更新写入发生在其后，并验证另一目录的同名应用不受影响。这证明安装宏顺序，不替代真实桌面任务取消与网关停止测试。

Linux/macOS 包升级使用 `scripts/install/stop-before-upgrade.sh`，向标准安装路径的精确主进程发送 SIGTERM 并等待 40 秒，超时拒绝安装。macOS DMG 手工拖拽没有安装前钩子，须先退出应用；非标准安装位置不自动匹配。
