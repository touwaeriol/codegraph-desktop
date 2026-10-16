use crate::{models::*, persistence, AppState};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const AUTO_CHECK: &str = "autoCheckUpdates";
const RELEASES: &str = "https://github.com/touwaeriol/codegraph-desktop/releases";

fn can_install(app: &tauri::AppHandle) -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        app.env().appimage.is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        cfg!(windows) || cfg!(target_os = "macos")
    }
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    sequence: u64,
    status: String,
    current_version: String,
    latest_version: Option<String>,
    notes: Option<String>,
    checked_at: Option<String>,
    downloaded: u64,
    total: Option<u64>,
    auto_check: bool,
    can_install: bool,
    error: Option<AppError>,
}

pub struct UpdateState {
    snapshot: Mutex<Snapshot>,
    available: Mutex<Option<Update>>,
    operation: tokio::sync::Mutex<()>,
    cancel: AtomicBool,
}
impl UpdateState {
    fn publish(&self, app: &tauri::AppHandle, change: impl FnOnce(&mut Snapshot)) -> Snapshot {
        let mut snapshot = self.snapshot.lock().unwrap();
        change(&mut snapshot);
        snapshot.sequence += 1;
        let result = snapshot.clone();
        drop(snapshot);
        let _ = app.emit("app-update", &result);
        result
    }
    fn fail(&self, app: &tauri::AppHandle, error: AppError) -> Snapshot {
        self.publish(app, |s| {
            s.status = "error".into();
            s.error = Some(error);
        })
    }
}

