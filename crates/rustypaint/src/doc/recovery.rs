use super::io::{self, Loaded, SaveFormat};
use super::layers::Stack;
use super::ora;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// How often the beat looks for work that has moved on since the last snapshot, and the shortest gap
// allowed between two writes so a long brush stroke cannot keep the encoder busy end to end.
pub const SNAPSHOT_EVERY: Duration = Duration::from_secs(2);
pub const SNAPSHOT_GAP: Duration = Duration::from_secs(1);

// An editor holds this for as long as it is running. The kernel drops it however the process dies,
// including a kill, so a lock that can be taken is proof the session that wrote it is gone.
#[allow(
    dead_code,
    reason = "the lock lives exactly as long as this file handle is held open"
)]
pub struct Guard(std::fs::File);

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Entry {
    slot: String,
    path: Option<PathBuf>,
    transparent: bool,
    unsaved: bool,
    merged_only: bool,
}

// One open picture as the session index records it.
pub struct Open {
    pub slot: String,
    pub path: Option<PathBuf>,
    pub transparent: bool,
    pub unsaved: bool,
    pub merged_only: bool,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Index {
    active: usize,
    at: u64,
    document: Vec<Entry>,
}

pub struct Document {
    pub stack: Stack,
    pub path: Option<PathBuf>,
    pub unsaved: bool,
    pub merged_only: bool,
}

// One editor's whole set of open pictures, which is what a session is.
pub struct Session {
    pub documents: Vec<Document>,
    pub active: usize,
    pub id: String,
}

pub fn id() -> String {
    format!("{}-{}", std::process::id(), now())
}

pub fn hold(root: &Path, id: &str) -> Option<Guard> {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::File::create(dir.join("lock")).ok()?;
    file.try_lock().ok()?;
    Some(Guard(file))
}

// A flat picture is kept as a PNG and anything with layers as an OpenRaster project, and whichever
// is written replaces the other so a slot never comes back as an older picture.
pub fn write_document(root: &Path, id: &str, slot: &str, stack: &Stack) -> Result<(), String> {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (format, other) = if stack.layered() {
        (SaveFormat::Ora, SaveFormat::Png)
    } else {
        (SaveFormat::Png, SaveFormat::Ora)
    };
    let written = dir.join(format!("{slot}.{}", format.extension()));
    io::save_document(stack, &written, format)?;
    let _ = std::fs::remove_file(dir.join(format!("{slot}.{}", other.extension())));
    Ok(())
}

fn read_document(dir: &Path, slot: &str, transparent: bool) -> Option<(Stack, bool)> {
    let project = dir.join(format!("{slot}.{}", ora::EXTENSION));
    let (mut stack, merged_only) = if project.exists() {
        match io::open(&project).ok()? {
            Loaded::Layers { stack, .. } => (stack, false),
            Loaded::Merged(pixels) | Loaded::Image(pixels) => (flat(pixels), true),
        }
    } else {
        (
            flat(io::load(&dir.join(format!("{slot}.png"))).ok()?),
            false,
        )
    };
    stack.transparent = transparent;
    Some((stack, merged_only))
}

fn from_disk(path: &Path, transparent: bool) -> Option<(Stack, bool)> {
    Some(match io::open(path).ok()? {
        Loaded::Image(pixels) => {
            let mut stack = flat(pixels);
            stack.transparent = transparent;
            (stack, false)
        }
        Loaded::Layers { stack, .. } => (stack, false),
        Loaded::Merged(pixels) => (flat(pixels), true),
    })
}

fn flat(pixels: super::Rgba8) -> Stack {
    Stack {
        layers: vec![super::Layer {
            id: 1,
            name: crate::i18n::layer_background().to_owned(),
            visible: true,
            opacity: super::layers::OPAQUE,
            pixels,
        }],
        active: 0,
        transparent: false,
    }
}

// The index is the tab order. Slots with no entry here are no longer open and their pixels go.
pub fn write_index(root: &Path, id: &str, open: &[Open], active: usize) -> Result<(), String> {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let index = Index {
        active,
        at: now(),
        document: open
            .iter()
            .map(|one| Entry {
                slot: one.slot.clone(),
                path: one.path.clone(),
                transparent: one.transparent,
                unsaved: one.unsaved,
                merged_only: one.merged_only,
            })
            .collect(),
    };
    let text = toml::to_string_pretty(&index).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("session.toml"), text).map_err(|e| e.to_string())?;

