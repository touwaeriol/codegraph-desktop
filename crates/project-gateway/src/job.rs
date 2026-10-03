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
    terminated: std::sync::atomic::AtomicBool,
}
#[cfg(unix)]
impl ProcessJob {
    pub fn attach(child: &tokio::process::Child) -> anyhow::Result<Self> {
        let pid = child.id().ok_or_else(|| anyhow::anyhow!("进程已退出"))? as libc::pid_t;
        anyhow::ensure!(unsafe { libc::getpgid(pid) } == pid, "进程未进入独立进程组");
        Ok(Self {
            pgid: pid,
            terminated: std::sync::atomic::AtomicBool::new(false),
        })
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
    fn has_live_members(&self) -> std::io::Result<bool> {
        let output = std::process::Command::new("/bin/ps")
            .args(["-axo", "pgid=,stat="])
            .env("LC_ALL", "C")
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other("无法读取进程组状态"));
        }
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| std::io::Error::other("进程组状态编码无效"))?;
        group_has_live_members(text, self.pgid)
    }
    fn finish(&self) {
        self.terminated
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn signal_live_group(&self, signal: libc::c_int) -> std::io::Result<()> {
        match self.signal(signal) {
            Ok(()) => Ok(()),
            // Darwin may deny signalling a group containing only zombies. Never
            // suppress a permission error while any executable member remains.
            Err(error) => {
                if self.has_live_members()? {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }
}
#[cfg(unix)]
fn group_has_live_members(text: &str, pgid: libc::pid_t) -> std::io::Result<bool> {
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let group = fields
            .next()
            .and_then(|v| v.parse::<libc::pid_t>().ok())
            .ok_or_else(|| std::io::Error::other("进程组状态格式无效"))?;
        let state = fields
            .next()
            .ok_or_else(|| std::io::Error::other("进程状态缺失"))?;
        if group == pgid && !state.starts_with('Z') {
            return Ok(true);
        }
    }
    Ok(false)
}
#[cfg(unix)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        if !self.terminated.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = self.signal_live_group(libc::SIGKILL);
        }
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
            if !self.has_live_members()? {
                self.finish();
                return Ok(());
            }
            self.signal_live_group(libc::SIGTERM)?;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
            while self.has_live_members()? && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            if self.has_live_members()? {
                self.signal_live_group(libc::SIGKILL)?;
            }
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
            while self.has_live_members()? {
                anyhow::ensure!(
                    tokio::time::Instant::now() < deadline,
                    "进程组仍含活进程，停止未确认"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            self.finish();
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    async fn check_group_cleanup(script: &str) {
        let mut unrelated = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args(["-c", script])
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
    #[test]
    fn zombie_groups_are_distinct_from_running_or_uninterruptible_members() {
        assert!(!group_has_live_members("42 Z\n42 Z+\n43 S\n", 42).unwrap());
        assert!(group_has_live_members("42 Z\n42 S\n", 42).unwrap());
        assert!(group_has_live_members("42 D\n", 42).unwrap());
        assert!(group_has_live_members("bad state\n", 42).is_err());
    }
    #[tokio::test]
    async fn stopping_owned_group_stops_descendant_and_preserves_other_process() {
        check_group_cleanup("sleep 60 & echo $!; wait").await;
    }
    #[tokio::test]
    async fn sigterm_resistant_group_requires_confirmed_sigkill_cleanup() {
        check_group_cleanup("trap '' TERM; sleep 60 & echo $!; wait").await;
    }
}
