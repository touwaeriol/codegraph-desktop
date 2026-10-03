// Uses installed, authenticated Codex and Claude Code against a disposable project.
// Credentials stay in the clients' existing configuration; only sanitized results are saved here.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const cp = require('node:child_process');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');

const workspace = path.resolve(__dirname, '../..');
const sourceConnector = path.join(workspace, 'target/debug/cg-mcp-connector.exe');
const sourceHost = path.join(workspace, 'target/debug/examples/host.exe');
const npmRoot = path.join(process.env.APPDATA, 'npm');
const bundle = path.join(npmRoot, 'node_modules/@colbymchenry/codegraph/node_modules/@colbymchenry/codegraph-win32-x64');
const codexJs = path.join(npmRoot, 'node_modules/@openai/codex/bin/codex.js');
const claudeExe = path.join(npmRoot, 'node_modules/@anthropic-ai/claude-code/bin/claude.exe');
const children = [];
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph-real-clients-'));
const binaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph-test-bin-'));
const connector = path.join(binaryDirectory, 'cg-mcp-connector.exe');
const hostExe = path.join(binaryDirectory, 'host.exe');
const codexOnly = process.argv.includes('--codex-only');
const directHttp = !process.argv.includes('--stdio');
const modelIndex = process.argv.indexOf('--claude-model');
const claudeModel = modelIndex < 0 ? null : process.argv[modelIndex + 1];
const logsDirectory = path.join(workspace, '.tools/real-clients');
fs.mkdirSync(logsDirectory, { recursive: true });
const marker = 'VERIFIED_' + crypto.randomBytes(12).toString('hex');
const projectId = crypto.randomUUID();
const serverName = 'codegraph_validation';
let host;
let gatewayToken = '';
const redact = text => gatewayToken ? text.split(gatewayToken).join('[REDACTED]') : text;

function killOwned(child) {
  if (child?.pid && child.exitCode === null) {
    cp.spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
  }
}

