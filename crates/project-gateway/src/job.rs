#[cfg(windows)]
pub struct ProcessJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ProcessJob {}
#[cfg(windows)]
unsafe impl Sync for ProcessJob {}
#[cfg(windows)]
impl ProcessJob {
    pub fn attach(child: &tokio::process::Child) -> anyhow::Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            anyhow::ensure!(!handle.is_null(), "创建 Job Object 失败");
            let job = Self(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            anyhow::ensure!(
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as _,
                    std::mem::size_of_val(&info) as u32
                ) != 0,
                "设置 Job Object 失败"
            );
            anyhow::ensure!(
                AssignProcessToJobObject(
                    handle,
                    child
                        .raw_handle()
                        .ok_or_else(|| anyhow::anyhow!("进程已退出"))? as _
                ) != 0,
                "关联 Job Object 失败"
            );
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(unix)]
pub struct ProcessJob {
    pgid: libc::pid_t,
}
#[cfg(unix)]
impl ProcessJob {
    pub fn attach(child: &tokio::process::Child) -> anyhow::Result<Self> {
        let pid = child.id().ok_or_else(|| anyhow::anyhow!("进程已退出"))? as libc::pid_t;
        anyhow::ensure!(unsafe { libc::getpgid(pid) } == pid, "进程未进入独立进程组");
        Ok(Self { pgid: pid })
    }
    fn signal(&self, signal: libc::c_int) -> std::io::Result<()> {
        if unsafe { libc::kill(-self.pgid, signal) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
    fn exists(&self) -> bool {
        (unsafe { libc::kill(-self.pgid, 0) }) == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}
#[cfg(unix)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        let _ = self.signal(libc::SIGKILL);
    }
}
/// Spawn suspended on Windows, attach the Job before user code can create
/// descendants, then resume its initial thread. No process-name killing.
pub fn spawn_owned(
    command: &mut tokio::process::Command,
) -> anyhow::Result<(tokio::process::Child, ProcessJob)> {
    #[cfg(windows)]
    command.creation_flags(0x08000004);
    #[cfg(unix)]
    command.process_group(0);
    command.kill_on_drop(true);
    let child = command.spawn()?;
    let job = ProcessJob::attach(&child)?;
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::{Diagnostics::ToolHelp::*, Threading::*},
        };
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        anyhow::ensure!(snapshot != INVALID_HANDLE_VALUE, "无法枚举初始线程");
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut success = false;
        let mut next = Thread32First(snapshot, &mut entry);
        while next != 0 {
            if Some(entry.th32OwnerProcessID) == child.id() {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    success = ResumeThread(thread) != u32::MAX;
                    CloseHandle(thread);
                }
                break;
            }
            next = Thread32Next(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        anyhow::ensure!(success, "无法恢复受管进程线程");
    }
    Ok((child, job))
}
impl ProcessJob {
    /// Confirm the entire job is empty, not just the launcher PID.
    pub async fn terminate_and_wait(&self) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::JobObjects::*;
            unsafe {
                anyhow::ensure!(TerminateJobObject(self.0, 1) != 0, "终止进程树失败");
            }
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
                    unsafe { std::mem::zeroed() };
                unsafe {
                    anyhow::ensure!(
                        QueryInformationJobObject(
                            self.0,
                            JobObjectBasicAccountingInformation,
                            &mut info as *mut _ as _,
                            std::mem::size_of_val(&info) as u32,
                            std::ptr::null_mut()
                        ) != 0,
                        "读取进程树状态失败"
                    );
                }
                if info.ActiveProcesses == 0 {
                    return Ok(());
                }
                anyhow::ensure!(tokio::time::Instant::now() < deadline, "进程树停止超时");
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        #[cfg(unix)]
        {
            self.signal(libc::SIGTERM)?;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while self.exists() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            if self.exists() {
                self.signal(libc::SIGKILL)?;
            }
            // Parent owns/reaps the leader Child. kill(0) can still see its zombie
            // until that wait; do not confuse that with a live executing process.
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    #[tokio::test]
    async fn stopping_owned_group_stops_descendant_and_preserves_other_process() {
        let mut unrelated = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args(["-c", "sleep 60 & echo $!; wait"])
            .stdout(std::process::Stdio::piped());
        let (mut child, job) = spawn_owned(&mut command).unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .await
            .unwrap();
        let descendant: libc::pid_t = line.trim().parse().unwrap();
        job.terminate_and_wait().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
            .await
            .unwrap()
            .unwrap();
        // An orphan may be a zombie briefly until PID 1 reaps it; it must not execute.
        let status = std::process::Command::new("ps")
            .args(["-p", &descendant.to_string(), "-o", "stat="])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&status.stdout);
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "descendant still running: {state}"
        );
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().await.unwrap();
        unrelated.wait().await.unwrap();
    }
}
