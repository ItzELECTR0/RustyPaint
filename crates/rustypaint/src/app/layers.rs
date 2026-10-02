use crate::doc::{Document, Rgba8, image::CHANNELS};
use crate::i18n;
use crate::ui::layers::{MARGIN, TILE, WIDTH};

use iced::Task;
use iced::animation::Easing;
use std::time::Duration;

use super::*;

// Drawn at twice the size they show at, so they stay sharp on a high-density screen.
const THUMBNAIL: (u32, u32) = (TILE.0 as u32 * 2, TILE.1 as u32 * 2);
const CHECKER: u32 = 8;

const SLIDE: Duration = Duration::from_millis(220);

// Far enough right that the bar's shadow has left the canvas too.
const STOWED: f32 = WIDTH + MARGIN + 16.0;

pub(super) fn slide(shown: bool) -> Animation<bool> {
    Animation::new(shown)
        .duration(SLIDE)
        .easing(Easing::EaseInOutCubic)
}

impl App {
    // Anything that changes which layer the tools work on lands the live object first, in the
    // layer it was made on, the same way picking another tool does.
    pub(super) fn change_layers(&mut self, change: impl FnOnce(&mut Document)) {
        if self.cropping.is_some() || self.cutting_out.is_some() {
            return;
        }
        self.commit_floating();
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
            LayerAction::At(index, action) => {
                if held || index >= self.doc.layers().len() {
                    return Task::none();
                }
                self.change_layers(|doc| {
                    doc.select_layer(index);
                });
                return self.layer_action(*action);
            }
        }
        Task::none()
    }

    pub(super) fn toggle_layers(&mut self) {
        let shown = !self.config.layers_shown;
        self.config.layers_shown = shown;
        self.now = Instant::now();
        if self.config.reduced_motion {
            self.layers_slide = slide(shown);
        } else {
            self.layers_slide.go_mut(shown, self.now);
        }
        self.save_config();
    }

    pub(super) fn layers_sliding(&self) -> bool {
        self.layers_slide.is_animating(self.now)
    }

    // How far right of its place the bar is drawn, or None while it is put away.
    pub(super) fn layers_shift(&self) -> Option<f32> {
        let out = self.layers_slide.interpolate(0.0_f32, 1.0, self.now);
        (out > 0.0).then_some((1.0 - out) * STOWED)
    }

    // The width of canvas the open bar covers, which fitting the picture leaves alone.
    pub(super) fn layers_cover(&self) -> f32 {
        if self.config.layers_shown {
            WIDTH + 2.0 * MARGIN
        } else {
            0.0
        }
    }

    pub(super) fn fitted(&self, size: (u32, u32)) -> View {
        View::fitted_beside(self.viewport, size, self.layers_cover())
    }

    // Photoshop refuses to paint where nothing would show, and so does this.
    pub(super) fn on_hidden_layer(&mut self) -> bool {
        let hidden = !self.doc.active_layer().visible;
        if hidden {
            self.status = i18n::layer_hidden().to_owned();
        }
        hidden
    }

    // The active one changes with every stamp, so it waits for the stroke to end rather than
    // costing every pointer sample. None are drawn while the bar is put away.
    pub(super) fn refresh_thumbnails(&mut self) {
        if self.stroke.is_some() || !self.config.layers_shown {
            return;
        }
        let active = self.doc.active();
        let backing = self.doc.has_backing();
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
                    handle: thumbnail(&layer.pixels, index == 0 && backing),
                },
            });
        }
        self.thumbnails = kept;
    }
}

// Nearest samples over a small checkerboard, or over white for the bottom layer of a picture
// with a backing, as the canvas shows it: a few thousand reads whatever the canvas size.
fn thumbnail(pixels: &Rgba8, on_white: bool) -> iced::widget::image::Handle {
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
            let shade = match (on_white, (x / CHECKER + y / CHECKER).is_multiple_of(2)) {
                (true, _) => 255,
                (false, true) => 235,
                (false, false) => 200,
            };
            let mut pixel = [shade, shade, shade, 255];
            crate::doc::layers::blend(&mut pixel, &source[at..at + CHANNELS], 255);
            out.extend_from_slice(&pixel);
        }
    }
    iced::widget::image::Handle::from_rgba(w, h, out)
}
