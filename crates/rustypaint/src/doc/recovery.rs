use super::Rgba8;
use super::io::{self, SaveFormat};

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SNAPSHOT_EVERY: Duration = Duration::from_secs(15);

// An editor holds this for as long as it is running. The kernel drops it however the process dies,
// including a kill, so a lock that can be taken is proof the editor that wrote the snapshot is gone.
#[allow(
    dead_code,
    reason = "the lock lives exactly as long as this file handle is held open"
)]
pub struct Guard(std::fs::File);

pub fn hold(dir: &Path, id: &str) -> Option<Guard> {
    std::fs::create_dir_all(dir).ok()?;
    let file = std::fs::File::create(lock_path(dir, id)).ok()?;
    file.try_lock().ok()?;
    Some(Guard(file))
}

fn still_running(dir: &Path, id: &str) -> bool {
    let Ok(file) = std::fs::File::options()
        .read(true)
        .write(true)
        .open(lock_path(dir, id))
    else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        // A filesystem that cannot lock must not swallow the work it was guarding.
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

fn lock_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.lock"))
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Meta {
    path: Option<PathBuf>,
    transparent: bool,
    at: u64,
}

pub struct Recovered {
    pub pixels: Rgba8,
    pub path: Option<PathBuf>,
    pub transparent: bool,
    pub id: String,
}

// Two documents opened in the same second must not land on the same snapshot.
pub fn id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{}-{n}", std::process::id(), now())
}

pub fn write(
    dir: &Path,
    id: &str,
    pixels: &Rgba8,
    path: Option<&Path>,
    transparent: bool,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    io::save_as(pixels, &dir.join(format!("{id}.png")), SaveFormat::Png)?;
    stamp(dir, id, path.map(Path::to_path_buf), transparent)
}

pub fn clear(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(dir.join(format!("{id}.png")));
    let _ = std::fs::remove_file(dir.join(format!("{id}.toml")));
    let _ = std::fs::remove_file(lock_path(dir, id));
}

pub fn abandoned(dir: &Path) -> Vec<Recovered> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    sweep_locks(dir);

    let mut found: Vec<(u64, Recovered)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "toml"))
        .filter_map(|entry| {
            let meta = read_meta(&entry.path()).ok()?;
            let id = entry.path().file_stem()?.to_str()?.to_owned();
            if still_running(dir, &id) {
                return None;
            }
            let pixels = io::load(&dir.join(format!("{id}.png"))).ok()?;
            Some((
                meta.at,
                Recovered {
                    pixels,
                    path: meta.path,
                    transparent: meta.transparent,
                    id,
                },
            ))
        })
        .collect();

    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.into_iter().map(|(_, one)| one).collect()
}

// A lock nobody owns and nothing points at is left over from a session that ended.
fn sweep_locks(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "lock") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !dir.join(format!("{id}.toml")).exists() && !still_running(dir, id) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn stamp(dir: &Path, id: &str, path: Option<PathBuf>, transparent: bool) -> Result<(), String> {
    let meta = Meta {
        path,
        transparent,
        at: now(),
    };
    let text = toml::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{id}.toml")), text).map_err(|e| e.to_string())
}

fn read_meta(path: &Path) -> Result<Meta, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustypaint-recovery-{name}-{}", id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn red() -> Rgba8 {
        Rgba8::new(4, 3, [255, 0, 0, 255])
    }

    #[test]
    fn a_snapshot_comes_back_with_its_pixels_and_its_file() {
        let dir = scratch("roundtrip");
        let file = PathBuf::from("/tmp/thing.png");
        write(&dir, "one", &red(), Some(&file), true).unwrap();

        let found = abandoned(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pixels, red());
        assert_eq!(found[0].path.as_deref(), Some(file.as_path()));
        assert!(found[0].transparent, "the backing is part of the document");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_snapshot_a_running_editor_still_holds_is_left_alone() {
        let dir = scratch("held");
        write(&dir, "one", &red(), None, false).unwrap();
        let guard = hold(&dir, "one").expect("the running editor takes its lock");

        assert!(
            abandoned(&dir).is_empty(),
            "a live session must not be offered back to itself"
        );

        drop(guard);
        assert_eq!(
            abandoned(&dir).len(),
            1,
            "and the moment it lets go the work is recoverable"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn work_is_offered_the_instant_the_editor_is_gone() {
        let dir = scratch("instant");
        write(&dir, "one", &red(), None, false).unwrap();
        drop(hold(&dir, "one"));
        assert_eq!(
            abandoned(&dir).len(),
            1,
            "relaunching straight away must still find it"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn two_editors_cannot_hold_the_same_snapshot() {
        let dir = scratch("contended");
        write(&dir, "one", &red(), None, false).unwrap();
        let first = hold(&dir, "one").unwrap();
        assert!(hold(&dir, "one").is_none(), "the second one is turned away");
        drop(first);
        assert!(
            hold(&dir, "one").is_some(),
            "and can have it once it is free"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clearing_leaves_nothing_to_recover() {
        let dir = scratch("clear");
        write(&dir, "one", &red(), None, false).unwrap();
        clear(&dir, "one");
        assert!(abandoned(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_newest_abandoned_snapshot_comes_first() {
        let dir = scratch("order");
        write(&dir, "older", &red(), None, false).unwrap();
        std::fs::write(
            dir.join("older.toml"),
            toml::to_string_pretty(&Meta {
                path: None,
                transparent: false,
                at: now() - 500,
            })
            .unwrap(),
        )
        .unwrap();
        write(&dir, "newer", &red(), None, false).unwrap();

        let found = abandoned(&dir);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "newer");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        assert!(abandoned(&scratch("missing")).is_empty());
    }

    #[test]
    fn a_snapshot_with_no_image_beside_it_is_skipped() {
        let dir = scratch("orphan");
        std::fs::create_dir_all(&dir).unwrap();
        stamp(&dir, "one", None, false).unwrap();
        assert!(abandoned(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lock_nothing_points_at_is_swept_up() {
        let dir = scratch("sweep");
        drop(hold(&dir, "leftover"));
        assert!(dir.join("leftover.lock").exists());

        let _ = abandoned(&dir);
        assert!(
            !dir.join("leftover.lock").exists(),
            "an empty lock file does not pile up forever"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
