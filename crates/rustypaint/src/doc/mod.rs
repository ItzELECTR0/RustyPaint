pub mod clipboard;
pub mod history;
pub mod image;
pub mod io;
pub mod layers;
pub mod ora;
pub mod recovery;
pub mod rect;
pub mod transform;

pub use image::Rgba8;
pub use layers::{Layer, LayerId};
pub use rect::Rect;

use history::{Changed, Edit, History, Part};
use layers::{Arrangement, OPAQUE, Stack};
use std::path::PathBuf;

pub type Version = u64;

// Unique across documents, so a texture can never take one tab for another.
fn next_version() -> Version {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// Layers stack bottom first. The active layer is the one every tool works on, so `pixels` and
// `edit` keep meaning what they meant when there was only ever one. See `.agents/layers.md`.
pub struct Document {
    stack: Stack,
    below: Option<Rgba8>,
    above: Option<Rgba8>,
    version: Version,
    composites: Version,
    revision: Version,
    pub path: Option<PathBuf>,
    // The file at `path` holds more than could be opened, so saving must not write over it.
    pub merged_only: bool,
    touched: bool,
    saved: u64,
    history: History,
    next_id: LayerId,
    // How the stack stood before a slider started dragging, so the drag files one step.
    arranging: Option<Arrangement>,
}

impl Document {
    pub const DEFAULT_SIZE: (u32, u32) = (1152, 648);

    pub fn blank_sized(width: u32, height: u32, transparent: bool) -> Self {
        Self::single(Rgba8::transparent(width, height), None, transparent)
    }

    pub fn from_image(pixels: Rgba8, path: Option<PathBuf>) -> Self {
        let transparent = has_transparency(&pixels);
        Self::single(pixels, path, transparent)
    }

    fn single(pixels: Rgba8, path: Option<PathBuf>, transparent: bool) -> Self {
        let background = Layer {
            id: 1,
            name: crate::i18n::layer_background().to_owned(),
            visible: true,
            opacity: OPAQUE,
            pixels,
        };
        Self::from_stack(
            Stack {
                layers: vec![background],
                active: 0,
                transparent,
            },
            path,
        )
    }

    // Every layer has to be the canvas's size; the readers crop and pad them to it first.
    pub fn from_stack(stack: Stack, path: Option<PathBuf>) -> Self {
        let mut doc = Self::unshown(stack, path);
        doc.restack();
        doc
    }

    // Without the viewport's composites, for a document nothing will draw.
    fn unshown(stack: Stack, path: Option<PathBuf>) -> Self {
        assert!(!stack.layers.is_empty(), "a document always has a layer");
        let next_id = stack.layers.iter().map(|layer| layer.id).max().unwrap_or(0) + 1;
        let version = next_version();
        Self {
            stack,
            below: None,
            above: None,
            version,
            composites: version,
            revision: version,
            path,
            merged_only: false,
            touched: false,
            saved: 0,
            history: History::default(),
            next_id,
            arranging: None,
        }
    }

    // Restored work is unsaved by definition, whatever the file it came from says.
    pub fn recovered(stack: Stack, path: Option<PathBuf>) -> Self {
        let mut doc = Self::from_stack(stack, path);
        doc.touched = true;
        doc
    }

    // A copy sharing every pixel, for committing a live object into without touching this one.
    pub fn detached(&self) -> Self {
        Self::unshown(self.stack.clone(), None)
    }

    pub fn pixels(&self) -> &Rgba8 {
        &self.stack.layers[self.stack.active].pixels
    }

    pub fn layers(&self) -> &[Layer] {
        &self.stack.layers
    }

    pub fn stack(&self) -> &Stack {
        &self.stack
    }

    pub fn active(&self) -> usize {
        self.stack.active
    }

    pub fn active_layer(&self) -> &Layer {
        &self.stack.layers[self.stack.active]
    }

    // What the visible layers under and over the active one come to, or None where there are none.
    pub fn below(&self) -> Option<&Rgba8> {
        self.below.as_ref()
    }

    pub fn above(&self) -> Option<&Rgba8> {
        self.above.as_ref()
    }

    // Changes whenever `below` or `above` do, and only then.
    pub fn composites(&self) -> Version {
        self.composites
    }

    // The active layer's pixels, which is what the canvas texture holds.
    pub fn version(&self) -> Version {
        self.version
    }

    // Changes with anything at all, pixels or layers.
    pub fn revision(&self) -> Version {
        self.revision.max(self.version)
    }

    pub fn size(&self) -> (u32, u32) {
        self.stack.size()
    }

    pub fn transparent(&self) -> bool {
        self.stack.transparent
    }

    pub fn backdrop(&self) -> [u8; 4] {
        [0, 0, 0, 0]
    }

    pub fn has_backing(&self) -> bool {
        !self.stack.transparent
    }

    pub fn layered(&self) -> bool {
        self.stack.layered()
    }

    pub fn composite(&self) -> Rgba8 {
        self.stack.composite()
    }

    pub fn flattened(&self) -> Rgba8 {
        self.stack.flattened()
    }

    // The colour the layers show at one pixel, without the backing.
    pub fn sample(&self, x: i64, y: i64) -> Option<[u8; 4]> {
        let (width, height) = self.size();
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            return None;
        }
        let at = (y as usize * width as usize + x as usize) * image::CHANNELS;
        let mut out = [0u8; 4];
        for layer in &self.stack.layers {
            let pixel = &layer.pixels.as_bytes()[at..at + image::CHANNELS];
            layers::blend(&mut out, pixel, layer.strength());
        }
        Some(out)
    }

    pub fn edit(&mut self) -> &mut Rgba8 {
        self.version = next_version();
        self.touched = true;
        &mut self.stack.layers[self.stack.active].pixels
    }

    // True between an edit and the commit that files it, when the canvas is ahead of its history.
    pub fn touched(&self) -> bool {
        self.touched
    }

    pub fn modified(&self) -> bool {
        self.touched || self.history.mark() != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.settle();
        self.touched = false;
        self.saved = self.history.mark();
    }

    pub(crate) fn restore_live(&mut self, pixels: Rgba8, touched: bool) {
        self.stack.layers[self.stack.active].pixels = pixels;
        self.version = next_version();
        self.touched = touched;
    }

    pub fn commit(&mut self, label: &'static str, rect: Rect, before: &Rgba8) {
        let (width, height) = self.size();
        let rect = rect.clamped(width, height);
        self.commit_parts(label, vec![(rect, Edit::extract(before, rect))]);
    }

    // Each region comes with its old rows, top to bottom.
    pub fn commit_parts(&mut self, label: &'static str, regions: Vec<(Rect, Vec<u8>)>) {
        self.touched = false;
        let (width, height) = self.size();
        let layer = self.stack.active_id();
        let pixels = self.pixels();
        let mut parts: Vec<Part> = regions
            .into_iter()
            .filter(|(rect, before)| {
                let fits = rect.clamped(width, height) == *rect
                    && before.len() == rect.area() * image::CHANNELS;
                debug_assert!(fits, "{rect:?} does not match what was kept from under it");
                fits && !rect.is_empty()
            })
            .map(|(rect, before)| Part {
                rect,
                before,
                after: Edit::extract(pixels, rect),
            })
            .filter(|part| part.before != part.after)
            .collect();
        let edit = match parts.len() {
            0 => return,
            1 => {
                let Part {
                    rect,
                    before,
                    after,
                } = parts.remove(0);
                Edit::Region {
                    layer,
                    rect,
                    before,
                    after,
                }
            }
            _ => Edit::Regions { layer, parts },
        };
        self.history.push(label, edit);
    }

    // Recomputes what the viewport shows around the active layer.
    fn restack(&mut self) {
        let size = self.size();
        let active = self.stack.active;
        if self.stack.layers.len() == 1 {
            self.below = None;
            self.above = None;
        } else {
            self.below = layers::composite(&self.stack.layers[..active], size);
            self.above = layers::composite(&self.stack.layers[active + 1..], size);
        }
        self.composites = next_version();
        self.revision = self.composites;
    }

    fn reshape(
        &mut self,
        label: &'static str,
        change: impl Fn(&Rgba8) -> Rgba8,
        transparent: bool,
    ) {
        self.settle();
        let before = self.stack.clone();
        let mut after = before.clone();
        for layer in &mut after.layers {
            layer.pixels = change(&layer.pixels);
        }
        after.transparent = transparent;
        let same = before.transparent == after.transparent
            && before
                .layers
                .iter()
                .zip(&after.layers)
                .all(|(a, b)| a.pixels == b.pixels);
        if same {
            return;
        }
        self.stack = after.clone();
        self.history.push(label, Edit::Whole { before, after });
        self.version = next_version();
        self.touched = false;
        self.restack();
    }

    pub fn resize_canvas(&mut self, width: u32, height: u32, anchor: transform::Anchor) {
        if width == 0 || height == 0 {
            return;
        }
        let fill = self.backdrop();
        let transparent = self.transparent();
        self.reshape(
            "Resize canvas",
            |pixels| transform::resize_canvas(pixels, width, height, anchor, fill),
            transparent,
        );
    }

    pub fn resize_image(&mut self, width: u32, height: u32, resampling: transform::Resampling) {
        if width == 0 || height == 0 {
            return;
        }
        let transparent = self.transparent();
        self.reshape(
            "Resize image",
            |pixels| transform::scale(pixels, width, height, resampling),
            transparent,
        );
    }

    #[allow(dead_code, reason = "the crop tool that drives this is Phase 5")]
    pub fn crop(&mut self, rect: Rect) {
        let (width, height) = self.size();
        let rect = rect.clamped(width, height);
        if rect.is_empty() {
            return;
        }
        let transparent = self.transparent();
        self.reshape("Crop", |pixels| transform::crop(pixels, rect), transparent);
    }

    pub fn rotate(&mut self, clockwise: bool) {
        let label = if clockwise {
            "Rotate right"
        } else {
            "Rotate left"
        };
        let transparent = self.transparent();
        self.reshape(
            label,
            |pixels| transform::rotate_90(pixels, clockwise),
            transparent,
        );
    }

    pub fn flip(&mut self, horizontal: bool) {
        let label = if horizontal {
            "Flip horizontal"
        } else {
            "Flip vertical"
        };
        let transparent = self.transparent();
        self.reshape(
            label,
            |pixels| {
                if horizontal {
                    transform::flip_horizontal(pixels)
                } else {
                    transform::flip_vertical(pixels)
                }
            },
            transparent,
        );
    }

    pub fn set_transparent(&mut self, transparent: bool) {
        if transparent == self.transparent() {
            return;
        }
        let label = if transparent {
            "Transparent canvas"
        } else {
            "Opaque canvas"
        };
        self.reshape(label, Rgba8::clone, transparent);
    }

    // Files a change to the layers themselves. `kept` holds the pixels of any layer that the
    // change removed or created from something other than a blank canvas.
    fn arrange(&mut self, label: &'static str, before: Arrangement, kept: Vec<(LayerId, Rgba8)>) {
        let after = self.stack.arrangement();
        if before == after && kept.is_empty() {
            return;
        }
        // The canvas texture holds the active layer, so it is sent again only when that changes.
        if before.active != after.active {
            self.version = next_version();
        }
        self.history.push(
            label,
            Edit::Layers {
                before,
                after,
                kept,
            },
        );
        self.touched = false;
        self.restack();
    }

    // A slider drag in progress is one step, filed the moment anything else happens.
    pub fn settle(&mut self) {
        let Some(before) = self.arranging.take() else {
            return;
        };
        let after = self.stack.arrangement();
        self.touched = false;
        if before != after {
            self.history.push(
                "Layer opacity",
                Edit::Layers {
                    before,
                    after,
                    kept: Vec::new(),
                },
            );
        }
    }

    fn take_id(&mut self) -> LayerId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn select_layer(&mut self, index: usize) -> bool {
        self.settle();
        if index >= self.stack.layers.len() || index == self.stack.active {
            return false;
        }
        self.stack.active = index;
        self.version = next_version();
        self.restack();
        true
    }

    // A new layer goes straight over the one being worked on and takes over from it.
    pub fn add_layer(&mut self) {
        self.settle();
        let before = self.stack.arrangement();
        let id = self.take_id();
        let (width, height) = self.size();
        let at = self.stack.active + 1;
        self.stack.layers.insert(
            at,
            Layer {
                id,
                name: crate::i18n::layer_numbered(id),
                visible: true,
                opacity: OPAQUE,
                pixels: Rgba8::transparent(width, height),
            },
        );
        self.stack.active = at;
        self.arrange("New layer", before, Vec::new());
    }

    pub fn duplicate_layer(&mut self) {
        self.settle();
        let before = self.stack.arrangement();
        let id = self.take_id();
        let source = self.active_layer().clone();
        let at = self.stack.active + 1;
        self.stack.layers.insert(
            at,
            Layer {
                id,
                name: crate::i18n::layer_copy(&source.name),
                ..source.clone()
            },
        );
        self.stack.active = at;
        self.arrange("Duplicate layer", before, vec![(id, source.pixels)]);
    }

    // The layer under the deleted one takes over, as in Photoshop; deleting the bottom layer
    // hands over to the one that was above it.
    pub fn delete_layer(&mut self) -> bool {
        self.settle();
        if self.stack.layers.len() < 2 {
            return false;
        }
        let before = self.stack.arrangement();
        let gone = self.stack.layers.remove(self.stack.active);
        self.stack.active = self.stack.active.saturating_sub(1);
        self.arrange("Delete layer", before, vec![(gone.id, gone.pixels)]);
        true
    }

    pub fn move_layer(&mut self, from: usize, to: usize) -> bool {
        self.settle();
        let count = self.stack.layers.len();
        if from >= count || to >= count || from == to {
            return false;
        }
        let before = self.stack.arrangement();
        let active = self.stack.active_id();
        let layer = self.stack.layers.remove(from);
        self.stack.layers.insert(to, layer);
        self.stack.active = self.stack.index_of(active).unwrap_or(0);
        self.arrange("Move layer", before, Vec::new());
        true
    }

    // Both layers have to show for the merge to keep the picture as it looks.
    pub fn can_merge_down(&self) -> bool {
        let active = self.stack.active;
        active > 0 && self.stack.layers[active].visible && self.stack.layers[active - 1].visible
    }

    pub fn merge_down(&mut self) -> bool {
        self.settle();
        if !self.can_merge_down() {
            return false;
        }
        let upper = self.stack.active;
        let lower = upper - 1;
        let before = self.stack.arrangement();
        let size = self.size();
        let pair = &self.stack.layers[lower..=upper];
        let merged =
            layers::composite(pair, size).unwrap_or_else(|| Rgba8::transparent(size.0, size.1));
        let whole = Rect::new(0, 0, size.0, size.1);
        let lower_id = self.stack.layers[lower].id;
        let region = Edit::Region {
            layer: lower_id,
            rect: whole,
            before: Edit::extract(&self.stack.layers[lower].pixels, whole),
            after: Edit::extract(&merged, whole),
        };
        let gone = self.stack.layers.remove(upper);
        let target = &mut self.stack.layers[lower];
        target.pixels = merged;
        target.opacity = OPAQUE;
        self.stack.active = lower;
        let arranged = Edit::Layers {
            before,
            after: self.stack.arrangement(),
            kept: vec![(gone.id, gone.pixels)],
        };
        // Undone last to first: the lower layer's pixels return, then the upper layer does.
        self.history
            .push("Merge down", Edit::Many(vec![arranged, region]));
        self.touched = false;
        self.version = next_version();
        self.restack();
        true
    }

    // Hidden layers are dropped, the way Photoshop's flatten drops them; undo brings them back.
    pub fn flatten(&mut self) -> bool {
        self.settle();
        if self.stack.layers.len() < 2 && !self.layered() {
            return false;
        }
        let before = self.stack.arrangement();
        let composite = self.composite();
        let id = self.take_id();
        let kept: Vec<(LayerId, Rgba8)> = self
            .stack
            .layers
            .drain(..)
            .map(|layer| (layer.id, layer.pixels))
            .chain([(id, composite.clone())])
            .collect();
        self.stack.layers.push(Layer {
            id,
            name: crate::i18n::layer_background().to_owned(),
            visible: true,
            opacity: OPAQUE,
            pixels: composite,
        });
        self.stack.active = 0;
        self.arrange("Flatten", before, kept);
        true
    }

    pub fn set_visible(&mut self, index: usize, visible: bool) {
        self.settle();
        let Some(layer) = self.stack.layers.get_mut(index) else {
            return;
        };
        if layer.visible == visible {
            return;
        }
        let before = self.stack.arrangement();
        self.stack.layers[index].visible = visible;
        let label = if visible { "Show layer" } else { "Hide layer" };
        self.arrange(label, before, Vec::new());
    }

    pub fn rename(&mut self, index: usize, name: &str) {
        self.settle();
        let name = name.trim();
        let Some(layer) = self.stack.layers.get(index) else {
            return;
        };
        if name.is_empty() || layer.name == name {
            return;
        }
        let before = self.stack.arrangement();
        self.stack.layers[index].name = name.to_owned();
        self.arrange("Rename layer", before, Vec::new());
    }

    // Live, for a slider: the step is filed by `settle` once the drag lets go.
    pub fn set_opacity(&mut self, index: usize, opacity: u8) {
        let Some(layer) = self.stack.layers.get(index) else {
            return;
        };
        if layer.opacity == opacity {
            return;
        }
        if self.arranging.is_none() {
            self.arranging = Some(self.stack.arrangement());
        }
        self.stack.layers[index].opacity = opacity;
        self.touched = true;
        if index == self.stack.active {
            self.revision = next_version();
        } else {
            self.restack();
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo() || self.arranging.is_some()
    }

    #[cfg(test)]
    pub fn history_bytes(&self) -> usize {
        self.history.bytes()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> Option<Option<Rect>> {
        self.settle();
        let changed = self.history.undo(&mut self.stack)?;
        Some(self.stepped(changed))
    }

    pub fn redo(&mut self) -> Option<Option<Rect>> {
        self.settle();
        let changed = self.history.redo(&mut self.stack)?;
        Some(self.stepped(changed))
    }

    fn stepped(&mut self, changed: Changed) -> Option<Rect> {
        self.version = next_version();
        self.touched = false;
        match changed {
            Changed::Region(rect) => Some(rect),
            Changed::Everything => {
                self.restack();
                None
            }
        }
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| crate::i18n::untitled().to_owned());
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
        assert!(Document::from_image(px, None).transparent());
        assert!(!Document::from_image(Rgba8::white(4, 4), None).transparent());
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

    fn at(d: &Document, layer: usize, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * d.size().0 as usize + x as usize) * image::CHANNELS;
        d.layers()[layer].pixels.as_bytes()[i..i + 4]
            .try_into()
            .unwrap()
    }

    fn ids(d: &Document) -> Vec<LayerId> {
        d.layers().iter().map(|layer| layer.id).collect()
    }

    #[test]
    fn a_new_layer_goes_over_the_active_one_and_takes_the_brush() {
        let mut d = Document::blank_sized(8, 8, false);
        paint(&mut d, [255, 0, 0, 255]);
        d.add_layer();
        assert_eq!(d.layers().len(), 2);
        assert_eq!(d.active(), 1);
        assert_eq!(at(&d, 1, 0, 0), [0, 0, 0, 0], "it starts empty");
        assert!(d.layered());

        paint(&mut d, [0, 0, 255, 255]);
        assert_eq!(
            at(&d, 0, 0, 0),
            [255, 0, 0, 255],
            "the bottom layer is untouched"
        );
        assert_eq!(d.composite().as_bytes()[0..4], [0, 0, 255, 255]);
    }

    #[test]
    fn the_viewport_composites_follow_the_active_layer() {
        let mut d = Document::blank_sized(4, 4, true);
        assert!(
            d.below().is_none() && d.above().is_none(),
            "one layer needs none"
        );
        paint(&mut d, [255, 0, 0, 255]);
        d.add_layer();
        d.add_layer();
        let below = d.composites();
        assert!(d.below().is_some());
        assert!(d.above().is_none(), "the new layer is on top");

        d.select_layer(1);
        assert_ne!(d.composites(), below);
        assert!(d.above().is_some() && d.below().is_some());
    }

    #[test]
    fn undo_reaches_back_through_layer_changes() {
        let mut d = Document::blank_sized(8, 8, false);
        d.add_layer();
        paint(&mut d, [0, 255, 0, 255]);
        d.set_visible(1, false);
        d.rename(1, "Ink");
        assert_eq!(d.layers()[1].name, "Ink");

        d.undo().unwrap();
        assert_eq!(d.layers()[1].name, "Layer 2");
        d.undo().unwrap();
        assert!(d.layers()[1].visible);
        d.undo().unwrap();
        assert_eq!(at(&d, 1, 0, 0), [0, 0, 0, 0]);
        d.undo().unwrap();
        assert_eq!(d.layers().len(), 1);
        assert!(!d.modified(), "every step undone is no change at all");
    }

    #[test]
    fn deleting_a_layer_hands_over_to_the_one_below() {
        let mut d = Document::blank_sized(8, 8, false);
        d.add_layer();
        d.add_layer();
        d.select_layer(1);
        assert!(d.delete_layer());
        assert_eq!(ids(&d), [1, 3]);
        assert_eq!(d.active(), 0);

        d.undo().unwrap();
        assert_eq!(ids(&d), [1, 2, 3]);
        assert_eq!(d.active(), 1, "undo puts the deleted layer back in charge");

        d.select_layer(0);
        d.delete_layer();
        d.delete_layer();
        assert_eq!(d.layers().len(), 1);
        assert!(!d.delete_layer(), "the last layer stays");
    }

    #[test]
    fn merging_down_keeps_the_picture_as_it_looked() {
        let mut d = Document::blank_sized(4, 4, true);
        paint(&mut d, [255, 0, 0, 255]);
        d.add_layer();
        paint(&mut d, [0, 0, 255, 255]);
        d.set_opacity(1, 128);
        d.settle();
        let looked = d.composite();

        assert!(d.merge_down());
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.layers()[0].opacity, OPAQUE);
        assert_eq!(d.composite(), looked);

        d.undo().unwrap();
        assert_eq!(d.layers().len(), 2);
        assert_eq!(d.active(), 1, "back on the layer that was merged");
        assert_eq!(at(&d, 0, 0, 0), [255, 0, 0, 255]);
        assert_eq!(d.layers()[1].opacity, 128);
        d.redo().unwrap();
        assert_eq!(d.composite(), looked);
    }

    #[test]
    fn merging_into_a_hidden_layer_is_refused() {
        let mut d = Document::blank_sized(4, 4, true);
        d.add_layer();
        d.set_visible(0, false);
        assert!(!d.can_merge_down());
        assert!(!d.merge_down());
        assert_eq!(d.layers().len(), 2);
    }

    #[test]
    fn flattening_is_one_step_that_undo_takes_back() {
        let mut d = Document::blank_sized(4, 4, true);
        paint(&mut d, [255, 0, 0, 255]);
        d.add_layer();
        d.add_layer();
        paint(&mut d, [0, 255, 0, 128]);
        let looked = d.composite();

        assert!(d.flatten());
        assert_eq!(d.layers().len(), 1);
        assert!(!d.layered());
        assert_eq!(d.composite(), looked);
        d.undo().unwrap();
        assert_eq!(d.layers().len(), 3);
    }

    #[test]
    fn an_opacity_drag_is_one_step() {
        let mut d = Document::blank_sized(4, 4, true);
        d.add_layer();
        for opacity in [250, 200, 120, 64] {
            d.set_opacity(1, opacity);
        }
        assert!(d.modified());
        d.undo().unwrap();
        assert_eq!(d.layers()[1].opacity, OPAQUE, "the whole drag goes in one");
        d.undo().unwrap();
        assert_eq!(d.layers().len(), 1);
    }

    #[test]
    fn whole_canvas_changes_reach_every_layer() {
        let mut d = Document::blank_sized(8, 4, true);
        d.add_layer();
        d.rotate(true);
        assert!(d.layers().iter().all(|layer| layer.pixels.size() == (4, 8)));
        d.undo().unwrap();
        assert!(d.layers().iter().all(|layer| layer.pixels.size() == (8, 4)));
        assert_eq!(d.layers().len(), 2);
    }

    #[test]
    fn undoing_on_another_layer_selects_it() {
        let mut d = Document::blank_sized(4, 4, true);
        paint(&mut d, [255, 0, 0, 255]);
        d.add_layer();
        paint(&mut d, [0, 0, 255, 255]);
        d.select_layer(0);
        assert_eq!(d.undo().unwrap(), None, "the canvas texture changes layer");
        assert_eq!(d.active(), 1, "the stroke was on the top layer");
        assert_eq!(at(&d, 1, 0, 0), [0, 0, 0, 0]);
        assert_eq!(at(&d, 0, 0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn the_picked_colour_is_what_the_layers_show() {
        let mut d = Document::blank_sized(4, 4, true);
        paint(&mut d, [255, 0, 0, 255]);
        d.add_layer();
        assert_eq!(d.sample(0, 0), Some([255, 0, 0, 255]));
        paint(&mut d, [0, 0, 255, 255]);
        d.set_visible(1, false);
        assert_eq!(
            d.sample(0, 0),
            Some([255, 0, 0, 255]),
            "a hidden layer shows nothing"
        );
        assert_eq!(d.sample(3, 3), Some([0, 0, 0, 0]));
        assert_eq!(d.sample(4, 0), None);
    }

    #[test]
    fn a_flat_picture_is_not_layered() {
        let mut d = Document::from_image(Rgba8::white(4, 4), None);
        assert!(!d.layered());
        d.set_opacity(0, 100);
        assert!(d.layered(), "saving it flat would bake the opacity in");
    }

    // `cargo test --release -p rustypaint layer_timings -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement, not a check"]
    fn layer_timings() {
        for (size, count) in [((1920, 1080), 5), ((4000, 3000), 8), ((6000, 4000), 8)] {
            let mut d = Document::from_image(Rgba8::new(size.0, size.1, [90, 120, 200, 255]), None);
            for _ in 1..count {
                d.add_layer();
                let at = d.size().0 as usize * 4 * (size.1 as usize / 2);
                d.edit().pixels_mut()[at..at + 4 * 400].fill(200);
                d.commit_parts("Marker", Vec::new());
            }
            let time = |d: &mut Document, change: &dyn Fn(&mut Document)| {
                let start = std::time::Instant::now();
                change(d);
                start.elapsed().as_secs_f64() * 1e3
            };
            let select = time(&mut d, &|d| {
                d.select_layer(count / 2);
            });
            let hide = time(&mut d, &|d| d.set_visible(count - 1, false));
            let add = time(&mut d, &|d| d.add_layer());
            let merge = time(&mut d, &|d| {
                d.merge_down();
            });
            println!(
                "{}x{} {count} layers: select {select:.0} ms, hide another {hide:.0} ms, \
                 add {add:.0} ms, merge down {merge:.0} ms",
                size.0, size.1
            );
        }
    }
}
