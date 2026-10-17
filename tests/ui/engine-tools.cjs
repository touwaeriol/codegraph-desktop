// Browser IPC regression for per-engine tool filtering; configuration writes
// are verified by Rust tests.
const { chromium } = require(
  process.env.PLAYWRIGHT_MODULE_PATH || "playwright",
);
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || "chromium",
    headless: true,
  });
  const errors = [];
  try {
    for (const language of ["zh-CN", "en"]) {
      const zh = language === "zh-CN";
      const context = await browser.newContext({
        locale: language,
        viewport: { width: 1180, height: 820 },
      });
      const page = await context.newPage();
      page.on("pageerror", (error) => errors.push(error.message));
      await page.addInitScript((language) => {
        window.isTauri = true;
        let sequence = 0;
        const project = {
          id: "fixture",
          name: "Fixture",
          rootPath: "D:\\fixture",
          canonicalPath: "D:\\fixture",
          previousRoots: [],
          notes: "",
          autoStart: false,
          createdAt: "2026-10-17",
          updatedAt: "2026-10-17",
        };
        const test = (window.__tools = {
          calls: [],
          known: {
            serena: ["find_symbol", "rename_symbol", "get_diagnostics"],
          },
          disabled: { serena: ["rename_symbol"] },
          refreshError: null,
        });
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        window.__TAURI_INTERNALS__ = {
          transformCallback() {
            return ++sequence;
          },
          unregisterCallback() {},
          async invoke(command, args = {}) {
            test.calls.push({ command, args });
            if (command.startsWith("plugin:event|")) return ++sequence;
            if (
              ["list_serena_snapshots", "list_warpgrep_snapshots"].includes(
                command,
              )
            )
              return {};
            if (command === "list_projects") return [project];
            if (command === "get_project_snapshot")
              return {
                projectId: project.id,
                generation: "fixture",
                sequence: ++sequence,
                state: "stopped",
                sessions: 0,
                indexState: "ready",
                error: null,
              };
            if (
              [
                "get_client_config_status",
                "read_logs",
                "get_project_tasks",
                "list_config_backups",
              ].includes(command)
            )
              return [];
            if (command === "get_update_snapshot")
              return {
                sequence: 0,
                status: "idle",
                currentVersion: "0.1.27",
                latestVersion: null,
                notes: null,
                checkedAt: null,
                autoCheck: false,
                canInstall: true,
                downloaded: 0,
                total: null,
                error: null,
              };
            if (command === "get_settings")
              return {
                language,
                codegraphEntry: null,
                serenaEntry: null,
                indexConcurrency: 2,
                closeBehavior: "tray",
                activeClient: "codex",
                appDataDir: "D:\\fixture-data",
              };
            if (command === "get_rea_environment") return {};
            if (command === "get_engine_tools")
              return { known: test.known, disabled: test.disabled };
            if (command === "refresh_engine_tools") {
              if (test.refreshError) throw test.refreshError;
              const tools = Array.from(
                { length: 12 },
                (_, i) => `rea_tool_${i}`,
              );
              test.known = { ...test.known, [args.engine]: tools };
              return tools;
            }
            if (command === "save_disabled_tools") {
              test.disabled = { ...test.disabled, [args.engine]: args.tools };
              return {
                disabled: [...args.tools].sort(),
                updated: ["Fixture · Codex", "Fixture · Claude Code"],
                failed: [],
              };
            }
            throw Error(`Unexpected command ${command}`);
          },
        };
      }, language);
      await page.goto(process.env.UI_TEST_URL || "http://127.0.0.1:1420");
      await page
        .getByRole("button", { name: zh ? /设置/ : /Settings/ })
        .first()
        .click();

      const serena = page.getByTestId("tools-serena");
      await serena
        .locator("summary", {
          hasText: zh ? "工具（已停用 1 / 共 3）" : "Tools (1 of 3 disabled)",
        })
        .click();
      const box = (tools, name) =>
        tools.getByRole("checkbox", { name, exact: true });
      assert.equal(await box(serena, "rename_symbol").isChecked(), false);
      assert.equal(await box(serena, "find_symbol").isChecked(), true);
      const saveSerena = serena.getByRole("button", {
        name: zh ? "保存工具设置" : "Save tool settings",
      });
      assert.equal(await saveSerena.isDisabled(), true);
      await box(serena, "rename_symbol").check();
      await box(serena, "get_diagnostics").uncheck();
      await saveSerena.click();
      await page
        .getByText(
          zh
            ? "工具设置已保存，并更新 2 个客户端配置；请在客户端重新加载 MCP"
            : "Tool settings saved and 2 client configurations updated. Reload MCP in the client",
        )
        .waitFor();
      assert.deepEqual(
        await page.evaluate(
          () =>
            window.__tools.calls
              .filter((call) => call.command === "save_disabled_tools")
              .at(-1).args,
        ),
        { engine: "serena", tools: ["get_diagnostics"] },
      );
      assert.equal(await saveSerena.isDisabled(), true);
      await serena
        .getByRole("button", { name: zh ? "全部启用" : "Enable all" })
        .click();
      assert.equal(await box(serena, "get_diagnostics").isChecked(), true);
      assert.equal(await saveSerena.isDisabled(), false);

      const rea = page.getByTestId("tools-rea");
      await rea.locator("summary").click();
      await rea
        .getByText(
          zh
            ? "获取时会临时启动 REA 读取工具列表，完成后结束进程。"
            : "Loading starts REA temporarily to read its tools, then stops it.",
        )
        .waitFor();
      const load = rea.getByRole("button", {
        name: zh ? "获取工具列表" : "Load tool list",
      });
      await page.evaluate(
        () =>
          (window.__tools.refreshError = {
            code: "REA_MCP_FAILED",
            message: "REFRESH_ERROR_FIXTURE",
          }),
      );
      await load.click();
      await rea.getByText("REFRESH_ERROR_FIXTURE", { exact: true }).waitFor();
      await page.evaluate(() => (window.__tools.refreshError = null));
      await load.click();
      await box(rea, "rea_tool_11").waitFor();
      const filter = rea.getByRole("textbox", {
        name: zh ? "筛选工具" : "Filter tools",
      });
      await filter.fill("TOOL_1");
      assert.equal(await rea.getByRole("checkbox").count(), 3);
      await box(rea, "rea_tool_10").uncheck();
      await filter.fill("");
      assert.equal(await rea.getByRole("checkbox").count(), 12);
      await rea
        .getByRole("button", {
          name: zh ? "保存工具设置" : "Save tool settings",
        })
        .click();
      await rea
        .locator("summary", {
          hasText: zh ? "工具（已停用 1 / 共 12）" : "Tools (1 of 12 disabled)",
        })
        .waitFor();
      await rea.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `.tools/engine-tools-${language}.png` });
      await context.close();
    }
    assert.deepEqual(errors, []);
    console.log("Engine tool UI checks passed.");
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
