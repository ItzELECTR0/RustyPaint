use crate::doc::{Document, Rgba8, image::CHANNELS};
use crate::i18n;
use crate::ui::sidebar;

use iced::Task;

use super::*;

const THUMBNAIL: (u32, u32) = (44, 32);
const CHECKER: u32 = 4;

impl App {
    // Anything that changes which layer the tools work on lands the live object first, in the
    // layer it was made on, the same way picking another tool does.
    pub(super) fn change_layers(&mut self, change: impl FnOnce(&mut Document)) {
        if self.cropping.is_some() || self.cutting_out.is_some() {
            return;
        }
        self.commit_floating();
        self.renaming = None;
        change(&mut self.doc);
        self.damage.clear();
        self.panel.sync(self.doc.size());
    }

    pub(super) fn layer_action(&mut self, action: LayerAction) -> Task<Message> {
        let held = self.cropping.is_some() || self.cutting_out.is_some();
        match action {
            LayerAction::Select(index) => self.change_layers(|doc| {
                doc.select_layer(index);
            }),
            LayerAction::Add => self.change_layers(Document::add_layer),
            LayerAction::Duplicate => self.change_layers(Document::duplicate_layer),
            LayerAction::Delete => self.change_layers(|doc| {
                doc.delete_layer();
            }),
            LayerAction::Move(up) => self.change_layers(|doc| {
                let from = doc.active();
                let to = if up { from + 1 } else { from.wrapping_sub(1) };
                doc.move_layer(from, to);
            }),
            LayerAction::Merge => self.change_layers(|doc| {
                doc.merge_down();
            }),
            LayerAction::Flatten => self.change_layers(|doc| {
                doc.flatten();
            }),
            LayerAction::ToggleVisible(index) if !held => {
                let shown = self.doc.layers().get(index).is_some_and(|l| l.visible);
                self.doc.set_visible(index, !shown);
            }
            LayerAction::Opacity(opacity) if !held => {
                let level = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                self.doc.set_opacity(self.doc.active(), level);
            }
            LayerAction::ToggleVisible(_) | LayerAction::Opacity(_) => {}
            LayerAction::OpacitySettled => self.doc.settle(),
            LayerAction::RenameStarted(index) => {
                if held {
                    return Task::none();
                }
                self.change_layers(|doc| {
                    doc.select_layer(index);
                });
                self.renaming = Some(self.doc.active_layer().name.clone());
                return Task::batch([
                    iced::widget::operation::focus(sidebar::RENAME_ID),
                    iced::widget::operation::select_all(sidebar::RENAME_ID),
                ]);
            }
            LayerAction::NameEdited(name) => self.renaming = Some(name),
            LayerAction::Renamed => self.finish_renaming(),
        }
        Task::none()
    }

    pub(super) fn finish_renaming(&mut self) {
        if let Some(name) = self.renaming.take() {
            let active = self.doc.active();
            self.doc.rename(active, &name);
        }
    }

    // Photoshop refuses to paint where nothing would show, and so does this.
    pub(super) fn on_hidden_layer(&mut self) -> bool {
        let hidden = !self.doc.active_layer().visible;
        if hidden {
            self.status = i18n::layer_hidden().to_owned();
        }
        hidden
    }

    // The active layer's picture changes with every stamp; the others only when the stack does.
    pub(super) fn refresh_thumbnails(&mut self) {
        let active = self.doc.active();
        let mut kept = Vec::with_capacity(self.doc.layers().len());
        for (index, layer) in self.doc.layers().iter().enumerate() {
            let drawn_at = if index == active {
                self.doc.version()
            } else {
                self.doc.composites()
            };
            let reuse = self
                .thumbnails
                .iter()
                .position(|t| t.layer == layer.id && t.drawn_at == drawn_at);
            kept.push(match reuse {
                Some(at) => self.thumbnails.swap_remove(at),
                None => Thumbnail {
                    layer: layer.id,
                    drawn_at,
                    handle: thumbnail(&layer.pixels),
                },
            });
        }
        self.thumbnails = kept;
    }
}

// Nearest samples over a small checkerboard: a few hundred reads whatever the canvas size.
fn thumbnail(pixels: &Rgba8) -> iced::widget::image::Handle {
    let (width, height) = pixels.size();
    let fit = (THUMBNAIL.0 as f32 / width as f32).min(THUMBNAIL.1 as f32 / height as f32);
    let (w, h) = (
        ((width as f32 * fit).round() as u32).clamp(1, THUMBNAIL.0),
        ((height as f32 * fit).round() as u32).clamp(1, THUMBNAIL.1),
    );
    let source = pixels.as_bytes();
    let mut out = Vec::with_capacity((w * h) as usize * CHANNELS);
    for y in 0..h {
        let sy = ((y as f32 + 0.5) / fit) as usize;
        let sy = sy.min(height as usize - 1);
        for x in 0..w {
            let sx = (((x as f32 + 0.5) / fit) as usize).min(width as usize - 1);
            let at = (sy * width as usize + sx) * CHANNELS;
            let shade = if (x / CHECKER + y / CHECKER) % 2 == 0 {
                235
            } else {
                200
            };
            let mut pixel = [shade, shade, shade, 255];
            crate::doc::layers::blend(&mut pixel, &source[at..at + CHANNELS], 255);
            out.extend_from_slice(&pixel);
        }
    }
    iced::widget::image::Handle::from_rgba(w, h, out)
}
