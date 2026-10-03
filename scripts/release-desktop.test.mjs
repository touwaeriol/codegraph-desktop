import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const script = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "release-desktop.mjs",
);
const targets = {
  "windows-amd64": [".exe"],
  "windows-arm64": [".exe"],
  "macos-arm64": [".dmg", ".pkg"],
  "linux-amd64": [".deb", ".AppImage"],
  "linux-arm64": [".deb", ".AppImage"],
};
function fixture() {
  const directory = mkdtempSync(join(tmpdir(), "codegraph-release-test-"));
  writeFileSync(
    join(directory, "package.json"),
    JSON.stringify({ version: "0.1.4" }),
  );
  mkdirSync(join(directory, "release-assets"));
  for (const [target, extensions] of Object.entries(targets)) {
    const checksums = [];
    for (const extension of extensions) {
      const name = `CodeGraph-Desktop-v0.1.4-${target}${extension}`;
      const bytes = Buffer.from(`test fixture ${name}`);
      writeFileSync(join(directory, "release-assets", name), bytes);
      checksums.push(
        `${createHash("sha256").update(bytes).digest("hex")}  ${name}`,
      );
    }
    writeFileSync(
      join(directory, "release-assets", `SHA256SUMS-${target}.txt`),
      `${checksums.join("\n")}\n`,
    );
  }
  return directory;
}
function verify(cwd, tag = "v0.1.4") {
  return spawnSync(process.execPath, [script, "verify"], {
    cwd,
    env: { ...process.env, GITHUB_REF_NAME: tag },
    encoding: "utf8",
  });
}
test("release validation requires all native packages and exact checksums", () => {
  const directory = fixture();
  try {
    assert.equal(verify(directory).status, 0);
    assert.notEqual(verify(directory, "v0.1.5").status, 0);
    const asset = join(
      directory,
      "release-assets",
      "CodeGraph-Desktop-v0.1.4-macos-arm64.pkg",
    );
    const original = readFileSync(asset);
    writeFileSync(asset, "tampered");
    assert.match(verify(directory).stderr, /Checksum mismatch/);
    writeFileSync(asset, original);
    rmSync(asset);
    assert.notEqual(verify(directory).status, 0);
  } finally {
    assert.equal(dirname(directory), resolve(tmpdir()));
    rmSync(directory, { recursive: true, force: true });
  }
});
