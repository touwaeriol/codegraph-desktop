// Browser IPC regression for REA environment settings; the doctor run and
// configuration writes are verified by Rust tests.
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
        const test = (window.__rea = {
          calls: [],
          environment: {
            GHIDRA_INSTALL_DIR: "C:\\Tools\\ghidra",
            REA_GHIDRA_STARTUP_TIMEOUT_MS: "600000",
          },
          saveError: null,
          installed: { jadx: "D:\\fixture-data\\tools\\jadx.jar" },
          inUse: {},
        });
        const callbacks = new Map();
        const listeners = [];
        window.__emitInstall = (payload) => {
          for (const id of listeners) callbacks.get(id)?.({ payload });
        };
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        window.__TAURI_INTERNALS__ = {
          transformCallback(callback) {
            const id = ++sequence;
            callbacks.set(id, callback);
            return id;
          },
          unregisterCallback() {},
          async invoke(command, args = {}) {
            test.calls.push({ command, args });
            if (
              command === "plugin:event|listen" &&
              args.event === "rea-tool-install"
            )
              listeners.push(args.handler);
            if (command.startsWith("plugin:event|")) return ++sequence;
            if (command === "get_rea_tools")
              return {
                tools: ["jdk", "ghidra", "jadx", "ilspy"].map((id) => ({
                  id,
                  installed: test.installed[id] ?? null,
                  inUse: test.inUse[id] === true,
                })),
                installing: false,
                directory: "D:\\fixture-data\\tools",
              };
            if (command === "open_rea_tools_directory") return null;
            if (command === "install_rea_tool")
              return new Promise((resolve, reject) => {
                window.__finishInstall = { resolve, reject };
              });
            if (command === "cancel_rea_tool_install") return null;
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
            if (command === "get_rea_environment")
              return { ...test.environment };
            if (command === "get_engine_tools")
              return { known: {}, disabled: {} };
            if (command === "save_rea_environment") {
              if (test.saveError) throw test.saveError;
              test.environment = Object.fromEntries(
                Object.entries(args.environment)
                  .map(([key, value]) => [key.trim(), value.trim()])
                  .filter(([, value]) => value),
              );
              return {
                environment: { ...test.environment },
                updated: ["Fixture · Codex"],
                failed: ["Other · Claude Code: CONFIG_CHANGED_FIXTURE"],
              };
            }
            if (command === "check_rea_environment")
              return [
                {
                  name: "node",
                  ok: true,
                  detail: "24.20.0",
                  remediation: null,
                },
                {
                  name: "ghidra-installation",
                  ok: false,
                  detail: "GHIDRA_DETAIL_FIXTURE",
                  remediation: "GHIDRA_REMEDIATION_FIXTURE",
                },
              ];
            throw Error(`Unexpected command ${command}`);
          },
        };
      }, language);
      await page.goto(process.env.UI_TEST_URL || "http://127.0.0.1:1420");
      await page
        .getByRole("button", { name: zh ? /设置/ : /Settings/ })
        .first()
        .click();
      const card = page.getByTestId("rea-settings");
      await card.waitFor();
      const ghidra = card.locator("#rea-GHIDRA_INSTALL_DIR");
      await page.waitForFunction(
        () =>
          document.querySelector("#rea-GHIDRA_INSTALL_DIR")?.value ===
          "C:\\Tools\\ghidra",
      );
      const save = card.getByRole("button", {
        name: zh ? "保存" : "Save",
        exact: true,
      });
      const check = card.getByRole("button", {
        name: zh ? "检查分析引擎" : "Check analysis providers",
      });
      assert.equal(await save.isDisabled(), true);
      assert.equal(await check.isDisabled(), false);
      const custom = card.locator("summary", {
        hasText: zh ? "其他环境变量（1）" : "Other environment variables (1)",
      });
      await custom.click();
      assert.equal(
        await card.locator(".rea-env-row input").first().inputValue(),
        "REA_GHIDRA_STARTUP_TIMEOUT_MS",
      );

      await card.locator("#rea-REA_ANALYSIS_PROVIDER").selectOption("ida");
      await ghidra.fill("   ");
      await card
        .locator("#rea-REA_IDA_MCP_CONFIG")
        .fill("C:\\Tools\\ida-mcp.json");
      assert.equal(await save.isDisabled(), false);
      assert.equal(await check.isDisabled(), true);
      await card
        .getByRole("button", { name: zh ? "添加变量" : "Add variable" })
        .click();
      const rows = card.locator(".rea-env-row");
      await rows.nth(1).locator("input").nth(0).fill("REA_ADB_PATH");
      await rows.nth(1).locator("input").nth(1).fill("C:\\Android\\adb.exe");
      await rows
        .nth(0)
        .getByRole("button", { name: zh ? "删除变量" : "Remove variable" })
        .click();

      await page.evaluate(
        () =>
          (window.__rea.saveError = {
            code: "INVALID_REA_ENVIRONMENT",
            message: "SAVE_ERROR_FIXTURE",
          }),
      );
      await save.click();
      await card.getByText("SAVE_ERROR_FIXTURE", { exact: true }).waitFor();
      assert.equal(await save.isDisabled(), false);
      await page.evaluate(() => (window.__rea.saveError = null));
      await save.click();
      await page
        .getByText(
          zh
            ? "REA 环境已保存，并更新 1 个项目配置；请在客户端重新加载 MCP"
            : "REA environment saved and 1 project configurations updated. Reload MCP in the client",
        )
        .waitFor();
      await page.getByText(/CONFIG_CHANGED_FIXTURE/).waitFor();
      const saved = await page.evaluate(
        () =>
          window.__rea.calls
            .filter((call) => call.command === "save_rea_environment")
            .at(-1).args.environment,
      );
      assert.deepEqual(saved, {
        REA_ANALYSIS_PROVIDER: "ida",
        GHIDRA_INSTALL_DIR: "   ",
        REA_IDA_MCP_CONFIG: "C:\\Tools\\ida-mcp.json",
        REA_ADB_PATH: "C:\\Android\\adb.exe",
      });
      assert.equal(await ghidra.inputValue(), "");
      assert.equal(await save.isDisabled(), true);
      await card
        .locator("summary", {
          hasText: zh ? "其他环境变量（1）" : "Other environment variables (1)",
        })
        .waitFor();

      await check.click();
      const checks = card.getByTestId("rea-checks");
      await checks.waitFor();
      const text = await checks.innerText();
      assert.ok(text.includes("node"));
      assert.ok(text.includes("GHIDRA_DETAIL_FIXTURE"));
      assert.ok(text.includes("GHIDRA_REMEDIATION_FIXTURE"));
      assert.ok(text.includes(zh ? "未就绪" : "Not ready"));

      const tools = card.getByTestId("rea-tools");
      const row = (id) => tools.getByTestId(`rea-tool-${id}`);
      const directory = tools.getByTestId("rea-tools-directory");
      await directory.getByText("D:\\fixture-data\\tools").waitFor();
      await directory
        .getByRole("button", { name: zh ? "打开目录" : "Open folder" })
        .click();
      await page.waitForFunction(() =>
        window.__rea.calls.some(
          (call) => call.command === "open_rea_tools_directory",
        ),
      );
      await row("jadx")
        .getByText(zh ? "已安装" : "Installed", { exact: true })
        .waitFor();
      await row("ghidra")
        .getByRole("button", { name: zh ? "安装" : "Install" })
        .click();
      const status = tools.getByRole("status");
      await status.waitFor();
      assert.equal(await row("jadx").getByRole("button").isDisabled(), true);
      const mib = 1024 * 1024;
      await page.evaluate(
        (mib) =>
          window.__emitInstall({
            tool: "ghidra",
            item: "jdk",
            phase: "download",
            downloaded: 50 * mib,
            total: 200 * mib,
          }),
        mib,
      );
      await status
        .getByText(
          zh
            ? "正在下载 JDK 21：50 MB / 200 MB · 25%"
            : "Downloading JDK 21: 50 MB / 200 MB · 25%",
        )
        .waitFor();
      await page.evaluate(() =>
        window.__emitInstall({
          tool: "ghidra",
          item: "ghidra",
          phase: "extract",
          downloaded: 0,
          total: null,
        }),
      );
      await status
        .getByText(zh ? "正在解压 Ghidra…" : "Unpacking Ghidra…")
        .waitFor();
      await status
        .getByRole("button", { name: zh ? "取消" : "Cancel" })
        .click();
      assert.ok(
        await page.evaluate(() =>
          window.__rea.calls.some(
            (call) => call.command === "cancel_rea_tool_install",
          ),
        ),
      );
      await page.evaluate(() =>
        window.__finishInstall.reject({
          code: "CANCELLED",
          message: "CANCELLED_FIXTURE",
        }),
      );
      await tools.getByText("CANCELLED_FIXTURE", { exact: true }).waitFor();
      await status.waitFor({ state: "hidden" });

      await row("jadx")
        .getByRole("button", { name: zh ? "使用" : "Use" })
        .click();
      await page.evaluate(() => {
        window.__rea.inUse.jadx = true;
        window.__finishInstall.resolve({
          environment: {
            ...window.__rea.environment,
            JAVA_HOME: "D:\\fixture-data\\tools\\jdk-21",
            REA_JADX_MCP_JAR: "D:\\fixture-data\\tools\\jadx.jar",
          },
          updated: [],
          failed: [],
        });
      });
      await page
        .getByText(
          zh
            ? "JADX 已安装并写入 REA 环境"
            : "JADX is installed and saved to the REA environment",
        )
        .waitFor();
      assert.equal(
        await card.locator("#rea-REA_JADX_MCP_JAR").inputValue(),
        "D:\\fixture-data\\tools\\jadx.jar",
      );
      assert.equal(
        await card.locator("#rea-JAVA_HOME").inputValue(),
        "D:\\fixture-data\\tools\\jdk-21",
      );
      await row("jadx")
        .getByText(zh ? "使用中" : "In use", { exact: true })
        .waitFor();
      assert.equal(await row("jadx").getByRole("button").isDisabled(), true);
      assert.deepEqual(
        await page.evaluate(() =>
          window.__rea.calls
            .filter((call) => call.command === "install_rea_tool")
            .map((call) => call.args.tool),
        ),
        ["ghidra", "jadx"],
      );
      await card.scrollIntoViewIfNeeded();
      await page.screenshot({
        path: `.tools/rea-settings-${language}.png`,
        fullPage: false,
      });
      await context.close();
    }
    assert.deepEqual(errors, []);
    console.log("REA settings UI checks passed.");
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
