// Run after: cargo build -p cg-mcp-connector -p project-gateway --example host
// and node tests/m0/probe.cjs. An optional argument selects the CodeGraph shim.
const cp = require('child_process'), path = require('path'), fs = require('fs'), assert = require('assert'), crypto = require('crypto'), net = require('net');
const evidence = require('./upstream-evidence.json');
const shim = process.argv[2] || path.join(process.env.APPDATA, 'npm/codegraph.cmd');
const binaryDir = fs.mkdtempSync(path.join(require('os').tmpdir(), 'codegraph-m0-binaries-'));
const exe = path.join(binaryDir, 'cg-mcp-connector.exe'), hostExe = path.join(binaryDir, 'host.exe');
fs.copyFileSync(path.resolve('target/debug/cg-mcp-connector.exe'), exe);
fs.copyFileSync(path.resolve('target/debug/examples/host.exe'), hostExe);
const children = [];
const registries = [];
const addedFiles = [];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const alive = pid => { try {
    process.kill(pid, 0);
    return true;
}
catch {
    return false;
} };
function client(id) { const p = cp.spawn(exe, ['--project-id', id], { windowsHide: true }); children.push(p); let buffer = '', pending = new Map(); p.stdout.on('data', d => { buffer += d; let i; while ((i = buffer.indexOf('\n')) >= 0) {
    let row = buffer.slice(0, i);
    buffer = buffer.slice(i + 1);
    if (!row)
        continue;
    const m = JSON.parse(row);
    if (pending.has(m.id)) {
        pending.get(m.id)(m);
        pending.delete(m.id);
    }
} }); p.stderr.on('data', d => process.stderr.write(d)); const send = (method, params, id) => new Promise((resolve, reject) => { const timer = setTimeout(() => reject(Error('timeout ' + method)), 30000); pending.set(id, m => { clearTimeout(timer); resolve(m); }); p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n'); }); return { p, send, async init() { const r = await send('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'm0-connector-client', version: '0.1' } }, 0); assert(!r.error, JSON.stringify(r)); p.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n'); } }; }
async function host(root, existingId) { const id = existingId || crypto.randomUUID(); registries.push(path.join(process.env.APPDATA, 'ai.codegraph.desktop/runtime', id + '.json')); const p = cp.spawn(hostExe, [shim, root, id], { windowsHide: true }); children.push(p); p.stderr.on('data', d => process.stderr.write(d)); const info = await new Promise((resolve, reject) => { let buffer = ''; const timer = setTimeout(() => reject(Error('host timeout')), 40000); p.stdout.on('data', d => { buffer += d; let i = buffer.indexOf('\n'); if (i >= 0) {
    clearTimeout(timer);
    resolve(JSON.parse(buffer.slice(0, i)));
} }); p.on('exit', code => { if (!buffer)
    reject(Error('host exit ' + code)); }); }); return { p, id, ...info }; }
async function stopped(p) { if (p.exitCode !== null)
    return; const exit = new Promise(r => p.once('exit', r)); p.stdin.write('\n'); await exit; assert.equal(p.exitCode, 0); }
