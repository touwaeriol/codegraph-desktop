# GitHub 发布流程

公共仓库：https://github.com/touwaeriol/codegraph-desktop

## 发布新版本

1. 同步 `package.json`、`package-lock.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`Cargo.lock` 以及界面中的版本号。
2. 运行 `node scripts/build-desktop.mjs Check`，提交修改并推送 `main`。
3. 创建与版本一致的标签，例如 `git tag -a v0.1.4 -m "Release v0.1.4"`，再运行 `git push origin v0.1.4`。
4. Actions 在五种原生环境中检查并打包；任何平台失败都不会发布完整 Release。全部成功后，独立发布任务校验各平台产物与 SHA256，并创建或完成该标签对应的 Release。

构建矩阵：Windows AMD64/ARM64、macOS ARM64、Linux AMD64/ARM64。主分支和 PR 执行检查；`v*` 标签执行打包与发布。工作流只在最终发布任务授予 `contents: write`，通过 GitHub 自动提供的 `GITHUB_TOKEN` 发布，无需把个人访问令牌放进仓库。

## 更新与安装

- Windows NSIS：先请求同一安装路径的旧应用执行 `--shutdown-for-update`，停止受管任务和 CodeGraph 后退出，再处理旧连接器占用并覆盖文件；旧版本通过 Restart Manager 兼容退出。保留独立卸载入口。
- Linux DEB：安装前仅向 `/usr/bin/codegraph-desktop` 对应进程发送 SIGTERM，由应用完成受管进程清理；等待退出后才解包，超时退出并报错。
- macOS PKG：安装前只处理 `/Applications/CodeGraph Desktop.app/Contents/MacOS/codegraph-desktop` 对应进程，等待退出后覆盖。
- DMG 拖拽和 AppImage 手动文件替换没有安装前执行脚本，先退出应用再替换。

这些是安装包更新流程；应用内自动下载更新尚未实现。当前 Windows 与 Apple 产物未签名/未公证。不同系统的数据目录由 Tauri 提供，项目 HTTP 地址与令牌保存在本机应用数据中。

## 不应提交的文件

本机 `.codex/`、`.mcp.json`、`.codegraph/`、环境变量文件、构建输出、调试日志与本地测试快照均忽略。项目 MCP 配置包含本机令牌，不能作为可共享模板上传。源码中的测试固定值是测试夹具；示例文档使用占位令牌。
