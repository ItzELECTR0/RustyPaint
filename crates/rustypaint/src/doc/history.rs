use super::layers::{Arrangement, LayerId, Stack};
use super::rect::Rect;
use super::{Rgba8, image::CHANNELS};

const BUDGET_BYTES: usize = 256 << 20;

pub enum Edit {
    Region {
        layer: LayerId,
        rect: Rect,
        before: Vec<u8>,
        after: Vec<u8>,
    },
    // Kept apart so a long stroke costs what it touched, not the box around it.
    Regions {
        layer: LayerId,
        parts: Vec<Part>,
    },
    Whole {
        before: Stack,
        after: Stack,
    },
    // Pixels are kept only for layers missing from one side, so hiding or renaming a layer holds
    // no copy of anything the canvas still owns. See `.agents/layers.md`.
    Layers {
        before: Arrangement,
        after: Arrangement,
        kept: Vec<(LayerId, Rgba8)>,
    },
    // Undone last to first.
    Many(Vec<Edit>),
}

pub struct Part {
    pub rect: Rect,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

// What a step through history changed, so the canvas texture can catch up with the least work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    Region(Rect),
    Everything,
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
        let stack = |stack: &Stack| -> usize {
            stack
                .layers
                .iter()
                .map(|layer| layer.pixels.as_bytes().len())
                .sum()
        };
        match self {
            Edit::Region { before, after, .. } => before.len() + after.len(),
            Edit::Regions { parts, .. } => {
                parts.iter().map(|p| p.before.len() + p.after.len()).sum()
            }
            Edit::Whole { before, after } => stack(before) + stack(after),
            Edit::Layers { kept, .. } => kept.iter().map(|(_, p)| p.as_bytes().len()).sum(),
            Edit::Many(edits) => edits.iter().map(Edit::bytes).sum(),
        }
    }
}

struct Entry {
    #[allow(dead_code, reason = "surfaced by the history flyout, which is Phase 7")]
    label: &'static str,
    edit: Edit,
    serial: u64,
}

#[derive(Default)]
pub struct History {
    entries: Vec<Entry>,
    depth: usize,
    bytes: usize,
    serials: u64,
    base: u64,
}

impl History {
    pub fn push(&mut self, label: &'static str, edit: Edit) {
        for dropped in self.entries.drain(self.depth..) {
            self.bytes -= dropped.edit.bytes();
        }
        self.bytes += edit.bytes();
        self.serials += 1;
        self.entries.push(Entry {
            label,
            edit,
            serial: self.serials,
        });
        self.depth = self.entries.len();
        self.trim();
    }

    // Names the state the canvas is in, so a save can be recognised again after undo and redo.
    pub fn mark(&self) -> u64 {
        self.depth
            .checked_sub(1)
            .map_or(self.base, |i| self.entries[i].serial)
    }

