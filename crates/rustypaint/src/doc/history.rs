use super::rect::Rect;
use super::{Rgba8, image::CHANNELS};

const BUDGET_BYTES: usize = 256 << 20;

#[derive(Clone)]
pub struct Snapshot {
    pub pixels: Rgba8,
    pub transparent: bool,
}

pub enum Edit {
    Region {
        rect: Rect,
        before: Vec<u8>,
        after: Vec<u8>,
    },
    Whole {
        before: Snapshot,
        after: Snapshot,
    },
}

impl Edit {
    pub fn extract(image: &Rgba8, rect: Rect) -> Vec<u8> {
        let stride = image.width() as usize * CHANNELS;
        let span = rect.width() as usize * CHANNELS;
        let mut out = Vec::with_capacity(rect.area() * CHANNELS);
        for y in rect.rows() {
            let start = y as usize * stride + rect.x0 as usize * CHANNELS;
            out.extend_from_slice(&image.as_bytes()[start..start + span]);
        }
        out
    }

    fn apply(image: &mut Rgba8, rect: Rect, pixels: &[u8]) {
        let stride = image.width() as usize * CHANNELS;
        let span = rect.width() as usize * CHANNELS;
        let dst = image.pixels_mut();
        for (row, y) in rect.rows().enumerate() {
            let start = y as usize * stride + rect.x0 as usize * CHANNELS;
            dst[start..start + span].copy_from_slice(&pixels[row * span..(row + 1) * span]);
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Edit::Region { before, after, .. } => before.len() + after.len(),
            Edit::Whole { before, after } => {
                before.pixels.as_bytes().len() + after.pixels.as_bytes().len()
            }
        }
    }
}

struct Entry {
    #[allow(dead_code, reason = "surfaced by the history flyout, which is Phase 7")]
    label: &'static str,
    edit: Edit,
}

#[derive(Default)]
pub struct History {
    entries: Vec<Entry>,
    depth: usize,
    bytes: usize,
}

impl History {
    pub fn push(&mut self, label: &'static str, edit: Edit) {
        for dropped in self.entries.drain(self.depth..) {
            self.bytes -= dropped.edit.bytes();
        }
        self.bytes += edit.bytes();
        self.entries.push(Entry { label, edit });
        self.depth = self.entries.len();
        self.trim();
    }

    fn trim(&mut self) {
        while self.bytes > BUDGET_BYTES && self.entries.len() > 1 {
            let dropped = self.entries.remove(0);
            self.bytes -= dropped.edit.bytes();
            self.depth -= 1;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.depth > 0
    }

    pub fn can_redo(&self) -> bool {
        self.depth < self.entries.len()
    }

    #[allow(dead_code, reason = "surfaced by the history flyout, which is Phase 7")]
    pub fn undo_label(&self) -> Option<&'static str> {
        self.entries
            .get(self.depth.checked_sub(1)?)
            .map(|e| e.label)
    }

    pub fn undo(&mut self, image: &mut Rgba8, transparent: &mut bool) -> Option<Option<Rect>> {
        let entry = self.entries.get(self.depth.checked_sub(1)?)?;
        self.depth -= 1;
        Some(restore(&entry.edit, image, transparent, false))
    }

    pub fn redo(&mut self, image: &mut Rgba8, transparent: &mut bool) -> Option<Option<Rect>> {
        let entry = self.entries.get(self.depth)?;
        self.depth += 1;
        Some(restore(&entry.edit, image, transparent, true))
    }
}

fn restore(edit: &Edit, image: &mut Rgba8, transparent: &mut bool, forwards: bool) -> Option<Rect> {
    match edit {
        Edit::Region {
            rect,
            before,
            after,
        } => {
            Edit::apply(image, *rect, if forwards { after } else { before });
            Some(*rect)
        }
        Edit::Whole { before, after } => {
            let target = if forwards { after } else { before };
            *image = target.pixels.clone();
            *transparent = target.transparent;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(fill: [u8; 4]) -> Rgba8 {
        Rgba8::new(8, 8, fill)
    }

    fn paint(image: &mut Rgba8, rect: Rect, colour: [u8; 4]) -> Edit {
        let before = Edit::extract(image, rect);
        let stride = image.width() as usize * CHANNELS;
        let dst = image.pixels_mut();
        for y in rect.rows() {
            for x in rect.cols() {
                let i = y as usize * stride + x as usize * CHANNELS;
                dst[i..i + CHANNELS].copy_from_slice(&colour);
            }
        }
        let after = Edit::extract(image, rect);
        Edit::Region {
            rect,
            before,
            after,
        }
    }

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    #[test]
    fn undo_restores_the_original_bytes_exactly() {
        let mut img = image(WHITE);
        let original = img.clone();
        let mut h = History::default();

        let edit = paint(&mut img, Rect::new(2, 2, 5, 5), RED);
        h.push("Marker", edit);
        assert_ne!(img.as_bytes(), original.as_bytes());

        h.undo(&mut img, &mut false).unwrap();
        assert_eq!(img.as_bytes(), original.as_bytes());
    }

    #[test]
    fn redo_puts_it_back() {
        let mut img = image(WHITE);
        let mut h = History::default();
        let edit = paint(&mut img, Rect::new(1, 1, 4, 4), RED);
        h.push("Marker", edit);
        let painted = img.clone();

        h.undo(&mut img, &mut false).unwrap();
        h.redo(&mut img, &mut false).unwrap();
        assert_eq!(img.as_bytes(), painted.as_bytes());
    }

    #[test]
    fn a_stack_of_edits_unwinds_in_order() {
        let mut img = image(WHITE);
        let states = {
            let mut v = vec![img.clone()];
            let mut h = History::default();
            for colour in [RED, BLUE, [0, 255, 0, 255]] {
                let edit = paint(&mut img, Rect::new(0, 0, 8, 8), colour);
                h.push("Marker", edit);
                v.push(img.clone());
            }
            for expected in v.iter().rev().skip(1) {
                h.undo(&mut img, &mut false).unwrap();
                assert_eq!(img.as_bytes(), expected.as_bytes());
            }
            v
        };
        assert_eq!(states.len(), 4);
    }

    #[test]
    fn editing_after_undo_drops_the_redo_branch() {
        let mut img = image(WHITE);
        let mut h = History::default();
        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), RED));
        h.undo(&mut img, &mut false).unwrap();
        assert!(h.can_redo());

        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), BLUE));
        assert!(!h.can_redo(), "the undone edit should be gone");
        assert!(h.can_undo());
    }

    #[test]
    fn undo_at_the_bottom_does_nothing() {
        let mut img = image(WHITE);
        let mut h = History::default();
        assert!(!h.can_undo());
        assert!(h.undo(&mut img, &mut false).is_none());
    }

    #[test]
    fn a_whole_canvas_edit_reports_no_region() {
        let mut img = image(WHITE);
        let before = img.clone();
        let after = Rgba8::new(4, 4, RED);
        let mut h = History::default();
        h.push(
            "Resize canvas",
            Edit::Whole {
                before: Snapshot {
                    pixels: before,
                    transparent: false,
                },
                after: Snapshot {
                    pixels: after.clone(),
                    transparent: false,
                },
            },
        );

        img = after.clone();
        assert_eq!(h.undo(&mut img, &mut false).unwrap(), None);
        assert_eq!(img.size(), (8, 8));
    }
}
