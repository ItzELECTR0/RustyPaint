use crate::doc::image::CHANNELS;
use crate::doc::{self, Rect, Rgba8, Version};
use crate::gpu::Handle;
use crate::paint::{Tool, curve, shapes};
use crate::select::cutout::{
    refine::{Dab, Settings},
    workflow::{self, ResultMask},
};
use crate::select::{self, Floating, Xform};
use crate::ui::sidebar;

use iced::Point;

use super::*;

pub(super) struct Grabbed {
    pub(super) at: (f32, f32),
    pub(super) pointer: (f32, f32),
    pub(super) xform: Xform,
    pub(super) points: Vec<(f32, f32)>,
}

impl Grabbed {
    pub(super) fn rotated(&self, snap: bool) -> Xform {
        let mut target = self.xform.rotated_towards(self.pointer.0, self.pointer.1);
        if snap {
            target.rotation = select::xform::snap_angle(target.rotation);
        }
        target
    }
}

pub(super) struct LiveRedo {
    pub(super) floating: Floating,
    pub(super) canvas: Option<(Rgba8, bool)>,
    pub(super) version: Version,
}

pub struct Cropping {
    pub rect: Rect,
    pub framing: Option<sidebar::Framing>,
    pub lock: bool,
    pub fields: (String, String),
    pub(super) grabbed: Option<(Rect, Handle)>,
}

impl Cropping {
    pub(super) fn new(canvas: (u32, u32)) -> Self {
        let rect = Rect::new(0, 0, canvas.0, canvas.1);
        Self {
            rect,
            framing: None,
            lock: false,
            fields: (rect.width().to_string(), rect.height().to_string()),
            grabbed: None,
        }
    }

    pub(super) fn sync(&mut self) {
        self.fields = (
            self.rect.width().to_string(),
            self.rect.height().to_string(),
        );
    }

    pub(super) fn reframe(&mut self, ratio: f32, canvas: (u32, u32)) {
        let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
        let (mut w, mut h) = (cw, cw / ratio);
        if h > ch {
            h = ch;
            w = ch * ratio;
        }
        let centre = (
            (self.rect.x0 + self.rect.x1) as f32 / 2.0,
            (self.rect.y0 + self.rect.y1) as f32 / 2.0,
        );
        let x0 = (centre.0 - w / 2.0).round().clamp(0.0, cw - w);
        let y0 = (centre.1 - h / 2.0).round().clamp(0.0, ch - h);
        self.rect = Rect::new(
            x0 as u32,
            y0 as u32,
            (x0 + w).round() as u32,
            (y0 + h).round() as u32,
        );
        self.sync();
    }

    pub(super) fn dragged(&mut self, handle: Handle, to: (f32, f32), canvas: (u32, u32)) {
        let Some((from, _)) = self.grabbed else {
            return;
        };
        self.rect = dragged_edges(from, handle, to, canvas);

        let ratio = self.framing.map(|f| f.ratio()).or_else(|| {
            self.lock
                .then(|| from.width() as f32 / from.height().max(1) as f32)
        });
        if let Some(ratio) = ratio {
            self.keep_ratio(handle, ratio, canvas);
        }
        self.sync();
    }

    pub(super) fn keep_ratio(&mut self, handle: Handle, ratio: f32, canvas: (u32, u32)) {
        let (w, h) = (self.rect.width() as f32, self.rect.height() as f32);
        let follow_height = match handle {
            Handle::Left | Handle::Right => true,
            Handle::Top | Handle::Bottom => false,
            _ => w / ratio > h,
        };
        let (mut w, mut h) = if follow_height {
            (w, w / ratio)
        } else {
            (h * ratio, h)
        };

        let fit = (canvas.0 as f32 / w).min(canvas.1 as f32 / h).min(1.0);
        w *= fit;
        h *= fit;

        let west = matches!(handle, Handle::Left | Handle::TopLeft | Handle::BottomLeft);
        let north = matches!(handle, Handle::Top | Handle::TopLeft | Handle::TopRight);
        let x1 = if west {
            self.rect.x1 as f32
        } else {
            self.rect.x0 as f32 + w
        };
        let x0 = if west {
            self.rect.x1 as f32 - w
        } else {
            self.rect.x0 as f32
        };
        let y1 = if north {
            self.rect.y1 as f32
        } else {
            self.rect.y0 as f32 + h
        };
        let y0 = if north {
            self.rect.y1 as f32 - h
        } else {
            self.rect.y0 as f32
        };

        let slide = |low: f32, high: f32, limit: f32| {
            let shift = (-low).max(0.0) - (high - limit).max(0.0);
            (low + shift, high + shift)
        };
        let (x0, x1) = slide(x0, x1, canvas.0 as f32);
        let (y0, y1) = slide(y0, y1, canvas.1 as f32);
        self.rect = Rect::new(
            x0.round().max(0.0) as u32,
            y0.round().max(0.0) as u32,
            x1.round().min(canvas.0 as f32) as u32,
            y1.round().min(canvas.1 as f32) as u32,
        );
    }
}

