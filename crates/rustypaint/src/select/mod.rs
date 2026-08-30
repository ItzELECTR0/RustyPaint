#[allow(
    dead_code,
    reason = "the segmentation is written and judged before any of it is wired to a button"
)]
pub mod cutout;
pub mod lasso;
pub mod xform;

pub use lasso::Lasso;
pub use xform::Xform;

use crate::doc::{Document, Rect, Rgba8, image::CHANNELS, transform};
use crate::gpu::Handle;
use crate::paint::curve::{self, CurveKind};
use crate::paint::shapes::{self, ShapeKind, ShapeStyle};
use crate::text::{TextBox, TextStyle};

pub const MAX_DRAW: u32 = 8192;

pub enum Source {
    Bitmap,
    Shape {
        kind: ShapeKind,
        style: ShapeStyle,
    },
    Curve {
        kind: CurveKind,
        style: ShapeStyle,
        points: Vec<(f32, f32)>,
        closed: bool,
    },
    Text(Box<TextBox>),
}

pub struct Floating {
    pub pixels: Rgba8,
    pub source: Source,
    pub lifted_from: Option<Rect>,
    pub xform: Xform,
    pub editing: bool,
    caret: bool,
    opacity: f32,
    turns: u8,
    flip: (bool, bool),
    masked: bool,
    // The grip the box was last dragged by, so arrow keys stretch the same edge it did.
    pub stretched: Option<Handle>,
    backup: Rgba8,
    backup_touched: bool,
}

impl Floating {
    pub fn lift_masked(doc: &mut Document, rect: Rect, mask: Option<&[u8]>) -> Option<Self> {
        let rect = rect.clamped(doc.size().0, doc.size().1);
        if rect.is_empty() {
            return None;
        }
        let mask = match mask {
            Some(mask) if mask.len() == (rect.width() * rect.height()) as usize => Some(mask),
            _ => None,
        };
        let masked = mask.is_some();
        let backup = doc.pixels().clone();
        let backup_touched = doc.touched();
        let mut pixels = crate::doc::transform::crop(doc.pixels(), rect);
        if let Some(mask) = mask {
            for (px, cover) in pixels
                .pixels_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(mask)
            {
                px[3] = ((px[3] as u16 * *cover as u16) / 255) as u8;
            }
        }

        let canvas = doc.edit();
        let stride = canvas.width() as usize * CHANNELS;
        let bytes = canvas.pixels_mut();
        for (row, y) in rect.rows().enumerate() {
            let start = y as usize * stride + rect.x0 as usize * CHANNELS;
            let span = rect.width() as usize * CHANNELS;
            match mask {
                None => bytes[start..start + span].fill(0),
                Some(mask) => {
                    for (column, px) in bytes[start..start + span]
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .enumerate()
                    {
                        let cover = mask[row * rect.width() as usize + column] as u16;
                        px[3] = ((px[3] as u16 * (255 - cover)) / 255) as u8;
                    }
                }
            }
        }

        Some(Self {
            pixels,
            source: Source::Bitmap,
            lifted_from: Some(rect),
            xform: Xform::from_rect(rect),
            editing: false,
            caret: false,
            opacity: 1.0,
            turns: 0,
            flip: (false, false),
            masked,
            stretched: None,
            backup,
            backup_touched,
        })
    }

    pub fn place(doc: &Document, pixels: Rgba8, at: (f32, f32)) -> Self {
        let (cw, ch) = doc.size();
        let (iw, ih) = pixels.size();

        let fit = (cw as f32 / iw as f32).min(ch as f32 / ih as f32).min(1.0);
        let (w, h) = (iw as f32 * fit, ih as f32 * fit);

        Self {
            pixels,
            source: Source::Bitmap,
            lifted_from: None,
            xform: Xform {
                x: at.0 - w / 2.0,
                y: at.1 - h / 2.0,
                width: w,
                height: h,
                rotation: 0.0,
            },
            editing: false,
            caret: false,
            opacity: 1.0,
            turns: 0,
            flip: (false, false),
            masked: false,
            stretched: None,
            backup: doc.pixels().clone(),
            backup_touched: doc.touched(),
        }
    }

