#![allow(dead_code, reason = "reference table, filled in ahead of the widgets")]

pub const MENU: &str = "Menu";
pub const UNDO: &str = "Undo";
pub const REDO: &str = "Redo";
pub const HISTORY: &str = "History";
pub const PASTE: &str = "Paste";
pub const CUT: &str = "Cut";
pub const COPY: &str = "Copy";
pub const SELECT: &str = "Select";
pub const SELECT_ALL: &str = "Select all";
pub const CROP: &str = "Crop";
pub const ZOOM: &str = "Zoom";
pub const ZOOM_IN: &str = "Zoom in";
pub const ZOOM_OUT: &str = "Zoom out";
pub const ACTUAL_SIZE: &str = "100%";
pub const NEW: &str = "New";
pub const OPEN: &str = "Open";
pub const SAVE: &str = "Save";
pub const SAVE_AS: &str = "Save as";
pub const INSERT: &str = "Insert";
pub const SETTINGS: &str = "Settings";
pub const BACK: &str = "Back";
pub const ROTATE_LEFT: &str = "Rotate left";
pub const ROTATE_RIGHT: &str = "Rotate right";
pub const FLIP_HORIZONTAL: &str = "Flip horizontal";
pub const FLIP_VERTICAL: &str = "Flip vertical";
pub const TRANSPARENT_CANVAS: &str = "Transparent canvas";
pub const SHOW_CANVAS: &str = "Show canvas";
pub const FILL_TYPE: &str = "Fill type";
pub const LINE_TYPE: &str = "Line type";
pub const THICKNESS: &str = "Thickness";
pub const OPACITY: &str = "Opacity";
pub const STICKER_OPACITY: &str = "Sticker opacity";
pub const ROTATE_AND_FLIP: &str = "Rotate and flip";
pub const TEXT: &str = "2D text";
pub const SMART_CUTOUT: &str = "Smart cutout";

pub const SELECT_BOX: &str = "Rectangular select";
pub const SELECT_FREEFORM: &str = "Freeform select";

pub const FIT_TO_WINDOW: &str = "Fit to window";

pub fn with_key(label: &str, key: &str) -> String {
    format!("{label} ({key})")
}

pub fn shift_key(key: &str) -> String {
    command_key(&format!("Shift+{key}"))
}

pub fn command_key(key: &str) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "Command"
    } else {
        "Ctrl"
    };
    format!("{modifier}+{key}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_key_hint_reads_the_way_windows_writes_one() {
        assert_eq!(super::with_key(super::UNDO, "Ctrl+Z"), "Undo (Ctrl+Z)");
    }

    #[test]
    fn command_hints_name_the_platform_modifier() {
        let expected = if cfg!(target_os = "macos") {
            "Command+Z"
        } else {
            "Ctrl+Z"
        };
        assert_eq!(super::command_key("Z"), expected);
    }
}
