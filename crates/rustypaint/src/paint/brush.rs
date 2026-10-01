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
    Blur,
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
    pub square: bool,
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
            square: false,
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
    pub const COUNT: usize = Tool::Blur as usize + 1;

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
            Tool::Blur => i18n::blur_box(),
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

            Tool::Fill | Tool::Pipette | Tool::Select | Tool::Text | Tool::Shape | Tool::Blur => {
                return None;
            }
        })
    }

    pub fn snaps_to_pixels(self) -> bool {
        matches!(self, Tool::PixelPen)
    }

    pub fn edge_is_tunable(self) -> bool {
        matches!(self, Tool::Eraser)
    }

    pub fn sprays(self) -> bool {
        matches!(self, Tool::SprayCan)
    }
}

pub const MIN_THICKNESS: f32 = 1.0;
pub const MAX_THICKNESS: f32 = 200.0;
// Where the slider stops is not where the brush does. The field takes anything up to a brush as
// wide as the largest canvas, past which the extra size has nowhere to land.
pub const THICKNESS_CEILING: f32 = 20_000.0;

// What a tool keeps to itself rather than handing on to whichever tool is picked next.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub thickness: f32,
    pub opacity: f32,
    pub hardness: f32,
    pub antialiased: bool,
    pub square_tip: bool,
    pub pixel_perfect: bool,
    pub stabilizer: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thickness: 12.0,
            opacity: 1.0,
            hardness: 1.0,
            antialiased: false,
            square_tip: false,
            pixel_perfect: true,
            stabilizer: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Brush {
    pub tool: Tool,
    pub colour: [u8; 4],
    pub tolerance: f32,
    pub settings: [Settings; Tool::COUNT],
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            tool: Tool::Marker,
            colour: [0, 0, 0, 255],
            tolerance: 0.12,
            settings: [Settings::default(); Tool::COUNT],
        }
    }
}

impl Brush {
    fn current(&self) -> &Settings {
        &self.settings[self.tool as usize]
    }

    fn current_mut(&mut self) -> &mut Settings {
        &mut self.settings[self.tool as usize]
    }

    pub fn thickness(&self) -> f32 {
        self.current().thickness
    }

    pub fn set_thickness(&mut self, thickness: f32) {
        self.current_mut().thickness = thickness.clamp(MIN_THICKNESS, THICKNESS_CEILING);
    }

    pub fn opacity(&self) -> f32 {
        self.current().opacity
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.current_mut().opacity = opacity.clamp(0.0, 1.0);
    }

    pub fn hardness(&self) -> f32 {
        self.current().hardness
    }

    pub fn set_hardness(&mut self, hardness: f32) {
        self.current_mut().hardness = hardness.clamp(0.0, 1.0);
    }

    pub fn antialiased(&self) -> bool {
        self.current().antialiased
    }

    pub fn set_antialiased(&mut self, antialiased: bool) {
        self.current_mut().antialiased = antialiased;
    }

    pub fn stabilizer(&self) -> f32 {
        self.current().stabilizer
    }

    pub fn set_stabilizer(&mut self, stabilizer: f32) {
        self.current_mut().stabilizer = stabilizer.clamp(0.0, 1.0);
    }

    // How far the brush closes on the pointer with each sample. Full strength still leaves some of
    // it, so a stroke can always catch up with the hand that drew it.
    pub fn follow(&self) -> f32 {
        (1.0 - self.stabilizer()).max(0.05)
    }

    pub fn square_tip(&self) -> bool {
        self.current().square_tip
    }

    pub fn set_square_tip(&mut self, square_tip: bool) {
        self.current_mut().square_tip = square_tip;
    }

    pub fn pixel_perfect(&self) -> bool {
        self.current().pixel_perfect
    }

    pub fn set_pixel_perfect(&mut self, pixel_perfect: bool) {
        self.current_mut().pixel_perfect = pixel_perfect;
    }

    // Dropping a corner means dropping a whole stamp, so it only reads as a thin line while the
    // tip covers one pixel.
    pub fn drops_corners(&self) -> bool {
        self.tool.snaps_to_pixels() && self.pixel_perfect() && self.stamp_radius() <= 0.5
    }

    pub fn radius(&self) -> f32 {
        self.thickness().clamp(MIN_THICKNESS, THICKNESS_CEILING) / 2.0
    }

    pub fn stamp_radius(&self) -> f32 {
        (self.radius() * self.profile().dot).max(0.5)
    }