    pub fn shape(doc: &Document, kind: ShapeKind, style: ShapeStyle, rect: Rect) -> Self {
        let mut floating = Self {
            pixels: Rgba8::new(1, 1, [0, 0, 0, 0]),
            source: Source::Shape { kind, style },
            lifted_from: None,
            xform: Xform::from_rect(rect),
            editing: false,
            caret: false,
            opacity: 1.0,
            turns: 0,
            flip: (false, false),
            masked: false,
            stretched: None,
            backup: doc.pixels().clone(),
            backup_touched: doc.touched(),
        };
        floating.redraw();
        floating
    }

    pub fn curve(
        doc: &Document,
        kind: CurveKind,
        style: ShapeStyle,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Self {
        let mut floating = Self {
            pixels: Rgba8::new(1, 1, [0, 0, 0, 0]),
            source: Source::Curve {
                kind,
                style,
                points: curve::lay_out(kind, from, to),
                closed: false,
            },
            lifted_from: None,
            xform: Xform {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                rotation: 0.0,
            },
            editing: false,
            caret: false,
            opacity: 1.0,
            turns: 0,
            flip: (false, false),
            masked: false,
            stretched: None,
            backup: doc.pixels().clone(),
            backup_touched: doc.touched(),
        };
        floating.redraw();
        floating
    }

    pub fn text(doc: &Document, style: TextStyle, rect: Rect) -> Self {
        let layout = (rect.width() as f32 - style.slack()).max(1.0);
        let mut floating = Self {
            pixels: Rgba8::new(1, 1, [0, 0, 0, 0]),
            source: Source::Text(Box::new(TextBox::new(style, layout))),
            lifted_from: None,
            xform: Xform::from_rect(rect),
            editing: true,
            caret: true,
            opacity: 1.0,
            turns: 0,
            flip: (false, false),
            masked: false,
            stretched: None,
            backup: doc.pixels().clone(),
            backup_touched: doc.touched(),
        };
        floating.redraw();
        floating
    }

    pub fn pasted_text(doc: &Document, style: TextStyle, text: &str, at: (f32, f32)) -> Self {
        let room = (doc.size().0 as f32 - style.slack()).max(1.0);
        let mut boxed = TextBox::restyled_from(text, style, room);
        boxed.set_width(boxed.laid_width().clamp(1.0, room));

        let mut floating = Self {
            pixels: Rgba8::new(1, 1, [0, 0, 0, 0]),
            source: Source::Text(Box::new(boxed)),
            lifted_from: None,
            xform: Xform {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                rotation: 0.0,
            },
            editing: true,
            caret: true,
            opacity: 1.0,
            turns: 0,
            flip: (false, false),
            masked: false,
            stretched: None,
            backup: doc.pixels().clone(),
            backup_touched: doc.touched(),
        };
        floating.redraw();
        floating.xform.x = at.0 - floating.xform.width / 2.0;
        floating.xform.y = at.1 - floating.xform.height / 2.0;
        floating
    }

    pub fn text_is_empty(&self) -> bool {
        matches!(&self.source, Source::Text(boxed) if boxed.is_empty())
    }

    pub fn text_box(&mut self) -> Option<&mut TextBox> {
        match &mut self.source {
            Source::Text(boxed) => Some(boxed),
            _ => None,
        }
    }

    pub fn can_redo_text(&self) -> bool {
        matches!(&self.source, Source::Text(boxed) if boxed.can_redo())
    }

    pub fn undo_text(&mut self) -> Option<TextStyle> {
        let Source::Text(boxed) = &mut self.source else {
            return None;
        };
        if !boxed.undo() {
            return None;
        }
        let style = boxed.style.clone();
        self.redraw();
        Some(style)
    }

    pub fn redo_text(&mut self) -> Option<TextStyle> {
        let Source::Text(boxed) = &mut self.source else {
            return None;
        };
        if !boxed.redo() {
            return None;
        }
        let style = boxed.style.clone();
        self.redraw();
        Some(style)
    }

    pub fn relay(&mut self, kind: CurveKind, from: (f32, f32), to: (f32, f32)) {
        if let Source::Curve {
            kind: current,
            points,
            closed: false,
            ..
        } = &mut self.source
        {
            *current = kind;
            *points = curve::lay_out(kind, from, to);
            self.redraw();
        }
    }

    pub fn stretch(&mut self, from: (f32, f32), to: (f32, f32)) {
        if let Source::Curve { kind, points, .. } = &mut self.source {
            *points = curve::lay_out(*kind, from, to);
            self.redraw();
        }
    }

    pub fn is_closed(&self) -> bool {
        matches!(self.source, Source::Curve { closed: true, .. })
    }

    pub fn add_point(&mut self, x: f32, y: f32, reach: f32) -> bool {
        let Source::Curve { points, closed, .. } = &mut self.source else {
            return false;
        };
        if points.len() >= curve::MAX_POINTS {
            return false;
        }
        if curve::distance_to(points, (x, y), *closed) > reach {
            return false;
        }
        let at = curve::nearest_span(points, (x, y), *closed);
        points.insert(at.min(points.len()), (x, y));
        self.redraw();
        true
    }

    pub fn remove_point(&mut self, index: usize) -> bool {
        let Source::Curve { points, closed, .. } = &mut self.source else {
            return false;
        };
        let fewest = if *closed { 3 } else { 2 };
        if points.len() <= fewest || index >= points.len() {
            return false;
        }
        points.remove(index);
        self.redraw();
        true
    }

    pub fn add_bones(&mut self) -> bool {
        let Source::Shape { kind, style } = &self.source else {
            return false;
        };
        let (kind, style) = (*kind, *style);

        let (w, h) = (self.xform.width.max(1.0), self.xform.height.max(1.0));
        let inset = style.outline.map_or(0.0, |_| style.thickness / 2.0);
        let path = shapes::outline_of(
            kind,
            (w - style.thickness).max(1.0),
            (h - style.thickness).max(1.0),
        );
        let Some(path) = path else { return false };

        let points: Vec<(f32, f32)> = curve::sample(&path, curve::SHAPE_BONES)
            .into_iter()
            .map(|(x, y)| (self.xform.x + inset + x, self.xform.y + inset + y))
            .collect();
        if points.len() < 3 {
            return false;
        }

        self.source = Source::Curve {
            kind: CurveKind::Curve5,
            style,
            points,
            closed: true,
        };
        self.turns = 0;
        self.flip = (false, false);
        self.redraw();
        true
    }

    pub fn bend(&mut self, index: usize, x: f32, y: f32) {
        if let Source::Curve { points, .. } = &mut self.source
            && let Some(point) = points.get_mut(index)
        {
            *point = (x, y);
            self.redraw();
        }
    }

    pub fn shift_to(&mut self, x: f32, y: f32) {
        let (dx, dy) = (x - self.xform.x, y - self.xform.y);
        if let Source::Curve { points, .. } = &mut self.source {
            for point in points.iter_mut() {
                *point = (point.0 + dx, point.1 + dy);
            }
        }
        self.xform.x = x;
        self.xform.y = y;
    }

    pub fn points(&self) -> &[(f32, f32)] {
        match &self.source {
            Source::Curve { points, .. } => points,
            _ => &[],
        }
    }

    pub fn redraw(&mut self) {
        if matches!(self.source, Source::Text(_)) {
            return self.redraw_text();
        }
        match &self.source {
            Source::Bitmap | Source::Text(_) => {}
            Source::Shape { kind, style } => {
                let (w, h) = (side(self.xform.width), side(self.xform.height));
                let (w, h) = if self.turns % 2 == 1 { (h, w) } else { (w, h) };
                if let Some(drawn) = shapes::render(*kind, style, w, h) {
                    self.pixels = drawn;
                }
                self.reapply_turns();
            }
            Source::Curve {
                style,
                points,
                closed,
                ..
            } => {
                let (x0, y0, x1, y1) = curve::bounds(points, style.thickness, *closed);
                let (x0, y0) = (x0.floor(), y0.floor());
                let (w, h) = (block(side(x1.ceil() - x0)), block(side(y1.ceil() - y0)));
                if let Some(drawn) = curve::render(points, style, (x0, y0), w, h, *closed) {
                    self.pixels = drawn;
                }
                self.xform = Xform {
                    x: x0,
                    y: y0,
                    width: w as f32,
                    height: h as f32,
                    rotation: 0.0,
                };
            }
        }
    }

    pub fn keeps_ratio(&self) -> bool {
        matches!(self.source, Source::Bitmap) && self.lifted_from.is_none()
    }

    pub fn masked(&self) -> bool {
        self.masked
    }

    pub fn is_curve(&self) -> bool {
        matches!(self.source, Source::Curve { .. })
    }

    pub fn refit(&mut self, was: Xform, to: Xform, original: &[(f32, f32)]) {
        let Source::Curve { points, .. } = &mut self.source else {
            return self.resize_to(to);
        };
        *points = original
            .iter()
            .map(|(x, y)| {
                let (u, v) = was.to_local(*x, *y);
                to.to_canvas(u, v)
            })
            .collect();
        self.redraw();
    }

    pub fn is_drawing(&self) -> bool {
        matches!(self.source, Source::Shape { .. } | Source::Curve { .. })
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    pub fn turn(&mut self, clockwise: bool) {
        if let Source::Curve { points, .. } = &mut self.source {
            let (cx, cy) = centre_of(points);
            for point in points.iter_mut() {
                let (dx, dy) = (point.0 - cx, point.1 - cy);
                *point = if clockwise {
                    (cx - dy, cy + dx)
                } else {
                    (cx + dy, cy - dx)
                };
            }
            return self.redraw();
        }

        self.turns = (self.turns + if clockwise { 1 } else { 3 }) % 4;
        self.pixels = transform::rotate_90(&self.pixels, clockwise);
        let (w, h) = (self.xform.width, self.xform.height);
        self.xform.x += (w - h) / 2.0;
        self.xform.y += (h - w) / 2.0;
        self.xform.width = h;
        self.xform.height = w;
    }

    pub fn mirror(&mut self, horizontal: bool) {
        if let Source::Curve { points, .. } = &mut self.source {
            let (cx, cy) = centre_of(points);
            for point in points.iter_mut() {
                *point = if horizontal {
                    (cx * 2.0 - point.0, point.1)
                } else {
                    (point.0, cy * 2.0 - point.1)
                };
            }
            return self.redraw();
        }

        if horizontal {
            self.flip.0 = !self.flip.0;
            self.pixels = transform::flip_horizontal(&self.pixels);
        } else {
            self.flip.1 = !self.flip.1;
            self.pixels = transform::flip_vertical(&self.pixels);
        }
    }

    fn reapply_turns(&mut self) {
        if self.flip.0 {
            self.pixels = transform::flip_horizontal(&self.pixels);
        }
        if self.flip.1 {
            self.pixels = transform::flip_vertical(&self.pixels);
        }
        for _ in 0..self.turns {
            self.pixels = transform::rotate_90(&self.pixels, true);
        }
    }

    fn redraw_text(&mut self) {
        let (editing, caret) = (self.editing, self.caret);
        let dragged = self.xform.height;

        let Source::Text(boxed) = &mut self.source else {
            return;
        };
        let slack = boxed.slack();
        let width = boxed.width();
        let height = boxed.height().max((dragged - slack).max(1.0));
        let drawn = boxed.render(side(width + slack), side(height + slack), editing, caret);

        self.xform.width = width + slack;
        self.xform.height = height + slack;
        if let Some(drawn) = drawn {
            self.pixels = drawn;
        }
    }

    pub fn resize_to(&mut self, xform: Xform) {
        self.xform = xform;
        if let Source::Text(boxed) = &mut self.source {
            let slack = boxed.slack();
            boxed.set_width((xform.width - slack).max(1.0));
        }
        self.redraw();
    }

    pub fn restyle(&mut self, new: ShapeStyle) {
        match &mut self.source {
            Source::Shape { style, .. } | Source::Curve { style, .. } => *style = new,
            Source::Bitmap | Source::Text(_) => return,
        }
        self.redraw();
    }

    pub fn restyle_text(&mut self, new: TextStyle) {
        let Source::Text(boxed) = &mut self.source else {
            return;
        };
        if boxed.style_selection(&new) {
            return self.redraw();
        }
        if boxed.is_empty() {
            boxed.restyle_empty(new);
            self.xform.height = 1.0;
        } else {
            boxed.adopt(new);
        }
        self.redraw();
    }

    pub fn blink(&mut self, on: bool) -> bool {
        if !self.editing || !matches!(self.source, Source::Text(_)) || self.caret == on {
            return false;
        }
        self.caret = on;
        self.redraw();
        true
    }

    pub fn backup(&self) -> &Rgba8 {
        &self.backup
    }

    pub fn cancel(&self, doc: &mut Document) -> bool {
        if self.lifted_from.is_none() {
            return false;
        }
        doc.restore_live(self.backup.clone(), self.backup_touched);
        true
    }

    pub fn label(&self) -> &'static str {
        match (&self.source, self.lifted_from) {
            (Source::Shape { kind, .. }, _) => kind.name(),
            (Source::Curve { kind, .. }, _) => kind.name(),
            (Source::Text(_), _) => "Text",
            (Source::Bitmap, Some(_)) => "Move selection",
            (Source::Bitmap, None) => "Paste",
        }
    }

    pub fn commit(&self, doc: &mut Document) -> Option<Rect> {
        let size = doc.size();
        let rect = self.xform.bounds(size)?;

        if let Source::Curve {
            style,
            points,
            closed,
            ..
        } = &self.source
        {
            let drawn = curve::render(
                points,
                style,
                (rect.x0 as f32, rect.y0 as f32),
                rect.width(),
                rect.height(),
                *closed,
            );
            if let Some(drawn) = drawn {
                composite(doc, rect, &drawn, self.opacity);
                return Some(rect);
            }
        }

        if let Source::Shape { kind, style } = &self.source {
            let drawn = self.drawn_in(rect, *kind, style);
            if let Some(drawn) = drawn {
                composite(doc, rect, &drawn, self.opacity);
                return Some(match self.lifted_from {
                    Some(hole) => hole.union(rect),
                    None => rect,
                });
            }
        }

        let plain = match &self.source {
            Source::Text(boxed) => boxed.render(
                side(self.xform.width),
                side(self.xform.height),
                false,
                false,
            ),
            _ => None,
        };
        let pixels = plain.as_ref().unwrap_or(&self.pixels);

        let (sw, sh) = pixels.size();
        if sw == 0 || sh == 0 {
            return None;
        }

        let source = pixels.as_bytes();
        let stride = size.0 as usize * CHANNELS;
        let canvas = doc.edit().pixels_mut();

        for y in rect.rows() {
            for x in rect.cols() {
                let (u, v) = self.xform.to_local(x as f32 + 0.5, y as f32 + 0.5);
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let sx = ((u * sw as f32) as usize).min(sw as usize - 1);
                let sy = ((v * sh as f32) as usize).min(sh as usize - 1);
                let s = (sy * sw as usize + sx) * CHANNELS;
                let mut src: [u8; 4] = source[s..s + CHANNELS].try_into().unwrap();
                src[3] = fade(src[3], self.opacity);
                if src[3] == 0 {
                    continue;
                }

                let d = y as usize * stride + x as usize * CHANNELS;
                let under: [u8; 4] = canvas[d..d + CHANNELS].try_into().unwrap();
                canvas[d..d + CHANNELS].copy_from_slice(&over(under, src));
            }
        }

        Some(match self.lifted_from {
            Some(hole) => hole.union(rect),
            None => rect,
        })
    }
}

impl Floating {
    fn drawn_in(&self, rect: Rect, kind: ShapeKind, style: &ShapeStyle) -> Option<Rgba8> {
        let (cx, cy) = self.xform.centre();
        let inset = style.outline.map_or(0.0, |_| style.thickness / 2.0);
        let (w, h) = (
            (self.xform.width - style.thickness).max(1.0),
            (self.xform.height - style.thickness).max(1.0),
        );

        let place = tiny_skia::Transform::from_translate(-(rect.x0 as f32), -(rect.y0 as f32))
            .pre_translate(cx, cy)
            .pre_rotate(self.xform.rotation.to_degrees())
            .pre_translate(-self.xform.width / 2.0, -self.xform.height / 2.0)
            .pre_translate(inset, inset);

        shapes::render_placed(kind, style, rect.width(), rect.height(), (w, h), place)
    }
}

fn composite(doc: &mut Document, rect: Rect, src: &Rgba8, opacity: f32) {
    let stride = doc.size().0 as usize * CHANNELS;
    let (sw, _) = src.size();
    let source = src.as_bytes();
    let canvas = doc.edit().pixels_mut();

    for (row, y) in rect.rows().enumerate() {
        for (col, x) in rect.cols().enumerate() {
            let s = (row * sw as usize + col) * CHANNELS;
            let Ok(mut pixel) = <[u8; 4]>::try_from(&source[s..s + CHANNELS]) else {
                continue;
            };
            pixel[3] = fade(pixel[3], opacity);
            if pixel[3] == 0 {
                continue;
            }
            let d = y as usize * stride + x as usize * CHANNELS;
            let under: [u8; 4] = canvas[d..d + CHANNELS].try_into().unwrap();
            canvas[d..d + CHANNELS].copy_from_slice(&over(under, pixel));
        }
    }
}

fn side(v: f32) -> u32 {
    (v.round().max(1.0) as u32).min(MAX_DRAW)
}

fn block(v: u32) -> u32 {
    v.next_multiple_of(16).min(MAX_DRAW)
}

fn fade(alpha: u8, opacity: f32) -> u8 {
    if opacity >= 1.0 {
        return alpha;
    }
    (alpha as f32 * opacity.clamp(0.0, 1.0)).round() as u8
}

fn centre_of(points: &[(f32, f32)]) -> (f32, f32) {
    let (mut x0, mut y0) = (f32::MAX, f32::MAX);
    let (mut x1, mut y1) = (f32::MIN, f32::MIN);
    for (x, y) in points {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

fn over(under: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let sa = src[3] as f32 / 255.0;
    if sa >= 1.0 {
        return src;
    }
    let ua = under[3] as f32 / 255.0;
    let out_a = sa + ua * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mix = |s: u8, u: u8| {
        (((s as f32 * sa + u as f32 * ua * (1.0 - sa)) / out_a).round()).clamp(0.0, 255.0) as u8
    };
    [
        mix(src[0], under[0]),
        mix(src[1], under[1]),
        mix(src[2], under[2]),
        (out_a * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::shapes::ShapeKind;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn doc_of(fill: [u8; 4]) -> Document {
        let mut d = Document::blank_sized(8, 8, false);
        d.edit()
            .pixels_mut()
            .as_chunks_mut::<CHANNELS>()
            .0
            .iter_mut()
            .for_each(|p| *p = fill);
        d
    }

    fn at(doc: &Document, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * doc.size().0 as usize + x as usize) * CHANNELS;
        doc.pixels().as_bytes()[i..i + 4].try_into().unwrap()
    }

    fn shape_doc() -> Document {
        Document::blank_sized(100, 100, false)
    }

    #[test]
    fn a_small_bend_reuses_the_buffer_it_already_has() {
        let doc = shape_doc();
        let style = ShapeStyle {
            fill: None,
            outline: Some(RED),
            thickness: 4.0,
        };
        let mut curve = Floating::curve(&doc, CurveKind::Curve3, style, (10.0, 10.0), (80.0, 80.0));

        let first = curve.pixels.size();
        for step in 1..=4 {
            curve.bend(1, 45.0 + step as f32, 45.0);
            assert_eq!(
                curve.pixels.size(),
                first,
                "a {step} pixel bend resized the buffer"
            );
        }

        curve.bend(1, 45.0, 200.0);
        assert_ne!(curve.pixels.size(), first);
    }

    #[test]
    fn a_shape_can_be_given_bones_and_still_looks_like_itself() {
        let mut doc = shape_doc();
        let style = ShapeStyle {
            fill: Some(BLUE),
            outline: None,
            thickness: 4.0,
        };
        let mut shape = Floating::shape(&doc, ShapeKind::Circle, style, Rect::new(10, 10, 90, 90));

        assert!(shape.add_bones(), "a shape takes bones");
        assert!(shape.is_closed(), "and comes out as a loop");
        assert_eq!(shape.points().len(), crate::paint::curve::SHAPE_BONES);
        assert!(!shape.add_bones(), "and does not take them twice");

        shape.commit(&mut doc);
        assert_eq!(at(&doc, 50, 50), BLUE, "the middle should be filled");
        assert_eq!(at(&doc, 12, 12), [0, 0, 0, 0], "and the corner left alone");
    }

    #[test]
    fn bones_can_be_put_in_a_curve_and_taken_out() {
        let doc = shape_doc();
        let style = ShapeStyle {
            fill: None,
            outline: Some(RED),
            thickness: 4.0,
        };
        let mut curve = Floating::curve(&doc, CurveKind::Line, style, (10.0, 10.0), (90.0, 10.0));
        assert_eq!(curve.points().len(), 2);

        assert!(curve.add_point(50.0, 12.0, 8.0));
        assert_eq!(curve.points().len(), 3);
        assert!(
            (curve.points()[1].0 - 50.0).abs() < 0.01,
            "{:?}",
            curve.points()
        );

        assert!(!curve.add_point(50.0, 80.0, 8.0));
        assert_eq!(curve.points().len(), 3);

        assert!(curve.remove_point(1));
        assert_eq!(curve.points().len(), 2);
        assert!(!curve.remove_point(0), "a line needs both its ends");
    }

    #[test]
    fn a_bent_curve_can_take_a_bone_on_the_line_it_draws() {
        let doc = shape_doc();
        let style = ShapeStyle {
            fill: None,
            outline: Some(RED),
            thickness: 4.0,
        };
        let mut curve = Floating::curve(&doc, CurveKind::Curve4, style, (0.0, 0.0), (100.0, 0.0));
        curve.bend(0, 0.0, 1000.0);
        curve.bend(1, 0.0, 0.0);
        curve.bend(2, 100.0, 0.0);
        curve.bend(3, 100.0, 1000.0);

        assert!(curve.add_point(50.0, -125.0, 2.0));
        assert_eq!(curve.points().len(), 5);
        assert_eq!(curve.points()[2], (50.0, -125.0));
    }

    #[test]
    fn a_text_box_leaves_room_round_the_letters() {
        let doc = shape_doc();
        let style = crate::text::TextStyle::default();
        let slack = style.slack();
        let mut boxed = Floating::text(&doc, style, Rect::new(0, 0, 300, 80));
        for c in "gjpqy".chars() {
            boxed
                .text_box()
                .unwrap()
                .act(crate::text::Action::Insert(c));
        }
        boxed.redraw();

        let width = boxed.xform.width;
        boxed.redraw();
        boxed.redraw();
        assert_eq!(boxed.xform.width, width, "the box grew on every redraw");

        let (w, h) = boxed.pixels.size();
        assert!(
            w as f32 >= 300.0 - slack,
            "the box is about as wide as it was dragged"
        );
        let bytes = boxed.pixels.as_bytes();
        let bottom = (h - 1) as usize;
        let touching = (0..w as usize).any(|x| bytes[(bottom * w as usize + x) * 4 + 3] > 8);
        assert!(!touching, "the letters run into the bottom edge of the box");
    }

    #[test]
    fn a_rotated_shape_lands_turned_rather_than_upright() {
        let mut doc = shape_doc();
        let style = ShapeStyle {
            fill: Some(RED),
            outline: None,
            thickness: 4.0,
        };
        let mut shape =
            Floating::shape(&doc, ShapeKind::Rectangle, style, Rect::new(20, 20, 80, 80));
        shape.xform.rotation = std::f32::consts::FRAC_PI_4;
        shape.commit(&mut doc);

        assert_eq!(at(&doc, 50, 50), RED, "the middle is filled");
        assert_eq!(at(&doc, 22, 22)[3], 0, "the corner of the box is not");
    }

    #[test]
    fn rotating_does_not_change_how_thick_the_outline_is() {
        let ink = |rotation: f32| {
            let mut doc = shape_doc();
            let style = ShapeStyle {
                fill: None,
                outline: Some(RED),
                thickness: 6.0,
            };
            let mut shape =
                Floating::shape(&doc, ShapeKind::Rectangle, style, Rect::new(20, 20, 80, 80));
            shape.xform.rotation = rotation;
            shape.commit(&mut doc);
            doc.pixels()
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 8)
                .count()
        };
        let (upright, turned) = (ink(0.0), ink(std::f32::consts::FRAC_PI_4));
        let ratio = turned as f32 / upright as f32;
        assert!(
            (0.8..1.3).contains(&ratio),
            "outline changed weight when turned: {ratio}"
        );
    }

    #[test]
    fn lifting_takes_the_pixels_and_leaves_a_hole() {
        let mut d = doc_of(RED);
        let f = Floating::lift_masked(&mut d, Rect::new(2, 2, 5, 5), None).unwrap();

        assert_eq!(f.pixels.size(), (3, 3));
        assert_eq!(at(&d, 3, 3), [0, 0, 0, 0], "the source region is emptied");
        assert_eq!(at(&d, 0, 0), RED, "everything else is untouched");
    }

    #[test]
    fn committing_where_it_started_puts_it_back() {
        let mut d = doc_of(RED);
        let before = d.pixels().clone();
        let f = Floating::lift_masked(&mut d, Rect::new(2, 2, 5, 5), None).unwrap();
        f.commit(&mut d);
        assert_eq!(d.pixels().as_bytes(), before.as_bytes());
    }

    #[test]
    fn moving_a_selection_leaves_the_hole_behind() {
        let mut d = doc_of(RED);
        let mut f = Floating::lift_masked(&mut d, Rect::new(0, 0, 3, 3), None).unwrap();
        f.xform = f.xform.moved_by(5.0, 5.0);
        f.commit(&mut d);

        assert_eq!(at(&d, 1, 1), [0, 0, 0, 0], "where it was is still empty");
        assert_eq!(at(&d, 6, 6), RED, "and it landed where it was dragged");
    }

    #[test]
    fn the_reported_region_covers_both_the_hole_and_the_landing() {
        let mut d = doc_of(RED);
        let mut f = Floating::lift_masked(&mut d, Rect::new(0, 0, 2, 2), None).unwrap();
        f.xform = f.xform.moved_by(6.0, 6.0);
        let touched = f.commit(&mut d).unwrap();
        assert_eq!((touched.x0, touched.y0), (0, 0));
        assert_eq!((touched.x1, touched.y1), (8, 8));
    }

    #[test]
    fn a_placed_image_composites_with_its_own_alpha() {
        let mut d = doc_of(RED);
        let mut stamp = Rgba8::new(2, 2, BLUE);
        stamp.pixels_mut()[0..4].copy_from_slice(&[0, 0, 0, 0]);

        let f = Floating::place(&d, stamp, (1.0, 1.0));
        f.commit(&mut d);
        assert_eq!(
            at(&d, 0, 0),
            RED,
            "a transparent pixel must not punch through"
        );
        assert_eq!(at(&d, 1, 1), BLUE);
    }

    #[test]
    fn an_oversized_image_is_shrunk_to_fit() {
        let d = doc_of(RED);
        let huge = Rgba8::new(80, 40, BLUE);
        let f = Floating::place(&d, huge, (4.0, 4.0));
        assert!(
            f.xform.width <= 8.0 && f.xform.height <= 8.0,
            "{:?}",
            f.xform
        );
        assert!((f.xform.width / f.xform.height - 2.0).abs() < 0.01);
    }

    #[test]
    fn a_scaled_object_fills_its_box_without_gaps() {
        let mut d = doc_of(RED);
        let mut f = Floating::place(&d, Rgba8::new(2, 2, BLUE), (0.0, 0.0));
        f.xform = Xform {
            x: 0.0,
            y: 0.0,
            width: 8.0,
            height: 8.0,
            rotation: 0.0,
        };
        f.commit(&mut d);

        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(at(&d, x, y), BLUE, "gap at {x},{y}");
            }
        }
    }

    #[test]
    fn a_placed_image_is_not_recorded_as_a_lift() {
        let d = doc_of(RED);
        let f = Floating::place(&d, Rgba8::new(2, 2, BLUE), (4.0, 4.0));
        assert!(f.lifted_from.is_none());
        assert_eq!(f.label(), "Paste");
    }
}
