// Browser IPC regression; actual entry discovery is verified separately by Rust tests.
const { chromium } = require(
  process.env.PLAYWRIGHT_MODULE_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
(async () => {
  const browser = await chromium.launch({ channel: "msedge", headless: true });
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
        window.__TAURI_INTERNALS__ = {
          transformCallback() {
            return ++sequence;
          },
          unregisterCallback() {},
          async invoke(command, args = {}) {
            window.__detection.calls.push({ command, args });
            if (command.startsWith("plugin:event|")) return ++sequence;
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
      await page
        .getByRole("button", { name: language === "en" ? /Settings/ : /设置/ })
        .click();
      const button = page.getByRole("button", {
        name: language === "en" ? "Detect and use" : "检测并使用",
        exact: true,
      });
      await button.click();
      await page
        .getByRole("dialog")
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
      assert.ok((await page.getByRole("dialog").innerText()).includes("1.6.2"));
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
      await page.getByRole("dialog").evaluate((el) => (el.scrollTop = 0));
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
