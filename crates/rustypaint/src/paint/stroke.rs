use super::brush::{Brush, Build, Mode};
use crate::doc::rect::{Bounds, Rect};
use crate::doc::{Document, Rgba8, image::CHANNELS};

const DOTS_PER_PUFF: usize = 12;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mirror {
    pub horizontal: bool,
    pub vertical: bool,
}

pub struct Stroke {
    brush: Brush,
    mirror: Mirror,
    backup: Rgba8,
    coverage: Vec<u8>,
    size: (u32, u32),
    touched: Bounds,
    dirty: Bounds,
    last: Option<(f32, f32)>,
    residue: f32,
    puffs: u64,
    trail: (Option<Cell>, Option<Cell>),
}

type Cell = (i64, i64);

impl Stroke {
    pub fn begin_with_mirror(brush: Brush, doc: &Document, x: f32, y: f32, mirror: Mirror) -> Self {
        let size = doc.size();
        let mut stroke = Self {
            brush,
            mirror,
            backup: doc.pixels().clone(),
            coverage: vec![0; size.0 as usize * size.1 as usize],
            size,
            touched: Bounds::default(),
            dirty: Bounds::default(),
            last: None,
            residue: 0.0,
            puffs: 0,
            trail: (None, None),
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

        if !self.brush.drops_corners() {
            self.stamp_mirrored(x, y);
            return;
        }

        let cell = (x.floor() as i64, y.floor() as i64);
        if self.trail.1 == Some(cell) {
            return;
        }
        self.stamp_mirrored(x, y);

        // A corner is only known once the pixel after it arrives, so it is laid down and taken
        // back rather than held while the pointer waits somewhere else.
        if let (Some(before), Some(middle)) = self.trail
            && corners(before, middle, cell)
        {
            self.unstamp(middle);
            self.trail.1 = Some(cell);
            return;
        }
        self.trail = (self.trail.1, Some(cell));
    }

    fn stamp_mirrored(&mut self, x: f32, y: f32) {
        let (points, count) = self.mirrors(x, y);
        for point in points.into_iter().take(count) {
            self.stamp_at(point.0, point.1);
        }
    }

    fn unstamp(&mut self, cell: Cell) {
        let (points, count) = self.mirrors(cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
        for (x, y) in points.into_iter().take(count) {
            let Some(rect) = Rect::around(x, y, 0.1, self.size.0, self.size.1) else {
                continue;
            };
            let index = rect.y0 as usize * self.size.0 as usize + rect.x0 as usize;
            if std::mem::replace(&mut self.coverage[index], 0) != 0 {
                self.dirty.add(rect);
            }
        }
    }

    fn mirrors(&self, x: f32, y: f32) -> ([(f32, f32); 4], usize) {
        let mirrored_x = self.mirror.horizontal.then_some(self.size.0 as f32 - x);
        let mirrored_y = self.mirror.vertical.then_some(self.size.1 as f32 - y);
        let candidates = [
            (x, y),
            (mirrored_x.unwrap_or(x), y),
            (x, mirrored_y.unwrap_or(y)),
            (mirrored_x.unwrap_or(x), mirrored_y.unwrap_or(y)),
        ];
        let mut points = [(0.0, 0.0); 4];
        let mut count = 0;
        for point in candidates {
            if !points[..count].contains(&point) {
                points[count] = point;
                count += 1;
            }
        }
        (points, count)
    }

    fn stamp_at(&mut self, x: f32, y: f32) {
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
        let opacity = self.brush.opacity().clamp(0.0, 1.0);
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

// A pixel between two that already touch diagonally is the doubled corner of a thin line.
fn corners(before: Cell, middle: Cell, after: Cell) -> bool {
    let touches = |a: Cell, b: Cell| (a.0 - b.0).abs() + (a.1 - b.1).abs() == 1;
    touches(before, middle)
        && touches(after, middle)
        && (before.0 - after.0).abs() == 1
        && (before.1 - after.1).abs() == 1
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
        let mut b = Brush {
            tool: Tool::PixelPen,
            colour: [255, 0, 0, 255],
            ..Default::default()
        };
        b.set_thickness(1.0);
        b
    }

    #[test]
    fn a_stamp_lands_where_it_was_put() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 8.5, 4.5, Mirror::default());
        s.flush(&mut d);
        assert_eq!(at(&d, 8, 4), [255, 0, 0, 255]);
        assert_eq!(at(&d, 0, 0), [0, 0, 0, 0], "elsewhere is still empty");
    }

    #[test]
    fn mirror_axes_repeat_a_stamp_without_changing_its_undo_shape() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(
            red(),
            &d,
            3.5,
            4.5,
            Mirror {
                horizontal: true,
                vertical: true,
            },
        );
        s.flush(&mut d);

        for (x, y) in [(3, 4), (12, 4), (3, 11), (12, 11)] {
            assert_eq!(at(&d, x, y), [255, 0, 0, 255], "missing mirrored pixel");
        }
        assert_eq!(at(&d, 4, 4), [0, 0, 0, 0]);
        assert_eq!(s.touched().unwrap().width(), 10);
        assert_eq!(s.touched().unwrap().height(), 8);
    }

