use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRecord {
    pub project_id: String,
    pub generation: String,
    pub owner_pid: u32,
    pub endpoint: String,
    pub token: String,
    pub started_at: String,
}
pub fn runtime_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("无法确定应用数据目录"))?
        .join("ai.codegraph.desktop")
        .join("runtime"))
}
pub fn read_runtime(project_id: &str) -> anyhow::Result<RuntimeRecord> {
    uuid::Uuid::parse_str(project_id)?;
    let record: RuntimeRecord = serde_json::from_slice(&std::fs::read(
        runtime_dir()?.join(format!("{project_id}.json")),
    )?)?;
    anyhow::ensure!(record.project_id == project_id, "运行记录项目不匹配");
    validate_runtime(&record)?;
    Ok(record)
}
pub fn validate_runtime(record: &RuntimeRecord) -> anyhow::Result<()> {
    uuid::Uuid::parse_str(&record.project_id)?;
    anyhow::ensure!(
        record.owner_pid > 0
            && record.token.len() == 64
            && record.token.bytes().all(|b| b.is_ascii_hexdigit()),
        "运行记录无效"
    );
    let url = url::Url::parse(&record.endpoint)?;
    anyhow::ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && url.query().is_none()
            && (url.path() == "/mcp" || url.path() == format!("/mcp/{}", record.project_id)),
        "运行地址无效"
    );
    uuid::Uuid::parse_str(&record.generation)?;
    Ok(())
}

pub fn identity_endpoint(record: &RuntimeRecord) -> anyhow::Result<String> {
    validate_runtime(record)?;
    let mut url = url::Url::parse(&record.endpoint)?;
    url.set_path(&url.path().replacen("/mcp", "/identity", 1));
    Ok(url.into())
}

pub fn write_runtime(record: &RuntimeRecord) -> anyhow::Result<()> {
    validate_runtime(record)?;
    let directory = runtime_dir()?;
    std::fs::create_dir_all(&directory)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN")?,
            std::env::var("USERNAME")?
        );
        let result = std::process::Command::new("icacls.exe")
            .arg(&directory)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F")])
            .creation_flags(0x08000000)
            .output()?;
        anyhow::ensure!(result.status.success(), "设置运行记录私有权限失败");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    }
    let target = directory.join(format!("{}.json", record.project_id));
    let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec(record)?)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                a.as_ptr(),
                b.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            let e = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(temporary);
            return Err(e.into());
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(temporary, target)?;
    Ok(())
}
pub fn remove_runtime(project_id: &str) -> anyhow::Result<()> {
    uuid::Uuid::parse_str(project_id)?;
    match std::fs::remove_file(runtime_dir()?.join(format!("{project_id}.json"))) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record(endpoint: &str) -> RuntimeRecord {
        RuntimeRecord {
            project_id: uuid::Uuid::new_v4().to_string(),
            generation: uuid::Uuid::new_v4().to_string(),
            owner_pid: 1,
            endpoint: endpoint.into(),
            token: "a".repeat(64),
            started_at: String::new(),
        }
    }
    #[test]
    fn only_exact_loopback_mcp_endpoint() {
        assert!(validate_runtime(&record("http://127.0.0.1:43123/mcp")).is_ok());
        for bad in [
            "http://127.0.0.1:43123@evil.example/mcp",
            "http://evil.example:43123/mcp",
            "http://127.0.0.1:43123/other",
            "http://127.0.0.1:43123/mcp?redirect=x",
            "http://user@127.0.0.1:43123/mcp",
            "https://127.0.0.1:43123/mcp",
            "http://127.0.0.1/mcp",
        ] {
            assert!(validate_runtime(&record(bad)).is_err(), "{bad}");
        }
    }
    #[test]
    fn reject_invalid_identity() {
        let mut r = record("http://127.0.0.1:43123/mcp");
        r.generation = "old".into();
        assert!(validate_runtime(&r).is_err());
    }
    #[test]
    fn shared_endpoint_is_bound_to_exact_project_identity() {
        let mut r = record("http://127.0.0.1:43123/mcp");
        assert_eq!(
            identity_endpoint(&r).unwrap(),
            "http://127.0.0.1:43123/identity"
        );
        r.endpoint = format!("http://127.0.0.1:43123/mcp/{}", r.project_id);
        assert!(validate_runtime(&r).is_ok());
        assert_eq!(
            identity_endpoint(&r).unwrap(),
            format!("http://127.0.0.1:43123/identity/{}", r.project_id)
        );
        for suffix in ["/", "/extra", "?x=1", "#fragment"] {
            let mut bad = r.clone();
            bad.endpoint.push_str(suffix);
            assert!(validate_runtime(&bad).is_err());
        }
        r.project_id = uuid::Uuid::new_v4().to_string();
        assert!(validate_runtime(&r).is_err());
    }
}