pub fn setup(app: &tauri::AppHandle) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let auto_check = persistence::setting(&app.state::<AppState>().db.lock().unwrap(), AUTO_CHECK)
        .map_err(|e| std::io::Error::other(e.message))?
        .as_deref()
        != Some("false");
    app.manage(UpdateState {
        snapshot: Mutex::new(Snapshot {
            sequence: 0,
            status: "idle".into(),
            current_version: app.package_info().version.to_string(),
            latest_version: None,
            notes: None,
            checked_at: None,
            downloaded: 0,
            total: None,
            auto_check,
            // DEB 的安装前脚本会等待旧程序退出，应继续由系统包管理器升级。
            can_install: can_install(app),
            error: None,
        }),
        available: Mutex::new(None),
        operation: Default::default(),
        cancel: AtomicBool::new(false),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            if app
                .state::<UpdateState>()
                .snapshot
                .lock()
                .unwrap()
                .auto_check
                && !app.state::<AppState>().quitting.load(Ordering::SeqCst)
            {
                let _ = check(&app).await;
            }
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
    Ok(())
}

#[tauri::command]
pub fn get_update_snapshot(state: State<UpdateState>) -> Snapshot {
    state.snapshot.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_auto_update_check(
    app: tauri::AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<Snapshot> {
    persistence::set_setting(
        &state.db.lock().unwrap(),
        AUTO_CHECK,
        if enabled { "true" } else { "false" },
    )?;
    let snapshot = app
        .state::<UpdateState>()
        .publish(&app, |s| s.auto_check = enabled);
    if enabled {
        tauri::async_runtime::spawn(async move {
            let _ = check(&app).await;
        });
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<Snapshot> {
    check(&app).await
}

async fn check(app: &tauri::AppHandle) -> Result<Snapshot> {
    let state = app.state::<UpdateState>();
    let _guard = state.operation.try_lock().map_err(|_| {
        AppError::new(
            "UPDATE_BUSY",
            crate::i18n::tr(
                "更新操作正在进行中",
                "An update operation is already in progress",
            ),
        )
    })?;
    if app.state::<AppState>().quitting.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_EXITING", "应用正在退出"));
    }
    state.publish(app, |s| {
        s.status = "checking".into();
        s.error = None;
    });
    let result = async {
        let updater = build_updater(app)?;
        updater.check().await
    }
    .await;
    match result {
        Ok(update) => {
            if let Some(update) = &update {
                if !trusted_download(&update.version, update.download_url.as_str()) {
                    return Ok(state.fail(
                        app,
                        AppError::new(
                            "UPDATE_SOURCE_INVALID",
                            crate::i18n::tr(
                                "更新地址不是本项目的 GitHub Release",
                                "The update URL is not this project's GitHub Release",
                            ),
                        ),
                    ));
                }
            }
            let latest = update.as_ref().map(|u| u.version.clone());
            let notes = update.as_ref().and_then(|u| u.body.clone());
            *state.available.lock().unwrap() = update;
            Ok(state.publish(app, |s| {
                s.status = if latest.is_some() {
                    "available"
                } else {
                    "upToDate"
                }
                .into();
                s.latest_version = latest;
                s.notes = notes;
                s.checked_at = Some(now());
                s.downloaded = 0;
                s.total = None;
            }))
        }
        Err(error) => Ok(state.fail(
            app,
            AppError::new(
                "UPDATE_CHECK_FAILED",
                format!(
                    "{}: {error}",
                    crate::i18n::tr(
                        "无法检查更新，请检查网络或稍后重试",
                        "Unable to check for updates. Check your network or retry later"
                    )
                ),
            ),
        )),
    }
}

fn build_updater<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    app.updater_builder()
        .timeout(Duration::from_secs(30))
        // 受管进程已由安装流程停止。Windows 启动安装器失败时保留窗口和托盘以显示错误。
        .on_before_exit(|| {})
        .version_comparator(|current, remote| {
            remote.version > current && remote.version.pre.is_empty()
        })
        .build()
}

fn trusted_download(version: &str, url: &str) -> bool {
    let prefix = format!("{RELEASES}/download/v{version}/CodeGraph-Desktop-v{version}-");
    url.strip_prefix(&prefix).is_some_and(|name| {
        !name.contains(['/', '?', '#', '%'])
            && [
                "macos-arm64.app.tar.gz",
                "windows-amd64.exe",
                "windows-arm64.exe",
                "linux-amd64.AppImage",
                "linux-arm64.AppImage",
                "linux-amd64.deb",
                "linux-arm64.deb",
            ]
            .contains(&name)
    })
}

#[tauri::command]
pub fn cancel_update_download(state: State<UpdateState>) {
    let snapshot = state.snapshot.lock().unwrap();
    if snapshot.status == "downloading" {
        state.cancel.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<Snapshot> {
    let state = app.state::<UpdateState>();
    let _guard = state.operation.try_lock().map_err(|_| {
        AppError::new(
            "UPDATE_BUSY",
            crate::i18n::tr(
                "更新操作正在进行中",
                "An update operation is already in progress",
            ),
        )
    })?;
    if !state.snapshot.lock().unwrap().can_install {
        return Err(AppError::new(
            "UPDATE_MANUAL_INSTALL",
            crate::i18n::tr(
                "请通过系统包管理器安装新版 DEB，或使用 AppImage 进行应用内更新",
                "Install the new DEB with your package manager, or use AppImage for in-app updates",
            ),
        ));
    }
    let mut update = state.available.lock().unwrap().clone().ok_or_else(|| {
        AppError::new(
            "UPDATE_NOT_AVAILABLE",
            crate::i18n::tr("请先检查更新", "Check for updates first"),
        )
    })?;
    if app.state::<AppState>().quitting.load(Ordering::SeqCst) {
        return Err(AppError::new("APP_EXITING", "应用正在退出"));
    }
    state.cancel.store(false, Ordering::SeqCst);
    state.publish(&app, |s| {
        s.status = "downloading".into();
        s.error = None;
        s.downloaded = 0;
        s.total = None;
    });
    update.timeout = Some(Duration::from_secs(10 * 60));
    let mut downloaded = 0u64;
    let mut last_event = Instant::now();
    let download = update.download(
        |bytes, total| {
            downloaded += bytes as u64;
            if last_event.elapsed() >= Duration::from_millis(100) || Some(downloaded) == total {
                state.publish(&app, |s| {
                    s.downloaded = downloaded;
                    s.total = total;
                });
                last_event = Instant::now();
            }
        },
        || {},
    );
    let cancelled = async {
        while !state.cancel.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let bytes = tokio::select! {
        result = download => match result {
            Ok(bytes) => bytes,
            Err(error) => return Ok(state.fail(&app, AppError::new("UPDATE_DOWNLOAD_FAILED",
                format!("{}: {error}", crate::i18n::tr(
                    "下载或签名校验失败，当前版本未修改",
                    "Download or signature verification failed; the current version is unchanged"))))),
        },
        _ = cancelled => return Ok(state.publish(&app, |s| {
            s.status = "available".into(); s.downloaded = 0; s.total = None;
        })),
    };
    let cancelled_before_install = {
        let mut snapshot = state.snapshot.lock().unwrap();
        let cancelled = state.cancel.load(Ordering::SeqCst);
        if !cancelled {
            snapshot.status = "preparing".into();
        }
        cancelled
    };
    if cancelled_before_install {
        return Ok(state.publish(&app, |s| {
            s.status = "available".into();
            s.downloaded = 0;
            s.total = None;
        }));
    }
    let app_state = app.state::<AppState>();
    if app_state.quitting.swap(true, Ordering::SeqCst) {
        return Err(AppError::new("APP_EXITING", "应用正在退出"));
    }
    state.publish(&app, |s| {
        s.status = "preparing".into();
        s.downloaded = bytes.len() as u64;
    });
    let result = if crate::stop_managed(&app, &app_state).await {
        state.publish(&app, |s| s.status = "installing".into());
        tokio::task::spawn_blocking(move || update.install(bytes))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
    } else {
        Err(crate::i18n::tr(
            "受管实例未能全部停止，已取消安装",
            "Managed instances could not all stop; installation was cancelled",
        )
        .into())
    };
    match result {
        Ok(()) => {
            app_state.exit_ready.store(true, Ordering::SeqCst);
            app.restart();
        }
        Err(error) => {
            let can_resume = app_state.tasks.reopen();
            app_state.quitting.store(false, Ordering::SeqCst);
            if can_resume {
                tauri::async_runtime::spawn(crate::resume_engines(app.clone()));
            }
            Ok(state.fail(
                &app,
                AppError::new(
                    "UPDATE_INSTALL_FAILED",
                    format!(
                        "{}: {error}",
                        crate::i18n::tr(
                            "安装失败，请重试或退出应用后重新安装",
                            "Installation failed. Retry or exit the app and reinstall"
                        )
                    ),
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDlEQzVDNDc1QUFDQkRBQTEKUldTaDJzdXFkY1RGblYxN09zQWRGREVpN3E1S3Yrbi9SVWtJSk9OQTJPY3lhVndhSGNMQzQ0NE0K";
    const TEST_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTaDJzdXFkY1RGbldGNVVBNG9FMFZZZlV0V0JmYXNkL3BuSENsTWZxSGowQm1ENHoycWFNVnY4bzg1Vng5KzdlVFVQT25LVWpWNXdaODErUTFJMUxoZDYxOXZEQlBvcEE4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMTYyMjc2CWZpbGU6dXBkYXRlLWZpeHR1cmUuYmluCXZlcnNpb246OS4wLjAKcnlidjNXZmg0SzF5RVo0RzBaUlROM2NuTmdPM2lKaDNreFRlZ3VQTXIyQzNsQ2gyV1JDQU40T0pQUGMwbVBxQ2xzNDZuZlc1VkhOTFRFNkNyc0dSRHc9PQo=";
    const TEST_BYTES: &[u8] = b"codegraph-update-fixture\n";
    fn app(key: &str) -> tauri::App<MockRuntime> {
        let mut context = mock_context(noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
            "pubkey": key, "endpoints": ["https://example.test/latest.json"], "requireSignedVersion": true,
            }),
        );
        mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().pubkey(key).build())
            .build(context)
            .unwrap()
    }
    fn release(version: &str, url: &str) -> serde_json::Value {
        serde_json::json!({"version":version,"url":url,"signature":TEST_SIGNATURE})
    }
    #[test]
    fn signed_payload_and_announced_version_must_match() {
        let app = app(TEST_KEY);
        let updater = build_updater(app.handle()).unwrap();
        let update = updater
            .restore_update(release("9.0.0", "https://example.test/update"))
            .unwrap()
            .unwrap();
        update.verify(TEST_BYTES).unwrap();
        assert!(update.verify(b"tampered").is_err());
        assert!(updater
            .restore_update(release("9.0.1", "https://example.test/update"))
            .unwrap()
            .unwrap()
            .verify(TEST_BYTES)
            .is_err());
        assert!(updater
            .restore_update(release("0.0.1", "https://example.test/update"))
            .unwrap()
            .is_none());
        assert!(updater
            .restore_update(release("9.0.1-beta.1", "https://example.test/update"))
            .unwrap()
            .is_none());
    }
    #[tokio::test]
    async fn downloads_report_progress_and_verify_before_returning_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for bytes in [TEST_BYTES, b"tampered".as_slice()] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/update", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                assert!(stream.read(&mut request).await.unwrap() > 0);
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                stream.write_all(bytes).await.unwrap();
            });
            let app = app(TEST_KEY);
            let update = build_updater(app.handle())
                .unwrap()
                .restore_update(release("9.0.0", &url))
                .unwrap()
                .unwrap();
            let mut downloaded = 0;
            let result = update
                .download(
                    |n, total| {
                        downloaded += n;
                        assert_eq!(total, Some(bytes.len() as u64));
                    },
                    || {},
                )
                .await;
            assert_eq!(downloaded, bytes.len());
            assert_eq!(result.is_ok(), bytes == TEST_BYTES);
            server.await.unwrap();
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "需要 CODEGRAPH_UPDATE_TEST_PACKAGE 指定带签名的 macOS .app.tar.gz；只替换临时测试 app"]
    fn installs_signed_macos_bundle_in_an_isolated_directory() {
        let package =
            std::path::PathBuf::from(std::env::var("CODEGRAPH_UPDATE_TEST_PACKAGE").unwrap());
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let app = app(config["plugins"]["updater"]["pubkey"].as_str().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let executable = dir
            .path()
            .join("CodeGraph Desktop.app/Contents/MacOS/codegraph-desktop");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"old executable").unwrap();
        std::fs::write(dir.path().join("user-settings"), b"preserve").unwrap();
        let updater = app
            .updater_builder()
            .executable_path(&executable)
            .build()
            .unwrap();
        let update = updater.restore_update(serde_json::json!({
            "version":config["version"],
            "url":"https://github.com/touwaeriol/codegraph-desktop/releases/download/test/test.app.tar.gz",
            "signature":std::fs::read_to_string(format!("{}.sig", package.display())).unwrap().trim(),
        })).unwrap().unwrap();
        update.install(std::fs::read(package).unwrap()).unwrap();
        assert!(std::fs::metadata(executable).unwrap().len() > 1024);
        let validation = std::process::Command::new("codesign")
            .args(["--verify", "--deep", "--strict", "--verbose=2"])
            .arg(dir.path().join("CodeGraph Desktop.app"))
            .output()
            .unwrap();
        assert!(
            validation.status.success(),
            "{}",
            String::from_utf8_lossy(&validation.stderr)
        );
        assert_eq!(
            std::fs::read(dir.path().join("user-settings")).unwrap(),
            b"preserve"
        );
    }
    #[tokio::test]
    #[ignore = "需要访问已发布的 GitHub 更新清单；下载并校验，不执行安装"]
    async fn published_update_feed_download_is_verified() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let mut context = mock_context(noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), config["plugins"]["updater"].clone());
        let app = mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let mut update = build_updater(app.handle())
            .unwrap()
            .check()
            .await
            .unwrap()
            .unwrap();
        assert!(trusted_download(
            &update.version,
            update.download_url.as_str()
        ));
        update.timeout = Some(Duration::from_secs(600));
        let bytes = update.download(|_, _| {}, || {}).await.unwrap();
        assert!(!bytes.is_empty());
        println!(
            "GitHub 更新清单与签名验证通过：{}，{} 字节",
            update.version,
            bytes.len()
        );
    }
    #[test]
    fn only_current_release_assets_are_accepted() {
        let good =
            format!("{RELEASES}/download/v0.1.24/CodeGraph-Desktop-v0.1.24-macos-arm64.app.tar.gz");
        assert!(trusted_download("0.1.24", &good));
        for bad in [
            good.replace("https:", "http:"),
            good.replace("touwaeriol", "someone"),
            format!("{good}?redirect=other"),
            good.replace("v0.1.24-macos", "v0.1.23-macos"),
        ] {
            assert!(!trusted_download("0.1.24", &bad));
        }
    }
}
