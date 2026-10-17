//! One-click installation of the open-source analysis tools REA uses: a
//! Temurin JDK 21, Ghidra, the jadx-headless-mcp JAR and ilspycmd. Each tool
//! lands in the app data directory and its path is saved to REA's environment.
use crate::{i18n::tr, models::*, rea, AppState};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{Emitter, Manager, State};
use tokio::io::AsyncWriteExt;

const TASK: &str = "rea-tool-install";
const EVENT: &str = "rea-tool-install";
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Ghidra release REA 6.5.0 verifies; it runs on JDK 21.
const GHIDRA_VERSION: &str = "12.1.4";
const GHIDRA_URL: &str = "https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_12.1.4_build/ghidra_12.1.4_PUBLIC_20260921.zip";
const GHIDRA_SHA256: &str = "ddac49f903da9d5bac833e5cc79395098b9c33cfd3279be5f31bd00387d2d4db";
/// The JAR and digest REA 6.5.0 audits for Android analysis.
const JADX_JAR: &str = "jadx-headless-mcp-0.7.1-all.jar";
const JADX_URL: &str = "https://github.com/1013503897/jadx-headless-mcp/releases/download/v0.7.1/jadx-headless-mcp-0.7.1-all.jar";
const JADX_SHA256: &str = "6e5eacf500b64292bfb73c49797c1958f6ee44646e43e868039ae7feb573ff75";
const JDK_FEATURE: u32 = 21;

