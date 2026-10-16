// Browser IPC regression; actual entry discovery is verified separately by Rust tests.
const { chromium } = require(
  process.env.PLAYWRIGHT_MODULE_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || "chromium",
    headless: true,
  });
  const errors = [];
  try {
    for (const language of ["en", "zh-CN"]) {
      const context = await browser.newContext({
        locale: language,
        viewport: { width: 1000, height: 680 },
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
          createdAt: "2026-10-04",
          updatedAt: "2026-10-04",
        };
        window.__detection = {
          available: false,
          entry: null,
          version: null,
          error: "DETECTION_FAILED_FIXTURE",
          calls: [],
        };
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        const callbacks = new Map();
        const updateListeners = [];
        window.__update = {
          sequence: 0,
          status: "available",
          currentVersion: "0.1.23",
          latestVersion: "0.1.24",
          notes: "<script>not executable</script>",
          checkedAt: "2026-10-16T00:00:00Z",
          autoCheck: true,
          canInstall: true,
          downloaded: 0,
          total: null,
          error: null,
        };
        window.__publishUpdate = (change) => {
          Object.assign(window.__update, change, { sequence: ++sequence });
          for (const listener of updateListeners)
            callbacks.get(listener)?.({ payload: { ...window.__update } });
          return { ...window.__update };
        };
        window.__TAURI_INTERNALS__ = {
          transformCallback(callback) {
            const id = ++sequence;
            callbacks.set(id, callback);
            return id;
          },
          unregisterCallback() {},
          async invoke(command, args = {}) {
            if (
              ["list_serena_snapshots", "list_warpgrep_snapshots"].includes(
                command,
              )
            )
              return {};
            window.__detection.calls.push({ command, args });
            if (
              command === "plugin:event|listen" &&
              args.event === "app-update"
            )
              updateListeners.push(args.handler);
            if (command.startsWith("plugin:event|")) return ++sequence;
            if (command === "get_update_snapshot")
              return { ...window.__update };
            if (command === "set_auto_update_check")
              return window.__publishUpdate({ autoCheck: args.enabled });
            if (command === "check_for_updates") {
              window.__publishUpdate({ status: "checking", error: null });
              await new Promise((resolve) => setTimeout(resolve, 20));
              return window.__publishUpdate(
                window.__checkError
                  ? {
                      status: "error",
                      error: {
                        code: "NETWORK",
                        message: "UPDATE_OFFLINE_FIXTURE",
                      },
                    }
                  : {
                      status: "available",
                      latestVersion: "0.1.24",
                      error: null,
                    },
              );
            }
            if (command === "install_update") {
              window.__publishUpdate({
                status: "downloading",
                downloaded: 50,
                total: 200,
                error: null,
              });
              return new Promise((resolve) => {
                window.__finishUpdate = resolve;
              });
            }
            if (command === "cancel_update_download") {
              const snapshot = window.__publishUpdate({
                status: "available",
                downloaded: 0,
                total: null,
              });
              window.__finishUpdate(snapshot);
              return;
            }
            if (command === "plugin:opener|open_url") return;
            if (command === "list_projects") return [project];
            if (command === "get_project_snapshot")
              return {
                projectId: project.id,
                generation: "fixture",
                sequence: ++sequence,
                state: "stopped",
                sessions: 0,
                indexState: window.__detection.available ? "ready" : "error",
                error: window.__detection.available
                  ? null
                  : {
                      code: "CLI_NOT_FOUND",
                      message: "STALE_CLI_ERROR",
                      retryable: true,
                    },
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
            if (command === "get_settings")
              return {
                language,
                codegraphEntry: window.__detection.entry,
                indexConcurrency: 2,
                closeBehavior: "tray",
                appDataDir: "D:\\fixture-data",
              };
            if (
              command === "detect_codegraph" ||
              command === "set_codegraph_entry"
            )
              return { ...window.__detection };
            throw Error(`Unexpected command ${command}`);
          },
        };
      }, language);
      await page.goto(process.env.UI_TEST_URL || "http://127.0.0.1:1420");
      await page.getByTestId("update-notice").waitFor();
      assert.equal(
        await page.evaluate(
          () =>
            window.__detection.calls.filter(
              (c) => c.command === "install_update",
            ).length,
        ),
        0,
      );
      await page
        .getByRole("button", { name: language === "en" ? /Settings/ : /设置/ })
        .click();
      const button = page.getByRole("button", {
        name: language === "en" ? "Detect and use" : "检测并使用",
        exact: true,
      });
      await button.click();
      await page
        .getByTestId("settings-page")
        .getByText("DETECTION_FAILED_FIXTURE", { exact: true })
        .waitFor();
      assert.equal(
        await page.locator('[data-sonner-toast][data-type="success"]').count(),
        0,
      );
      assert.equal(await page.locator("#entry").inputValue(), "");
      assert.equal(await page.locator("html").getAttribute("lang"), language);
      const languageDraft = language === "en" ? "zh-CN" : "en";
      await page.locator("#language").selectOption(languageDraft);
      const snapshotsBefore = await page.evaluate(
        () =>
          window.__detection.calls.filter(
            (c) => c.command === "get_project_snapshot",
          ).length,
      );
      await page.evaluate(() =>
        Object.assign(window.__detection, {
          available: true,
          entry: "C:\\Users\\Fixture\\AppData\\Roaming\\npm\\codegraph.cmd",
          version: "1.6.2",
          error: null,
        }),
      );
      await button.click();
      await page.getByTestId("detected-entry").waitFor();
      assert.equal(
        await page.locator("#entry").inputValue(),
        "C:\\Users\\Fixture\\AppData\\Roaming\\npm\\codegraph.cmd",
      );
      assert.ok(
        (await page.getByTestId("settings-page").innerText()).includes("1.6.2"),
      );
      await page
        .getByText(
          language === "en"
            ? "CodeGraph detected successfully"
            : "CodeGraph 检测成功",
          { exact: true },
        )
        .waitFor();
      assert.equal(await page.locator("html").getAttribute("lang"), language);
      assert.equal(await page.locator("#language").inputValue(), languageDraft);
      assert.ok(
        await page.evaluate(
          (before) =>
            window.__detection.calls.filter(
              (c) => c.command === "get_project_snapshot",
            ).length > before,
          snapshotsBefore,
        ),
      );
      assert.equal(
        (await page.locator("main").innerText()).includes("STALE_CLI_ERROR"),
        false,
      );
      assert.equal(
        await page.evaluate(
          () =>
            window.__detection.calls.filter(
              (c) => c.command === "save_settings",
            ).length,
        ),
        0,
      );
      await page.locator("#entry").fill("D:\\custom\\codegraph.cmd");
      await button.click();
      assert.equal(
        await page.evaluate(
          () =>
            window.__detection.calls
              .filter((c) => c.command === "set_codegraph_entry")
              .at(-1).args.selectedPath,
        ),
        "D:\\custom\\codegraph.cmd",
      );
      await page.locator("main").evaluate((el) => (el.scrollTop = 0));
      const updates = page.getByTestId("update-settings");
      const check = updates.getByRole("button", {
        name: language === "en" ? "Check for updates" : "检查更新",
        exact: true,
      });
      await check.click();
      await updates.getByText(/0\.1\.24/).waitFor();
      const auto = updates.getByRole("checkbox");
      await auto.uncheck();
      assert.equal(await page.evaluate(() => window.__update.autoCheck), false);
      await page.evaluate(() => (window.__checkError = true));
      await check.click();
      await updates
        .getByText("UPDATE_OFFLINE_FIXTURE", { exact: true })
        .waitFor();
      await page.evaluate(() => (window.__checkError = false));
      await check.click();
      const install = updates.getByRole("button", {
        name: language === "en" ? "Download and install" : "下载并安装",
        exact: true,
      });
      await install.click();
      const dialog = page.getByRole("dialog");
      await dialog.waitFor();
      assert.equal(
        await page.evaluate(
          () =>
            window.__detection.calls.filter(
              (c) => c.command === "install_update",
            ).length,
        ),
        0,
      );
      await dialog
        .getByRole("button", {
          name:
            language === "en" ? "Install update and restart" : "安装更新并重启",
          exact: true,
        })
        .click();
      await updates.getByRole("progressbar").waitFor();
      assert.equal(
        await updates.getByRole("progressbar").getAttribute("value"),
        "25",
      );
      await updates
        .getByRole("button", {
          name: language === "en" ? "Cancel download" : "取消下载",
          exact: true,
        })
        .click();
      await install.click();
      await dialog
        .getByRole("button", {
          name:
            language === "en" ? "Install update and restart" : "安装更新并重启",
          exact: true,
        })
        .click();
      await page.evaluate(() =>
        window.__publishUpdate({ status: "preparing" }),
      );
      await dialog.waitFor();
      await page.keyboard.press("Escape");
      assert(await dialog.isVisible());
      await page.evaluate(() => {
        const snapshot = window.__publishUpdate({
          status: "error",
          error: { code: "INSTALL", message: "UPDATE_INSTALL_FAILED_FIXTURE" },
        });
        window.__finishUpdate(snapshot);
      });
      await dialog.waitFor({ state: "hidden" });
      await updates
        .getByText("UPDATE_INSTALL_FAILED_FIXTURE", { exact: true })
        .waitFor();
      await page.evaluate(() => window.__publishUpdate({ canInstall: false }));
      assert.equal(await install.count(), 0);
      await updates.getByRole("link").click();
      assert.equal(
        await page.evaluate(
          () =>
            window.__detection.calls
              .filter((c) => c.command === "plugin:opener|open_url")
              .at(-1).args.url,
        ),
        "https://github.com/touwaeriol/codegraph-desktop/releases",
      );
      await page.locator("main").evaluate((el) => (el.scrollTop = 0));
      await page.screenshot({
        path: `.tools/environment-detection-${language}.png`,
      });
      await context.close();
    }
    assert.deepEqual(errors, []);
    fs.writeFileSync(
      "tests/ui/environment-detection-evidence.json",
      JSON.stringify(
        {
          checkedAt: new Date().toISOString(),
          scope: "Mocked Tauri browser IPC",
          checks: [
            "available=false keeps persistent error and emits no success toast",
            "available=true displays resolved entry and version and fills entry field",
            "manual entry uses set_codegraph_entry",
            "successful detection refreshes cached project snapshot and clears stale CLI error",
            "unsaved language selection remains a draft after detection",
            "English and Chinese stay selected, no preference write",
            "update notice, explicit install confirmation, download progress and cancellation",
            "network/install errors remain retryable and unsupported installers open official releases",
          ],
          pageErrors: errors,
        },
        null,
        2,
      ),
    );
    console.log("Environment detection UI checks passed.");
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
