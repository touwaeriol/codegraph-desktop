// Browser regression with mocked Tauri IPC; does not claim native OS installation coverage.
const { chromium } = require(
  process.env.PLAYWRIGHT_MODULE_PATH || "playwright",
);
const assert = require("node:assert/strict");
const fs = require("node:fs");
const url = process.env.UI_TEST_URL || "http://127.0.0.1:1420";

async function mock(page, delaySettings = false) {
  await page.addInitScript((delaySettings) => {
    window.isTauri = true;
    let preferencesReady = Promise.resolve();
    if (delaySettings) {
      localStorage.setItem("test-language", "en");
      preferencesReady = new Promise((resolve) => {
        window.__releasePreferences = resolve;
      });
    }
    const project = {
      id: "test-project",
      name: "用户项目 / User project",
      rootPath: "D:\\fixture",
      canonicalPath: "D:\\fixture",
      previousRoots: [],
      notes: "用户原始备注",
      autoStart: false,
      createdAt: "2026-10-04T01:02:03Z",
      updatedAt: "2026-10-04T01:02:03Z",
    };
    let sequence = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      transformCallback() {
        return ++sequence;
      },
      unregisterCallback() {},
      async invoke(command, args = {}) {
        if (command.startsWith("plugin:event|")) return ++sequence;
        if (command === "list_projects") return [project];
        if (command === "get_settings") {
          await preferencesReady;
          return {
            language:
              localStorage.getItem("test-language") ||
              (/^zh/i.test(navigator.languages[0]) ? "zh-CN" : "en"),
            codegraphEntry: null,
            indexConcurrency: 2,
            closeBehavior: "tray",
            appDataDir: "D:\\fixture-data",
          };
        }
        if (command === "save_settings") {
          localStorage.setItem("test-language", args.language);
          return {
            ...args,
            appDataDir: "D:\\fixture-data",
            codegraphEntry: null,
          };
        }
        if (command === "detect_codegraph")
          return {
            available: true,
            entry: "fixture",
            version: "1.6.2",
            error: null,
          };
        if (command === "get_project_snapshot")
          return {
            projectId: project.id,
            generation: "fixture",
            sequence: 1,
            state: "running",
            sessions: 1,
            port: 43123,
            startedAt: "2026-10-04T01:02:03Z",
            indexState: "ready",
            indexStats: {
              fileCount: 2,
              nodeCount: 3,
              edgeCount: 1,
              checkedAt: "2026-10-04T01:02:03Z",
            },
          };
        if (
          command === "get_project_tasks" ||
          command === "list_config_backups"
        )
          return [];
        if (command === "read_logs")
          return [
            {
              projectId: project.id,
              generation: "fixture",
              sequence: 1,
              timestamp: "2026-10-04T01:02:03Z",
              level: "info",
              stage: "fixture",
              message: "上游原始输出 / untouched",
            },
          ];
        if (command === "get_client_config_status")
          return ["codex", "claude"].map((client) => ({
            client,
            path: "D:\\fixture",
            state: "configured",
            message: null,
          }));
        if (command === "preview_client_config")
          return {
            previewId: "fixture",
            projectId: project.id,
            serviceName: "codegraph",
            files: [
              {
                client: "codex",
                path: "D:\\fixture\\.codex\\config.toml",
                before: "# 用户文件原文\n",
                after:
                  '# 用户文件原文\n[mcp_servers.codegraph]\nurl="http://127.0.0.1:43123/mcp/test-project"\n',
                existed: true,
                conflict: false,
              },
            ],
          };
        throw Error(`Unhandled test command: ${command}`);
      },
    };
  }, delaySettings);
}
async function bounds(page, locator) {
  const box = await locator.boundingBox();
  assert.ok(
    box &&
      box.x >= 0 &&
      box.y >= 0 &&
      box.x + box.width <= 1001 &&
      box.y + box.height <= 681,
    JSON.stringify(box),
  );
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
    true,
  );
}
(async () => {
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  const errors = [];
  const results = [];
  fs.mkdirSync(".tools", { recursive: true });
  try {
    for (const [locale, expected] of [
      ["zh-CN", "zh-CN"],
      ["en-US", "en"],
      ["fr-FR", "en"],
    ]) {
      const context = await browser.newContext({
        locale,
        viewport: { width: 1000, height: 680 },
      });
      const page = await context.newPage();
      page.on("pageerror", (e) => errors.push(e.message));
      await page.goto(url);
      await page.waitForFunction(
        (lang) => document.documentElement.lang === lang,
        expected,
      );
      await page
        .getByRole("button", {
          name: expected === "en" ? "Choose project folder" : "选择项目目录",
          exact: true,
        })
        .waitFor();
      await context.close();
      results.push(`${locale} browser default -> ${expected}`);
    }
    const delayedContext = await browser.newContext({
      locale: "zh-CN",
      viewport: { width: 1000, height: 680 },
    });
    const delayedPage = await delayedContext.newPage();
    delayedPage.on("pageerror", (e) => errors.push(e.message));
    await mock(delayedPage, true);
    await delayedPage.goto(url);
    await delayedPage.getByRole("button", { name: /设置/ }).click();
    await delayedPage
      .getByRole("status")
      .filter({ hasText: "正在读取设置" })
      .waitFor();
    assert.equal(await delayedPage.locator("#language").count(), 0);
    await delayedPage.evaluate(() => window.__releasePreferences());
    await delayedPage.getByLabel("Display language").waitFor();
    assert.equal(await delayedPage.locator("#language").inputValue(), "en");
    await delayedContext.close();
    results.push(
      "Opening settings before preferences load does not overwrite saved language",
    );
    const context = await browser.newContext({
      locale: "zh-CN",
      viewport: { width: 1000, height: 680 },
    });
    const page = await context.newPage();
    page.on("pageerror", (e) => errors.push(e.message));
    await mock(page);
    await page.goto(url);
    await page.getByRole("button", { name: /设置/ }).click();
    await page.locator("#language").selectOption("en");
    await page.getByRole("button", { name: "保存偏好", exact: true }).click();
    await page
      .getByRole("heading", { name: "Settings", exact: true })
      .waitFor();
    assert.equal(await page.locator("html").getAttribute("lang"), "en");
    await bounds(page, page.locator("main"));
    assert.equal(await page.getByRole("dialog").count(), 0);
    await page.getByLabel("Display language").scrollIntoViewIfNeeded();
    await page.locator("main").evaluate((el) => (el.scrollTop = 0));
    await page.screenshot({ path: ".tools/language-settings-en.png" });
    await page.locator("#entry").fill("unsaved entry draft");
    await page
      .getByRole("button", { name: "Back to projects", exact: true })
      .click();
    await page.getByRole("tab", { name: "Overview", exact: true }).waitFor();
    await page.keyboard.press("Control+,");
    await page.getByLabel("Display language").waitFor();
    assert.equal(
      await page.locator("#entry").inputValue(),
      "unsaved entry draft",
    );
    await page
      .getByRole("navigation", { name: "Project list" })
      .getByRole("button")
      .first()
      .click();
    await page.getByRole("tab", { name: "Overview", exact: true }).waitFor();
    await page.reload();
    await page.getByRole("tab", { name: "Overview", exact: true }).waitFor();
    assert.ok(
      (await page.locator("main").innerText()).includes(
        new Date("2026-10-04T01:02:03Z").toLocaleString("en"),
      ),
    );
    await page
      .getByRole("tab", { name: "MCP configuration", exact: true })
      .click();
    assert.ok(
      (await page.locator("main").innerText()).includes(
        "http://127.0.0.1:43123/mcp/test-project",
      ),
    );
    await page
      .getByRole("button", { name: "Preview configuration", exact: true })
      .click();
    await page.getByRole("dialog").waitFor();
    await page.getByLabel("Write mode").selectOption("edit");
    await page.getByLabel("Full file content (TOML)").waitFor();
    assert.ok(
      (await page.getByRole("dialog").innerText()).includes("# 用户文件原文"),
    );
    await bounds(page, page.getByRole("dialog"));
    await page.screenshot({ path: ".tools/language-preview-en.png" });
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.getByRole("tab", { name: "Logs", exact: true }).click();
    await page.getByText("上游原始输出 / untouched", { exact: true }).waitFor();
    await page.getByRole("button", { name: /Settings/ }).click();
    await page.locator("#language").selectOption("zh-CN");
    await page
      .getByRole("button", { name: "Save preferences", exact: true })
      .click();
    await page.getByRole("heading", { name: "设置", exact: true }).waitFor();
    await page.getByRole("button", { name: "返回项目", exact: true }).click();
    await page.reload();
    await page.getByRole("tab", { name: "概览", exact: true }).waitFor();
    assert.equal(await page.locator("html").getAttribute("lang"), "zh-CN");
    assert.deepEqual(errors, []);
    results.push(
      "zh -> saved en -> reload en -> saved zh -> reload zh",
      "English settings page, diff editor, timestamps and 1000x680 bounds",
      "Settings is not a dialog; Ctrl+, and sidebar navigation preserve unsaved form drafts",
      "User project names, notes, file text and upstream logs stay unchanged",
    );
    fs.writeFileSync(
      "tests/ui/language-evidence.json",
      JSON.stringify(
        {
          checkedAt: new Date().toISOString(),
          scope: "Mocked browser IPC, not native installation",
          checks: results,
          pageErrors: errors,
        },
        null,
        2,
      ),
    );
    console.log("Language UI checks passed.");
    await context.close();
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
