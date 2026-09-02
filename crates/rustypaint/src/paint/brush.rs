use crate::i18n;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Paint,
    Erase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    Max,
    Accumulate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Marker,
    Calligraphy,
    OilBrush,
    Watercolour,
    PixelPen,
    Pencil,
    Eraser,
    Crayon,
    SprayCan,
    Fill,
    Pipette,
    Select,
    Text,
    Shape,
}

pub const PANEL_ORDER: [Tool; 10] = [
    Tool::Marker,
    Tool::Calligraphy,
    Tool::OilBrush,
    Tool::Watercolour,
    Tool::PixelPen,
    Tool::Pencil,
    Tool::Eraser,
    Tool::Crayon,
    Tool::SprayCan,
    Tool::Fill,
];

#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub aspect: f32,
    pub angle: f32,
    pub feather: f32,
    pub grain: f32,
    pub scatter: f32,
    pub spacing: f32,
    pub dot: f32,
    pub build: Build,
    pub flow: f32,
}

impl Profile {
    const fn round(feather: f32, spacing: f32) -> Self {
        Self {
            aspect: 1.0,
            angle: 0.0,
            feather,
            grain: 0.0,
            scatter: 0.0,
            spacing,
            dot: 1.0,
            build: Build::Max,
            flow: 1.0,
        }
    }
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Marker => i18n::tool_marker(),
            Tool::Calligraphy => i18n::tool_calligraphy(),
            Tool::OilBrush => i18n::tool_oil_brush(),
            Tool::Watercolour => i18n::tool_watercolour(),
            Tool::PixelPen => i18n::tool_pixel_pen(),
            Tool::Pencil => i18n::tool_pencil(),
            Tool::Eraser => i18n::tool_eraser(),
            Tool::Crayon => i18n::tool_crayon(),
            Tool::SprayCan => i18n::tool_spray_can(),
            Tool::Fill => i18n::tool_fill(),
            Tool::Pipette => i18n::tool_pipette(),
            Tool::Select => i18n::tool_select(),
            Tool::Text => i18n::tool_text(),
            Tool::Shape => i18n::tool_shape(),
        }
    }

    pub fn mode(self) -> Mode {
        match self {
            Tool::Eraser => Mode::Erase,
            _ => Mode::Paint,
        }
    }

    pub fn profile(self) -> Option<Profile> {
        Some(match self {
            Tool::Marker | Tool::Eraser => Profile::round(1.0, 0.10),

            Tool::Calligraphy => Profile {
                aspect: 0.22,
                angle: -std::f32::consts::FRAC_PI_4,
                spacing: 0.05,
                ..Profile::round(0.8, 0.05)
            },

            Tool::OilBrush => Profile {
                grain: 0.55,
                ..Profile::round(1.6, 0.07)
            },

            Tool::Watercolour => Profile {
                grain: 0.18,
                build: Build::Accumulate,
                flow: 0.035,
                ..Profile::round(2.5, 0.08)
            },

            Tool::PixelPen => Profile::round(0.0, 0.34),

            Tool::Pencil => Profile {
                grain: 0.75,
                scatter: 0.30,
                build: Build::Accumulate,
                flow: 0.09,
                ..Profile::round(0.6, 0.07)
            },

            Tool::Crayon => Profile {
                grain: 0.95,
                scatter: 0.18,
                build: Build::Accumulate,
                flow: 0.16,
                ..Profile::round(1.0, 0.07)
            },

            Tool::SprayCan => Profile {
                dot: 0.07,
                build: Build::Accumulate,
                flow: 0.5,
                ..Profile::round(1.0, 0.5)
            },

            Tool::Fill | Tool::Pipette | Tool::Select | Tool::Text | Tool::Shape => return None,
        })
    }

    pub fn snaps_to_pixels(self) -> bool {
        matches!(self, Tool::PixelPen)
    }

    pub fn sprays(self) -> bool {
        matches!(self, Tool::SprayCan)
    }
}

pub const MIN_THICKNESS: f32 = 1.0;
pub const MAX_THICKNESS: f32 = 100.0;

#[derive(Debug, Clone, Copy)]
pub struct Brush {
    pub tool: Tool,
    pub thickness: f32,
    pub opacity: f32,
    pub colour: [u8; 4],
    pub tolerance: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            tool: Tool::Marker,
            thickness: 12.0,
            opacity: 1.0,
            colour: [0, 0, 0, 255],
            tolerance: 0.12,
        }
    }
}

impl Brush {
    pub fn radius(&self) -> f32 {
        self.thickness.clamp(MIN_THICKNESS, MAX_THICKNESS) / 2.0
    }

