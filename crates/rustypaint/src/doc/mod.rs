pub mod clipboard;
pub mod history;
pub mod image;
pub mod io;
pub mod rect;
pub mod transform;

pub use image::Rgba8;
pub use rect::Rect;

use history::{Edit, History, Snapshot};
use std::path::PathBuf;

pub type Version = u64;

pub struct Document {
    pixels: Rgba8,
    version: Version,
    pub transparent: bool,
    pub path: Option<PathBuf>,
    touched: bool,
    saved: u64,
    history: History,
}

impl Document {
    pub const DEFAULT_SIZE: (u32, u32) = (1152, 648);

    pub fn blank_sized(width: u32, height: u32, transparent: bool) -> Self {
        Self {
            pixels: Rgba8::transparent(width, height),
            version: 1,
            transparent,
            path: None,
            touched: false,
            saved: 0,
            history: History::default(),
        }
    }

    pub fn from_image(pixels: Rgba8, path: Option<PathBuf>) -> Self {
        let transparent = has_transparency(&pixels);
        Self {
            pixels,
            version: 1,
            transparent,
            path,
            touched: false,
            saved: 0,
            history: History::default(),
        }
    }

    pub fn pixels(&self) -> &Rgba8 {
        &self.pixels
    }

    pub fn version(&self) -> Version {
        self.version
    }

    pub fn size(&self) -> (u32, u32) {
        self.pixels.size()
    }

    pub fn backdrop(&self) -> [u8; 4] {
        [0, 0, 0, 0]
    }

    pub fn has_backing(&self) -> bool {
        !self.transparent
    }

    pub fn flattened(&self) -> Rgba8 {
        if self.has_backing() && has_transparency(&self.pixels) {
            self.pixels.flattened_onto([255, 255, 255, 255])
        } else {
            self.pixels.clone()
        }
    }

    pub fn edit(&mut self) -> &mut Rgba8 {
        self.version += 1;
        self.touched = true;
        &mut self.pixels
    }

    // True between an edit and the commit that files it, when the canvas is ahead of its history.
    pub fn touched(&self) -> bool {
        self.touched
    }

    pub fn modified(&self) -> bool {
        self.touched || self.history.mark() != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.touched = false;
        self.saved = self.history.mark();
    }

    pub(crate) fn restore_live(&mut self, pixels: Rgba8, touched: bool) {
        self.pixels = pixels;
        self.version += 1;
        self.touched = touched;
    }

    pub fn commit(&mut self, label: &'static str, rect: Rect, before: &Rgba8) {
        self.touched = false;
        let rect = rect.clamped(self.pixels.width(), self.pixels.height());
        if rect.is_empty() {
            return;
        }
        let (was, now) = (
            Edit::extract(before, rect),
            Edit::extract(&self.pixels, rect),
        );
        if was == now {
            return;
        }
        self.history.push(
            label,
            Edit::Region {
                rect,
                before: was,
                after: now,
            },
        );
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            pixels: self.pixels.clone(),
            transparent: self.transparent,
        }
    }

    fn reshape(&mut self, label: &'static str, pixels: Rgba8, transparent: bool) {
        let before = self.snapshot();
        if before.pixels.size() == pixels.size()
            && before.transparent == transparent
            && before.pixels == pixels
        {
            return;
        }
        self.pixels = pixels;
        self.transparent = transparent;
        self.history.push(
            label,
            Edit::Whole {
                before,
                after: self.snapshot(),
            },
        );
        self.version += 1;
        self.touched = false;
    }

    pub fn resize_canvas(&mut self, width: u32, height: u32, anchor: transform::Anchor) {
        if width == 0 || height == 0 {
            return;
        }
        let fill = self.backdrop();
        let out = transform::resize_canvas(&self.pixels, width, height, anchor, fill);
        self.reshape("Resize canvas", out, self.transparent);
    }

    pub fn resize_image(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let out = transform::scale(&self.pixels, width, height);
        self.reshape("Resize image", out, self.transparent);
    }

    #[allow(dead_code, reason = "the crop tool that drives this is Phase 5")]
    pub fn crop(&mut self, rect: Rect) {
        let rect = rect.clamped(self.pixels.width(), self.pixels.height());
        if rect.is_empty() {
            return;
        }
        let out = transform::crop(&self.pixels, rect);
        self.reshape("Crop", out, self.transparent);
    }

    pub fn rotate(&mut self, clockwise: bool) {
        let out = transform::rotate_90(&self.pixels, clockwise);
        let label = if clockwise {
            "Rotate right"
        } else {
            "Rotate left"
        };
        self.reshape(label, out, self.transparent);
    }

    pub fn flip(&mut self, horizontal: bool) {
        let out = if horizontal {
            transform::flip_horizontal(&self.pixels)
        } else {
            transform::flip_vertical(&self.pixels)
        };
        let label = if horizontal {
            "Flip horizontal"
        } else {
            "Flip vertical"
        };
        self.reshape(label, out, self.transparent);
    }

    pub fn set_transparent(&mut self, transparent: bool) {
        if transparent == self.transparent {
            return;
        }
        let label = if transparent {
            "Transparent canvas"
        } else {
            "Opaque canvas"
        };
        let pixels = self.pixels.clone();
        self.reshape(label, pixels, transparent);
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> Option<Option<Rect>> {
        let changed = self.history.undo(&mut self.pixels, &mut self.transparent)?;
        self.version += 1;
        self.touched = false;
        Some(changed)
    }

    pub fn redo(&mut self) -> Option<Option<Rect>> {
        let changed = self.history.redo(&mut self.pixels, &mut self.transparent)?;
        self.version += 1;
        self.touched = false;
        Some(changed)
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());
        if self.modified() {
            format!("{name}*")
        } else {
            name
        }
    }
}

