use super::Rgba8;
use super::io::{self, SaveFormat};

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SNAPSHOT_EVERY: Duration = Duration::from_secs(15);

// A running editor rewrites its stamp every snapshot, so anything this old belongs to a dead one.
pub const STALE_AFTER: Duration = Duration::from_secs(60);

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

pub fn id() -> String {
    format!("{}-{}", std::process::id(), now())
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

// Keeps a quiet session's snapshot from ageing into one a later launch would offer to restore.
pub fn touch(dir: &Path, id: &str) -> Result<(), String> {
    let meta = read_meta(&dir.join(format!("{id}.toml")))?;
    stamp(dir, id, meta.path, meta.transparent)
}

pub fn clear(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(dir.join(format!("{id}.png")));
    let _ = std::fs::remove_file(dir.join(format!("{id}.toml")));
}

pub fn abandoned(dir: &Path, stale_after: Duration) -> Vec<Recovered> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let cutoff = now().saturating_sub(stale_after.as_secs());

    let mut found: Vec<(u64, Recovered)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "toml"))
        .filter_map(|entry| {
            let meta = read_meta(&entry.path()).ok()?;
            if meta.at > cutoff {
                return None;
            }
            let id = entry.path().file_stem()?.to_str()?.to_owned();
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

// Ages a snapshot so a test can reach the state a crash would have left behind.
#[cfg(test)]
pub fn backdate(dir: &Path, id: &str, by: Duration) -> Result<(), String> {
    let path = dir.join(format!("{id}.toml"));
    let mut meta = read_meta(&path)?;
    meta.at = meta.at.saturating_sub(by.as_secs());
    let text = toml::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
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

        let found = abandoned(&dir, Duration::ZERO);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pixels, red());
        assert_eq!(found[0].path.as_deref(), Some(file.as_path()));
        assert!(found[0].transparent, "the backing is part of the document");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fresh_snapshot_belongs_to_a_running_editor_and_is_left_alone() {
        let dir = scratch("fresh");
        write(&dir, "one", &red(), None, false).unwrap();
        assert!(
            abandoned(&dir, STALE_AFTER).is_empty(),
            "a live session must not be offered back to itself"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn touching_keeps_a_quiet_session_from_going_stale() {
        let dir = scratch("touch");
        write(&dir, "one", &red(), None, false).unwrap();
        touch(&dir, "one").unwrap();

        let meta = read_meta(&dir.join("one.toml")).unwrap();
        assert!(meta.at >= now() - 1, "the stamp moved forward");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clearing_leaves_nothing_to_recover() {
        let dir = scratch("clear");
        write(&dir, "one", &red(), None, false).unwrap();
        clear(&dir, "one");
        assert!(abandoned(&dir, Duration::ZERO).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_newest_abandoned_snapshot_comes_first() {
        let dir = scratch("order");
        write(&dir, "older", &red(), None, false).unwrap();
        stamp(&dir, "older", None, false).unwrap();
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

        let found = abandoned(&dir, Duration::ZERO);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "newer");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        assert!(abandoned(&scratch("missing"), Duration::ZERO).is_empty());
    }

    #[test]
    fn a_snapshot_with_no_image_beside_it_is_skipped() {
        let dir = scratch("orphan");
        std::fs::create_dir_all(&dir).unwrap();
        stamp(&dir, "one", None, false).unwrap();
        assert!(abandoned(&dir, Duration::ZERO).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