pub struct CuttingOut {
    pub refining: bool,
    pub rect: Rect,
    pub(crate) result: Option<std::sync::Arc<ResultMask>>,
    pub(super) mask: Option<Vec<u8>>,
    pub(super) overlay: Option<std::sync::Arc<Vec<u8>>>,
    pub adding: bool,
    pub autofill: bool,
    pub(super) grabbed: Option<(Rect, Handle)>,
    pub(super) painting: bool,
    pub(super) id: u64,
    pub(super) revision: u64,
    pub(super) applied_revision: u64,
    pub(crate) failed: bool,
    pub(crate) object: bool,
    pub(crate) model_path: Option<std::path::PathBuf>,
    pub(crate) busy: bool,
    pub(super) cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) strokes: Vec<Dab>,
    pub(super) edit_field: Option<Field>,
    pub(crate) settings: Settings,
    pub(super) history: Vec<CutoutEdit>,
    pub(super) redo: Vec<CutoutEdit>,
    pub(crate) brush: f32,
    pub(super) previous: Option<(f32, f32)>,
    pub(super) stroke: u32,
    pub(crate) aim: workflow::Aim,
    pub(super) sampled: bool,
    pub(crate) preview: CutoutPreview,
    // What the colour preview sits the cut on, kept here so choosing it never touches the paint.
    pub(crate) ground: [u8; 4],
}

impl CuttingOut {
    pub(super) const BRUSH: f32 = 16.0;

    pub(super) fn new(canvas: (u32, u32)) -> Self {
        Self {
            refining: false,
            rect: Rect::new(0, 0, canvas.0, canvas.1),
            result: None,
            mask: None,
            overlay: None,
            adding: true,
            autofill: true,
            grabbed: None,
            painting: false,
            id: next_cutout_id(),
            revision: 0,
            applied_revision: 0,
            failed: false,
            object: true,
            model_path: None,
            busy: false,
            cancelled: Default::default(),
            strokes: Vec::new(),
            edit_field: None,
            settings: Settings::default(),
            history: Vec::new(),
            redo: Vec::new(),
            brush: Self::BRUSH,
            previous: None,
            stroke: 0,
            aim: workflow::Aim::default(),
            sampled: false,
            preview: CutoutPreview::Overlay,
            ground: [255, 255, 255, 255],
        }
    }

