import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const version = JSON.parse(readFileSync("package.json", "utf8")).version;
const tag = process.env.GITHUB_REF_NAME;
if (tag !== `v${version}`)
  throw new Error("Release tag does not match package version");
const targets = {
  "windows-amd64": [".exe"],
  "windows-arm64": [".exe"],
  "macos-arm64": [".dmg", ".pkg"],
  "linux-amd64": [".deb", ".AppImage"],
  "linux-arm64": [".deb", ".AppImage"],
};
const directory = "release-assets";
const expected = [];
for (const [target, extensions] of Object.entries(targets)) {
  const checksum = `SHA256SUMS-${target}.txt`;
  expected.push(checksum);
  const lines = readFileSync(join(directory, checksum), "utf8")
    .trim()
    .split("\n");
  if (lines.length !== extensions.length)
    throw new Error(`Unexpected checksum count: ${target}`);
  for (const extension of extensions) {
    const name = `CodeGraph-Desktop-${tag}-${target}${extension}`;
    expected.push(name);
    const digest = createHash("sha256")
      .update(readFileSync(join(directory, name)))
      .digest("hex");
    if (!lines.includes(`${digest}  ${name}`))
      throw new Error(`Checksum mismatch: ${name}`);
  }
}
if (readdirSync(directory).some((name) => !expected.includes(name)))
  throw new Error("Unexpected release asset");
console.log(
  `Verified ${expected.length} release assets from all five native targets`,
);
if (process.argv[2] === "publish") {
  function gh(args, check = true) {
    const result = spawnSync("gh", args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    if (result.error) throw result.error;
    if (check && result.status !== 0) throw new Error(result.stderr);
    return result;
  }
  // Keep incomplete uploads as a draft. A rerun resumes the same release instead of creating duplicates.
  const existing = gh(["release", "view", tag, "--json", "isDraft"], false);
  if (existing.status !== 0)
    gh([
      "release",
      "create",
      tag,
      "--verify-tag",
      "--draft",
      "--title",
      `CodeGraph Desktop ${tag}`,
      "--generate-notes",
      "--notes",
      "Unsigned native packages. macOS packages are not notarized. Windows ARM64 app is native; its NSIS installer runs through x86 emulation. Checksums are supplied for all packages.",
      ...(version.includes("-") ? ["--prerelease"] : []),
    ]);
  gh([
    "release",
    "upload",
    tag,
    ...expected.map((name) => join(directory, name)),
    "--clobber",
  ]);
  gh(["release", "edit", tag, "--draft=false"]);
  console.log(`Published ${tag}`);
} else if (process.argv[2] !== "verify")
  throw new Error("Expected verify or publish");