    let keep: Vec<&str> = open
        .iter()
        .filter(|one| one.unsaved)
        .map(|one| one.slot.as_str())
        .collect();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|e| e != "png" && e != ora::EXTENSION)
            {
                continue;
            }
            let stale = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_none_or(|slot| !keep.contains(&slot));
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

pub fn clear(root: &Path, id: &str) {
    let _ = std::fs::remove_dir_all(root.join(id));
}

pub fn abandoned(root: &Path) -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut found: Vec<(u64, Session)> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_owned();
            if still_running(root, &id) {
                return None;
            }
            let index = read_index(&entry.path().join("session.toml")).ok()?;
            // A session nobody had unsaved work in is a workspace, not a rescue. Leave it be.
            if !index.document.iter().any(|one| one.unsaved) {
                clear(root, &id);
                return None;
            }
            let documents: Vec<Document> = index
                .document
                .iter()
                .filter_map(|one| {
                    // Saved pictures are not copied into the session; they come back off disk.
                    let (stack, merged_only) = if one.unsaved {
                        read_document(&entry.path(), &one.slot, one.transparent)?
                    } else {
                        from_disk(one.path.as_deref()?, one.transparent)?
                    };
                    Some(Document {
                        stack,
                        path: one.path.clone(),
                        unsaved: one.unsaved,
                        merged_only: one.merged_only || merged_only,
                    })
                })
                .collect();
            if documents.is_empty() {
                clear(root, &id);
                return None;
            }
            Some((
                index.at,
                Session {
                    active: index.active.min(documents.len() - 1),
                    documents,
                    id,
                },
            ))
        })
        .collect();

    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.into_iter().map(|(_, one)| one).collect()
}

