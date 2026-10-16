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
  "windows-amd64": [".exe", ".exe.sig"],
  "windows-arm64": [".exe", ".exe.sig"],
  "macos-arm64": [".dmg", ".pkg", ".app.tar.gz", ".app.tar.gz.sig"],
  "linux-amd64": [".deb", ".AppImage", ".AppImage.sig"],
  "linux-arm64": [".deb", ".AppImage", ".AppImage.sig"],
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
      const bytes = extension.endsWith(".sig")
        ? Buffer.from(
            Buffer.from(
              "untrusted comment: fixture\nfixture\ntrusted comment: timestamp:1\tversion:0.1.4\nfixture\n",
            ).toString("base64"),
          )
        : Buffer.from(`test fixture ${name}`);
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
    const manifest = JSON.parse(
      readFileSync(join(directory, "release-assets", "latest.json"), "utf8"),
    );
    assert.equal(manifest.version, "0.1.4");
    assert.equal(Object.keys(manifest.platforms).length, 5);
    assert(
      manifest.platforms["darwin-aarch64"].url.endsWith(
        "-macos-arm64.app.tar.gz",
      ),
    );
    assert(
      manifest.platforms["windows-aarch64"].url.endsWith("-windows-arm64.exe"),
    );
    assert(
      manifest.platforms["linux-x86_64"].url.endsWith("-linux-amd64.AppImage"),
    );
    assert(manifest.platforms["darwin-aarch64"].signature.length > 0);
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

test("release rejects a signature for a different version even with a matching checksum", () => {
  const directory = fixture();
  try {
    const name = "CodeGraph-Desktop-v0.1.4-windows-amd64.exe.sig";
    const path = join(directory, "release-assets", name);
    const original = readFileSync(path);
    const changed = Buffer.from(
      Buffer.from(
        Buffer.from(original.toString(), "base64")
          .toString()
          .replace("version:0.1.4", "version:0.1.3"),
      ).toString("base64"),
    );
    writeFileSync(path, changed);
    const checksum = join(
      directory,
      "release-assets",
      "SHA256SUMS-windows-amd64.txt",
    );
    writeFileSync(
      checksum,
      readFileSync(checksum, "utf8").replace(
        createHash("sha256").update(original).digest("hex"),
        createHash("sha256").update(changed).digest("hex"),
      ),
    );
    assert.match(verify(directory).stderr, /signed version/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