    #[test]
    fn a_dragged_stroke_leaves_no_gaps() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 1.5, 8.5, Mirror::default());
        s.extend(14.5, 8.5);
        s.flush(&mut d);
        for x in 1..=14 {
            assert_eq!(at(&d, x, 8), [255, 0, 0, 255], "gap at x={x}");
        }
    }

    #[test]
    fn a_staircase_keeps_one_pixel_at_every_corner() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 2.5, 2.5, Mirror::default());
        for step in 1..=5 {
            s.extend(2.5 + step as f32, 1.5 + step as f32);
            s.extend(2.5 + step as f32, 2.5 + step as f32);
        }
        s.flush(&mut d);

        let painted = |x: u32, y: u32| at(&d, x, y) == [255, 0, 0, 255];
        for step in 0..=5 {
            assert!(
                painted(2 + step, 2 + step),
                "the diagonal itself is missing"
            );
        }
        for step in 1..=5 {
            assert!(
                !painted(2 + step, 1 + step),
                "the corner beside the diagonal was kept"
            );
        }
    }

    #[test]
    fn a_corner_stays_when_pixel_perfect_is_turned_off() {
        let mut blunt = red();
        blunt.set_pixel_perfect(false);

        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(blunt, &d, 2.5, 2.5, Mirror::default());
        s.extend(3.5, 2.5);
        s.extend(3.5, 3.5);
        s.flush(&mut d);

        assert_eq!(at(&d, 3, 2), [255, 0, 0, 255], "the corner should be kept");
    }

    #[test]
    fn a_dropped_corner_goes_back_to_what_was_under_it() {
        let mut d = doc(false);
        let mut under = Brush {
            tool: Tool::Marker,
            colour: [0, 0, 255, 255],
            ..red()
        };
        under.set_thickness(1.0);
        let mut first = Stroke::begin_with_mirror(under, &d, 3.5, 2.5, Mirror::default());
        first.flush(&mut d);
        let blue = at(&d, 3, 2);

        let mut s = Stroke::begin_with_mirror(red(), &d, 2.5, 2.5, Mirror::default());
        s.extend(3.5, 2.5);
        s.extend(3.5, 3.5);
        s.flush(&mut d);

        assert_eq!(at(&d, 3, 2), blue, "the corner was not put back");
    }

    #[test]
    fn the_pixel_pen_lays_down_a_square() {
        let mut d = doc(false);
        let mut square = red();
        square.set_thickness(5.0);
        let mut s = Stroke::begin_with_mirror(square, &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);

        for (x, y) in [(6, 6), (10, 10), (6, 10), (10, 6)] {
            assert_eq!(at(&d, x, y), [255, 0, 0, 255], "the corner {x},{y} is bare");
        }
        assert_eq!(at(&d, 5, 8), [0, 0, 0, 0], "and it stops at its own width");
    }

    #[test]
    fn overlapping_passes_do_not_darken_within_one_stroke() {
        let mut d = doc(false);
        let mut half = red();
        half.set_opacity(0.5);
        let mut s = Stroke::begin_with_mirror(half, &d, 8.5, 8.5, Mirror::default());
        for _ in 0..12 {
            s.extend(8.5, 8.5);
            s.extend(8.6, 8.5);
        }
        s.flush(&mut d);

        let once = {
            let mut d2 = doc(false);
            let mut s2 = Stroke::begin_with_mirror(half, &d2, 8.5, 8.5, Mirror::default());
            s2.flush(&mut d2);
            at(&d2, 8, 8)
        };
        assert_eq!(at(&d, 8, 8), once, "a stroke crossing itself changed shade");
    }

    #[test]
    fn the_eraser_clears_alpha_on_a_transparent_canvas() {
        let mut d = doc(true);
        let mut paint = Stroke::begin_with_mirror(red(), &d, 8.5, 8.5, Mirror::default());
        paint.flush(&mut d);
        assert_eq!(at(&d, 8, 8)[3], 255);

        let mut rubber = Brush {
            tool: Tool::Eraser,
            ..red()
        };
        rubber.set_thickness(4.0);
        let mut s = Stroke::begin_with_mirror(rubber, &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);
        assert_eq!(at(&d, 8, 8)[3], 0, "pixel should be fully transparent");
    }

    #[test]
    fn the_eraser_never_paints_white_it_only_removes() {
        let mut d = doc(false);
        let mut paint = Stroke::begin_with_mirror(red(), &d, 8.5, 8.5, Mirror::default());
        paint.flush(&mut d);

        let mut rubber = Brush {
            tool: Tool::Eraser,
            ..red()
        };
        rubber.set_thickness(4.0);
        let mut s = Stroke::begin_with_mirror(rubber, &d, 8.5, 8.5, Mirror::default());
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
    fn a_soft_eraser_leaves_a_fading_edge() {
        let mut d = doc(true);
        let mut ink = Brush {
            tool: Tool::Marker,
            colour: [255, 0, 0, 255],
            ..Default::default()
        };
        ink.set_thickness(100.0);
        let mut paint = Stroke::begin_with_mirror(ink, &d, 8.0, 8.0, Mirror::default());
        paint.flush(&mut d);

        let mut rubber = Brush {
            tool: Tool::Eraser,
            ..Default::default()
        };
        rubber.set_thickness(12.0);
        rubber.set_antialiased(true);
        rubber.set_hardness(0.0);
        let mut s = Stroke::begin_with_mirror(rubber, &d, 8.0, 8.0, Mirror::default());
        s.flush(&mut d);

        let alpha: Vec<u8> = (8..14).map(|x| at(&d, x, 8)[3]).collect();
        assert!(
            alpha[0] < 16,
            "the centre should be all but gone: {alpha:?}"
        );
        assert!(
            alpha.windows(2).all(|w| w[0] < w[1]),
            "alpha should climb outwards: {alpha:?}"
        );
        assert!(alpha[5] > 200, "and barely touch the rim: {alpha:?}");
    }

    #[test]
    fn only_the_touched_region_is_reported() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);
        let touched = s.touched().unwrap();
        assert!(
            touched.width() <= 3 && touched.height() <= 3,
            "{touched:?} is too broad"
        );
    }
}
