use crate::{models::*, persistence};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub enum Platform {
    Windows,
    Macos,
    Unix,
}
pub struct SearchPaths {
    pub platform: Platform,
    pub path: Vec<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub npm_prefix: Option<PathBuf>,
    pub extra_bins: Vec<PathBuf>,
}
impl SearchPaths {
    pub fn current() -> Self {
        let platform = if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Unix
        };
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let home =
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
        let npm_prefix = std::env::var_os("npm_config_prefix")
            .or_else(|| std::env::var_os("NPM_CONFIG_PREFIX"))
            .map(PathBuf::from);
        let mut extra_bins = Vec::new();
        if cfg!(windows) {
            if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
                extra_bins.push(PathBuf::from(dir).join("Programs/nodejs"));
            }
            if let Some(dir) = std::env::var_os("ProgramFiles") {
                extra_bins.push(PathBuf::from(dir).join("nodejs"));
            }
        }
        Self {
            platform,
            path,
            app_data,
            home,
            npm_prefix,
            extra_bins,
        }
    }
    pub fn candidates(&self) -> Vec<PathBuf> {
        let mut dirs = self.path.clone();
        if let Some(prefix) = &self.npm_prefix {
            dirs.push(if matches!(self.platform, Platform::Windows) {
                prefix.clone()
            } else {
                prefix.join("bin")
            });
        }
        if matches!(self.platform, Platform::Windows) {
            if let Some(app_data) = &self.app_data {
                dirs.push(app_data.join("npm"));
            } else if let Some(home) = &self.home {
                dirs.push(home.join("AppData/Roaming/npm"));
            }
        }
        if let Some(home) = &self.home {
            for suffix in [
                ".npm-global/bin",
                ".local/bin",
                ".volta/bin",
                ".nvm/current/bin",
            ] {
                dirs.push(home.join(suffix));
            }
        }
        if matches!(self.platform, Platform::Macos) {
            dirs.extend([
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ]);
        }
        if !matches!(self.platform, Platform::Windows) {
            dirs.extend([PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")]);
        }
        dirs.extend(self.extra_bins.clone());
        let names: &[&str] = if matches!(self.platform, Platform::Windows) {
            &["codegraph.exe", "codegraph.cmd", "codegraph.ps1"]
        } else {
            &["codegraph"]
        };
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for dir in dirs {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for name in names {
                let candidate = dir.join(name);
                let key = candidate.to_string_lossy().to_string();
                let key = if matches!(self.platform, Platform::Windows) {
                    key.to_lowercase()
                } else {
                    key
                };
                if seen.insert(key) {
                    result.push(candidate);
                }
            }
        }
        result
    }
}
pub fn candidates(saved: Option<&str>, paths: &SearchPaths) -> Vec<PathBuf> {
    match saved {
        Some(path) => vec![PathBuf::from(path)],
        None => paths.candidates(),
    }
}
pub fn resolve(path: &Path) -> Result<(PathBuf, project_gateway::CodeGraphEntry)> {
    if !path.is_file() {
        return Err(AppError::new(
            "CLI_NOT_FOUND",
            "CodeGraph 入口不存在或不是文件",
        ));
    }
    let entry =
        project_gateway::resolve_entry(path).map_err(|e| AppError::new("CLI_NOT_FOUND", e))?;
    let selected = dunce::canonicalize(path).map_err(|e| AppError::new("CLI_NOT_FOUND", e))?;
    Ok((selected, entry))
}
pub fn select_with<T>(
    saved: Option<&str>,
    paths: &SearchPaths,
    mut resolver: impl FnMut(&Path) -> Result<T>,
) -> Result<(PathBuf, T)> {
    let mut error = AppError::new("CLI_NOT_FOUND", "未找到 CodeGraph，请选择安装入口");
    for path in candidates(saved, paths) {
        match resolver(&path) {
            Ok(entry) => return Ok((path, entry)),
            Err(e) => {
                error = e;
                if saved.is_some() {
                    break;
                }
            }
        }
    }
    Err(error)
}
pub fn persist_detected(
    db: &mut rusqlite::Connection,
    expected: Option<&str>,
    environment: &Environment,
) -> Result<bool> {
    if !environment.available {
        return Ok(false);
    }
    let selected = environment
        .entry
        .as_deref()
        .ok_or_else(|| AppError::new("CLI_INVALID", "CodeGraph 检测失败"))?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if persistence::setting(&tx, "codegraphEntry")?.as_deref() != expected {
        return Ok(false);
    }
    persistence::set_setting(&tx, "codegraphEntry", selected)?;
    tx.commit()?;
    Ok(true)
}
pub fn clear_cli_index_error(snapshot: &mut RuntimeSnapshot) -> bool {
    if snapshot.index_state == "error"
        && ["stopped", "error"].contains(&snapshot.state.as_str())
        && snapshot
            .error
            .as_ref()
            .is_some_and(|e| ["CLI_NOT_FOUND", "CLI_INVALID"].contains(&e.code.as_str()))
    {
        snapshot.index_state = "unknown".into();
        snapshot.error = None;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_detection_clears_only_stopped_cli_index_errors() {
        let mut snapshot = RuntimeSnapshot::new("project");
        snapshot.index_state = "error".into();
        snapshot.error = Some(AppError::new("CLI_NOT_FOUND", "missing"));
        assert!(clear_cli_index_error(&mut snapshot));
        assert_eq!(snapshot.index_state, "unknown");
        assert!(snapshot.error.is_none());
        for state in ["running", "starting", "stopping"] {
            snapshot.state = state.into();
            snapshot.index_state = "error".into();
            snapshot.error = Some(AppError::new("CLI_INVALID", "bad entry"));
            assert!(!clear_cli_index_error(&mut snapshot));
            assert_eq!(snapshot.index_state, "error");
            assert!(snapshot.error.is_some());
        }
        snapshot.state = "stopped".into();
        snapshot.error = Some(AppError::new("INDEX_NOT_READY", "real index error"));
        assert!(!clear_cli_index_error(&mut snapshot));
    }
    fn paths(root: &Path) -> SearchPaths {
        SearchPaths {
            platform: Platform::Windows,
            path: vec![root.join("system")],
            app_data: Some(root.join("roaming")),
            home: Some(root.join("home")),
            npm_prefix: None,
            extra_bins: vec![],
        }
    }
    #[test]
    fn windows_npm_prefix_found_without_path_and_explicit_failure_does_not_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let npm = dir.path().join("roaming/npm/codegraph.cmd");
        std::fs::create_dir_all(npm.parent().unwrap()).unwrap();
        std::fs::write(&npm, b"shim").unwrap();
        let check = |path: &Path| {
            if path.is_file() {
                Ok(())
            } else {
                Err(AppError::new("CLI_NOT_FOUND", "missing"))
            }
        };
        let (selected, _) = select_with(None, &paths, check).unwrap();
        assert_eq!(selected, npm);
        let explicit = dir.path().join("missing.exe");
        let mut calls = 0;
        let failure = select_with(Some(&explicit.to_string_lossy()), &paths, |path| {
            calls += 1;
            check(path)
        });
        assert!(failure.is_err());
        assert_eq!(calls, 1);
    }
    #[test]
    fn path_precedes_known_locations_and_candidates_are_platform_specific() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let candidates = paths.candidates();
        assert_eq!(candidates[0], dir.path().join("system/codegraph.exe"));
        let mac = SearchPaths {
            platform: Platform::Macos,
            path: vec![],
            app_data: None,
            home: Some(dir.path().into()),
            npm_prefix: Some(dir.path().join("custom")),
            extra_bins: vec![],
        };
        assert_eq!(mac.candidates()[0], dir.path().join("custom/bin/codegraph"));
        assert!(mac
            .candidates()
            .contains(&PathBuf::from("/opt/homebrew/bin/codegraph")));
        assert!(mac
            .candidates()
            .iter()
            .all(|path| path.extension().is_none()));
    }
    #[test]
    fn detection_success_persists_failure_does_not_and_races_preserve_user_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = persistence::open(dir.path()).unwrap();
        let mut env = Environment {
            available: false,
            entry: Some("detected-codegraph".into()),
            version: None,
            error: Some("failed".into()),
        };
        assert!(!persist_detected(&mut db, None, &env).unwrap());
        assert!(persistence::setting(&db, "codegraphEntry")
            .unwrap()
            .is_none());
        env.available = true;
        env.error = None;
        assert!(persist_detected(&mut db, None, &env).unwrap());
        drop(db);
        let mut db = persistence::open(dir.path()).unwrap();
        assert_eq!(
            persistence::setting(&db, "codegraphEntry")
                .unwrap()
                .as_deref(),
            Some("detected-codegraph")
        );
        persistence::set_setting(&db, "codegraphEntry", "user-choice").unwrap();
        assert!(!persist_detected(&mut db, Some("detected-codegraph"), &env).unwrap());
        assert_eq!(
            persistence::setting(&db, "codegraphEntry")
                .unwrap()
                .as_deref(),
            Some("user-choice")
        );
        env.available = false;
        assert!(!persist_detected(&mut db, Some("user-choice"), &env).unwrap());
        assert_eq!(
            persistence::setting(&db, "codegraphEntry")
                .unwrap()
                .as_deref(),
            Some("user-choice")
        );
    }
    #[cfg(windows)]
    #[test]
    fn discovered_npm_shim_resolves_with_the_real_bundle_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let prefix = dir.path().join("roaming/npm");
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::write(prefix.join("codegraph.cmd"), b"shim").unwrap();
        let architecture = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        let bundle = prefix.join(format!(
            "node_modules/@colbymchenry/codegraph-win32-{architecture}"
        ));
        std::fs::create_dir_all(bundle.join("lib/dist/bin")).unwrap();
        std::fs::write(bundle.join("node.exe"), []).unwrap();
        std::fs::write(bundle.join("lib/dist/bin/codegraph.js"), []).unwrap();
        let (_, (_, entry)) = select_with(None, &paths, resolve).unwrap();
        assert!(entry.program.ends_with("node.exe"));
        assert!(entry.prefix_args.iter().any(|arg| arg == "--liftoff-only"));
    }
}
