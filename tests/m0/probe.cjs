// Usage: node tests/m0/probe.cjs [CodeGraph bundle directory]
const fs = require('fs'), os = require('os'), path = require('path');
const cp = require('child_process'), assert = require('assert');
const bundle = process.argv[2] || path.join(process.env.APPDATA, 'npm/node_modules/@colbymchenry/codegraph/node_modules/@colbymchenry/codegraph-win32-x64');
const program = path.join(bundle, 'node.exe');
const prefix = ['--liftoff-only', '--disable-warning=ExperimentalWarning', path.join(bundle, 'lib/dist/bin/codegraph.js')];
const env = { ...process.env, CODEGRAPH_NO_DAEMON: '1' };
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph-m0-'));
function rpc(child) {
  let sequence = 0, buffer = ''; const pending = new Map();
  child.stdout.on('data', chunk => {
    buffer += chunk; let newline;
    while ((newline = buffer.indexOf('\n')) >= 0) {
      const raw = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
      if (!raw) continue;
      const message = JSON.parse(raw);
      if (pending.has(message.id)) { pending.get(message.id)(message); pending.delete(message.id); }
    }
  });
  child.stderr.on('data', () => {});
  return (method, params) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => reject(Error('timeout ' + method)), 20000);
    pending.set(id, message => { clearTimeout(timer); resolve(message); });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}
async function stop(child) {
  if (child.exitCode !== null) return;
  const exited = new Promise(resolve => child.once('exit', resolve));
  child.stdin.end(); const timer = setTimeout(() => child.kill(), 5000);
  await exited; clearTimeout(timer);
}
(async () => {
  const version = cp.spawnSync(program, [...prefix, '--version'], { encoding: 'utf8', windowsHide: true }).stdout.trim();
  const output = { version, root, directMode: true, projects: [] }; let child;
  try {
    for (const label of ['A', 'B']) {
      const dir = path.join(root, label + ' 中文 & project'); fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, 'sample.ts'), `export function uniqueMarker(){return '${label}_ONLY_739';}\n`);
      fs.writeFileSync(path.join(dir, 'secondary.ts'), `export function secondaryMarker(){return '${label}_SECOND_842';}\n`);
      const init = cp.spawnSync(program, [...prefix, 'init', '--yes', dir], { encoding: 'utf8', env, windowsHide: true });
      assert.equal(init.status, 0, init.stderr + init.stdout);
      child = cp.spawn(program, [...prefix, 'serve', '--mcp', '--path', dir], { cwd: dir, env, windowsHide: true });
      const request = rpc(child);
      await request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'm0-probe', version: '0.1' } });
      child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
      const schemas = await request('tools/list', {});
      const result = await request('tools/call', { name: 'codegraph_explore', arguments: { query: 'uniqueMarker', projectPath: dir, maxFiles: 1 } });
      assert(JSON.stringify(result).includes(`${label}_ONLY_739`), 'wrong source');
      assert(!JSON.stringify(result).includes(`${label}_SECOND_842`), 'unexpected secondary source');
      const secondary = await request('tools/call', { name: 'codegraph_explore', arguments: { query: 'secondaryMarker', projectPath: dir, maxFiles: 1 } });
      assert(JSON.stringify(secondary).includes(`${label}_SECOND_842`), 'wrong secondary source');
      assert(!JSON.stringify(secondary).includes(`${label}_ONLY_739`), 'unexpected primary source');
      await stop(child);
      output.projects.push({ label, dir, pid: child.pid, tools: schemas.result.tools, result, secondary, exitCode: child.exitCode }); child = null;
    }
  } finally { if (child) await stop(child); }
  fs.writeFileSync(path.join(__dirname, 'upstream-evidence.json'), JSON.stringify(output, null, 2));
  console.log(JSON.stringify({ version, root, projects: output.projects.map(p => ({ label: p.label, pid: p.pid, sourceVerified: true, exitCode: p.exitCode })) }, null, 2));
})().catch(error => { console.error(error); process.exitCode = 1; });
