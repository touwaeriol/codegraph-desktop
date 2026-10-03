use crate::models::{AppError, Result};
use std::{
    collections::HashMap,
    future::Future,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub const UPDATE_FLAG: &str = "--shutdown-for-update";
pub fn requests_update(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == UPDATE_FLAG)
}
pub fn update_targets_current_executable(args: &[String], current: &Path) -> bool {
    requests_update(args)
        && args
            .first()
            .and_then(|path| dunce::canonicalize(path).ok())
            .zip(dunce::canonicalize(current).ok())
            .is_some_and(|(requested, current)| requested == current)
}
struct Task {
    cancel: Arc<AtomicBool>,
    abort: tokio::task::AbortHandle,
}
#[derive(Default)]
struct Registry {
    closed: bool,
    tasks: HashMap<String, Task>,
}
#[derive(Default)]
pub struct TaskRegistry {
    inner: Mutex<Registry>,
}
struct Completion {
    registry: Arc<TaskRegistry>,
    id: String,
}
impl Drop for Completion {
    fn drop(&mut self) {
        self.registry.inner.lock().unwrap().tasks.remove(&self.id);
    }
}
pub struct DrainReport {
    pub aborted: usize,
    pub remaining: usize,
}
impl TaskRegistry {
    pub fn spawn(
        self: &Arc<Self>,
        id: String,
        cancel: Arc<AtomicBool>,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(AppError::new("APP_EXITING", "应用正在退出"));
        }
        let completion = Completion {
            registry: self.clone(),
            id: id.clone(),
        };
        let handle = tauri::async_runtime::spawn(async move {
            let _completion = completion;
            future.await;
        });
        inner.tasks.insert(
            id,
            Task {
                cancel,
                abort: handle.inner().abort_handle(),
            },
        );
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let inner = self.inner.lock().unwrap();
        let task = inner
            .tasks
            .get(id)
            .ok_or_else(|| AppError::new("TASK_NOT_FOUND", "任务已结束"))?;
        task.cancel.store(true, Ordering::SeqCst);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().tasks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    async fn wait_empty(&self, budget: Duration) {
        let _ = tokio::time::timeout(budget, async {
            while !self.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
    }
    pub async fn close_and_drain(&self, grace: Duration, abort_budget: Duration) -> DrainReport {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.closed = true;
            for task in inner.tasks.values() {
                task.cancel.store(true, Ordering::SeqCst);
            }
        }
        self.wait_empty(grace).await;
        let aborts = self
            .inner
            .lock()
            .unwrap()
            .tasks
            .values()
            .map(|task| task.abort.clone())
            .collect::<Vec<_>>();
        for abort in &aborts {
            abort.abort();
        }
        self.wait_empty(abort_budget).await;
        DrainReport {
            aborted: aborts.len(),
            remaining: self.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_flag_is_exact_and_targets_only_same_executable() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join("ours.exe");
        let other = dir.path().join("other.exe");
        std::fs::write(&ours, []).unwrap();
        std::fs::write(&other, []).unwrap();
        let args = vec![ours.to_string_lossy().into(), UPDATE_FLAG.into()];
        assert!(update_targets_current_executable(&args, &ours));
        assert!(!update_targets_current_executable(&args, &other));
        assert!(!requests_update(&[
            "app".into(),
            "--shutdown-for-update-extra".into()
        ]));
        assert!(!requests_update(&[UPDATE_FLAG.into()]));
    }
    #[tokio::test]
    async fn shutdown_waits_for_cooperative_cleanup_and_rejects_new_work() {
        let registry = Arc::new(TaskRegistry::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let cleaned = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let marker = cleaned.clone();
        registry
            .spawn("index".into(), cancel, async move {
                while !flag.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
                marker.store(true, Ordering::SeqCst);
            })
            .unwrap();
        let report = registry
            .close_and_drain(Duration::from_secs(1), Duration::from_secs(1))
            .await;
        assert_eq!(report.aborted, 0);
        assert_eq!(report.remaining, 0);
        assert!(cleaned.load(Ordering::SeqCst));
        assert!(registry
            .spawn("later".into(), Arc::new(AtomicBool::new(false)), async {})
            .is_err());
    }
    #[tokio::test]
    async fn blocked_task_is_aborted_and_resource_drops_before_shutdown_returns() {
        struct Resource(Arc<AtomicBool>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let registry = Arc::new(TaskRegistry::default());
        let dropped = Arc::new(AtomicBool::new(false));
        let resource = Resource(dropped.clone());
        registry
            .spawn(
                "starting".into(),
                Arc::new(AtomicBool::new(false)),
                async move {
                    let _resource = resource;
                    std::future::pending::<()>().await;
                },
            )
            .unwrap();
        let started = tokio::time::Instant::now();
        let report = registry
            .close_and_drain(Duration::from_millis(20), Duration::from_secs(1))
            .await;
        assert_eq!(report.aborted, 1);
        assert_eq!(report.remaining, 0);
        assert!(dropped.load(Ordering::SeqCst));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