function portClosed(port) { return new Promise(resolve => { let socket = net.connect(port, '127.0.0.1'); socket.once('connect', () => { socket.destroy(); resolve(false); }); socket.once('error', () => resolve(true)); }); }
(async () => { try {
    const a = await host(evidence.projects[0].dir), b = await host(evidence.projects[1].dir);
    const a1 = client(a.id), a2 = client(a.id), b1 = client(b.id);
    await Promise.all([a1.init(), a2.init(), b1.init()]);
    const call = (c, id, args = { query: 'uniqueMarker', maxFiles: 1, includeSource: true }) => c.send('tools/call', { name: 'codegraph_explore', arguments: args }, id);
    const tools = await a1.send('tools/list', {}, 1);
    assert.deepStrictEqual(tools.result.tools.map(tool => tool.name).sort(), ['codegraph_callers', 'codegraph_explore', 'codegraph_impact', 'codegraph_search']);
    const lightweight = await call(a1, 2, { query: 'uniqueMarker', maxFiles: 1, projectPath: evidence.projects[0].dir });
    assert(!lightweight.error && lightweight.result.isError !== true, JSON.stringify(lightweight));
    const lightweightText = JSON.stringify(lightweight.result);
    assert(lightweightText.includes('uniqueMarker') && lightweightText.includes('sample.ts'), 'default explore lost symbol name/location');
    assert(!lightweightText.includes('A_ONLY_739'), 'default explore leaked function-body marker');
    for (const [name, arguments_] of [
        ['codegraph_explore', { query: 'uniqueMarker', maxFiles: 1 }],
        ['codegraph_search', { query: 'uniqueMarker', limit: 1 }],
        ['codegraph_callers', { symbol: 'uniqueMarker' }],
        ['codegraph_impact', { symbol: 'uniqueMarker' }],
    ]) {
        const response = await a1.send('tools/call', { name, arguments: { ...arguments_, maxChars: 512 } }, 3);
        assert(!response.error && response.result.isError !== true, name + ' failed on small project: ' + JSON.stringify(response));
        assert([...JSON.stringify(response.result)].length <= 512, name + ' exceeded serialized maxChars');
    }
    const results = await Promise.all([call(a1, 7), call(a2, 7, { query: 'secondaryMarker', maxFiles: 1, includeSource: true }), call(b1, 7)]);
    assert(JSON.stringify(results[0]).includes('A_ONLY_739'));
    assert(JSON.stringify(results[1]).includes('A_SECOND_842'));
    assert(!JSON.stringify(results[0]).includes('A_SECOND_842'));
    assert(!JSON.stringify(results[1]).includes('A_ONLY_739'));
    assert(JSON.stringify(results[2]).includes('B_ONLY_739'));
    const record = JSON.parse(fs.readFileSync(path.join(process.env.APPDATA, 'ai.codegraph.desktop/runtime', a.id + '.json')));
    const post = (endpoint, headers, body) => headers.host ? new Promise((resolve, reject) => { const request = require('http').request(endpoint, { method: 'POST', headers: { 'content-type': 'application/json', ...headers } }, response => { response.resume(); resolve({ status: response.statusCode }); }); request.on('error', reject); request.end(JSON.stringify(body || {})); }) : fetch(endpoint, { method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', ...headers }, body: JSON.stringify(body || {}) });
    for (const headers of [{ authorization: 'Bearer wrong' }, { authorization: 'Bearer ' + record.token, origin: 'http://untrusted.example' }, { authorization: 'Bearer ' + record.token, host: 'untrusted.example' }]) {
        assert.equal((await post(record.endpoint, headers)).status, 401);
    }
    const init = await post(record.endpoint, { authorization: 'Bearer ' + record.token }, { jsonrpc: '2.0', id: 55, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'old-session', version: '0.1' } } });
    assert.equal(init.status, 200);
    const oldSession = init.headers.get('mcp-session-id');
    assert(oldSession);
    await init.body.cancel();
    const forbidden = await call(a1, 8, { query: 'uniqueMarker', projectPath: evidence.projects[1].dir });
    assert(forbidden.error, 'cross project should reject');
    for (const bad of [{ query: 'uniqueMarker', projectPath: path.dirname(evidence.projects[0].dir) }, { query: 'uniqueMarker', projectPath: path.join(evidence.projects[0].dir, '..', path.basename(evidence.projects[1].dir)) }, { query: 'uniqueMarker', filePath: path.join(evidence.projects[1].dir, 'sample.ts') }]) {
        assert((await call(a1, 80, bad)).error, 'path boundary should reject');
    }
    const junction = path.join(evidence.projects[0].dir, 'outside-junction');
    fs.symlinkSync(evidence.projects[1].dir, junction, 'junction');
    try {
        assert((await call(a1, 82, { query: 'uniqueMarker', projectPath: junction })).error, 'junction escape should reject');
    }
    finally {
        fs.unlinkSync(junction);
    }
    const absoluteQuery = await call(a1, 81, { query: path.join(evidence.projects[1].dir, 'sample.ts'), maxFiles: 1, includeSource: true });
    assert(!JSON.stringify(absoluteQuery).includes('B_ONLY_739'), 'natural query leaked B');
    const watchedName = 'watcherAdded' + Date.now();
    const watchedMarker = 'WATCHER_NEW_' + crypto.randomUUID();
    const watchedFile = path.join(evidence.projects[0].dir, watchedName + '.ts');
    const before = await call(a1, 90, { query: watchedName, maxFiles: 1, includeSource: true });
    assert(!JSON.stringify(before).includes(watchedMarker));
    fs.writeFileSync(watchedFile, `export function ${watchedName}(){return '${watchedMarker}_${'padding'.repeat(200)}';}\n`);
    addedFiles.push(watchedFile);
    let watcherUpdated = false;
    for (let attempt = 0; attempt < 30; attempt++) {
        await delay(1000);
        const updated = await call(a1, 91 + attempt, { query: watchedName, maxFiles: 1, includeSource: true });
        if (JSON.stringify(updated).includes(watchedMarker)) {
            watcherUpdated = true;
            break;
        }
    }
    assert(watcherUpdated, 'watcher did not index newly added symbol');
    const watchedDefault = await call(a1, 121, { query: watchedName, maxFiles: 1 });
    assert(!watchedDefault.error && watchedDefault.result.isError !== true, JSON.stringify(watchedDefault));
    const watchedDefaultText = JSON.stringify(watchedDefault.result);
    assert(watchedDefaultText.includes(watchedName) && watchedDefaultText.includes(watchedName + '.ts'), 'default query lost newly indexed symbol/location');
    assert(!watchedDefaultText.includes(watchedMarker), 'default query leaked random function-body marker');
    const truncated = await call(a1, 122, { query: watchedName, maxFiles: 1, includeSource: true, maxChars: 512 });
    assert(!truncated.error && truncated.result.isError !== true, JSON.stringify(truncated));
    assert([...JSON.stringify(truncated.result)].length <= 512, 'source response exceeded serialized maxChars');
    assert.equal(truncated.result.content.length, 1);
    assert.equal(truncated.result.content[0].type, 'text');
    assert(truncated.result.content[0].text.includes('[TRUNCATED:'), 'oversized source did not report truncation');
    assert(truncated.result.content[0].text.includes('only shown excerpts are available; narrow query or use LSP, not a complete file read'), 'truncation warning missing source completeness guidance');
    a1.p.stdin.end();
    a2.p.stdin.end();
    await stopped(a.p);
    assert(await portClosed(a.port), 'port still bound');
    const continued = await call(b1, 9);
    assert(JSON.stringify(continued).includes('B_ONLY_739'), 'B affected by stopping A');
    const restarted = await host(evidence.projects[0].dir, a.id);
    assert.notEqual(restarted.generation, a.generation);
    const current = JSON.parse(fs.readFileSync(path.join(process.env.APPDATA, 'ai.codegraph.desktop/runtime', a.id + '.json')));
    assert.equal((await post(current.endpoint, { authorization: 'Bearer ' + record.token })).status, 401);
    const old = await post(current.endpoint, { authorization: 'Bearer ' + current.token, 'mcp-session-id': oldSession }, { jsonrpc: '2.0', id: 56, method: 'tools/list', params: {} });
    assert([400, 401, 404].includes(old.status), 'old session not rejected: ' + old.status);
    await old.body?.cancel();
    await stopped(restarted.p);
    const crash = await host(evidence.projects[0].dir);
    assert(alive(crash.pid), 'upstream must be alive before crash');
    const listing = cp.spawnSync('powershell.exe', ['-NoProfile', '-Command', `$all=Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId; $all | ConvertTo-Json -Compress`], { encoding: 'utf8', windowsHide: true });
    assert.equal(listing.status, 0);
    const processes = JSON.parse(listing.stdout);
    const descendants = new Set([crash.pid]);
    let changed = true;
    while (changed) {
        changed = false;
        for (const process of processes) {
            if (descendants.has(process.ParentProcessId) && !descendants.has(process.ProcessId)) {
                descendants.add(process.ProcessId);
                changed = true;
            }
        }
    }
    const exited = new Promise(resolve => crash.p.once('exit', resolve));
    crash.p.kill('SIGKILL');
    await exited;
    let crashClean = false;
    for (let attempt = 0; attempt < 50; attempt++) {
        if ([...descendants].every(pid => !alive(pid)) && await portClosed(crash.port)) {
            crashClean = true;
            break;
        }
        await delay(100);
    }
    assert(crashClean, 'host crash left owned process or port');
    assert(JSON.stringify(await call(b1, 130)).includes('B_ONLY_739'), 'crashing A affected B');
    b1.p.stdin.end();
    await stopped(b.p);
    assert(await portClosed(b.port));
    const report = { defaultExploreSourceFree: true, lightweightToolsCallable: true, serializedMaxCharsRespected: true, sourceTruncationReported: true, watcherNewSymbolIndexed: true, hostCrashProcessesReleased: true, hostCrashPortReleased: true, crashOwnedPids: [...descendants], sameIdRouting: true, dualConnectorOneUpstream: true, crossProjectRejected: true, stopAAllowsB: true, portsReleased: true, wrongTokenHostOriginRejected: true, oldGenerationSessionRejected: true, upstreamPids: [a.pid, b.pid] };
    fs.writeFileSync(path.join(__dirname, 'gateway-evidence.json'), JSON.stringify(report, null, 2));
    console.log(report);
}
finally {
    for (const p of children) {
        if (p.exitCode === null) {
            p.stdin.end();
            p.kill();
        }
    }
    for (const f of [...registries, ...addedFiles]) {
        try {
            fs.unlinkSync(f);
        }
        catch { }
    }
    await delay(200);
    for (const f of [exe, hostExe]) {
        try {
            fs.unlinkSync(f);
        }
        catch { }
    }
    try {
        fs.rmdirSync(binaryDir);
    }
    catch { }
} })().catch(e => { console.error(e); process.exitCode = 1; });