    pub(super) fn build_overlay(&mut self, pixels: &Rgba8) {
        let Some(mask) = &self.mask else { return };
        let pixels = self
            .result
            .as_ref()
            .and_then(|r| r.foreground.as_ref())
            .unwrap_or(pixels);
        let canvas = pixels.size();
        let mut out = vec![0u8; canvas.0 as usize * canvas.1 as usize * CHANNELS];
        for (i, pixel) in out.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
            let alpha = mask.get(i).copied().unwrap_or(0) as u32;
            if self.preview == CutoutPreview::Overlay {
                *pixel = [0, 0, 0, ((255 - alpha) * 150 / 255) as u8];
            } else {
                let checker = if ((i % canvas.0 as usize) / 12 + (i / canvas.0 as usize) / 12)
                    .is_multiple_of(2)
                {
                    180
                } else {
                    225
                };
                let alpha = alpha * pixels.as_bytes()[i * 4 + 3] as u32 / 255;
                for (c, value) in pixel[..3].iter_mut().enumerate() {
                    let ground = match self.preview {
                        CutoutPreview::Colour => self.ground[c] as u32,
                        _ => checker,
                    };
                    *value = ((pixels.as_bytes()[i * 4 + c] as u32 * alpha
                        + ground * (255 - alpha))
                        / 255) as u8;
                }
                pixel[3] = 255;
            }
        }
        self.overlay = Some(std::sync::Arc::new(out));
    }

    pub(super) fn dab(&mut self, at: (f32, f32), radius: f32, canvas: (u32, u32)) {
        let adding = self.adding;
        if let Some(mask) = &mut self.mask {
            let value = if adding { 255 } else { 0 };
            let x0 = (at.0 - radius).floor().max(self.rect.x0 as f32) as u32;
            let y0 = (at.1 - radius).floor().max(self.rect.y0 as f32) as u32;
            let x1 = ((at.0 + radius).ceil().max(0.0) as u32).min(self.rect.x1);
            let y1 = ((at.1 + radius).ceil().max(0.0) as u32).min(self.rect.y1);
            for y in y0..y1 {
                for x in x0..x1 {
                    let d = (x as f32 + 0.5 - at.0).powi(2) + (y as f32 + 0.5 - at.1).powi(2);
                    if d <= radius * radius {
                        mask[y as usize * canvas.0 as usize + x as usize] = value;
                    }
                }
            }
        }
        self.strokes.push(Dab {
            at,
            radius,
            foreground: adding,
            stroke: self.stroke,
        });
    }

    pub(super) fn bounds(&self, canvas: (u32, u32)) -> Option<Rect> {
        let mask = self.mask.as_ref()?;
        let (mut x0, mut y0) = (canvas.0, canvas.1);
        let (mut x1, mut y1) = (0u32, 0u32);
        for y in 0..canvas.1 {
            for x in 0..canvas.0 {
                if mask[y as usize * canvas.0 as usize + x as usize] > 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1, y1))
    }

    pub(super) fn mask_in(&self, rect: Rect, canvas: (u32, u32)) -> Option<Vec<u8>> {
        let mask = self.mask.as_ref()?;
        let mut out = Vec::with_capacity(rect.area());
        for y in rect.rows() {
            for x in rect.cols() {
                out.push(mask[y as usize * canvas.0 as usize + x as usize]);
            }
        }
        Some(out)
    }
}

pub struct Sticker {
    pub(super) pixels: Rgba8,
    pub(super) thumb: iced::widget::image::Handle,
}

impl Sticker {
    const THUMB: f32 = 36.0;

    pub(super) fn new(pixels: Rgba8) -> Self {
        let (w, h) = pixels.size();
        let fit = (Self::THUMB / w.max(1) as f32)
            .min(Self::THUMB / h.max(1) as f32)
            .min(1.0);
        let small = doc::transform::scale(
            &pixels,
            ((w as f32 * fit).round() as u32).max(1),
            ((h as f32 * fit).round() as u32).max(1),
            doc::transform::Resampling::Smooth,
        );
        let (tw, th) = small.size();
        let thumb = iced::widget::image::Handle::from_rgba(tw, th, small.as_bytes().to_vec());
        Self { pixels, thumb }
    }

    pub fn thumb(&self) -> &iced::widget::image::Handle {
        &self.thumb
    }
}

impl App {
    pub(super) fn draw_drawing(&mut self, from: (f32, f32), to: (f32, f32)) {
        match self.drawing {
            Drawing::Shape(kind) => {
                let Some(rect) = drag_rect(from, to, self.doc.size()) else {
                    return;
                };
                self.draw_shape(kind, rect);
            }
            Drawing::Curve(kind) => {
                let (from, to) = (self.within_reach(from), self.within_reach(to));
                let to = if self.mods.shift() {
                    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                    let angle = select::xform::snap_angle(dy.atan2(dx));
                    let length = dx.hypot(dy);
                    (from.0 + length * angle.cos(), from.1 + length * angle.sin())
                } else {
                    to
                };
                self.draw_curve(kind, from, to)
            }
        }
    }

