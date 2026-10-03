// Requires cargo build -p project-gateway --example http_direct_smoke and probe.cjs.
const fs = require('fs'), path = require('path'), os = require('os'), cp = require('child_process'), assert = require('assert');
const fixtures = require('./upstream-evidence.json');
const shim = process.argv[2] || path.join(process.env.APPDATA, 'npm/codegraph.cmd');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'codegraph-direct-http-'));
const executable = path.join(directory, 'http_direct_smoke.exe');
try {
    fs.copyFileSync(path.resolve('target/debug/examples/http_direct_smoke.exe'), executable);
    const result = cp.spawnSync(executable, [shim, fixtures.projects[0].dir, fixtures.projects[1].dir], { encoding: 'utf8', windowsHide: true, timeout: 90000 });
    if (result.stderr) process.stderr.write(result.stderr);
    assert.equal(result.status, 0, result.error?.message || 'HTTP direct smoke failed');
    const evidence = JSON.parse(result.stdout.trim());
    fs.writeFileSync(path.join(__dirname, 'http-direct-evidence.json'), JSON.stringify(evidence, null, 2));
    console.log(evidence);
} finally {
    try { fs.unlinkSync(executable); } catch {}
    try { fs.rmdirSync(directory); } catch {}
}
