// Browser integration for one-click engine activation with mocked Tauri IPC.
const { chromium } = require(
  process.env.PLAYWRIGHT_MODULE_PATH || "playwright",
);
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch({
    channel: process.env.PLAYWRIGHT_CHANNEL || "chromium",
    headless: true,
  });
  try {
    const page = await browser.newPage({
      viewport: { width: 1180, height: 820 },
      locale: "zh-CN",
    });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.addInitScript(() => {
      window.isTauri = true;
      let seq = 0;
      const project = {
        id: "fixture",
        name: "activation-fixture",
        rootPath: "/projects/activation-fixture",
        canonicalPath: "/projects/activation-fixture",
        previousRoots: [],
        notes: "",
        autoStart: false,
        createdAt: "2026-10-17",
        updatedAt: "2026-10-17",
      };
      const test = (window.__uiTest = {
        calls: [],
        activeClient: "codex",
        running: {},
        configs: {},
      });
      const settings = () => ({
        language: "zh-CN",
        codegraphEntry: "/opt/homebrew/bin/codegraph",
        serenaEntry: null,
        indexConcurrency: 2,
        closeBehavior: "tray",
        activeClient: test.activeClient,
        appDataDir: "/app-data",
      });
      const engineSnapshot = (engine) => ({
        state: test.running[engine] ? "running" : "stopped",
        pid: null,
        endpoint: null,
        startedAt: null,
        tools: [],
        error: null,
      });
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        transformCallback() {
          return ++seq;
        },
        unregisterCallback() {},
        async invoke(command, args = {}) {
          test.calls.push({ command, args });
          if (command.startsWith("plugin:event|")) return ++seq;
          if (command === "get_update_snapshot")
            return {
              sequence: 0,
              status: "idle",
              currentVersion: "0.1.25",
              latestVersion: null,
              notes: null,
              checkedAt: null,
              autoCheck: false,
              canInstall: true,
              downloaded: 0,
              total: null,
              error: null,
            };
          if (command === "list_projects") return [project];
          if (command === "get_settings") return settings();
          if (command === "set_active_client") {
            test.activeClient = args.client;
            return settings();
          }
          if (command === "detect_codegraph")
            return {
              available: true,
              entry: "/opt/homebrew/bin/codegraph",
              version: "1.6.2",
              error: null,
            };
          if (command === "get_project_snapshot")
            return {
              projectId: "fixture",
              state: test.running.codegraph ? "running" : "stopped",
              sequence: ++seq,
              generation: "fixture",
              indexState: "ready",
              sessions: 0,
              error: null,
            };
          if (command === "list_serena_snapshots")
            return { fixture: engineSnapshot("serena") };
          if (command === "list_warpgrep_snapshots")
            return { fixture: engineSnapshot("warpgrep") };
          if (command === "get_client_config_status")
            return ["codex", "claude"].map((client) => ({
              client,
              state: test.configs[`${args.engine}:${client}`] ?? "missing",
              path: client === "codex" ? ".codex/config.toml" : ".mcp.json",
              message: null,
            }));
          if (command === "set_engine_enabled") {
            if (args.engine === "warpgrep" && args.enabled)
              throw {
                code: "ENVIRONMENT_REQUIRED",
                message: "WARPGREP_KEY_FIXTURE",
              };
            if (args.engine !== "rea") test.running[args.engine] = args.enabled;
            test.configs[`${args.engine}:${test.activeClient}`] = args.enabled
              ? "configured"
              : "missing";
            return {
              client: test.activeClient,
              warnings: args.engine === "rea" ? ["NODE_WARNING_FIXTURE"] : [],
            };
          }
          if (
            ["read_logs", "list_config_backups", "get_project_tasks"].includes(
              command,
            )
          )
            return [];
          throw new Error("Unexpected IPC " + command);
        },
      };
    });
    await page.goto(process.env.UI_TEST_URL || "http://127.0.0.1:1420");
    const sidebar = page.getByRole("navigation", { name: "项目列表" });
    const marker = (name) =>
      sidebar.getByRole("button", { name: new RegExp(`^${name} · `) });
    const calls = (command) =>
      page.evaluate(
        (command) =>
          window.__uiTest.calls
            .filter((c) => c.command === command)
            .map((c) => c.args),
        command,
      );
    const waitState = (name, state) =>
      page.waitForFunction(
        ([name, state]) =>
          [...document.querySelectorAll(".sidebar-engine-marker")].some(
            (el) =>
              el.getAttribute("aria-label").startsWith(`${name} · `) &&
              el.dataset.state === state,
          ),
        [name, state],
      );
    const engines = page.getByRole("group", { name: "项目引擎" });
    await engines.getByRole("button", { name: /CodeGraph/ }).waitFor();

    // The sidebar marker starts the engine and writes the active client.
    await marker("CodeGraph").click();
    await waitState("CodeGraph", "running");
    assert.deepEqual((await calls("set_engine_enabled")).at(-1), {
      projectId: "fixture",
      engine: "codegraph",
      enabled: true,
    });
    await page
      .getByTestId("engine-row-codegraph")
      .getByText("已注入 Codex")
      .waitFor();
    await marker("CodeGraph").click();
    await waitState("CodeGraph", "stopped");
    assert.equal((await calls("set_engine_enabled")).at(-1).enabled, false);

    // The client choice is global: the sidebar switch drives the MCP list.
    await page
      .getByRole("group", { name: "图标启用写入的客户端" })
      .getByRole("button", { name: "Claude Code", exact: true })
      .click();
    assert.deepEqual((await calls("set_active_client")).at(-1), {
      client: "claude",
    });
    await page.getByRole("tab", { name: "MCP 配置" }).click();
    await page
      .getByTestId("mcp-list")
      .getByRole("button", { name: "Claude Code", exact: true })
      .and(page.locator('[aria-pressed="true"]'))
      .waitFor();

    // REA only toggles its registration; version warnings are surfaced.
    await marker("REA").click();
    await waitState("REA", "running");
    await page.getByText("NODE_WARNING_FIXTURE").waitFor();
    await page
      .getByTestId("engine-row-rea")
      .getByText("已注入 Claude Code")
      .waitFor();

    // The engine list switch behaves like the marker.
    await page.getByRole("switch", { name: "启用 Serena" }).click();
    await waitState("Serena", "running");
    assert.deepEqual((await calls("set_engine_enabled")).at(-1), {
      projectId: "fixture",
      engine: "serena",
      enabled: true,
    });

    // Missing prerequisites point straight to Settings.
    await marker("WarpGrep").click();
    await page.getByText("WARPGREP_KEY_FIXTURE").waitFor();
    await waitState("WarpGrep", "stopped");
    await page.getByRole("button", { name: "前往设置" }).click();
    await page.getByTestId("settings-page").waitFor();
    if (process.env.UI_SCREENSHOT) {
      await page
        .getByRole("navigation", { name: "项目列表" })
        .getByRole("button", { name: "activation-fixture" })
        .click();
      await page.getByRole("tab", { name: "概览" }).click();
      await page.screenshot({ path: process.env.UI_SCREENSHOT });
    }
    assert.deepEqual(errors, []);
    console.log(
      "Activation UI: sidebar markers, engine list switch, shared client, REA warnings and missing-environment prompt passed",
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
