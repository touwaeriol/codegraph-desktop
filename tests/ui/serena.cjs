// Browser integration with mocked Tauri IPC. Native HTTP is covered separately.
const { chromium } = require(
  process.env.PLAYWRIGHT_MODULE_PATH || "playwright",
);
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  try {
    for (const language of ["en", "zh-CN"]) {
      const page = await browser.newPage({
        viewport: { width: 1100, height: 780 },
        locale: language,
      });
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      await page.addInitScript((language) => {
        window.isTauri = true;
        let seq = 0;
        let running = false;
        const project = {
          id: "fixture",
          name: "codegraph-desktop",
          rootPath: "D:\\projects\\codegraph-desktop",
          canonicalPath: "D:\\projects\\codegraph-desktop",
          previousRoots: [],
          notes: "",
          autoStart: false,
          createdAt: "2026-10-04",
          updatedAt: "2026-10-04",
        };
        window.__calls = [];
        window.__serenaAvailable = false;
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        window.__TAURI_INTERNALS__ = {
          transformCallback() {
            return ++seq;
          },
          unregisterCallback() {},
          async invoke(command, args = {}) {
            window.__calls.push({ command, args });
            if (command.startsWith("plugin:event|")) return ++seq;
            if (command === "list_projects") return [project];
            if (command === "get_settings")
              return {
                language,
                codegraphEntry: null,
                serenaEntry: null,
                indexConcurrency: 2,
                closeBehavior: "tray",
                appDataDir: "D:\\app-data",
              };
            if (command === "detect_codegraph")
              return {
                available: false,
                entry: null,
                version: null,
                error: "CODEGRAPH_UNAVAILABLE_FIXTURE",
              };
            if (command === "get_project_snapshot")
              return {
                projectId: "fixture",
                state: "stopped",
                sequence: ++seq,
                generation: "fixture",
                indexState: "error",
                sessions: 0,
                error: {
                  code: "CLI_NOT_FOUND",
                  message: "CODEGRAPH_UNAVAILABLE_FIXTURE",
                },
              };
            if (command === "list_serena_snapshots")
              return {
                fixture: {
                  state: running ? "running" : "stopped",
                  pid: running ? 21340 : null,
                  endpoint: running ? "http://127.0.0.1:58310/mcp" : null,
                  startedAt: running ? "2026-10-04T06:00:00Z" : null,
                  tools: running
                    ? [
                        "find_symbol",
                        "find_referencing_symbols",
                        "get_symbols_overview",
                        "replace_symbol_body",
                      ]
                    : [],
                  error: null,
                },
              };
            if (command === "serena_operation") {
              running = args.action !== "stop";
              return "serena-op";
            }
            if (command === "get_project_tasks")
              return [
                {
                  operationId: "serena-op",
                  projectId: "fixture",
                  state: "completed",
                  kind: "serena-start",
                  startedAt: "2026-10-04T06:00:00Z",
                },
              ];
            if (command === "get_client_config_status")
              return ["codex", "claude"].map((client) => ({
                client,
                state: "missing",
                path: client === "codex" ? ".codex/config.toml" : ".mcp.json",
                message: null,
              }));
            if (["read_logs", "list_config_backups"].includes(command))
              return [];
            if (command === "test_project_mcp")
              return {
                success: true,
                tools: ["find_symbol"],
                message: "SERENA_HANDSHAKE_OK",
                checkedAt: "2026-10-04",
              };
            if (command === "preview_client_config")
              return {
                previewId: "preview",
                projectId: "fixture",
                serviceName: args.engine,
                files: [
                  {
                    client: "codex",
                    path: ".codex/config.toml",
                    before:
                      '[mcp_servers.codegraph]\nurl="http://127.0.0.1:1234/mcp/fixture"\n',
                    after:
                      '[mcp_servers.codegraph]\nurl="http://127.0.0.1:1234/mcp/fixture"\n[mcp_servers.serena]\nurl="http://127.0.0.1:58310/mcp"\n',
                    existed: true,
                    conflict: false,
                  },
                ],
              };
            if (command === "apply_client_config")
              return {
                operationId: "applied",
                backupPath: "D:\\backup",
                files: [
                  {
                    path: ".codex/config.toml",
                    status: "success",
                    message: "",
                  },
                ],
              };
            if (command === "detect_serena")
              return window.__serenaAvailable
                ? {
                    available: true,
                    entry: "C:\\tools\\serena.exe",
                    version: "Serena 1.7.0",
                    error: null,
                  }
                : {
                    available: false,
                    entry: null,
                    version: null,
                    error: "SERENA_MISSING_FIXTURE",
                  };
            throw new Error("Unexpected IPC " + command);
          },
        };
      }, language);
      await page.goto(process.env.UI_TEST_URL || "http://127.0.0.1:1420");
      const zh = language === "zh-CN";
      await page
        .getByRole("button", { name: /Serena.*(Symbols|符号)/ })
        .click();
      await page.getByTestId("serena-panel").waitFor();
      assert.equal(
        await page
          .getByText("CODEGRAPH_UNAVAILABLE_FIXTURE", { exact: true })
          .count(),
        0,
      );
      await page
        .getByRole("button", {
          name: zh ? "启动 Serena" : "Start Serena",
          exact: true,
        })
        .click();
      await page.getByText("21340", { exact: true }).waitFor();
      await page
        .getByRole("button", {
          name: zh ? "停止 Serena" : "Stop Serena",
          exact: true,
        })
        .waitFor();
      await page.screenshot({
        path: `.tools/serena-overview-${language}.png`,
        fullPage: true,
      });
      await page
        .getByRole("tab", {
          name: zh ? "MCP 配置" : "MCP configuration",
          exact: true,
        })
        .click();
      await page
        .getByRole("button", {
          name: zh ? "测试连接" : "Test connection",
          exact: true,
        })
        .click();
      await page.getByText("SERENA_HANDSHAKE_OK").waitFor();
      await page
        .getByRole("button", {
          name: zh ? "预览并配置" : "Preview configuration",
          exact: true,
        })
        .click();
      await page.getByRole("dialog").waitFor();
      assert(
        await page
          .getByRole("dialog")
          .textContent()
          .then((s) => s.includes("serena")),
      );
      const calls = await page.evaluate(() => window.__calls);
      for (const name of ["test_project_mcp", "preview_client_config"])
        assert.equal(
          calls.find((c) => c.command === name).args.engine,
          "serena",
        );
      await page
        .getByRole("button", { name: zh ? "取消" : "Cancel", exact: true })
        .click();
      await page.keyboard.press("Control+,");
      await page.getByTestId("serena-settings").waitFor();
      await page
        .getByRole("button", {
          name: zh ? "检测 Serena 并使用" : "Detect and use Serena",
          exact: true,
        })
        .click();
      await page.getByText("SERENA_MISSING_FIXTURE", { exact: true }).waitFor();
      assert.equal(
        await page
          .getByText(zh ? "Serena 检测成功" : "Serena detected successfully", {
            exact: true,
          })
          .count(),
        0,
      );
      await page.evaluate(() => (window.__serenaAvailable = true));
      await page
        .getByRole("button", {
          name: zh ? "检测 Serena 并使用" : "Detect and use Serena",
          exact: true,
        })
        .click();
      await page.getByText("Serena 1.7.0", { exact: true }).waitFor();
      assert.equal(
        await page
          .getByLabel(zh ? "Serena 入口" : "Serena executable", { exact: true })
          .inputValue(),
        "C:\\tools\\serena.exe",
      );
      await page.getByTestId("serena-settings").scrollIntoViewIfNeeded();
      await page.screenshot({
        path: `.tools/serena-settings-${language}.png`,
        fullPage: true,
      });
      assert.deepEqual(errors, []);
      await page.close();
    }
    console.log(
      "Serena UI: bilingual engine selection, independent startup, configuration targeting, failure/success detection passed",
    );
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
