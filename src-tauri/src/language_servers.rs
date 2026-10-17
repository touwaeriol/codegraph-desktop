//! Language servers Serena expects on PATH but never installs itself.
//!
//! Before Serena starts, and again when it reports a language server that
//! failed to start, missing servers with a known recipe are installed into the
//! application data directory. That directory is prepended to Serena's PATH,
//! so installs neither touch the user's environment nor break when the
//! toolchain version changes.
use crate::{models::*, AppState};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, BufReader};

struct Recipe {
    /// Serena language id, as written in `.serena/project.yml`.
    language: &'static str,
    server: &'static str,
    toolchain: &'static str,
    toolchain_name: &'static str,
    args: &'static [&'static str],
    /// Variable that points the toolchain's installer at our directory.
    target_env: &'static str,
}
const RECIPES: &[Recipe] = &[Recipe {
    language: "go",
    server: "gopls",
    toolchain: "go",
    toolchain_name: "Go",
    args: &["install", "golang.org/x/tools/gopls@latest"],
    target_env: "GOBIN",
}];

pub fn dir(state: &AppState) -> PathBuf {
    state.data.join("language-servers").join("bin")
}

/// Languages from `.serena/project.yml`; `project.local.yml` takes precedence.
pub fn configured(root: &Path) -> Vec<String> {
    let dir = root.join(".serena");
    ["project.local.yml", "project.yml"]
        .iter()
        .find_map(|file| {
            std::fs::read_to_string(dir.join(file))
                .ok()
                .and_then(|text| parse_languages(&text))
        })
        .unwrap_or_default()
}
fn parse_languages(text: &str) -> Option<Vec<String>> {
    fn strip(line: &str) -> &str {
        let line = line.trim();
        if line.starts_with('#') {
            return "";
        }
        line.split(" #").next().unwrap_or_default().trim()
    }
    fn clean(item: &str) -> String {
        item.trim().trim_matches(['"', '\'']).to_string()
    }
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        // Top-level keys only; `languages` is the pre-rename spelling.
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        if key != "language_servers" && key != "languages" {
            continue;
        }
        let rest = strip(rest);
        if let Some(inline) = rest.strip_prefix('[') {
            return Some(
                inline
                    .trim_end_matches(']')
                    .split(',')
                    .map(clean)
                    .filter(|s| !s.is_empty())
                    .collect(),
            );
        }
        if !rest.is_empty() {
            return None;
        }
        let mut languages = vec![];
        for line in lines.by_ref() {
            let item = strip(line);
            if item.is_empty() {
                continue;
            }
            match item.strip_prefix('-') {
                Some(value) => languages.push(clean(value)),
                None => break,
            }
        }
        return Some(languages);
    }
    None
}

/// Installs every missing server with a recipe among `languages`, searching
/// and installing with Serena's `path`. Returns whether anything was installed.
pub async fn ensure(
    app: &tauri::AppHandle,
    id: &str,
    root: &Path,
    path: &OsStr,
    languages: &[String],
) -> Result<bool> {
    let state = app.state::<AppState>();
    let mut installed = false;
    for recipe in RECIPES
        .iter()
        .filter(|r| languages.iter().any(|l| l == r.language))
    {
        if which::which_in(recipe.server, Some(path), root).is_ok() {
            continue;
        }
        let zh = crate::i18n::current() == "zh-CN";
        let toolchain = which::which_in(recipe.toolchain, Some(path), root).map_err(|_| {
            AppError::new(
                "LANGUAGE_SERVER_MISSING",
                if zh {
                    format!(
                        "项目使用 {0}，但未找到 {1} 命令。请先安装 {0}，再重启 Serena，{2} 会自动安装",
                        recipe.toolchain_name, recipe.toolchain, recipe.server
                    )
                } else {
                    format!(
                        "This project uses {0}, but the {1} command was not found. Install {0} and restart Serena; {2} will be installed automatically",
                        recipe.toolchain_name, recipe.toolchain, recipe.server
                    )
                },
            )
        })?;
        let target = dir(&state);
        std::fs::create_dir_all(&target)?;
        state.log(
            app,
            id,
            "info",
            "serena",
            &if zh {
                format!(
                    "正在安装 {} 语言服务器 {}…",
                    recipe.toolchain_name, recipe.server
                )
            } else {
                format!(
                    "Installing the {} language server {}…",
                    recipe.toolchain_name, recipe.server
                )
            },
        );
        let failed = || {
            AppError::new(
                "LANGUAGE_SERVER_INSTALL_FAILED",
                if zh {
                    format!(
                        "{} 自动安装失败，请检查网络或代理设置后重启 Serena，详情见运行日志",
                        recipe.server
                    )
                } else {
                    format!(
                        "Installing {} failed. Check the network or proxy settings and restart Serena; see project logs for details",
                        recipe.server
                    )
                },
            )
        };
        let mut command = tokio::process::Command::new(toolchain);
        if let Some(proxy) = system_proxy().await {
            state.log(
                app,
                id,
                "info",
                "serena",
                &if zh {
                    format!("使用系统代理 {proxy}")
                } else {
                    format!("Using the system proxy {proxy}")
                },
            );
            command.env("HTTPS_PROXY", &proxy).env("HTTP_PROXY", &proxy);
        }
        command
            .args(recipe.args)
            .env(recipe.target_env, &target)
            .env("PATH", path)
            // Keep the project's own module and workspace files out of the install.
            .current_dir(&target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, _job) = project_gateway::spawn_owned(&mut command)
            .map_err(|e| AppError::new("LANGUAGE_SERVER_INSTALL_FAILED", e))?;
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        ]
        .into_iter()
        .flatten()
        {
            let app = app.clone();
            let id = id.to_string();
            tauri::async_runtime::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    app.state::<AppState>()
                        .log(&app, &id, "info", "serena", &line);
                }
            });
        }
        let status = tokio::time::timeout(Duration::from_secs(600), child.wait())
            .await
            .map_err(|_| failed())??;
        if !status.success() || which::which_in(recipe.server, Some(path), root).is_err() {
            return Err(failed());
        }
        state.log(
            app,
            id,
            "info",
            "serena",
            &if zh {
                format!("{} 已安装", recipe.server)
            } else {
                format!("{} installed", recipe.server)
            },
        );
        installed = true;
    }
    Ok(installed)
}