    pub(super) fn draw_shape(&mut self, kind: shapes::ShapeKind, rect: Rect) {
        match &mut self.floating {
            Some(floating) if matches!(floating.source, select::Source::Shape { .. }) => {
                floating.xform = Xform::from_rect(rect);
                floating.redraw();
            }
            _ => {
                self.floating = Some(Floating::shape(&self.doc, kind, self.shape_style, rect));
            }
        }
        self.float_version += 1;
        self.dirty = None;
    }

    pub(super) fn draw_blur(&mut self, from: (f32, f32), to: (f32, f32)) {
        let Some(rect) = drag_rect(from, to, self.doc.size()) else {
            return;
        };
        match &mut self.floating {
            Some(floating) if matches!(floating.source, select::Source::Blur { .. }) => {
                floating.xform = Xform::from_rect(rect);
            }
            _ => {
                self.floating = Some(Floating::blur(&self.doc, self.blur_settings, rect));
            }
        }
        self.float_version += 1;
        self.dirty = None;
    }

    pub(super) fn draw_curve(&mut self, kind: curve::CurveKind, from: (f32, f32), to: (f32, f32)) {
        match &mut self.floating {
            Some(floating) if matches!(floating.source, select::Source::Curve { .. }) => {
                floating.stretch(from, to);
            }
            _ => {
                let style = self.curve_style();
                self.floating = Some(Floating::curve(&self.doc, kind, style, from, to));
            }
        }
        self.float_version += 1;
        self.dirty = None;
    }

    pub(super) fn restyle_text(&mut self) {
        let style = self.text_style.clone();
        if let Some(floating) = &mut self.floating
            && floating.text_box().is_some()
        {
            floating.restyle_text(style);
            self.float_version += 1;
        }
    }

    pub(super) fn edit_text(&mut self, action: TextAction) {
        let style = self.text_style.clone();
        let Some(floating) = &mut self.floating else {
            return;
        };
        if !floating.editing {
            return;
        }
        let local = |x: f32, y: f32, xform: Xform| {
            let (u, v) = xform.to_local(x, y);
            (
                (u * xform.width).round() as i32,
                (v * xform.height).round() as i32,
            )
        };
        let xform = floating.xform;
        let Some(boxed) = floating.text_box() else {
            return;
        };

        use crate::text::Action as A;
        use cosmic_text::Motion as M;

        if let TextAction::Insert(c) = action {
            boxed.insert(c, &style);
            floating.redraw();
            self.float_version += 1;
            self.caret_on = true;
            return;
        }

        let action = match action {
            TextAction::Insert(c) => A::Insert(c),
            TextAction::Enter => A::Enter,
            TextAction::Backspace => A::Backspace,
            TextAction::Delete => A::Delete,
            TextAction::Click(x, y) => {
                let (x, y) = local(x, y, xform);
                A::Click { x, y }
            }
            TextAction::Drag(x, y) => {
                let (x, y) = local(x, y, xform);
                A::Drag { x, y }
            }
            TextAction::Motion(motion) => {
                let (select, motion) = match motion {
                    Motion::Left => (false, M::Left),
                    Motion::Right => (false, M::Right),
                    Motion::Up => (false, M::Up),
                    Motion::Down => (false, M::Down),
                    Motion::Home => (false, M::Home),
                    Motion::End => (false, M::End),
                    Motion::SelectLeft => (true, M::Left),
                    Motion::SelectRight => (true, M::Right),
                    Motion::SelectUp => (true, M::Up),
                    Motion::SelectDown => (true, M::Down),
                    Motion::SelectAll => {
                        boxed.select_all();
                        self.float_version += 1;
                        return;
                    }
                };
                boxed.set_selecting(select);
                A::Motion(motion)
            }
        };
        boxed.act(action);
        floating.redraw();
        self.float_version += 1;
        self.caret_on = true;
    }

    pub(super) fn paste_text(&mut self, text: &str) {
        let style = self.text_style.clone();
        if self.typing() {
            if let Some(floating) = &mut self.floating
                && let Some(boxed) = floating.text_box()
            {
                boxed.insert_str(text, &style);
                floating.redraw();
                self.float_version += 1;
                self.caret_on = true;
            }
            return;
        }

        self.commit_floating();
        if matches!(self.tab, Tab::Brushes | Tab::Symmetry) {
            self.stashed_tool = self.brush.tool;
        }
        self.tab = Tab::Text;
        self.brush.tool = Tool::Text;
        let at = self.looking_at();
        self.floating = Some(Floating::pasted_text(&self.doc, style, text, at));
        self.float_version += 1;
        self.dirty = None;
        self.status.clear();
    }