    fn trim(&mut self) {
        while self.bytes > BUDGET_BYTES && self.entries.len() > 1 {
            let dropped = self.entries.remove(0);
            self.bytes -= dropped.edit.bytes();
            self.base = dropped.serial;
            self.depth -= 1;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.depth > 0
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
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

    pub fn undo(&mut self, stack: &mut Stack) -> Option<Changed> {
        let entry = self.entries.get(self.depth.checked_sub(1)?)?;
        self.depth -= 1;
        Some(restore(&entry.edit, stack, false))
    }

    pub fn redo(&mut self, stack: &mut Stack) -> Option<Changed> {
        let entry = self.entries.get(self.depth)?;
        self.depth += 1;
        Some(restore(&entry.edit, stack, true))
    }
}

// A step on another layer brings that layer forward, so the change happens where it can be seen
// and the next stroke lands on the layer that was just put back.
fn reach(stack: &mut Stack, layer: LayerId) -> Option<(usize, bool)> {
    let index = stack.index_of(layer)?;
    let moved = index != stack.active;
    stack.active = index;
    Some((index, moved))
}

fn restore(edit: &Edit, stack: &mut Stack, forwards: bool) -> Changed {
    match edit {
        Edit::Region {
            layer,
            rect,
            before,
            after,
        } => {
            let Some((index, moved)) = reach(stack, *layer) else {
                return Changed::Everything;
            };
            let image = &mut stack.layers[index].pixels;
            Edit::apply(image, *rect, if forwards { after } else { before });
            if moved {
                Changed::Everything
            } else {
                Changed::Region(*rect)
            }
        }
        Edit::Regions { layer, parts } => {
            let Some((index, moved)) = reach(stack, *layer) else {
                return Changed::Everything;
            };
            let image = &mut stack.layers[index].pixels;
            let mut changed = Rect::new(0, 0, 0, 0);
            for part in parts {
                let pixels = if forwards { &part.after } else { &part.before };
                Edit::apply(image, part.rect, pixels);
                changed = changed.union(part.rect);
            }
            if moved {
                Changed::Everything
            } else {
                Changed::Region(changed)
            }
        }
        Edit::Whole { before, after } => {
            *stack = if forwards { after } else { before }.clone();
            Changed::Everything
        }
        Edit::Layers {
            before,
            after,
            kept,
        } => {
            stack.rearrange(if forwards { after } else { before }, kept);
            Changed::Everything
        }
        Edit::Many(edits) => {
            let mut changed = None;
            let mut step = |edit: &Edit| {
                let now = restore(edit, stack, forwards);
                changed = match (changed, now) {
                    (None, now) => Some(now),
                    (Some(Changed::Region(a)), Changed::Region(b)) => {
                        Some(Changed::Region(a.union(b)))
                    }
                    _ => Some(Changed::Everything),
                };
            };
            if forwards {
                edits.iter().for_each(&mut step);
            } else {
                edits.iter().rev().for_each(&mut step);
            }
            changed.unwrap_or(Changed::Everything)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::layers::{Layer, OPAQUE};
    use super::*;

    fn layer(id: LayerId, fill: [u8; 4]) -> Layer {
        Layer {
            id,
            name: format!("Layer {id}"),
            visible: true,
            opacity: OPAQUE,
            pixels: Rgba8::new(8, 8, fill),
        }
    }

    fn image(fill: [u8; 4]) -> Stack {
        Stack {
            layers: vec![layer(1, fill)],
            active: 0,
            transparent: false,
        }
    }

    fn pixels(stack: &Stack) -> &[u8] {
        stack.layers[stack.active].pixels.as_bytes()
    }

    fn paint(stack: &mut Stack, rect: Rect, colour: [u8; 4]) -> Edit {
        let layer = stack.active_id();
        let image = &mut stack.layers[stack.active].pixels;
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
            layer,
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
        let original = pixels(&img).to_vec();
        let mut h = History::default();

        let edit = paint(&mut img, Rect::new(2, 2, 5, 5), RED);
        h.push("Marker", edit);
        assert_ne!(pixels(&img), original);

        h.undo(&mut img).unwrap();
        assert_eq!(pixels(&img), original);
    }

    #[test]
    fn redo_puts_it_back() {
        let mut img = image(WHITE);
        let mut h = History::default();
        let edit = paint(&mut img, Rect::new(1, 1, 4, 4), RED);
        h.push("Marker", edit);
        let painted = pixels(&img).to_vec();

        h.undo(&mut img).unwrap();
        h.redo(&mut img).unwrap();
        assert_eq!(pixels(&img), painted);
    }

    #[test]
    fn a_stack_of_edits_unwinds_in_order() {
        let mut img = image(WHITE);
        let mut states = vec![pixels(&img).to_vec()];
        let mut h = History::default();
        for colour in [RED, BLUE, [0, 255, 0, 255]] {
            let edit = paint(&mut img, Rect::new(0, 0, 8, 8), colour);
            h.push("Marker", edit);
            states.push(pixels(&img).to_vec());
        }
        for expected in states.iter().rev().skip(1) {
            h.undo(&mut img).unwrap();
            assert_eq!(pixels(&img), expected);
        }
    }

    #[test]
    fn editing_after_undo_drops_the_redo_branch() {
        let mut img = image(WHITE);
        let mut h = History::default();
        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), RED));
        h.undo(&mut img).unwrap();
        assert!(h.can_redo());

        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), BLUE));
        assert!(!h.can_redo(), "the undone edit should be gone");
        assert!(h.can_undo());
    }

    #[test]
    fn the_mark_names_a_state_rather_than_a_step() {
        let mut img = image(WHITE);
        let mut h = History::default();
        let pristine = h.mark();

        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), RED));
        let painted = h.mark();
        assert_ne!(painted, pristine);

        h.undo(&mut img).unwrap();
        assert_eq!(h.mark(), pristine, "undo comes back to where it started");
        h.redo(&mut img).unwrap();
        assert_eq!(h.mark(), painted, "and redo goes back to the edit");
    }

    #[test]
    fn a_mark_on_a_dropped_branch_never_comes_back() {
        let mut img = image(WHITE);
        let mut h = History::default();
        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), RED));
        let dropped = h.mark();

        h.undo(&mut img).unwrap();
        h.push("Marker", paint(&mut img, Rect::new(0, 0, 4, 4), BLUE));
        assert_ne!(h.mark(), dropped);
    }

    #[test]
    fn undo_at_the_bottom_does_nothing() {
        let mut img = image(WHITE);
        let mut h = History::default();
        assert!(!h.can_undo());
        assert!(h.undo(&mut img).is_none());
    }

    #[test]
    fn a_whole_canvas_edit_reports_no_region() {
        let mut img = image(WHITE);
        let before = img.clone();
        let mut after = image(RED);
        after.layers[0].pixels = Rgba8::new(4, 4, RED);
        let mut h = History::default();
        h.push(
            "Resize canvas",
            Edit::Whole {
                before,
                after: after.clone(),
            },
        );

        img = after;
        assert_eq!(h.undo(&mut img).unwrap(), Changed::Everything);
        assert_eq!(img.size(), (8, 8));
    }

    #[test]
    fn undoing_a_stroke_on_another_layer_brings_that_layer_forward() {
        let mut img = image(WHITE);
        img.layers.push(layer(2, [0, 0, 0, 0]));
        let mut h = History::default();
        h.push("Marker", paint(&mut img, Rect::new(0, 0, 2, 2), RED));
        img.active = 1;

        assert_eq!(h.undo(&mut img).unwrap(), Changed::Everything);
        assert_eq!(img.active, 0, "the stroke was on the bottom layer");
        assert_eq!(img.layers[0].pixels, Rgba8::new(8, 8, WHITE));
        assert_eq!(
            h.redo(&mut img).unwrap(),
            Changed::Region(Rect::new(0, 0, 2, 2))
        );
    }

    #[test]
    fn a_deleted_layer_comes_back_with_its_pixels_and_place() {
        let mut img = image(WHITE);
        img.layers.push(layer(2, BLUE));
        img.layers.push(layer(3, RED));
        img.active = 1;
        let before = img.arrangement();
        let gone = img.layers.remove(1);
        img.active = 1;
        let after = img.arrangement();
        let mut h = History::default();
        h.push(
            "Delete layer",
            Edit::Layers {
                before,
                after,
                kept: vec![(gone.id, gone.pixels)],
            },
        );

        h.undo(&mut img).unwrap();
        let ids: Vec<_> = img.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(img.active_id(), 2);
        assert_eq!(img.layers[1].pixels, Rgba8::new(8, 8, BLUE));

        h.redo(&mut img).unwrap();
        let ids: Vec<_> = img.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn hiding_a_layer_holds_no_pixels() {
        let mut img = image(WHITE);
        let before = img.arrangement();
        img.layers[0].visible = false;
        let mut h = History::default();
        h.push(
            "Hide layer",
            Edit::Layers {
                before,
                after: img.arrangement(),
                kept: Vec::new(),
            },
        );
        assert_eq!(h.bytes(), 0);
        h.undo(&mut img).unwrap();
        assert!(img.layers[0].visible);
        assert_eq!(img.layers[0].pixels, Rgba8::new(8, 8, WHITE));
    }

    #[test]
    fn a_layer_that_was_only_added_comes_back_blank() {
        let mut img = image(WHITE);
        let before = img.arrangement();
        img.layers.push(layer(2, [0, 0, 0, 0]));
        img.active = 1;
        let mut h = History::default();
        h.push(
            "New layer",
            Edit::Layers {
                before,
                after: img.arrangement(),
                kept: Vec::new(),
            },
        );
        h.undo(&mut img).unwrap();
        assert_eq!(img.layers.len(), 1);
        h.redo(&mut img).unwrap();
        assert_eq!(img.layers.len(), 2);
        assert_eq!(img.active_id(), 2);
        assert_eq!(img.layers[1].pixels, Rgba8::transparent(8, 8));
    }
}
