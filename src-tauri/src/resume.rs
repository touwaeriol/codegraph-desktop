//! Engines to bring back on the next launch, kept in a small JSON file in the
//! application data directory.
//!
//! A successful start records an engine; an explicit stop or a failed start
//! removes it. App exits, crashes and update shutdowns leave the file alone,
//! so the next launch resumes exactly what was running.
use crate::models::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub const ENGINES: [&str; 3] = ["codegraph", "serena", "warpgrep"];
const FILE: &str = "engine-resume.json";
static LOCK: Mutex<()> = Mutex::new(());

/// Project id to the engines that should resume.
type Store = BTreeMap<String, Vec<String>>;

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}
fn read(dir: &Path) -> Store {
    match std::fs::read(path(dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            eprintln!("Ignoring unreadable {FILE}: {error}");
            Store::new()
        }),
        Err(_) => Store::new(),
    }
}
fn write(dir: &Path, store: &Store) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(store).map_err(|e| AppError::new("SERIALIZATION", e))?;
    let temp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path(dir))?;
    Ok(())
}
fn update(dir: &Path, change: impl FnOnce(&mut Store)) -> Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = read(dir);
    let before = store.clone();
    change(&mut store);
    store.retain(|_, engines| !engines.is_empty());
    if store == before {
        return Ok(());
    }
    write(dir, &store)
}

pub fn set(dir: &Path, project_id: &str, engine: &str, resume: bool) -> Result<()> {
    if !ENGINES.contains(&engine) {
        return Err(AppError::new("INVALID_ENGINE", engine));
    }
    update(dir, |store| {
        let engines = store.entry(project_id.into()).or_default();
        engines.retain(|e| e != engine);
        if resume {
            engines.push(engine.into());
        }
    })
}
pub fn clear_engine(dir: &Path, engine: &str) -> Result<()> {
    update(dir, |store| {
        for engines in store.values_mut() {
            engines.retain(|e| e != engine);
        }
    })
}
pub fn forget_project(dir: &Path, project_id: &str) -> Result<()> {
    update(dir, |store| {
        store.remove(project_id);
    })
}
/// Engines to resume, in project order: remembered engines plus CodeGraph for
/// projects marked auto-start. Entries for removed projects are ignored.
pub fn plan(dir: &Path, projects: &[Project]) -> Vec<(String, Vec<&'static str>)> {
    let store = {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        read(dir)
    };
    projects
        .iter()
        .filter_map(|p| {
            let remembered = store.get(&p.id);
            let engines = ENGINES
                .into_iter()
                .filter(|engine| {
                    (*engine == "codegraph" && p.auto_start)
                        || remembered.is_some_and(|r| r.iter().any(|e| e == engine))
                })
                .collect::<Vec<_>>();
            (!engines.is_empty()).then(|| (p.id.clone(), engines))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project(id: &str, auto_start: bool) -> Project {
        Project {
            previous_roots: vec![],
            id: id.into(),
            name: id.into(),
            root_path: id.into(),
            canonical_path: id.into(),
            notes: String::new(),
            auto_start,
            created_at: now(),
            updated_at: now(),
        }
    }
    #[test]
    fn records_survive_restart_and_follow_explicit_stops() {
        let dir = tempfile::tempdir().unwrap();
        let projects = [project("a", false), project("b", false), project("c", true)];
        set(dir.path(), "a", "serena", true).unwrap();
        set(dir.path(), "a", "codegraph", true).unwrap();
        set(dir.path(), "a", "codegraph", true).unwrap();
        set(dir.path(), "b", "warpgrep", true).unwrap();
        set(dir.path(), "removed", "codegraph", true).unwrap();
        assert!(set(dir.path(), "a", "unknown", true).is_err());
        assert_eq!(
            plan(dir.path(), &projects),
            vec![
                ("a".to_owned(), vec!["codegraph", "serena"]),
                ("b".to_owned(), vec!["warpgrep"]),
                ("c".to_owned(), vec!["codegraph"]),
            ]
        );
        set(dir.path(), "a", "serena", false).unwrap();
        clear_engine(dir.path(), "warpgrep").unwrap();
        forget_project(dir.path(), "removed").unwrap();
        assert_eq!(
            plan(dir.path(), &projects[..2]),
            vec![("a".to_owned(), vec!["codegraph"])]
        );
        let saved: Store =
            serde_json::from_slice(&std::fs::read(path(dir.path())).unwrap()).unwrap();
        assert_eq!(saved, Store::from([("a".into(), vec!["codegraph".into()])]));
        assert!(!dir.path().join(format!("{FILE}.tmp")).exists());
    }
    #[test]
    fn missing_or_corrupt_file_resumes_nothing_but_auto_start() {
        let dir = tempfile::tempdir().unwrap();
        let projects = [project("a", false), project("b", true)];
        assert_eq!(
            plan(dir.path(), &projects),
            vec![("b".to_owned(), vec!["codegraph"])]
        );
        std::fs::write(path(dir.path()), b"{not json").unwrap();
        assert_eq!(
            plan(dir.path(), &projects),
            vec![("b".to_owned(), vec!["codegraph"])]
        );
        set(dir.path(), "a", "serena", true).unwrap();
        assert_eq!(
            plan(dir.path(), &projects),
            vec![
                ("a".to_owned(), vec!["serena"]),
                ("b".to_owned(), vec!["codegraph"]),
            ]
        );
    }
}
