use super::brush::{Brush, Build, Mode};
use crate::doc::rect::{Bounds, Rect};
use crate::doc::{Document, Rgba8, image::CHANNELS};

const DOTS_PER_PUFF: usize = 12;

pub struct Stroke {
    brush: Brush,
    backup: Rgba8,
    coverage: Vec<u8>,
    size: (u32, u32),
    touched: Bounds,
    dirty: Bounds,
    last: Option<(f32, f32)>,
    residue: f32,
    puffs: u64,
}

impl Stroke {
    pub fn begin(brush: Brush, doc: &Document, x: f32, y: f32) -> Self {
        let size = doc.size();
        let mut stroke = Self {
            brush,
            backup: doc.pixels().clone(),
            coverage: vec![0; size.0 as usize * size.1 as usize],
            size,
            touched: Bounds::default(),
            dirty: Bounds::default(),
            last: None,
            residue: 0.0,
            puffs: 0,
        };
        stroke.stamp(x, y);
        stroke
    }

    pub fn extend(&mut self, x: f32, y: f32) {
        let Some((lx, ly)) = self.last else {
            self.stamp(x, y);
            return;
        };

        let (dx, dy) = (x - lx, y - ly);
        let distance = (dx * dx + dy * dy).sqrt();
        if distance <= f32::EPSILON {
            return;
        }

        let step = self.brush.step();
        let mut travelled = step - self.residue;
        while travelled <= distance {
            let t = travelled / distance;
            self.stamp(lx + dx * t, ly + dy * t);
            travelled += step;
        }
        self.residue = distance - (travelled - step);
        self.last = Some((x, y));
    }

    fn stamp(&mut self, x: f32, y: f32) {
        let (x, y) = if self.brush.tool.snaps_to_pixels() {
            (x.floor() + 0.5, y.floor() + 0.5)
        } else {
            (x, y)
        };
        self.last = Some((x, y));

        let radius = self.brush.stamp_radius() + 1.0;
        let Some(box_) = Rect::around(x, y, radius, self.size.0, self.size.1) else {
            return;
        };

        let build = self.brush.profile().build;
        let mut changed = Bounds::default();
        for py in box_.rows() {
            let row = py as usize * self.size.0 as usize;
            for px in box_.cols() {
                let c = self
                    .brush
                    .coverage_at(x, y, px as f32 + 0.5, py as f32 + 0.5);
                if c == 0 {
                    continue;
                }
                let slot = &mut self.coverage[row + px as usize];
                let next = match build {
                    Build::Max => c.max(*slot),
                    Build::Accumulate => {
                        let have = *slot as u32;
                        (have + c as u32 * (255 - have) / 255).min(255) as u8
                    }
                };
                if next != *slot {
                    *slot = next;
                    changed.add(Rect::new(px, py, px + 1, py + 1));
                }
            }
        }

        if let Some(rect) = changed.get() {
            self.touched.add(rect);
            self.dirty.add(rect);
        }
    }

    pub fn puff(&mut self, x: f32, y: f32) {
        use std::f32::consts::TAU;

        let radius = self.brush.radius();
        for _ in 0..DOTS_PER_PUFF {
            self.puffs = self.puffs.wrapping_add(1);
            let angle = super::brush::hash01(self.puffs.wrapping_mul(2)) * TAU;
            let away = super::brush::hash01(self.puffs.wrapping_mul(2).wrapping_add(1)).sqrt();
            let r = away * radius;
            self.stamp(x + angle.cos() * r, y + angle.sin() * r);
        }
        self.last = Some((x, y));
        self.residue = 0.0;
    }

    pub fn flush(&mut self, doc: &mut Document) -> Option<Rect> {
        let rect = self.dirty.take()?;
        self.composite(doc, rect);
        Some(rect)
    }

    fn composite(&self, doc: &mut Document, rect: Rect) {
        let width = self.size.0 as usize;
        let opacity = self.brush.opacity.clamp(0.0, 1.0);
        let erase = self.brush.tool.mode() == Mode::Erase;
        let colour = self.brush.colour;
        let backup = self.backup.as_bytes();
        let pixels = doc.edit().pixels_mut();

        for py in rect.rows() {
            for px in rect.cols() {
                let index = py as usize * width + px as usize;
                let a = (self.coverage[index] as f32 / 255.0) * opacity;
                let i = index * CHANNELS;
                let under: [u8; 4] = backup[i..i + CHANNELS].try_into().unwrap();

                let out = if erase {
                    let mut px = under;
                    px[3] = (under[3] as f32 * (1.0 - a)).round() as u8;
                    px
                } else {
                    over(under, colour, a)
                };
                pixels[i..i + CHANNELS].copy_from_slice(&out);
            }
        }
    }