    pub(super) fn copy_selected_text(&mut self, cut: bool) {
        let Some(floating) = &mut self.floating else {
            return;
        };
        let Some(boxed) = floating.text_box() else {
            return;
        };
        let Some(text) = boxed.selected_text() else {
            return;
        };
        let _ = doc::clipboard::copy_text(&text);
        if !cut {
            return;
        }
        boxed.delete_selection();
        floating.redraw();
        self.float_version += 1;
    }

    pub(super) fn end_text(&mut self, a: (f32, f32), b: (f32, f32), tiny: bool) {
        let canvas = self.doc.size();
        let rect = if tiny {
            let (x, y) = self.within_reach(a);
            let (w, h) = (self.text_style.size * 6.0, self.text_style.line_height());
            drag_rect((x, y), (x + w, y + h), canvas)
        } else {
            drag_rect(a, b, canvas)
        };
        if let Some(rect) = rect {
            self.draw_text(rect);
        }
    }

    pub(super) fn draw_text(&mut self, rect: Rect) {
        let busy = self
            .floating
            .as_ref()
            .is_some_and(|f| f.editing && matches!(f.source, select::Source::Text(_)));
        if busy {
            return;
        }
        self.commit_floating();
        self.floating = Some(Floating::text(&self.doc, self.text_style.clone(), rect));
        self.float_version += 1;
        self.dirty = None;
    }

    pub(super) fn curve_style(&self) -> shapes::ShapeStyle {
        shapes::ShapeStyle {
            fill: None,
            outline: Some(self.brush.colour),
            thickness: self.shape_style.thickness,
        }
    }

    pub(super) fn begin_float_from(&mut self, rect: Rect, mask: Option<&[u8]>) {
        self.commit_floating();
        if let Some(floating) = Floating::lift_masked(&mut self.doc, rect, mask) {
            self.float_version += 1;
            self.floating = Some(floating);
            self.dirty = None;
        }
    }

    pub(super) fn middle(&self) -> (f32, f32) {
        let (w, h) = self.doc.size();
        (w as f32 / 2.0, h as f32 / 2.0)
    }

    pub(super) fn looking_at(&self) -> (f32, f32) {
        let centre = Point::new(self.viewport.width / 2.0, self.viewport.height / 2.0);
        let (x, y) = self.view.to_image(centre, self.viewport, self.doc.size());
        let (w, h) = self.doc.size();
        (x.clamp(0.0, w as f32), y.clamp(0.0, h as f32))
    }

    pub(super) fn remember_sticker(&mut self, pixels: &Rgba8) {
        let print = fingerprint(pixels);
        self.stickers.retain(|s| fingerprint(&s.pixels) != print);
        self.stickers.push(Sticker::new(pixels.clone()));
        while self.stickers.len() > STICKER_HISTORY {
            self.stickers.remove(0);
        }
    }

    pub(super) fn float_at(&mut self, pixels: Rgba8, at: (f32, f32)) {
        self.commit_floating();
        self.remember_sticker(&pixels);
        self.floating = Some(Floating::place(&self.doc, pixels, at));
        self.float_version += 1;
        self.status.clear();
    }

    pub(super) fn commit_floating(&mut self) {
        let Some(mut floating) = self.floating.take() else {
            return;
        };
        self.grab = None;
        self.grab_from = None;
        if floating.text_box().is_some_and(|boxed| boxed.is_empty()) {
            self.dirty = None;
            return;
        }
        floating.editing = false;
        if let Some(touched) = floating.commit(&mut self.doc) {
            self.doc
                .commit(floating.label(), touched, floating.backup());
        }
        self.dirty = None;
    }

    pub(super) fn restyle_shape(&mut self) {
        let curve = self.curve_style();
        let Some(floating) = &mut self.floating else {
            return;
        };
        let style = match floating.source {
            select::Source::Curve { closed: false, .. } => curve,
            _ => self.shape_style,
        };
        floating.restyle(style);
        self.float_version += 1;
    }