    pub fn profile(&self) -> Profile {
        let profile = self.tool.profile().unwrap_or(Profile::round(1.0, 0.1));
        match self.tool.snaps_to_pixels() {
            true => Profile {
                square: self.square_tip(),
                ..profile
            },
            false => profile,
        }
    }

    pub fn step(&self) -> f32 {
        (self.thickness() * self.profile().spacing).max(1.0)
    }

    #[cfg(test)]
    pub fn coverage_at(&self, cx: f32, cy: f32, px: f32, py: f32) -> u8 {
        let tip = self.tip();
        let d = tip.length(tip.local(px - cx, py - cy));
        tip.coverage(d, px.floor() as i64, py.floor() as i64)
    }

    pub fn tip(&self) -> Tip {
        let profile = self.profile();
        let r = self.stamp_radius();
        let (sin, cos) = profile.angle.sin_cos();
        Tip {
            sin,
            cos,
            aspect: profile.aspect.max(0.01),
            square: profile.square,
            edge: self.edge(profile, r),
            grain: profile.grain,
            scatter: profile.scatter,
            flow: profile.flow,
            bump: bump(),
        }
    }

    // Hardness draws the solid core in towards the centre while the rim stays a half pixel past
    // the stamp radius, so softening a stamp never widens it. Other tools keep a fixed-width edge.
    fn edge(&self, profile: Profile, r: f32) -> Edge {
        if !self.tool.edge_is_tunable() {
            return if profile.feather <= 0.0 {
                Edge::Hard(r.max(0.5))
            } else {
                Edge::Feathered {
                    radius: r,
                    feather: profile.feather,
                }
            };
        }
        if !self.antialiased() {
            return Edge::Hard(r.max(0.5));
        }

        let hardness = self.hardness().clamp(0.0, 1.0);
        Edge::Tunable {
            core: hardness * (r - 0.5).max(0.0),
            rim: r + 0.5,
            hardness,
        }
    }
}

// A brush worked out once per stroke. Distances are in the tip's own frame, turned and squashed so
// the tip is round there, or square for a square pixel pen.
#[derive(Clone, Copy)]
pub struct Tip {
    sin: f32,
    cos: f32,
    aspect: f32,
    square: bool,
    edge: Edge,
    grain: f32,
    scatter: f32,
    flow: f32,
    bump: &'static [f32; BUMP_SIDE * BUMP_SIDE],
}

#[derive(Clone, Copy)]
pub struct Sweep {
    tip: Tip,
    from: (f32, f32),
    along: (f32, f32),
    inverse: f32,
}

impl Sweep {
    // Closest approach to the centre of pixel (x, y), in the tip's frame.
    #[inline]
    pub fn distance(&self, x: u32, y: u32) -> f32 {
        let (dx, dy) = (x as f32 + 0.5 - self.from.0, y as f32 + 0.5 - self.from.1);
        if !self.tip.is_round() {
            return self
                .tip
                .distance_to_segment(self.tip.local(dx, dy), self.along);
        }
        let t = ((dx * self.along.0 + dy * self.along.1) * self.inverse).clamp(0.0, 1.0);
        let (ex, ey) = (dx - self.along.0 * t, dy - self.along.1 * t);
        (ex * ex + ey * ey).sqrt()
    }
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    Hard(f32),
    Feathered { radius: f32, feather: f32 },
    Tunable { core: f32, rim: f32, hardness: f32 },
}

impl Edge {
    #[inline]
    fn at(self, d: f32) -> f32 {
        match self {
            Edge::Hard(radius) => {
                if d <= radius {
                    1.0
                } else {
                    0.0
                }
            }
            Edge::Feathered { radius, feather } => ((radius - d) / feather + 0.5).clamp(0.0, 1.0),
            Edge::Tunable {
                core,
                rim,
                hardness,
            } => {
                let t = ((rim - d) / (rim - core)).clamp(0.0, 1.0);
                t + (1.0 - hardness) * (t * t * (3.0 - 2.0 * t) - t)
            }
        }
    }

    fn reach(self) -> f32 {
        match self {
            Edge::Hard(radius) => radius,
            Edge::Feathered { radius, feather } => radius + feather / 2.0,
            Edge::Tunable { rim, .. } => rim,
        }
    }

    fn solid(self) -> f32 {
        match self {
            Edge::Hard(radius) => radius,
            Edge::Feathered { radius, feather } => radius - feather / 2.0,
            Edge::Tunable { core, .. } => core,
        }
    }
}

