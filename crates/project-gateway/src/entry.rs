use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodeGraphEntry {
    pub program: PathBuf,
    pub prefix_args: Vec<String>,
}
fn platform_package(os: &str, arch: &str) -> anyhow::Result<String> {
    let platform = match os {
        "windows" => "win32",
        "macos" => "darwin",
        "linux" => "linux",
        _ => anyhow::bail!("不支持的 CodeGraph 平台"),
    };
    let architecture = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => anyhow::bail!("不支持的 CodeGraph 架构"),
    };
    Ok(format!("codegraph-{platform}-{architecture}"))
}
fn bundled_entry(base: &Path) -> Option<CodeGraphEntry> {
    if cfg!(windows) {
        let program = base.join("node.exe");
        let entry = base.join("lib/dist/bin/codegraph.js");
        if program.is_file() && entry.is_file() {
            return Some(CodeGraphEntry {
                program: dunce::canonicalize(program).ok()?,
                prefix_args: vec![
                    "--liftoff-only".into(),
                    "--disable-warning=ExperimentalWarning".into(),
                    dunce::canonicalize(entry).ok()?.to_string_lossy().into(),
                ],
            });
        }
    } else {
        // The official Unix launcher execs its bundled runtime with required flags.
        let launcher = base.join("bin/codegraph");
        if launcher.is_file() {
            return Some(CodeGraphEntry {
                program: dunce::canonicalize(launcher).ok()?,
                prefix_args: vec![],
            });
        }
    }
    None
}
pub fn resolve_entry(path: &Path) -> anyhow::Result<CodeGraphEntry> {
    let p = dunce::canonicalize(path)?;
    let extension = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ["cmd", "ps1", "js"].contains(&extension.as_str()) {
        let parent = p
            .parent()
            .ok_or_else(|| anyhow::anyhow!("CodeGraph 入口没有父目录"))?;
        let package = platform_package(std::env::consts::OS, std::env::consts::ARCH)?;
        let candidates = [
            parent.join(format!(
                "node_modules/@colbymchenry/codegraph/node_modules/@colbymchenry/{package}"
            )),
            parent.join(format!("node_modules/@colbymchenry/{package}")),
            parent.join(format!("../{package}")),
            parent.join(".."),
        ];
        for base in candidates {
            if let Some(entry) = bundled_entry(&base) {
                return Ok(entry);
            }
        }
        anyhow::bail!("无法找到当前平台的 CodeGraph bundle ({package})，请安装对应 optional dependency 或选择官方独立入口");
    }
    Ok(CodeGraphEntry {
        program: p,
        prefix_args: vec![],
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_package_names() {
        for (os, arch, expected) in [
            ("windows", "x86_64", "codegraph-win32-x64"),
            ("windows", "aarch64", "codegraph-win32-arm64"),
            ("macos", "aarch64", "codegraph-darwin-arm64"),
            ("linux", "x86_64", "codegraph-linux-x64"),
            ("linux", "aarch64", "codegraph-linux-arm64"),
        ] {
            assert_eq!(platform_package(os, arch).unwrap(), expected)
        }
        assert!(platform_package("linux", "x86").is_err());
    }
}