fn tools_dir(state: &AppState) -> PathBuf {
    state.data.join("tools")
}
fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}
fn jdk_dir(state: &AppState) -> PathBuf {
    tools_dir(state).join(format!("jdk-{JDK_FEATURE}"))
}
/// `JAVA_HOME` inside an extracted Temurin archive.
fn java_home(jdk: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        jdk.join("Contents/Home")
    } else {
        jdk.to_path_buf()
    }
}
fn has_java(home: &Path) -> bool {
    home.join("bin").join(exe("java")).is_file()
}
fn ghidra_dir(state: &AppState) -> PathBuf {
    tools_dir(state).join(format!("ghidra-{GHIDRA_VERSION}"))
}
fn jadx_jar(state: &AppState) -> PathBuf {
    tools_dir(state).join(JADX_JAR)
}
fn ilspy_exe(state: &AppState) -> PathBuf {
    tools_dir(state).join("ilspycmd").join(exe("ilspycmd"))
}
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    id: &'static str,
    /// Where this app installed the tool, when present.
    installed: Option<String>,
    /// Whether REA's environment points at that installation.
    in_use: bool,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    tools: Vec<Tool>,
    installing: bool,
    /// The directory every tool is installed into.
    directory: String,
}
#[tauri::command]
pub fn open_rea_tools_directory(state: State<AppState>) -> Result<()> {
    let directory = tools_dir(&state);
    std::fs::create_dir_all(&directory)?;
    crate::commands::open_directory(&directory)
}
#[tauri::command]
pub fn get_rea_tools(state: State<AppState>) -> Result<Tools> {
    let environment = rea::environment(&state)?;
    let home = java_home(&jdk_dir(&state));
    let tool = |id, key: &str, path: PathBuf, present: bool| Tool {
        id,
        in_use: present && environment.get(key) == Some(&text(&path)),
        installed: present.then(|| text(&path)),
    };
    let ghidra = ghidra_dir(&state);
    let jadx = jadx_jar(&state);
    let ilspy = ilspy_exe(&state);
    Ok(Tools {
        tools: vec![
            tool("jdk", "JAVA_HOME", home.clone(), has_java(&home)),
            tool(
                "ghidra",
                "GHIDRA_INSTALL_DIR",
                ghidra.clone(),
                ghidra.join("Ghidra/application.properties").is_file(),
            ),
            tool("jadx", "REA_JADX_MCP_JAR", jadx.clone(), jadx.is_file()),
            tool(
                "ilspy",
                "REA_ILSPY_CMD_PATH",
                ilspy.clone(),
                ilspy.is_file(),
            ),
        ],
        installing: RUNNING.load(Ordering::SeqCst),
        directory: text(&tools_dir(&state)),
    })
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Progress {
    tool: String,
    /// The download in progress, which may be a dependency such as the JDK.
    item: &'static str,
    phase: &'static str,
    pub(crate) downloaded: u64,
    total: Option<u64>,
}

pub(crate) struct Installer {
    tool: String,
    cancel: Arc<AtomicBool>,
    client: reqwest::Client,
    emit: Box<dyn Fn(Progress) + Send + Sync>,
}
impl Installer {
    /// Uses the system proxy when the environment does not name one.
    pub(crate) async fn new(
        tool: &str,
        cancel: Arc<AtomicBool>,
        emit: impl Fn(Progress) + Send + Sync + 'static,
    ) -> Result<Self> {
        let mut client = reqwest::Client::builder()
            .user_agent(concat!("codegraph-desktop/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30));
        if let Some(proxy) = crate::language_servers::system_proxy().await {
            client = client.proxy(
                reqwest::Proxy::all(&proxy)
                    .map_err(|e| AppError::new("PROXY_INVALID", e.to_string()))?,
            );
        }
        Ok(Self {
            tool: tool.into(),
            cancel,
            client: client
                .build()
                .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?,
            emit: Box::new(emit),
        })
    }
    fn report(&self, item: &'static str, phase: &'static str, downloaded: u64, total: Option<u64>) {
        (self.emit)(Progress {
            tool: self.tool.clone(),
            item,
            phase,
            downloaded,
            total,
        });
    }
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "CANCELLED",
                tr("安装已取消", "Installation cancelled"),
            ));
        }
        Ok(())
    }
    /// Downloads `url` to `target`, verifying its SHA-256 before it appears.
    async fn download(
        &self,
        item: &'static str,
        url: &str,
        sha256: &str,
        target: &Path,
    ) -> Result<()> {
        let failed = |detail: String| {
            AppError::new(
                "DOWNLOAD_FAILED",
                format!(
                    "{}: {detail}",
                    tr(
                        "下载失败，请检查网络或代理设置",
                        "Download failed. Check the network or proxy settings"
                    )
                ),
            )
        };
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| failed(e.to_string()))?;
        let total = response.content_length();
        let partial = target.with_extension("part");
        let mut file = tokio::fs::File::create(&partial).await?;
        let mut hasher = Sha256::new();
        let (mut downloaded, mut reported) = (0u64, 0u64);
        self.report(item, "download", 0, total);
        let result: Result<()> = async {
            loop {
                self.check()?;
                let chunk = tokio::time::timeout(Duration::from_secs(60), response.chunk())
                    .await
                    .map_err(|_| failed("timeout".into()))?
                    .map_err(|e| failed(e.to_string()))?;
                let Some(chunk) = chunk else { break };
                hasher.update(&chunk);
                file.write_all(&chunk).await?;
                downloaded += chunk.len() as u64;
                if downloaded - reported >= 1 << 20 {
                    reported = downloaded;
                    self.report(item, "download", downloaded, total);
                }
            }
            file.flush().await?;
            drop(file);
            self.report(item, "verify", downloaded, total);
            let digest = format!("{:x}", hasher.finalize());
            if !digest.eq_ignore_ascii_case(sha256) {
                return Err(AppError::new(
                    "CHECKSUM_MISMATCH",
                    format!(
                        "{} ({item})",
                        tr(
                            "下载内容校验失败，已丢弃",
                            "The download failed its checksum and was discarded"
                        )
                    ),
                ));
            }
            tokio::fs::rename(&partial, target).await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&partial).await;
        }
        result
    }
    /// Unpacks an archive whose content sits in one top-level directory and
    /// moves that directory to `target`, replacing an earlier installation.
    async fn unpack(&self, item: &'static str, archive: &Path, target: &Path) -> Result<()> {
        self.check()?;
        self.report(item, "extract", 0, None);
        let staging = target.with_file_name(format!(".staging-{}", uuid::Uuid::new_v4()));
        let (archive, target_dir, staged) =
            (archive.to_path_buf(), target.to_path_buf(), staging.clone());
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            extract(&archive, &staged)?;
            let mut entries = std::fs::read_dir(&staged)?.collect::<std::io::Result<Vec<_>>>()?;
            let top = match entries.as_slice() {
                [only] if only.file_type()?.is_dir() => entries.remove(0).path(),
                _ => staged.clone(),
            };
            if target_dir.exists() {
                std::fs::remove_dir_all(&target_dir)?;
            }
            std::fs::rename(&top, &target_dir)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::new("EXTRACT_FAILED", e.to_string()))?;
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    /// Installs this app's JDK unless it is already present and returns its
    /// `JAVA_HOME`.
    async fn jdk(&self, state: &AppState) -> Result<String> {
        let dir = jdk_dir(state);
        if has_java(&java_home(&dir)) {
            return Ok(text(&java_home(&dir)));
        }
        let (os, arch, extension) = platform()?;
        let url = format!(
            "https://api.adoptium.net/v3/assets/latest/{JDK_FEATURE}/hotspot?architecture={arch}&image_type=jdk&os={os}&vendor=eclipse"
        );
        let release: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?
            .json()
            .await
            .map_err(|e| AppError::new("DOWNLOAD_FAILED", e.to_string()))?;
        let package = &release[0]["binary"]["package"];
        let (Some(link), Some(checksum)) = (package["link"].as_str(), package["checksum"].as_str())
        else {
            return Err(AppError::new(
                "DOWNLOAD_FAILED",
                tr(
                    "未找到适用于当前系统的 JDK 21",
                    "No JDK 21 build is available for this system",
                ),
            ));
        };
        let archive = downloads(state)?.join(format!("jdk-{JDK_FEATURE}.{extension}"));
        self.download("jdk", link, checksum, &archive).await?;
        let unpacked = self.unpack("jdk", &archive, &dir).await;
        let _ = std::fs::remove_file(&archive);
        unpacked?;
        let home = java_home(&dir);
        if !has_java(&home) {
            return Err(AppError::new(
                "EXTRACT_FAILED",
                tr(
                    "JDK 解压后未找到 java",
                    "java was not found in the unpacked JDK",
                ),
            ));
        }
        Ok(text(&home))
    }
}