fn has_transparency(pixels: &Rgba8) -> bool {
    pixels
        .as_bytes()
        .iter()
        .skip(3)
        .step_by(image::CHANNELS)
        .any(|&a| a != 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_image_with_alpha_starts_transparent() {
        let mut px = Rgba8::white(4, 4);
        px.pixels_mut()[3] = 0;
        assert!(Document::from_image(px, None).transparent);
        assert!(!Document::from_image(Rgba8::white(4, 4), None).transparent);
    }

    #[test]
    fn resizing_is_undoable_and_restores_the_old_size() {
        let mut d = Document::blank_sized(8, 8, false);
        d.resize_canvas(16, 4, transform::Anchor::TopLeft);
        assert_eq!(d.size(), (16, 4));

        assert_eq!(d.undo().unwrap(), None, "a resize changes everything");
        assert_eq!(d.size(), (8, 8));
    }

    #[test]
    fn the_version_advances_on_every_change() {
        let mut d = Document::blank_sized(8, 8, false);
        let v0 = d.version();
        d.edit();
        assert!(d.version() > v0);
        let v1 = d.version();
        d.resize_canvas(4, 4, transform::Anchor::TopLeft);
        assert!(d.version() > v1);
    }

    fn paint(d: &mut Document, colour: [u8; 4]) {
        let before = d.pixels().clone();
        let rect = Rect::new(0, 0, 2, 2);
        let stride = d.size().0 as usize * image::CHANNELS;
        let dst = d.edit().pixels_mut();
        for y in rect.rows() {
            for x in rect.cols() {
                let i = y as usize * stride + x as usize * image::CHANNELS;
                dst[i..i + image::CHANNELS].copy_from_slice(&colour);
            }
        }
        d.commit("Marker", rect, &before);
    }

    #[test]
    fn undoing_every_change_leaves_nothing_to_save() {
        let mut d = Document::blank_sized(8, 8, false);
        paint(&mut d, [255, 0, 0, 255]);
        assert!(d.modified());

        d.undo().unwrap();
        assert!(!d.modified(), "the canvas is back where it started");
        d.redo().unwrap();
        assert!(d.modified(), "and the change is a change again");
    }

    #[test]
    fn saving_moves_the_mark_the_undo_stack_is_measured_against() {
        let mut d = Document::blank_sized(8, 8, false);
        paint(&mut d, [255, 0, 0, 255]);
        d.mark_saved();
        assert!(!d.modified());

        paint(&mut d, [0, 0, 255, 255]);
        assert!(d.modified());
        d.undo().unwrap();
        assert!(!d.modified(), "back at what is on disk");
        d.undo().unwrap();
        assert!(d.modified(), "past it is a change again");
    }

    #[test]
    fn an_edit_that_changes_no_pixels_is_no_change_at_all() {
        let mut d = Document::blank_sized(8, 8, false);
        paint(&mut d, [0, 0, 0, 0]);
        assert!(!d.modified(), "the canvas was already empty there");
        assert!(!d.can_undo(), "and there is nothing to undo");
    }

    #[test]
    fn the_title_marks_unsaved_changes() {
        let mut d = Document::blank_sized(4, 4, false);
        assert_eq!(d.title(), "Untitled");
        d.edit();
        assert_eq!(d.title(), "Untitled*");
    }
}