function runClient(name, program, args) {
  return new Promise(resolve => {
    const child = cp.spawn(program, args, { cwd: temporary, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    children.push(child);
    let stdout = '', stderr = '';
    const timer = setTimeout(() => killOwned(child), 300000);
    child.stdout.on('data', b => { stdout += b.toString(); });
    child.stderr.on('data', b => { stderr += b.toString(); });
    child.on('error', e => { clearTimeout(timer); resolve({ name, exitCode: -1, events: [], errorCode: e.code }); });
    child.on('close', code => {
      clearTimeout(timer);
      fs.writeFileSync(path.join(logsDirectory, name + '.jsonl'), redact(stdout));
      fs.writeFileSync(path.join(logsDirectory, name + '.stderr.log'), redact(stderr));
      const events = stdout.split(/\r?\n/).filter(Boolean).flatMap(line => { try { return [JSON.parse(line)]; } catch { return []; } });
      resolve({ name, exitCode: code, events });
    });
  });
}

async function main() {
  for (const file of [...(directHttp ? [] : [sourceConnector]), sourceHost, codexJs, ...(codexOnly ? [] : [claudeExe])]) assert(fs.existsSync(file), 'Required executable is missing: ' + file);
  if (!directHttp) fs.copyFileSync(sourceConnector, connector);
  fs.copyFileSync(sourceHost, hostExe);
  fs.writeFileSync(path.join(temporary, 'sample.ts'), `export function realClientValidationMarker() { return '${marker}'; }\n`);
  const init = cp.spawnSync(path.join(bundle, 'node.exe'), ['--liftoff-only', '--disable-warning=ExperimentalWarning', path.join(bundle, 'lib/dist/bin/codegraph.js'), 'init', '--yes', temporary], { cwd: temporary, windowsHide: true, encoding: 'utf8', timeout: 90000, env: { ...process.env, CODEGRAPH_NO_DAEMON: '1' } });
  assert.equal(init.status, 0, 'CodeGraph fixture initialization failed');
  fs.mkdirSync(path.join(temporary, '.codex'));
  const claudeConfig = path.join(temporary, '.mcp.json');

  host = cp.spawn(hostExe, [path.join(npmRoot, 'codegraph.cmd'), temporary, projectId], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let hostError = '';
  host.stderr.on('data', b => { hostError += b; });
  const info = await new Promise((resolve, reject) => {
    let data = '';
    const timer = setTimeout(() => reject(Error('Gateway startup timed out')), 60000);
    host.stdout.on('data', b => { data += b; const end = data.indexOf('\n'); if (end >= 0) { clearTimeout(timer); try { resolve(JSON.parse(data.slice(0, end))); } catch (e) { reject(e); } } });
    host.once('error', reject);
    host.once('exit', code => { clearTimeout(timer); reject(Error('Gateway exited before startup: ' + code)); });
  });

  const runtime = JSON.parse(fs.readFileSync(path.join(process.env.APPDATA, 'ai.codegraph.desktop/runtime', projectId + '.json'), 'utf8'));
  gatewayToken = runtime.token;
  const codexTransport = directHttp
    ? `url = ${JSON.stringify(runtime.endpoint)}\nhttp_headers = { Authorization = "Bearer ${gatewayToken}" }\n`
    : `command = ${JSON.stringify(connector)}\nargs = ["--project-id", "${projectId}"]\n`;
  fs.writeFileSync(path.join(temporary, '.codex/config.toml'), `[mcp_servers.${serverName}]\n${codexTransport}required = true\nstartup_timeout_sec = 30\ntool_timeout_sec = 120\ndefault_tools_approval_mode = "approve"\n`);
  fs.writeFileSync(claudeConfig, JSON.stringify({ mcpServers: { [serverName]: directHttp
    ? { type: 'http', url: runtime.endpoint, headers: { Authorization: `Bearer ${gatewayToken}` } }
    : { type: 'stdio', command: connector, args: ['--project-id', projectId] } } }, null, 2));

  const prompt = 'Use only the codegraph_validation MCP server codegraph_explore tool to query realClientValidationMarker with maxFiles 1. Do not use shell, filesystem, search or other tools. Return the exact string returned by that function. You must actually call the MCP tool; do not guess.';
  const codexArgs = [codexJs, 'exec', '--skip-git-repo-check', '--ephemeral', '--sandbox', 'read-only', '--json', '-C', temporary, '-c', `projects.${JSON.stringify(temporary)}.trust_level="trusted"`];
  // Explicit per-invocation config exercises the client without persisting project trust.
  // Loading .codex/config.toml after interactive project trust remains a separate manual check.
  if (directHttp) codexArgs.push('-c', `mcp_servers.${serverName}.url=${JSON.stringify(runtime.endpoint)}`,
    '-c', `mcp_servers.${serverName}.http_headers={Authorization="Bearer ${gatewayToken}"}`);
  else codexArgs.push('-c', `mcp_servers.${serverName}.command=${JSON.stringify(connector)}`,
    '-c', `mcp_servers.${serverName}.args=["--project-id","${projectId}"]`);
  codexArgs.push('-c', `mcp_servers.${serverName}.required=true`,
    '-c', `mcp_servers.${serverName}.default_tools_approval_mode="approve"`);
  // Disable unrelated globally configured MCP servers for this invocation only.
  const userConfigPath = path.join(process.env.CODEX_HOME || path.join(os.homedir(), '.codex'), 'config.toml');
  if (fs.existsSync(userConfigPath)) {
    for (const match of fs.readFileSync(userConfigPath, 'utf8').matchAll(/^\[mcp_servers\.([\w-]+)\]\s*$/gm)) {
      if (match[1] !== serverName) codexArgs.push('-c', `mcp_servers.${match[1]}.enabled=false`);
    }
  }
  codexArgs.push(prompt);
  const claudeArgs = ['--print', '--verbose', '--output-format', 'stream-json', '--no-session-persistence', '--strict-mcp-config', '--mcp-config', claudeConfig, '--tools', '', '--allowedTools', `mcp__${serverName}__codegraph_explore`, '--max-budget-usd', '1', prompt];
  if (claudeModel) claudeArgs.push('--model', claudeModel);
  const results = await Promise.all([runClient('codex', process.execPath, codexArgs), ...(codexOnly ? [] : [runClient('claude', claudeExe, claudeArgs)])]);
  const clients = results.map(result => {
    let callSeen = false, markerSeen = false;
    for (const event of result.events) {
      if (result.name === 'codex' && event.type === 'item.completed') {
        if (event.item?.type === 'mcp_tool_call' && event.item?.server === serverName && event.item?.tool === 'codegraph_explore') callSeen = true;
        if (event.item?.type === 'agent_message' && event.item.text?.includes(marker)) markerSeen = true;
      }
      if (result.name === 'claude') {
        for (const item of event.message?.content || []) if (item.type === 'tool_use' && item.name === `mcp__${serverName}__codegraph_explore`) callSeen = true;
        if (event.type === 'result' && event.result?.includes(marker)) markerSeen = true;
      }
    }
    return { name: result.name, exitCode: result.exitCode, toolCallObserved: callSeen, exactMarkerReturned: markerSeen, passed: result.exitCode === 0 && callSeen && markerSeen };
  });
  const report = { checkedAt: new Date().toISOString(), scope: codexOnly ? 'codex-only' : 'both-clients', transport: directHttp ? 'streamable-http-direct' : 'stdio-connector', configMode: 'explicit-invocation', claudeModel: codexOnly ? null : claudeModel || 'configured-default', projectId, upstreamPid: info.pid, clients, passed: clients.every(c => c.passed) };
  fs.writeFileSync(path.join(__dirname, codexOnly ? 'codex-evidence.json' : 'real-client-evidence.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  if (!report.passed) process.exitCode = 1;
  fs.writeFileSync(path.join(logsDirectory, 'host.stderr.log'), redact(hostError));
}

main().catch(error => { console.error(redact(error.message)); process.exitCode = 1; }).finally(async () => {
  for (const child of children) killOwned(child);
  if (host?.exitCode === null) {
    host.stdin.end('\n');
    await Promise.race([new Promise(resolve => host.once('exit', resolve)), new Promise(resolve => setTimeout(resolve, 10000))]);
    killOwned(host);
  }
  const record = path.join(process.env.APPDATA, 'ai.codegraph.desktop/runtime', projectId + '.json');
  try { fs.unlinkSync(record); } catch (error) { if (error.code !== 'ENOENT') console.error('Temporary runtime record cleanup failed'); }
  for (const executable of [connector, hostExe]) { try { fs.unlinkSync(executable); } catch {} }
  try { fs.rmdirSync(binaryDirectory); } catch {}
  for (const config of [path.join(temporary, '.codex/config.toml'), path.join(temporary, '.mcp.json')]) { try { fs.unlinkSync(config); } catch {} }
});
