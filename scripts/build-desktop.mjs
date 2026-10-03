import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
process.chdir(root);
const mode = process.argv[2] ?? "Check";
const supported = new Map([
  [
    "x86_64-pc-windows-msvc",
    {
      platform: "windows",
      arch: "amd64",
      bundles: "nsis",
      extensions: [".exe"],
    },
  ],
  [
    "aarch64-pc-windows-msvc",
    {
      platform: "windows",
      arch: "arm64",
      bundles: "nsis",
      extensions: [".exe"],
    },
  ],
  [
    "aarch64-apple-darwin",
    {
      platform: "macos",
      arch: "arm64",
      bundles: "dmg",
      extensions: [".dmg", ".pkg"],
    },
  ],
  [
    "x86_64-unknown-linux-gnu",
    {
      platform: "linux",
      arch: "amd64",
      bundles: "deb,appimage",
      extensions: [".deb", ".AppImage"],
    },
  ],
  [
    "aarch64-unknown-linux-gnu",
    {
      platform: "linux",
      arch: "arm64",
      bundles: "deb,appimage",
      extensions: [".deb", ".AppImage"],
    },
  ],
]);
function run(program, args, capture = false) {
  const result = spawnSync(program, args, {
    cwd: root,
    encoding: "utf8",
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${program} exited with ${result.status}`);
  return result.stdout?.trim();
}
function node(relative, ...args) {
  return run(process.execPath, [join(root, relative), ...args]);
}
function verifyVersions(tag) {
  const version = JSON.parse(readFileSync("package.json", "utf8")).version;
  const tauri = JSON.parse(
    readFileSync("src-tauri/tauri.conf.json", "utf8"),
  ).version;
  const cargo = readFileSync("src-tauri/Cargo.toml", "utf8").match(
    /^version\s*=\s*"([^"]+)"/m,
  )?.[1];
  const lock = JSON.parse(readFileSync("package-lock.json", "utf8"));
  if (
    !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version) ||
    [tauri, cargo, lock.version, lock.packages[""].version].some(
      (v) => v !== version,
    )
  )
    throw new Error(
      "package.json, package-lock.json, Tauri and desktop Cargo versions must match",
    );
  if (tag && tag !== `v${version}`)
    throw new Error(`Tag ${tag} does not match v${version}`);
  console.log(`Version verified: ${tag ?? version}`);
  return version;
}
if (!["Check", "Dev", "Bundle", "Prepare", "VerifyVersion"].includes(mode))
  throw new Error(`Unknown mode: ${mode}`);
const version = verifyVersions(
  mode === "VerifyVersion"
    ? process.argv[3] || process.env.RELEASE_TAG
    : undefined,
);
if (mode !== "VerifyVersion") {
  const host = run("rustc", ["--print", "host-tuple"], true);
  const platform = supported.get(host);
  if (!platform) throw new Error(`Unsupported native host: ${host}`);
  if (process.env.BUILD_TARGET && process.env.BUILD_TARGET !== host)
    throw new Error(
      `Expected ${process.env.BUILD_TARGET}, got ${host}; install the native Rust host toolchain`,
    );
  if (process.env.CARGO_BUILD_TARGET && process.env.CARGO_BUILD_TARGET !== host)
    throw new Error(
      "Cross compilation is not supported; run on a native runner",
    );
  if (!existsSync("node_modules"))
    throw new Error("Run npm ci before this script");
  const metadata = JSON.parse(
    run(
      "cargo",
      ["metadata", "--no-deps", "--format-version", "1", "--locked"],
      true,
    ),
  );
  const targetDir = metadata.target_directory;
  const profile = mode === "Bundle" ? "release" : "debug";
  const buildArgs = ["build", "--locked", "-p", "cg-mcp-connector"];
  if (profile === "release") buildArgs.push("--release");
  run("cargo", buildArgs);
  const extension = process.platform === "win32" ? ".exe" : "";
  const buildDir = process.env.CARGO_BUILD_TARGET
    ? join(targetDir, host)
    : targetDir;
  const source = join(buildDir, profile, `cg-mcp-connector${extension}`);
  const sidecar = join(
    root,
    "src-tauri",
    "binaries",
    `cg-mcp-connector-${host}${extension}`,
  );
  mkdirSync(dirname(sidecar), { recursive: true });
  copyFileSync(source, sidecar);
  if (mode === "Check") {
    node("node_modules/typescript/bin/tsc", "-b");
    node("node_modules/vite/bin/vite.js", "build");
    run("cargo", ["fmt", "--all", "--", "--check"]);
    run("cargo", ["test", "--locked", "--workspace"]);
    run("cargo", [
      "clippy",
      "--locked",
      "--workspace",
      "--all-targets",
      "--",
      "-D",
      "warnings",
    ]);
  } else if (mode === "Dev") {
    node("node_modules/@tauri-apps/cli/tauri.js", "dev");
  } else if (mode === "Bundle") {
    node(
      "node_modules/@tauri-apps/cli/tauri.js",
      "build",
      "--bundles",
      platform.bundles,
    );
    const bundleRoot = join(buildDir, "release", "bundle");
    if (process.platform === "darwin") {
      const scripts = join(buildDir, "release", "pkg-scripts");
      mkdirSync(scripts, { recursive: true });
      copyFileSync(
        join(root, "scripts", "install", "stop-before-upgrade.sh"),
        join(scripts, "preinstall"),
      );
      chmodSync(join(scripts, "preinstall"), 0o755);
      const app = join(bundleRoot, "macos", "CodeGraph Desktop.app");
      run("pkgbuild", [
        "--component",
        app,
        "--install-location",
        "/Applications",
        "--identifier",
        "ai.codegraph.desktop",
        "--version",
        version,
        "--scripts",
        scripts,
        join(bundleRoot, `CodeGraph-Desktop_${version}_unsigned.pkg`),
      ]);
    }
    const output = join(
      root,
      "target",
      "release-assets",
      `${platform.platform}-${platform.arch}`,
    );
    mkdirSync(output, { recursive: true });
    const files = readdirSync(bundleRoot, {
      recursive: true,
      withFileTypes: true,
    })
      .filter((e) => e.isFile())
      .map((e) => join(e.parentPath, e.name));
    const hashes = [];
    for (const suffix of platform.extensions) {
      const matching = files.filter(
        (p) => p.endsWith(suffix) && p.includes(`_${version}_`),
      );
      if (matching.length !== 1)
        throw new Error(
          `Expected one current-version ${suffix} package in ${bundleRoot}, found ${matching.length}`,
        );
      const name = `CodeGraph-Desktop-v${version}-${platform.platform}-${platform.arch}${suffix}`;
      const bytes = readFileSync(matching[0]);
      writeFileSync(join(output, name), bytes);
      hashes.push(
        `${createHash("sha256").update(bytes).digest("hex")}  ${name}`,
      );
    }
    writeFileSync(
      join(output, `SHA256SUMS-${platform.platform}-${platform.arch}.txt`),
      `${hashes.join("\n")}\n`,
    );
    console.log(`Unsigned native packages and SHA-256 checksums: ${output}`);
  }
}