fn downloads(state: &AppState) -> Result<PathBuf> {
    let dir = tools_dir(state).join(".downloads");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
/// Adoptium's names for this system and the archive format it publishes.
fn platform() -> Result<(&'static str, &'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "mac",
        "linux" => "linux",
        _ => "",
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        _ => "",
    };
    if os.is_empty() || arch.is_empty() {
        return Err(AppError::new(
            "UNSUPPORTED_PLATFORM",
            tr(
                "当前系统不支持自动安装 JDK",
                "Automatic JDK installation is not supported on this system",
            ),
        ));
    }
    Ok((os, arch, if os == "windows" { "zip" } else { "tar.gz" }))
}
fn extract(archive: &Path, target: &Path) -> Result<()> {
    let failed = |e: String| AppError::new("EXTRACT_FAILED", e);
    let file = std::fs::File::open(archive)?;
    std::fs::create_dir_all(target)?;
    if archive.extension().is_some_and(|ext| ext == "zip") {
        zip::ZipArchive::new(file)
            .and_then(|mut zip| zip.extract(target))
            .map_err(|e| failed(e.to_string()))
    } else {
        tar::Archive::new(flate2::read::GzDecoder::new(file))
            .unpack(target)
            .map_err(|e| failed(e.to_string()))
    }
}

pub(crate) async fn install(
    installer: &Installer,
    state: &AppState,
    tool: &str,
) -> Result<rea::Environment> {
    let mut environment = rea::environment(state)?;
    match tool {
        "jdk" => {
            let home = installer.jdk(state).await?;
            environment.insert("JAVA_HOME".into(), home);
        }
        "ghidra" => {
            ensure_java(installer, state, &mut environment).await?;
            let dir = ghidra_dir(state);
            if !dir.join("Ghidra/application.properties").is_file() {
                let archive = downloads(state)?.join(format!("ghidra-{GHIDRA_VERSION}.zip"));
                installer
                    .download("ghidra", GHIDRA_URL, GHIDRA_SHA256, &archive)
                    .await?;
                let unpacked = installer.unpack("ghidra", &archive, &dir).await;
                let _ = std::fs::remove_file(&archive);
                unpacked?;
            }
            environment.insert("GHIDRA_INSTALL_DIR".into(), text(&dir));
        }
        "jadx" => {
            ensure_java(installer, state, &mut environment).await?;
            let jar = jadx_jar(state);
            if !jar.is_file() {
                std::fs::create_dir_all(tools_dir(state))?;
                installer
                    .download("jadx", JADX_URL, JADX_SHA256, &jar)
                    .await?;
            }
            environment.insert("REA_JADX_MCP_JAR".into(), text(&jar));
        }
        "ilspy" => {
            let path = ilspy_exe(state);
            if !path.is_file() {
                install_ilspy(installer, state).await?;
            }
            environment.insert("REA_ILSPY_CMD_PATH".into(), text(&path));
        }
        _ => {
            return Err(AppError::new(
                "INVALID_TOOL",
                tr("不支持的工具", "Unknown tool"),
            ))
        }
    }
    installer.report("configure", "configure", 0, None);
    Ok(environment)
}
/// Keeps a configured `JAVA_HOME` that has a Java launcher; otherwise points
/// it at this app's JDK, installing that first if needed.
async fn ensure_java(
    installer: &Installer,
    state: &AppState,
    environment: &mut rea::Environment,
) -> Result<()> {
    if environment
        .get("JAVA_HOME")
        .is_some_and(|home| has_java(Path::new(home)))
    {
        return Ok(());
    }
    let home = installer.jdk(state).await?;
    environment.insert("JAVA_HOME".into(), home);
    Ok(())
}
/// ilspycmd ships as a .NET tool, so the .NET SDK installs it.
async fn install_ilspy(installer: &Installer, state: &AppState) -> Result<()> {
    let dotnet = which::which("dotnet").map_err(|_| {
        AppError::new(
            crate::activation::ENVIRONMENT_REQUIRED,
            tr(
                "ilspycmd 需要 .NET SDK，请先安装 .NET 8 SDK 或更新版本",
                "ilspycmd needs the .NET SDK. Install the .NET 8 SDK or newer first",
            ),
        )
    })?;
    installer.check()?;
    installer.report("ilspy", "download", 0, None);
    let dir = tools_dir(state).join("ilspycmd");
    let mut command = tokio::process::Command::new(dotnet);
    command
        .args(["tool", "install", "ilspycmd", "--tool-path"])
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(proxy) = crate::language_servers::system_proxy().await {
        command.env("HTTPS_PROXY", &proxy).env("HTTP_PROXY", &proxy);
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(600), command.output())
        .await
        .map_err(|_| AppError::new("INSTALL_FAILED", "timeout"))??;
    if !output.status.success() || !ilspy_exe(state).is_file() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(AppError::new(
            "INSTALL_FAILED",
            format!(
                "{}: {detail}",
                tr("ilspycmd 安装失败", "Installing ilspycmd failed")
            ),
        ));
    }
    Ok(())
}

