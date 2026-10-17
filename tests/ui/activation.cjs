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
            return ["codex", "claude", "cursor", "devin"].map((client) => ({
              client,
              state: test.configs[`${args.engine}:${client}`] ?? "missing",
              path: `.${client}/mcp.json`,
              message: null,
            }));
          if (command === "set_engine_enabled") {
            if (args.engine === "warpgrep" && args.enabled)
              throw {
                code: "ENVIRONMENT_REQUIRED",
                message: "WARPGREP_KEY_FIXTURE",
              };
            // Mirrors activation.rs: enabling starts the engine and writes one
            // client; disabling removes one client, or every client from the
            // sidebar, and stops the engine once no client remains.
            const all = ["codex", "claude", "cursor", "devin"];
            const configured = () =>
              all.filter(
                (client) =>
                  test.configs[`${args.engine}:${client}`] === "configured",
              );
            const clients = args.enabled
              ? [args.client ?? test.activeClient]
              : args.client
                ? [args.client]
                : configured();
            for (const client of clients)
              test.configs[`${args.engine}:${client}`] = args.enabled
                ? "configured"
                : "missing";
            const running = args.enabled || configured().length > 0;
            if (args.engine !== "rea") test.running[args.engine] = running;
            return {
              clients,
              running,
              warnings:
                args.engine === "rea" && args.enabled
                  ? ["NODE_WARNING_FIXTURE"]
                  : [],
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
    const clientSwitch = (engine, client) =>
      page.getByRole("switch", { name: `在 ${client} 中启用 ${engine}` });
    const waitChecked = (engine, client, checked) =>
      page.waitForFunction(
        ([label, checked]) =>
          document.querySelector(`[aria-label="${label}"]`)?.checked ===
          checked,
        [`在 ${client} 中启用 ${engine}`, checked],
      );
    const last = async () => (await calls("set_engine_enabled")).at(-1);

    // The sidebar marker starts the engine and writes the default client.
    await marker("CodeGraph").click();
    await waitState("CodeGraph", "running");
    assert.deepEqual(await last(), {
      projectId: "fixture",
      engine: "codegraph",
      enabled: true,
      client: null,
    });
    await waitChecked("CodeGraph", "Codex", true);
    for (const client of ["Claude Code", "Cursor", "Devin"])
      assert.equal(await clientSwitch("CodeGraph", client).isChecked(), false);

    // Each row has one switch per client; the engine keeps running until its
    // last client is turned off.
    await clientSwitch("CodeGraph", "Cursor").click();
    await waitChecked("CodeGraph", "Cursor", true);
    assert.deepEqual(await last(), {
      projectId: "fixture",
      engine: "codegraph",
      enabled: true,
      client: "cursor",
    });
    await clientSwitch("CodeGraph", "Codex").click();
    await waitChecked("CodeGraph", "Codex", false);
    await page.getByText("CodeGraph 已从 Codex 项目配置移除").waitFor();
    await waitState("CodeGraph", "running");
    await clientSwitch("CodeGraph", "Cursor").click();
    await waitChecked("CodeGraph", "Cursor", false);
    await waitState("CodeGraph", "stopped");
    await page.getByText("CodeGraph 已停用，并移除 Cursor 项目配置").waitFor();
    assert.equal((await last()).client, "cursor");

    // The sidebar picks the default client among all four.
    const picker = page.getByRole("combobox", { name: "图标启用写入的客户端" });
    assert.deepEqual(await picker.locator("option").allTextContents(), [
      "Codex",
      "Claude Code",
      "Cursor",
      "Devin",
    ]);
    await picker.selectOption("claude");
    assert.deepEqual((await calls("set_active_client")).at(-1), {
      client: "claude",
    });
    await page.getByRole("tab", { name: "MCP 配置" }).click();
    const mcpList = page.getByTestId("mcp-list");
    await mcpList
      .getByRole("button", { name: "Claude Code", exact: true })
      .and(page.locator('[aria-pressed="true"]'))
      .waitFor();
    await mcpList.getByRole("button", { name: "Devin", exact: true }).click();
    await mcpList.getByText(".devin/mcp_config.json").waitFor();

    // REA only toggles registrations; version warnings are surfaced.
    await marker("REA").click();
    await waitState("REA", "running");
    await page.getByText("NODE_WARNING_FIXTURE").waitFor();
    // The MCP list switched the default client to Devin.
    assert.equal((await calls("set_active_client")).at(-1).client, "devin");
    await waitChecked("REA", "Devin", true);

    // A client switch on a stopped engine starts it for that client.
    await clientSwitch("Serena", "Devin").click();
    await waitState("Serena", "running");
    assert.deepEqual(await last(), {
      projectId: "fixture",
      engine: "serena",
      enabled: true,
      client: "devin",
    });
    await waitChecked("Serena", "Devin", true);

    // Disabling from the sidebar removes every client registration.
    await clientSwitch("REA", "Cursor").click();
    await waitChecked("REA", "Cursor", true);
    await marker("REA").click();
    await waitState("REA", "stopped");
    assert.deepEqual(await last(), {
      projectId: "fixture",
      engine: "rea",
      enabled: false,
      client: null,
    });
    await waitChecked("REA", "Devin", false);
    await waitChecked("REA", "Cursor", false);

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
      "Activation UI: sidebar markers, per-client engine switches, default client picker, REA warnings and missing-environment prompt passed",
    );
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