fn still_running(root: &Path, id: &str) -> bool {
    let Ok(file) = std::fs::File::options()
        .read(true)
        .write(true)
        .open(root.join(id).join("lock"))
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

#[cfg(test)]
pub fn open_slots(root: &Path, id: &str) -> Vec<String> {
    read_index(&root.join(id).join("session.toml"))
        .map(|index| index.document.into_iter().map(|one| one.slot).collect())
        .unwrap_or_default()
}

fn read_index(path: &Path) -> Result<Index, String> {
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
        let dir = std::env::temp_dir().join(format!("rustypaint-session-{name}-{}", id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn shade(v: u8) -> super::super::Rgba8 {
        super::super::Rgba8::new(4, 3, [v, 0, 0, 255])
    }

    fn picture(v: u8) -> Stack {
        flat(shade(v))
    }

    fn open(slot: &str, path: Option<PathBuf>, unsaved: bool) -> Open {
        Open {
            slot: slot.into(),
            path,
            transparent: false,
            unsaved,
            merged_only: false,
        }
    }

    fn seed(root: &Path, id: &str, docs: &[(&str, u8)]) {
        for (slot, v) in docs {
            write_document(root, id, slot, &picture(*v)).unwrap();
        }
        let open: Vec<Open> = docs
            .iter()
            .map(|(slot, _)| open(slot, None, true))
            .collect();
        write_index(root, id, &open, 0).unwrap();
    }

    #[test]
    fn a_session_comes_back_with_every_picture_that_was_open() {
        let root = scratch("whole");
        seed(&root, "dead", &[("0", 10), ("1", 20), ("2", 30)]);

        let found = abandoned(&root);
        assert_eq!(found.len(), 1, "one session, not three loose pictures");
        assert_eq!(found[0].documents.len(), 3);
        assert_eq!(found[0].documents[1].stack.layers[0].pixels, shade(20));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_tab_that_was_in_front_comes_back_in_front() {
        let root = scratch("active");
        write_document(&root, "dead", "0", &picture(1)).unwrap();
        write_document(&root, "dead", "1", &picture(2)).unwrap();
        write_index(
            &root,
            "dead",
            &[open("0", None, true), open("1", None, true)],
            1,
        )
        .unwrap();

        assert_eq!(abandoned(&root)[0].active, 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_session_a_running_editor_holds_is_left_alone() {
        let root = scratch("held");
        let guard = hold(&root, "live").unwrap();
        seed(&root, "live", &[("0", 10)]);

        assert!(abandoned(&root).is_empty(), "that editor is still working");
        drop(guard);
        assert_eq!(
            abandoned(&root).len(),
            1,
            "and the instant it dies the session is offered, with no waiting"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_session_where_nothing_was_at_risk_is_not_worth_offering() {
        let root = scratch("clean");
        write_index(&root, "dead", &[open("0", None, false)], 0).unwrap();

        assert!(abandoned(&root).is_empty(), "nothing was unsaved");
        assert!(
            !root.join("dead").exists(),
            "and it tidies itself up rather than lingering"
        );
    }

    #[test]
    fn a_saved_picture_in_the_session_comes_back_off_disk() {
        let root = scratch("ondisk");
        let file = root.join("on-disk.png");
        std::fs::create_dir_all(&root).unwrap();
        io::save_as(&shade(77), &file, SaveFormat::Png).unwrap();

        write_document(&root, "dead", "1", &picture(9)).unwrap();
        write_index(
            &root,
            "dead",
            &[open("0", Some(file), false), open("1", None, true)],
            0,
        )
        .unwrap();

        let found = abandoned(&root);
        assert_eq!(
            found[0].documents.len(),
            2,
            "the workspace comes back whole"
        );
        assert_eq!(found[0].documents[0].stack.layers[0].pixels, shade(77));
        assert!(!found[0].documents[0].unsaved, "it was already on disk");
        assert!(found[0].documents[1].unsaved);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_saved_picture_is_not_copied_into_the_session() {
        let root = scratch("nocopy");
        write_document(&root, "dead", "0", &picture(1)).unwrap();
        write_index(&root, "dead", &[open("0", None, false)], 0).unwrap();
        assert!(
            !root.join("dead").join("0.png").exists(),
            "pixels already on disk are not worth a second copy"
        );
    }

    #[test]
    fn a_closed_tab_takes_its_pixels_with_it() {
        let root = scratch("closed");
        seed(&root, "dead", &[("0", 10), ("1", 20)]);
        assert!(root.join("dead").join("1.png").exists());

        write_index(&root, "dead", &[open("0", None, true)], 0).unwrap();
        assert!(
            !root.join("dead").join("1.png").exists(),
            "the index is the tab order, so anything not in it is gone"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn clearing_leaves_nothing_behind() {
        let root = scratch("clear");
        seed(&root, "dead", &[("0", 10)]);
        clear(&root, "dead");
        assert!(abandoned(&root).is_empty());
    }

    #[test]
    fn the_newest_session_comes_first() {
        let root = scratch("order");
        seed(&root, "older", &[("0", 1)]);
        let text = std::fs::read_to_string(root.join("older").join("session.toml")).unwrap();
        let mut index: Index = toml::from_str(&text).unwrap();
        index.at -= 500;
        std::fs::write(
            root.join("older").join("session.toml"),
            toml::to_string_pretty(&index).unwrap(),
        )
        .unwrap();
        seed(&root, "newer", &[("0", 2)]);

        let found = abandoned(&root);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "newer");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        assert!(abandoned(&scratch("missing")).is_empty());
    }

    #[test]
    fn layered_work_comes_back_with_its_layers() {
        let root = scratch("layers");
        let mut stack = picture(10);
        stack.layers.push(super::super::Layer {
            id: 2,
            name: "Ink".into(),
            visible: false,
            opacity: 99,
            pixels: shade(200),
        });
        stack.active = 1;
        write_document(&root, "dead", "0", &picture(5)).unwrap();
        write_document(&root, "dead", "0", &stack).unwrap();
        assert!(
            !root.join("dead").join("0.png").exists(),
            "the flat snapshot it replaced is gone"
        );
        write_index(&root, "dead", &[open("0", None, true)], 0).unwrap();

        let found = abandoned(&root);
        let back = &found[0].documents[0].stack;
        assert_eq!(back.layers.len(), 2);
        assert_eq!(back.active, 1);
        assert_eq!(
            (
                back.layers[1].name.as_str(),
                back.layers[1].visible,
                back.layers[1].opacity
            ),
            ("Ink", false, 99)
        );
        assert_eq!(back.layers[1].pixels, shade(200));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
