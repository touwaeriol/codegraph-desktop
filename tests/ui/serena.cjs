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
            if (command === "plugin:opener|open_url") {
              if (window.__openerError)
                throw new Error("BROWSER_OPEN_FAILED_FIXTURE");
              return;
            }
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
                available: location.search.includes("healthy"),
                entry: null,
                version: null,
                error: location.search.includes("healthy")
                  ? null
                  : "CODEGRAPH_UNAVAILABLE_FIXTURE",
              };
            if (command === "get_project_snapshot")
              return {
                projectId: "fixture",
                state: "stopped",
                sequence: ++seq,
                generation: "fixture",
                indexState: location.search.includes("healthy")
                  ? "ready"
                  : "error",
                sessions: 0,
                error: location.search.includes("healthy")
                  ? null
                  : {
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
                serviceName: args.engines.join(", "),
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
          name: zh ? "启动实例" : "Start instance",
          exact: true,
        })
        .click();
      await page.getByText("21340", { exact: true }).waitFor();
      await page
        .getByRole("button", {
          name: zh ? "停止实例" : "Stop instance",
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
      assert(
        await page
          .getByRole("checkbox", { name: "CodeGraph MCP", exact: true })
          .isChecked(),
      );
      assert(
        await page
          .getByRole("checkbox", { name: "Serena MCP", exact: true })
          .isChecked(),
      );
      await page
        .getByRole("checkbox", { name: "CodeGraph MCP", exact: true })
        .uncheck();
      await page
        .getByRole("button", {
          name: zh ? "测试连接" : "Test connection",
          exact: true,
        })
        .click();
      await page.getByText("SERENA_HANDSHAKE_OK").waitFor();
      await page
        .getByRole("checkbox", { name: "CodeGraph MCP", exact: true })
        .check();
      await page.screenshot({
        path: `.tools/mcp-batch-${language}.png`,
        fullPage: true,
      });
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
      assert.equal(
        calls.find((c) => c.command === "test_project_mcp").args.engine,
        "serena",
      );
      assert.deepEqual(
        calls
          .find((c) => c.command === "preview_client_config")
          .args.engines.slice()
          .sort(),
        ["codegraph", "serena"],
      );
      await page
        .getByRole("button", { name: zh ? "取消" : "Cancel", exact: true })
        .click();
      await page
        .getByRole("checkbox", { name: "Serena MCP", exact: true })
        .uncheck();
      await page
        .getByRole("button", {
          name: zh ? "预览并配置" : "Preview configuration",
          exact: true,
        })
        .click();
      await page.getByRole("dialog").waitFor();
      assert.deepEqual(
        await page.evaluate(
          () =>
            window.__calls
              .filter((c) => c.command === "preview_client_config")
              .at(-1).args.engines,
        ),
        ["codegraph"],
      );
      await page
        .getByRole("button", { name: zh ? "取消" : "Cancel", exact: true })
        .click();
      await page
        .getByRole("checkbox", { name: "CodeGraph MCP", exact: true })
        .uncheck();
      assert(
        await page
          .getByRole("button", {
            name: zh ? "预览并配置" : "Preview configuration",
            exact: true,
          })
          .isDisabled(),
      );
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
      await page
        .getByTestId("serena-settings")
        .getByText(/Serena 1\.7\.0/)
        .waitFor();
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
      await page.goto(
        (process.env.UI_TEST_URL || "http://127.0.0.1:1420") + "?healthy=1",
      );
      for (const width of [1280, 1000]) {
        await page.setViewportSize({ width, height: 820 });
        const positions = [];
        for (const engine of ["CodeGraph", "Serena"]) {
          await page
            .getByRole("group", { name: zh ? "项目引擎" : "Project engines" })
            .getByRole("button", { name: new RegExp(engine) })
            .click();
          await page.getByTestId("engine-runtime-card").waitFor();
          const action = await page.getByTestId("engine-actions").boundingBox();
          const card = await page
            .getByTestId("engine-runtime-card")
            .boundingBox();
          positions.push({ action, card });
          assert(
            await page
              .getByTestId("engine-actions")
              .getByRole("button", {
                name: zh ? "启动实例" : "Start instance",
                exact: true,
              })
              .isVisible(),
          );
          assert.equal(
            await page
              .locator(`[data-testid="${engine.toLowerCase()}-panel"]`)
              .getByRole("button", {
                name: zh ? "启动实例" : "Start instance",
                exact: true,
              })
              .count(),
            0,
          );
          assert(
            (await page.getByTestId("engine-connection-card").count()) === 1,
          );
          assert(
            await page.evaluate(
              () => document.documentElement.scrollWidth <= innerWidth,
            ),
          );
          await page.screenshot({
            path: `.tools/unified-${engine}-${language}-${width}.png`,
            fullPage: true,
          });
        }
        for (const key of ["x", "y", "width", "height"])
          assert(
            Math.abs(positions[0].action[key] - positions[1].action[key]) < 2,
            `Action ${key} differs`,
          );
        for (const key of ["x", "y", "width"])
          assert(
            Math.abs(positions[0].card[key] - positions[1].card[key]) < 2,
            `Runtime card ${key} differs`,
          );
      }
      await page.keyboard.press("Control+,");
      const cgSettings = page.getByTestId("codegraph-settings"),
        srSettings = page.getByTestId("serena-settings");
      const settingsUrl = page.url();
      for (const [card, url] of [
        [cgSettings, "https://github.com/colbymchenry/codegraph"],
        [
          srSettings,
          "https://oraios.github.io/serena/02-usage/010_installation.html",
        ],
      ]) {
        await card.locator("summary").click();
        const link = card.getByRole("link");
        for (const action of ["click", "keyboard", "middle", "control"]) {
          await page.evaluate(() => (window.__calls = []));
          if (action === "keyboard") {
            await link.focus();
            await page.keyboard.press("Enter");
          } else if (action === "middle") {
            await link.click({ button: "middle" });
          } else if (action === "control") {
            await link.click({ modifiers: ["Control"] });
          } else await link.click();
          assert.deepEqual(
            await page.evaluate(() =>
              window.__calls
                .filter((c) => c.command === "plugin:opener|open_url")
                .map((c) => c.args.url),
            ),
            [url],
          );
          assert.equal(page.url(), settingsUrl);
          assert.equal(page.context().pages().length, 1);
        }
        await page.evaluate(() => (window.__openerError = true));
        await link.click();
        await page
          .getByText("BROWSER_OPEN_FAILED_FIXTURE", { exact: true })
          .first()
          .waitFor();
        await page.evaluate(() => (window.__openerError = false));
        await card.locator("summary").click();
      }
      const cgInput = page.locator("#entry"),
        srInput = page.locator("#serena-entry");
      await cgInput.fill(
        "C:\\Users\\example\\a-long-installation-directory\\codegraph.cmd",
      );
      await srInput.fill(
        "C:\\Users\\example\\another-long-installation-directory\\serena.exe",
      );
      for (const [width, height] of [
        [1440, 900],
        [1280, 820],
        [1000, 680],
        [800, 560],
        [640, 448],
        [533, 373],
        [1280, 820],
      ]) {
        await page.setViewportSize({ width, height });
        await cgSettings.scrollIntoViewIfNeeded();
        const a = await cgSettings.boundingBox(),
          b = await srSettings.boundingBox();
        assert(
          Math.abs(a.width - b.width) < 2,
          "Engine settings widths differ",
        );
        if (width >= 1100) {
          assert(Math.abs(a.y - b.y) < 2, "Engine settings are not peers");
          assert(b.x > a.x, "Engine settings are not side-by-side");
        }
        if (width <= 800) {
          assert(Math.abs(a.x - b.x) < 2);
          assert(b.y > a.y, "Narrow settings did not stack");
        }
        assert(
          await page.evaluate(
            () =>
              document.documentElement.scrollWidth <= innerWidth &&
              document.querySelector("main").scrollWidth <=
                document.querySelector("main").clientWidth + 1,
          ),
          `Overflow at ${width}`,
        );
        assert((await cgInput.inputValue()).includes("a-long-installation"));
        assert(
          (await srInput.inputValue()).includes("another-long-installation"),
        );
        await page
          .getByRole("button", {
            name: zh ? "保存偏好" : "Save preferences",
            exact: true,
          })
          .scrollIntoViewIfNeeded();
        await cgSettings.scrollIntoViewIfNeeded();
        await page.locator("main").evaluate((el) => (el.scrollTop = 0));
        await page.screenshot({
          path: `.tools/settings-responsive-${language}-${width}.png`,
          fullPage: true,
        });
      }
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