impl Tip {
    pub fn sweep(&self, from: (f32, f32), to: (f32, f32)) -> Sweep {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let along = if self.is_round() {
            (dx, dy)
        } else {
            self.local(dx, dy)
        };
        let length = along.0 * along.0 + along.1 * along.1;
        Sweep {
            tip: *self,
            from,
            along,
            inverse: if length > 0.0 { 1.0 / length } else { 0.0 },
        }
    }

    pub fn local(&self, dx: f32, dy: f32) -> (f32, f32) {
        (
            dx * self.cos + dy * self.sin,
            (-dx * self.sin + dy * self.cos) / self.aspect,
        )
    }

    pub fn length(&self, (x, y): (f32, f32)) -> f32 {
        if self.square {
            x.abs().max(y.abs())
        } else {
            (x * x + y * y).sqrt()
        }
    }

    // Distance from `q` to the segment from the origin to `v`, both in the tip's frame.
    pub fn distance_to_segment(&self, q: (f32, f32), v: (f32, f32)) -> f32 {
        let along = v.0 * v.0 + v.1 * v.1;
        if along <= 0.0 {
            return self.length(q);
        }
        let at = |t: f32| self.length((q.0 - v.0 * t, q.1 - v.1 * t));
        if !self.square {
            return at(((q.0 * v.0 + q.1 * v.1) / along).clamp(0.0, 1.0));
        }
        // The larger of two V shapes is least where one bottoms out, they cross, or at an end.
        let mut best = at(0.0).min(at(1.0));
        let mut consider = |num: f32, den: f32| {
            if den != 0.0 {
                best = best.min(at((num / den).clamp(0.0, 1.0)));
            }
        };
        consider(q.0, v.0);
        consider(q.1, v.1);
        consider(q.0 - q.1, v.0 - v.1);
        consider(q.0 + q.1, v.0 + v.1);
        best
    }

    #[inline]
    pub fn is_round(&self) -> bool {
        !self.square && self.aspect == 1.0
    }

    // Where coverage reaches zero, in the tip's frame.
    pub fn reach(&self) -> f32 {
        self.edge.reach()
    }

    // Where coverage stops being full, in the tip's frame.
    pub fn solid(&self) -> f32 {
        self.edge.solid()
    }

    // The reach in canvas pixels, however the tip is turned.
    pub fn extent(&self) -> f32 {
        let stretched = self.reach() * self.aspect.max(1.0);
        if self.square {
            stretched * std::f32::consts::SQRT_2
        } else {
            stretched
        }
    }

    #[inline]
    pub fn strength(&self, d: f32, x: i64, y: i64) -> f32 {
        let mut coverage = self.edge.at(d);
        if coverage <= 0.0 {
            return 0.0;
        }
        if self.grain > 0.0 {
            coverage *= 1.0 - self.grain * bump_cell(self.bump, x, y);
        }
        if self.scatter > 0.0 {
            coverage *= 1.0 - self.scatter * noise_cell(x, y);
        }
        (coverage * self.flow).clamp(0.0, 1.0)
    }

    #[inline]
    pub fn coverage(&self, d: f32, x: i64, y: i64) -> u8 {
        to_level(self.strength(d, x, y))
    }
}

// Adding a half avoids the libm call `round` costs on CPUs without SSE4.1.
pub fn to_level(fraction: f32) -> u8 {
    (fraction * 255.0 + 0.5) as u8
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

#[inline]
fn bump_cell(table: &[f32; BUMP_SIDE * BUMP_SIDE], x: i64, y: i64) -> f32 {
    let ix = x.rem_euclid(BUMP_SIDE as i64) as usize;
    let iy = y.rem_euclid(BUMP_SIDE as i64) as usize;
    table[iy * BUMP_SIDE + ix]
}

pub fn hash01(n: u64) -> f32 {
    let mut h = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 40) as f32 / 16_777_215.0
}