    pub(super) fn refining(&self) -> bool {
        self.cutting_out.as_ref().is_some_and(|m| m.refining)
    }

    // Picking a colour out of the picture is what the colour target is steered by.
    pub(super) fn sample_cutout_tone(&mut self, x: f32, y: f32) {
        let Some(tone) =
            crate::paint::fill::pick(self.doc.pixels(), x.floor() as i64, y.floor() as i64)
        else {
            return;
        };
        if let Some(cutting_out) = &mut self.cutting_out
            && cutting_out.aim.target == workflow::Target::Colour
        {
            cutting_out.aim.tone = tone;
            cutting_out.sampled = true;
        }
    }

    pub(super) fn cutout_dab(&mut self, x: f32, y: f32, first: bool) {
        let canvas = self.doc.size();
        let Some(cutting_out) = &mut self.cutting_out else {
            return;
        };
        if first {
            cutting_out.checkpoint();
            cutting_out.painting = true;
            cutting_out.previous = None;
            cutting_out.stroke += 1;
        }
        if !cutting_out.painting {
            return;
        }
        let radius = cutting_out.brush / self.view.zoom.max(0.01);
        let end = (x.clamp(0.0, canvas.0 as f32), y.clamp(0.0, canvas.1 as f32));
        let from = cutting_out.previous.unwrap_or(end);
        let steps = (((end.0 - from.0).hypot(end.1 - from.1) / (radius * 0.4).max(0.5)).ceil()
            as usize)
            .max(1);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            cutting_out.dab(
                (from.0 + (end.0 - from.0) * t, from.1 + (end.1 - from.1) * t),
                radius,
                canvas,
            );
        }
        cutting_out.previous = Some(end);
        cutting_out.build_overlay(self.doc.pixels());
        self.float_version += 1;
    }

    pub(super) fn cutout_done(&mut self) {
        if self
            .cutting_out
            .as_ref()
            .is_some_and(|c| c.busy || c.painting || c.failed || c.revision != c.applied_revision)
        {
            return;
        }
        let canvas = self.doc.size();
        let Some(cutting_out) = self.cutting_out.take() else {
            return;
        };
        let Some(rect) = cutting_out.bounds(canvas) else {
            return;
        };
        let Some(mask) = cutting_out.mask_in(rect, canvas) else {
            return;
        };

        self.begin_float_from(rect, Some(&mask));
        if let Some(foreground) = cutting_out
            .result
            .as_ref()
            .and_then(|r| r.foreground.as_ref())
            && let Some(floating) = &mut self.floating
        {
            let mut corrected = crate::doc::transform::crop(foreground, rect);
            for (pixel, cover) in corrected
                .pixels_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(&mask)
            {
                pixel[3] = (u16::from(pixel[3]) * u16::from(*cover) / 255) as u8;
            }
            floating.pixels = corrected;
        }
        if cutting_out.autofill {
            let filled = crate::select::cutout::fill_behind(self.doc.pixels(), &mask, rect);
            *self.doc.edit() = filled;
        }

        self.brush.tool = Tool::Select;
        self.float_version += 1;
        self.dirty = None;
    }

    pub(super) fn cancel_floating(&mut self) {
        let Some(floating) = self.floating.take() else {
            return;
        };
        let active = (self.doc.pixels().clone(), self.doc.touched());
        let canvas = floating.cancel(&mut self.doc).then_some(active);
        self.live_redo = Some(LiveRedo {
            floating,
            canvas,
            version: self.doc.version(),
        });
        self.grab = None;
        self.grab_from = None;
        self.float_version += 1;
        self.dirty = None;
    }

    pub(super) fn redo_floating(&mut self) -> bool {
        let Some(redo) = self.live_redo.take() else {
            return false;
        };
        if redo.version != self.doc.version() {
            return false;
        }
        if let Some((pixels, touched)) = redo.canvas {
            self.doc.restore_live(pixels, touched);
        }
        self.floating = Some(redo.floating);
        self.float_version += 1;
        self.dirty = None;
        true
    }
}