    pub fn touched(&self) -> Option<Rect> {
        self.touched.get()
    }

    pub fn backup(&self) -> &Rgba8 {
        &self.backup
    }

    pub fn label(&self) -> &'static str {
        self.brush.tool.name()
    }
}

fn over(under: [u8; 4], src: [u8; 4], alpha: f32) -> [u8; 4] {
    let sa = alpha * (src[3] as f32 / 255.0);
    if sa <= 0.0 {
        return under;
    }
    let ua = under[3] as f32 / 255.0;
    let out_a = sa + ua * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mix = |s: u8, u: u8| {
        let v = (s as f32 * sa + u as f32 * ua * (1.0 - sa)) / out_a;
        v.round().clamp(0.0, 255.0) as u8
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
    use crate::paint::brush::Tool;

    fn doc(transparent: bool) -> Document {
        Document::blank_sized(16, 16, transparent)
    }

    fn at(doc: &Document, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * doc.size().0 as usize + x as usize) * CHANNELS;
        doc.pixels().as_bytes()[i..i + 4].try_into().unwrap()
    }

    fn red() -> Brush {
        Brush {
            tool: Tool::PixelPen,
            thickness: 1.0,
            opacity: 1.0,
            colour: [255, 0, 0, 255],
            ..Default::default()
        }
    }

    #[test]
    fn a_stamp_lands_where_it_was_put() {
        let mut d = doc(false);
        let mut s = Stroke::begin(red(), &d, 8.5, 4.5);
        s.flush(&mut d);
        assert_eq!(at(&d, 8, 4), [255, 0, 0, 255]);
        assert_eq!(at(&d, 0, 0), [0, 0, 0, 0], "elsewhere is still empty");
    }

    #[test]
    fn a_dragged_stroke_leaves_no_gaps() {
        let mut d = doc(false);
        let mut s = Stroke::begin(red(), &d, 1.5, 8.5);
        s.extend(14.5, 8.5);
        s.flush(&mut d);
        for x in 1..=14 {
            assert_eq!(at(&d, x, 8), [255, 0, 0, 255], "gap at x={x}");
        }
    }

    #[test]
    fn overlapping_passes_do_not_darken_within_one_stroke() {
        let mut d = doc(false);
        let half = Brush {
            opacity: 0.5,
            ..red()
        };
        let mut s = Stroke::begin(half, &d, 8.5, 8.5);
        for _ in 0..12 {
            s.extend(8.5, 8.5);
            s.extend(8.6, 8.5);
        }
        s.flush(&mut d);

        let once = {
            let mut d2 = doc(false);
            let mut s2 = Stroke::begin(half, &d2, 8.5, 8.5);
            s2.flush(&mut d2);
            at(&d2, 8, 8)
        };
        assert_eq!(at(&d, 8, 8), once, "a stroke crossing itself changed shade");
    }

    #[test]
    fn the_eraser_clears_alpha_on_a_transparent_canvas() {
        let mut d = doc(true);
        let mut paint = Stroke::begin(red(), &d, 8.5, 8.5);
        paint.flush(&mut d);
        assert_eq!(at(&d, 8, 8)[3], 255);

        let rubber = Brush {
            tool: Tool::Eraser,
            thickness: 4.0,
            ..red()
        };
        let mut s = Stroke::begin(rubber, &d, 8.5, 8.5);
        s.flush(&mut d);
        assert_eq!(at(&d, 8, 8)[3], 0, "pixel should be fully transparent");
    }

    #[test]
    fn the_eraser_never_paints_white_it_only_removes() {
        let mut d = doc(false);
        let mut paint = Stroke::begin(red(), &d, 8.5, 8.5);
        paint.flush(&mut d);

        let rubber = Brush {
            tool: Tool::Eraser,
            thickness: 4.0,
            ..red()
        };
        let mut s = Stroke::begin(rubber, &d, 8.5, 8.5);
        s.flush(&mut d);
        assert_eq!(
            at(&d, 8, 8),
            [255, 0, 0, 0],
            "alpha gone, no white painted in"
        );

        d.set_transparent(true);
        assert_eq!(at(&d, 8, 8)[3], 0, "and it is genuinely see-through now");
    }

    #[test]
    fn only_the_touched_region_is_reported() {
        let mut d = doc(false);
        let mut s = Stroke::begin(red(), &d, 8.5, 8.5);
        s.flush(&mut d);
        let touched = s.touched().unwrap();
        assert!(
            touched.width() <= 3 && touched.height() <= 3,
            "{touched:?} is too broad"
        );
    }
}