#[inline]
fn noise_cell(x: i64, y: i64) -> f32 {
    let (xi, yi) = (x as u64, y as u64);
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
        sized(tool, 20.0)
    }

    fn sized(tool: Tool, thickness: f32) -> Brush {
        let mut b = Brush {
            tool,
            ..Default::default()
        };
        b.set_thickness(thickness);
        b
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
        let b = sized(Tool::PixelPen, 5.0);
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
        let b = sized(Tool::PixelPen, 1.0);
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

    fn eraser(thickness: f32, hardness: f32) -> Brush {
        let mut b = sized(Tool::Eraser, thickness);
        b.set_antialiased(true);
        b.set_hardness(hardness);
        b
    }

    fn disc_alpha(b: &Brush) -> f32 {
        let reach = (b.radius() + 3.0).ceil() as i32;
        (-reach..reach)
            .flat_map(|y| (-reach..reach).map(move |x| (x as f32 + 0.5, y as f32 + 0.5)))
            .map(|(x, y)| b.coverage_at(0.0, 0.0, x, y) as f32 / 255.0)
            .sum()
    }

    #[test]
    fn each_tool_keeps_its_own_opacity_and_hardness() {
        let mut b = Brush {
            tool: Tool::Marker,
            ..Default::default()
        };
        b.set_thickness(40.0);
        b.set_opacity(0.25);

        b.tool = Tool::Eraser;
        assert_eq!(b.opacity(), 1.0, "the eraser starts on its own setting");
        assert_eq!(b.thickness(), 12.0);
        b.set_opacity(0.75);
        b.set_hardness(0.4);
        b.set_thickness(90.0);
        b.set_antialiased(true);

        b.tool = Tool::Marker;
        assert_eq!(b.opacity(), 0.25, "the marker kept what it was given");
        assert_eq!(b.thickness(), 40.0);
        assert!(!b.antialiased());

        b.tool = Tool::Eraser;
        assert_eq!(b.opacity(), 0.75);
        assert_eq!(b.hardness(), 0.4);
        assert_eq!(b.thickness(), 90.0);
        assert!(b.antialiased());
    }

    #[test]
    fn a_brush_can_be_set_wider_than_the_slider_reaches() {
        let mut b = sized(Tool::Marker, 500.0);
        assert_eq!(
            b.thickness(),
            500.0,
            "past the slider is still a real width"
        );
        assert_eq!(b.radius(), 250.0);
        b.set_thickness(THICKNESS_CEILING * 2.0);
        assert_eq!(b.thickness(), THICKNESS_CEILING);
    }

    #[test]
    fn an_eraser_without_antialiasing_is_all_or_nothing_at_every_size() {
        for thickness in [5.0, 9.0, 10.0, 60.0, 200.0] {
            let b = sized(Tool::Eraser, thickness);
            assert!(!b.antialiased(), "and that is how one starts");
            let r = b.radius();
            for step in 0..80 {
                let d = step as f32 * r / 40.0;
                let c = b.coverage_at(0.0, 0.0, d, 0.0);
                assert!(c == 0 || c == 255, "{thickness}px read {c} at {d}");
            }
            assert_eq!(b.coverage_at(0.0, 0.0, r - 0.01, 0.0), 255);
            assert_eq!(b.coverage_at(0.0, 0.0, r + 0.01, 0.0), 0);
        }
    }

    #[test]
    fn no_antialiasing_answers_hardness_rather_than_shrinking_the_stamp() {
        let mut b = sized(Tool::Eraser, 60.0);
        b.set_hardness(0.0);
        let r = b.radius();
        assert_eq!(b.coverage_at(0.0, 0.0, r * 0.9, 0.0), 255, "still solid");
        assert_eq!(
            b.coverage_at(0.0, 0.0, r + 0.01, 0.0),
            0,
            "still the same width"
        );
    }

    #[test]
    fn a_full_hardness_eraser_is_a_plain_antialiased_disc() {
        for thickness in [5.0, 10.0, 30.0, 100.0] {
            let b = eraser(thickness, 1.0);
            let r = b.radius();
            assert_eq!(
                b.coverage_at(0.0, 0.0, r - 0.6, 0.0),
                255,
                "{thickness} rim"
            );
            assert_eq!(
                b.coverage_at(0.0, 0.0, r + 0.6, 0.0),
                0,
                "{thickness} past it"
            );

            let area = disc_alpha(&b);
            let want = std::f32::consts::PI * r * r;
            assert!(
                (area - want).abs() < want * 0.02,
                "{thickness}px covered {area:.1} where a disc of radius {r} is {want:.1}"
            );
        }
    }

    #[test]
    fn softening_the_eraser_fades_it_without_widening_it() {
        let hard = eraser(30.0, 1.0);
        let soft = eraser(30.0, 0.0);
        let r = soft.radius();

        assert_eq!(
            soft.coverage_at(0.0, 0.0, 0.0, 0.0),
            255,
            "solid at the centre"
        );
        assert_eq!(
            soft.coverage_at(0.0, 0.0, r + 0.6, 0.0),
            0,
            "no wider than a hard one"
        );

        let midway = soft.coverage_at(0.0, 0.0, r / 2.0, 0.0);
        assert!(
            (100..=200).contains(&midway),
            "halfway out it read {midway}"
        );

        for d in [r * 0.25, r * 0.5, r * 0.75, r - 0.6] {
            let (a, b) = (
                soft.coverage_at(0.0, 0.0, d, 0.0),
                hard.coverage_at(0.0, 0.0, d, 0.0),
            );
            assert!(a < b, "at {d} the soft eraser read {a} against {b}");
        }
    }

    #[test]
    fn eraser_hardness_falls_off_smoothly_between_the_ends() {
        let r = eraser(40.0, 1.0).radius();
        let at = |h: f32, d: f32| eraser(40.0, h).coverage_at(0.0, 0.0, d, 0.0);
        let mut previous = 0;
        for step in 0..=10 {
            let c = at(step as f32 / 10.0, r * 0.6);
            assert!(
                c >= previous,
                "hardness {step} of 10 read {c} after {previous}"
            );
            previous = c;
        }
        assert!(at(0.0, r * 0.6) < at(1.0, r * 0.6));
    }

    #[test]
    fn hardness_leaves_the_painting_brushes_alone() {
        let mut b = brush(Tool::Marker);
        let before: Vec<u8> = (0..40)
            .map(|i| b.coverage_at(0.0, 0.0, i as f32 * 0.5, 0.0))
            .collect();
        b.set_hardness(0.0);
        let after: Vec<u8> = (0..40)
            .map(|i| b.coverage_at(0.0, 0.0, i as f32 * 0.5, 0.0))
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn the_reach_and_the_solid_core_agree_with_the_edge() {
        for tool in PANEL_ORDER.into_iter().filter(|t| t.profile().is_some()) {
            for thickness in [1.0, 3.0, 12.0, 60.0] {
                for (antialiased, hardness) in [(false, 1.0), (true, 1.0), (true, 0.3), (true, 0.0)]
                {
                    let mut b = sized(tool, thickness);
                    b.set_antialiased(antialiased);
                    b.set_hardness(hardness);
                    let tip = b.tip();
                    let (px, py) = (3, 7);
                    assert_eq!(
                        tip.strength(tip.reach() + 0.001, px, py),
                        0.0,
                        "{tool:?} {thickness} reaches past its reach"
                    );
                    assert!(tip.strength(tip.reach() - 0.05, px, py) > 0.0);
                    if tip.solid() > 0.0 {
                        assert_eq!(
                            tip.strength(tip.solid() - 0.001, px, py),
                            tip.strength(0.0, px, py),
                            "{tool:?} {thickness} is not solid out to its core"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_swept_tip_is_as_close_as_its_nearest_pass() {
        for (tool, square) in [
            (Tool::PixelPen, true),
            (Tool::PixelPen, false),
            (Tool::Calligraphy, false),
        ] {
            let mut b = sized(tool, 20.0);
            b.set_square_tip(square);
            let tip = b.tip();
            for v in [(0.0, 0.0), (7.0, 0.0), (3.0, -5.0), (-6.0, 6.0), (2.0, 9.0)] {
                for qy in -8..=8 {
                    for qx in -8..=8 {
                        let q = (qx as f32 * 1.3, qy as f32 * 1.1);
                        let exact = tip.distance_to_segment(q, v);
                        let sampled = (0..=4000)
                            .map(|i| i as f32 / 4000.0)
                            .map(|t| tip.length((q.0 - v.0 * t, q.1 - v.1 * t)))
                            .fold(f32::INFINITY, f32::min);
                        assert!(
                            exact <= sampled + 1e-4 && sampled - exact < 0.01,
                            "{tool:?} square {square}: {exact} against {sampled} at {q:?} {v:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn stamp_spacing_never_collapses_to_zero() {
        for thickness in [1.0, 2.0, 50.0, 100.0] {
            for tool in PANEL_ORDER {
                let b = sized(tool, thickness);
                assert!(
                    b.step() >= 1.0,
                    "{tool:?} step was {} at {thickness}",
                    b.step()
                );
            }
        }
    }
}