/// Proxy for installers. Toolchains such as Go only read `HTTPS_PROXY`, which
/// GUI launches rarely carry, so fall back to the system proxy setting.
pub(crate) async fn system_proxy() -> Option<String> {
    if ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
    {
        return None;
    }
    #[cfg(windows)]
    let proxy = windows_proxy(
        &read(
            "reg",
            &[
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            ],
        )
        .await?,
    );
    #[cfg(target_os = "macos")]
    let proxy = macos_proxy(&read("scutil", &["--proxy"]).await?);
    #[cfg(not(any(windows, target_os = "macos")))]
    let proxy = None;
    proxy
}
#[cfg(any(windows, target_os = "macos"))]
async fn read(program: &str, args: &[&str]) -> Option<String> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (child, _job) = project_gateway::spawn_owned(&mut command).ok()?;
    let out = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
/// `reg query` output for the Internet Settings key.
#[cfg(any(windows, test))]
fn windows_proxy(text: &str) -> Option<String> {
    let value = |name: &str| {
        text.lines()
            .map(str::split_whitespace)
            .find_map(|mut parts| (parts.next() == Some(name)).then(|| parts.last()).flatten())
    };
    if value("ProxyEnable")? != "0x1" {
        return None;
    }
    let server = value("ProxyServer")?;
    // Either one proxy for everything or per-scheme `https=host:port;...` entries.
    let server = if server.contains('=') {
        let entries = server.split(';').filter_map(|e| e.split_once('='));
        let entries = entries.collect::<Vec<_>>();
        ["https", "http"]
            .iter()
            .find_map(|scheme| entries.iter().find(|(s, _)| s == scheme))?
            .1
    } else {
        server
    };
    Some(if server.contains("://") {
        server.into()
    } else {
        format!("http://{server}")
    })
}
/// `scutil --proxy` output.
#[cfg(any(target_os = "macos", test))]
fn macos_proxy(text: &str) -> Option<String> {
    let value = |name: &str| {
        text.lines()
            .filter_map(|line| line.split_once(" : "))
            .find_map(|(key, value)| (key.trim() == name).then(|| value.trim()))
    };
    ["HTTPS", "HTTP"].iter().find_map(|scheme| {
        (value(&format!("{scheme}Enable"))? == "1").then_some(())?;
        Some(format!(
            "http://{}:{}",
            value(&format!("{scheme}Proxy"))?,
            value(&format!("{scheme}Port"))?
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_block_inline_and_legacy_language_lists() {
        let block = "project_name: \"x\"\nlanguage_servers:\n# comment\n- go\n- \"typescript\"  # main\nencoding: \"utf-8\"\n";
        assert_eq!(parse_languages(block).unwrap(), ["go", "typescript"]);
        assert_eq!(
            parse_languages("languages: [python, 'rust']\n").unwrap(),
            ["python", "rust"]
        );
        assert_eq!(
            parse_languages("language_servers:\n  - go\n").unwrap(),
            ["go"]
        );
        // Commented-out and nested keys are not language lists.
        assert!(parse_languages("#language_servers:\n#- go\n").is_none());
        assert!(parse_languages("ls:\n  languages: [go]\n").is_none());
    }
    #[test]
    fn reads_system_proxy_settings() {
        let reg = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings\r\n    ProxyEnable    REG_DWORD    0x1\r\n    ProxyServer    REG_SZ    127.0.0.1:7890\r\n";
        assert_eq!(windows_proxy(reg).unwrap(), "http://127.0.0.1:7890");
        assert!(windows_proxy(&reg.replace("0x1", "0x0")).is_none());
        let schemes = reg.replace("127.0.0.1:7890", "http=h:1;https=s:2;ftp=f:3");
        assert_eq!(windows_proxy(&schemes).unwrap(), "http://s:2");
        let mac = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 7890\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 0\n}\n";
        assert_eq!(macos_proxy(mac).unwrap(), "http://127.0.0.1:7890");
        assert!(macos_proxy(&mac.replace("HTTPEnable : 1", "HTTPEnable : 0")).is_none());
    }
    #[test]
    fn local_override_wins_and_template_falls_through() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".serena");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(configured(root.path()).is_empty());
        std::fs::write(dir.join("project.yml"), "language_servers:\n- go\n").unwrap();
        std::fs::write(dir.join("project.local.yml"), "# overrides\n").unwrap();
        assert_eq!(configured(root.path()), ["go"]);
        std::fs::write(dir.join("project.local.yml"), "language_servers: [rust]\n").unwrap();
        assert_eq!(configured(root.path()), ["rust"]);
    }
}
