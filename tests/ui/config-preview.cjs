// Browser interaction regression with mocked Tauri IPC, not a native desktop test.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE_PATH || 'playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');

(async () => {
  const browser = await chromium.launch({ channel: 'msedge', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1000, height: 680 } });
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.addInitScript(() => {
      window.isTauri = true;
      window.__uiTest = { calls: [], external: false, rejectApply: false, invalidOriginal: false };
      const project = { id: '11111111-1111-4111-8111-111111111111', name: '配置交互测试项目', rootPath: 'D:\\fixture', canonicalPath: 'D:\\fixture', previousRoots: [], notes: '', autoStart: false, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() };
      let sequence = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        transformCallback() { return ++sequence; }, unregisterCallback() {},
        async invoke(command, args = {}) {
          window.__uiTest.calls.push({ command, args });
          if (command.startsWith('plugin:event|')) return ++sequence;
          if (command === 'list_projects') return [project];
          if (command === 'get_settings') return { language: 'zh-CN', codegraphEntry: null, indexConcurrency: 1, closeBehavior: 'tray', appDataDir: 'D:\\fixture-data' };
          if (command === 'detect_codegraph') return { available: true, entry: 'fixture.js', version: 'test', error: null };
          if (command === 'get_project_snapshot') return { projectId: project.id, generation: 'test', sequence: 1, state: 'stopped', sessions: 0, indexState: 'ready', indexStats: null };
          if (['read_logs', 'get_project_tasks', 'list_config_backups'].includes(command)) return [];
          if (command === 'get_client_config_status') return ['codex', 'claude'].map(client => ({ client, path: 'D:\\fixture', state: 'configured', message: null }));
          if (command === 'preview_client_config') {
            return { previewId: `preview-${++sequence}`, projectId: project.id, serviceName: 'codegraph', files: args.clients.map(client => {
              const draft = args.overrides?.find(o => o.client === client) || { mode: 'merge' };
              if (draft.mode !== 'edit' && draft.content !== undefined) throw new Error('非编辑模式不能发送 content');
              if (window.__uiTest.invalidOriginal && draft.mode === 'merge') throw new Error('现有文件格式错误');
              if (draft.mode === 'edit' && draft.content === 'INVALID') throw new Error('配置语法错误，请修正');
              const before = client === 'codex' ? `# existing setting\nmodel = "existing"\n${window.__uiTest.external ? '# external-change\n' : ''}` : '{"mcpServers":{"other":{"command":"other"}}}';
              const generated = client === 'codex' ? '[mcp_servers.codegraph]\nurl = "http://127.0.0.1:43123/mcp"\nhttp_headers = { Authorization = "Bearer TEST_FIXTURE_ONLY" }\n' : '{"mcpServers":{"codegraph":{"type":"http","url":"http://127.0.0.1:43123/mcp","headers":{"Authorization":"Bearer TEST_FIXTURE_ONLY"}}}}';
              return { client, path: `D:\\fixture\\${client === 'codex' ? '.codex\\config.toml' : '.mcp.json'}`, before, after: draft.mode === 'edit' ? draft.content : draft.mode === 'overwrite' ? generated : client === 'codex' ? before + generated : generated, existed: true, conflict: true };
            }) };
          }
          if (command === 'apply_client_config') {
            if (window.__uiTest.rejectApply) throw new Error('文件已变化，请刷新预览');
            return { operationId: 'test', backupPath: 'D:\\fixture-backup', files: [{ path: 'D:\\fixture', status: 'success', message: '配置已写入' }] };
          }
          throw new Error(`Unhandled mock command: ${command}`);
        }
      };
    });
    await page.goto(process.env.UI_TEST_URL || 'http://127.0.0.1:1420');
    await page.getByRole('tab', { name: 'MCP 配置' }).click();
    await page.getByRole('button', { name: '预览并配置', exact: true }).click();
    const dialog = page.getByRole('dialog');
    const apply = dialog.getByRole('button', { name: '应用配置', exact: true });
    const refresh = dialog.getByRole('button', { name: '重新读取 / 刷新预览', exact: true });
    await dialog.waitFor();
    assert.equal(await apply.isEnabled(), true);
    await page.locator('#config-mode-codex').selectOption('edit');
    const editor = page.locator('#config-content-codex');
    const draft = `${await editor.inputValue()}# manual-draft\n`;
    await editor.fill(draft);
    assert.equal(await apply.isDisabled(), true);
    await refresh.click();
    await page.waitForFunction(() => !document.querySelector('[role="dialog"] button:last-of-type')?.disabled);
    assert.equal(await apply.isEnabled(), true);
    await page.evaluate(() => window.__uiTest.external = true);
    await refresh.click();
    assert.equal(await editor.inputValue(), draft);
    await dialog.getByText('# external-change', { exact: false }).waitFor();
    await editor.fill('INVALID');
    await refresh.click();
    await dialog.getByText('配置语法错误，请修正', { exact: true }).waitFor();
    assert.equal(await apply.isDisabled(), true);
    assert.equal(await editor.inputValue(), 'INVALID');
    await editor.fill(draft);
    await refresh.click();
    await page.locator('#config-mode-codex').selectOption('merge');
    await refresh.click();
    await page.locator('#config-mode-codex').selectOption('edit');
    assert.equal(await editor.inputValue(), draft);
    await refresh.click();
    await page.locator('#config-mode-claude').selectOption('overwrite');
    assert.equal(await apply.isDisabled(), true);
    await dialog.getByText('覆盖整个文件', { exact: true }).last().waitFor();
    await refresh.click();
    await page.evaluate(() => window.__uiTest.rejectApply = true);
    await apply.click();
    await dialog.getByText('文件已变化，请刷新预览', { exact: true }).waitFor();
    assert.equal(await apply.isDisabled(), true);
    assert.equal(await editor.inputValue(), draft);
    await page.evaluate(() => window.__uiTest.rejectApply = false);
    await refresh.click();
    await dialog.evaluate(el => el.scrollTop = 0);
    await page.screenshot({ path: '.tools/config-preview-0.1.3.png' });
    const box = await dialog.boundingBox();
    assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= 1001 && box.y + box.height <= 681);
    await apply.click();
    await dialog.waitFor({ state: 'hidden' });
    await page.evaluate(() => window.__uiTest.invalidOriginal = true);
    await page.getByRole('button', { name: '预览并配置', exact: true }).click();
    await page.getByRole('button', { name: '以覆盖模式预览', exact: true }).click();
    await dialog.waitFor();
    assert.equal(await page.locator('#config-mode-codex').inputValue(), 'overwrite');
    await page.locator('#config-mode-codex').selectOption('edit');
    await editor.fill(draft);
    await refresh.click();
    assert.equal(await apply.isEnabled(), true);
    assert.deepEqual(errors, []);
    fs.writeFileSync('tests/ui/config-preview-evidence.json', JSON.stringify({ checkedAt: new Date().toISOString(), scope: 'Browser UI with mocked Tauri IPC; actual file writes tested by Rust tests separately', viewport: '1000x680', checks: ['edit requires repreview', 'external refresh preserves draft and changes diff', 'invalid content blocks apply without losing draft', 'mode switching preserves draft without sending edit-only fields', 'overwrite requires repreview', 'external-change apply error requires refresh', 'retry after refresh succeeds', 'invalid original can enter overwrite and edit preview', 'dialog fits viewport'], pageErrors: errors }, null, 2));
    console.log('Config preview UI checks passed (mocked IPC, 1000x680).');
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; });