/// Installs one tool, then saves its path to REA's environment, which also
/// updates the REA entries this app manages.
#[tauri::command]
pub async fn install_rea_tool(
    app: tauri::AppHandle,
    tool: String,
) -> Result<rea::EnvironmentSaved> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::new(
            "TASK_RUNNING",
            tr("已有工具正在安装", "Another tool is being installed"),
        ));
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let release = Release;
    let cancel = Arc::new(AtomicBool::new(false));
    let emitter = app.clone();
    let installer = Installer::new(&tool, cancel.clone(), move |progress| {
        let _ = emitter.emit(EVENT, progress);
    })
    .await?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.state::<AppState>()
        .tasks
        .spawn(TASK.into(), cancel, async move {
            let _release = release;
            let state = handle.state::<AppState>();
            let result = match install(&installer, &state, &tool).await {
                Ok(environment) => rea::store_environment(&state, environment).await,
                Err(error) => Err(error),
            };
            installer.report(
                "done",
                if result.is_ok() { "done" } else { "failed" },
                0,
                None,
            );
            let _ = send.send(result);
        })?;
    receive
        .await
        .map_err(|_| AppError::new("APP_EXITING", "应用正在退出"))?
}

#[tauri::command]
pub fn cancel_rea_tool_install(state: State<AppState>) -> Result<()> {
    state.tasks.cancel(TASK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn archives_unpack_into_their_top_level_directory() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("tool.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        writer
            .start_file("tool-1.0/bin/run", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"zip").unwrap();
        writer.finish().unwrap();
        let tar_path = dir.path().join("tool.tar.gz");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            std::fs::File::create(&tar_path).unwrap(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "tool-1.0/bin/run", &b"tar"[..])
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        for (archive, content) in [(zip_path, "zip"), (tar_path, "tar")] {
            let staged = dir.path().join(format!("staged-{content}"));
            extract(&archive, &staged).unwrap();
            assert_eq!(
                std::fs::read_to_string(staged.join("tool-1.0/bin/run")).unwrap(),
                content
            );
        }
    }

    #[test]
    fn extraction_rejects_paths_outside_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        writer
            .start_file("../escaped", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
        writer.finish().unwrap();
        let staged = dir.path().join("inner/staged");
        let _ = extract(&zip_path, &staged);
        assert!(!dir.path().join("inner/escaped").exists());
    }

    #[test]
    fn platform_maps_to_adoptium_names() {
        let (os, arch, extension) = platform().unwrap();
        assert!(["windows", "mac", "linux"].contains(&os));
        assert!(["x64", "aarch64"].contains(&arch));
        assert_eq!(extension == "zip", os == "windows");
    }
}