    pub fn stamp_radius(&self) -> f32 {
        (self.radius() * self.profile().dot).max(0.5)
    }

    pub fn profile(&self) -> Profile {
        self.tool.profile().unwrap_or(Profile::round(1.0, 0.1))
    }

    pub fn step(&self) -> f32 {
        (self.thickness * self.profile().spacing).max(1.0)
    }

    pub fn coverage_at(&self, cx: f32, cy: f32, px: f32, py: f32) -> u8 {
        let profile = self.profile();
        let r = self.stamp_radius();

        let (sin, cos) = profile.angle.sin_cos();
        let (dx, dy) = (px - cx, py - cy);
        let rx = dx * cos + dy * sin;
        let ry = (-dx * sin + dy * cos) / profile.aspect.max(0.01);
        let d = (rx * rx + ry * ry).sqrt();

        let mut coverage = if profile.feather <= 0.0 {
            if d <= r.max(0.5) { 1.0 } else { 0.0 }
        } else {
            ((r - d) / profile.feather + 0.5).clamp(0.0, 1.0)
        };
        if coverage <= 0.0 {
            return 0;
        }

        if profile.grain > 0.0 {
            coverage *= 1.0 - profile.grain * bump_at(px, py);
        }
        if profile.scatter > 0.0 {
            coverage *= 1.0 - profile.scatter * noise_at(px, py);
        }

        ((coverage * profile.flow).clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

fn bump() -> &'static [f32; BUMP_SIDE * BUMP_SIDE] {
    static BUMP: OnceLock<[f32; BUMP_SIDE * BUMP_SIDE]> = OnceLock::new();
    BUMP.get_or_init(|| {
        let mut out = [0.0; BUMP_SIDE * BUMP_SIDE];
        for (i, slot) in out.iter_mut().enumerate() {
            let (x, y) = (i % BUMP_SIDE, i / BUMP_SIDE);
            let height = 0.62 * value_noise(x, y, 4, 1013) + 0.38 * value_noise(x, y, 2, 7919);
            *slot = ((PIT - height).max(0.0) / (PIT * 0.5)).min(1.0);
        }
        out
    })
}

fn value_noise(x: usize, y: usize, cell: usize, salt: u64) -> f32 {
    let side = BUMP_SIDE / cell;
    let (fx, fy) = (x as f32 / cell as f32, y as f32 / cell as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(fx - x0 as f32), smooth(fy - y0 as f32));

    let at = |i: usize, j: usize| hash01(((j % side) * side + (i % side)) as u64 + salt);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = lerp(at(x0, y0), at(x0 + 1, y0), tx);
    let bottom = lerp(at(x0, y0 + 1), at(x0 + 1, y0 + 1), tx);
    lerp(top, bottom, ty)
}

const PIT: f32 = 0.34;

const BUMP_SIDE: usize = 64;

fn bump_at(x: f32, y: f32) -> f32 {
    let ix = (x.floor() as i64).rem_euclid(BUMP_SIDE as i64) as usize;
    let iy = (y.floor() as i64).rem_euclid(BUMP_SIDE as i64) as usize;
    bump()[iy * BUMP_SIDE + ix]
}

pub fn hash01(n: u64) -> f32 {
    let mut h = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 40) as f32 / 16_777_215.0
}

fn noise_at(x: f32, y: f32) -> f32 {
    let xi = x.floor() as i64 as u64;
    let yi = y.floor() as i64 as u64;
    let mut h = xi.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ yi.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 40) as f32 / 16_777_215.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brush(tool: Tool) -> Brush {
        Brush {
            tool,
            thickness: 20.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_soft_brush_is_solid_inside_and_empty_outside() {
        let b = brush(Tool::Marker);
        assert_eq!(b.coverage_at(0.0, 0.0, 0.0, 0.0), 255, "centre");
        assert_eq!(b.coverage_at(0.0, 0.0, 5.0, 0.0), 255, "well inside");
        assert_eq!(b.coverage_at(0.0, 0.0, 20.0, 0.0), 0, "well outside");

        let rim = b.coverage_at(0.0, 0.0, b.radius(), 0.0);
        assert!(rim > 0 && rim < 255, "rim coverage was {rim}");
    }

    #[test]
    fn the_pixel_pen_has_no_partial_coverage() {
        let b = Brush {
            tool: Tool::PixelPen,
            thickness: 5.0,
            ..Default::default()
        };
        for d in [0.0, 1.0, 2.0, 2.4, 2.6, 4.0, 10.0] {
            let c = b.coverage_at(0.0, 0.0, d, 0.0);
            assert!(
                c == 0 || c == 255,
                "coverage {c} at distance {d} is neither on nor off"
            );
        }
    }

    #[test]
    fn a_one_pixel_pen_still_marks_something() {
        let b = Brush {
            tool: Tool::PixelPen,
            thickness: 1.0,
            ..Default::default()
        };
        assert_eq!(b.coverage_at(0.0, 0.0, 0.0, 0.0), 255);
    }

    #[test]
    fn the_calligraphy_nib_is_narrow_across_and_wide_along() {
        let b = brush(Tool::Calligraphy);
        let r = b.radius();
        let (sin, cos) = b.profile().angle.sin_cos();
        let along = b.coverage_at(0.0, 0.0, cos * r * 0.9, sin * r * 0.9);
        let across = b.coverage_at(0.0, 0.0, -sin * r * 0.9, cos * r * 0.9);
        assert!(along > 0, "the nib should reach along its own axis");
        assert_eq!(across, 0, "and be thin across it");
    }

    #[test]
    fn textured_media_break_the_stroke_up() {
        for tool in [Tool::Crayon, Tool::Pencil, Tool::OilBrush] {
            let b = brush(tool);
            let inside: Vec<u8> = (-5..5)
                .flat_map(|y| (-5..5).map(move |x| (x as f32, y as f32)))
                .map(|(x, y)| b.coverage_at(0.0, 0.0, x, y))
                .collect();

            let high = *inside.iter().max().unwrap();
            assert!(high > 0, "{tool:?} covered nothing at all");
            let broken = inside.iter().filter(|c| **c < high).count();
            assert!(
                broken * 7 >= inside.len(),
                "{tool:?} left only {broken} of {} samples below full",
                inside.len()
            );
        }
    }

    #[test]
    fn a_smooth_brush_really_is_smooth() {
        let b = brush(Tool::Marker);
        let inside: Vec<u8> = (0..24)
            .map(|i| b.coverage_at(0.0, 0.0, i as f32 * 0.25 - 3.0, 0.0))
            .collect();
        assert!(
            inside.iter().all(|c| *c == 255),
            "marker interior varied: {inside:?}"
        );
    }

    #[test]
    fn the_grain_is_sparse_pits_rather_than_an_even_wash() {
        let table = bump();
        let pits: Vec<f32> = table.iter().copied().filter(|v| *v > 0.001).collect();
        let fraction = pits.len() as f32 / table.len() as f32;
        assert!(
            (0.10..0.30).contains(&fraction),
            "{:.3} of the table is pitted, which is not the sparse grain it should be",
            fraction
        );

        let mean = pits.iter().sum::<f32>() / pits.len() as f32;
        assert!(
            mean > 0.3,
            "the pits average {mean:.3} deep, which would barely show"
        );
        assert!(
            table.iter().any(|v| *v > 0.95),
            "nothing in it reaches full depth"
        );
    }

    #[test]
    fn grain_is_fixed_to_the_canvas_not_the_stamp() {
        let b = brush(Tool::Crayon);
        let from_left = b.coverage_at(-3.0, 0.0, 0.0, 0.0);
        let from_right = b.coverage_at(3.0, 0.0, 0.0, 0.0);
        assert_eq!(from_left, from_right);
    }

    #[test]
    fn wet_media_build_up_and_dry_media_do_not() {
        assert_eq!(Tool::Marker.profile().unwrap().build, Build::Max);
        assert_eq!(Tool::PixelPen.profile().unwrap().build, Build::Max);
        assert_eq!(
            Tool::Watercolour.profile().unwrap().build,
            Build::Accumulate
        );
        assert_eq!(Tool::Crayon.profile().unwrap().build, Build::Accumulate);
    }

    #[test]
    fn watercolour_lays_down_very_little_at_a_time() {
        let b = brush(Tool::Watercolour);
        assert!(
            b.coverage_at(0.0, 0.0, 0.0, 0.0) < 40,
            "one pass should be faint"
        );
    }

    #[test]
    fn the_fill_and_the_pipette_have_no_stamp() {
        assert!(Tool::Fill.profile().is_none());
        assert!(Tool::Pipette.profile().is_none());
        assert!(Tool::Marker.profile().is_some());
    }

    #[test]
    fn stamp_spacing_never_collapses_to_zero() {
        for thickness in [1.0, 2.0, 50.0, 100.0] {
            for tool in PANEL_ORDER {
                let b = Brush {
                    tool,
                    thickness,
                    ..Default::default()
                };
                assert!(
                    b.step() >= 1.0,
                    "{tool:?} step was {} at {thickness}",
                    b.step()
                );
            }
        }
    }
}
