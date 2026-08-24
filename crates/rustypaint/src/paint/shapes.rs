use crate::doc::Rgba8;

const OUTLINE_BOX: f32 = 1000.0;

macro_rules! shapes {
    ($($variant:ident => $file:literal, $label:literal;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(usize)]
        pub enum ShapeKind {
            $($variant,)*
        }

        pub const ALL: &[ShapeKind] = &[$(ShapeKind::$variant,)*];

        impl ShapeKind {
            pub fn name(self) -> &'static str {
                match self {
                    $(ShapeKind::$variant => $label,)*
                }
            }

            pub fn index(self) -> usize {
                self as usize
            }

            fn outline(self) -> &'static [u8] {
                match self {
                    $(ShapeKind::$variant => crate::assets::shape!($file),)*
                }
            }
        }
    };
}

shapes! {
    Circle => "Circle", "Circle";
    Capsule => "Capsule", "Capsule";
    Rectangle => "Rectangle", "Rectangle";
    RoundedRectangle => "RoundedRectangle", "Rounded rectangle";
    Triangle => "Triangle", "Triangle";
    Pentagon => "Pentagon", "Pentagon";
    Hexagon => "Hexagon", "Hexagon";
    Diamond => "Diamond", "Diamond";
    RightTriangle => "RightTriangle", "Right triangle";
    Arrow => "Arrow", "Arrow";
    PointedArrow => "PointedArrow", "Pointed arrow";
    HalfArc => "HalfArc", "Half arc";
    FivePointStar => "FivePointStar", "Five-point star";
    SixPointStar => "SixPointStar", "Six-point star";
    FourPointStar => "FourPointStar", "Four-point star";
    MultipointStar => "MultipointStar", "Multipoint star";
    SpeechBubble => "SpeechBubble", "Speech bubble";
    ThoughtBubble => "ThoughtBubble", "Thought bubble";
    Cross => "Cross", "Cross";
    CheckMark => "CheckMark", "Check mark";
    Moon => "Moon", "Moon";
    Banner => "Banner", "Banner";
    Lightning => "Lightning", "Lightning";
    Heart => "Heart", "Heart";
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeStyle {
    pub fill: Option<[u8; 4]>,
    pub outline: Option<[u8; 4]>,
    pub thickness: f32,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self {
            fill: None,
            outline: Some([0, 0, 0, 255]),
            thickness: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    None,
    Solid,
}

impl Paint {
    pub const ALL: [Paint; 2] = [Paint::None, Paint::Solid];

    pub fn of(colour: Option<[u8; 4]>) -> Self {
        if colour.is_some() {
            Paint::Solid
        } else {
            Paint::None
        }
    }
}

impl std::fmt::Display for Paint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Paint::None => "None",
            Paint::Solid => "Solid",
        })
    }
}

pub const MIN_THICKNESS: f32 = 1.0;
pub const MAX_THICKNESS: f32 = 100.0;

pub fn render(kind: ShapeKind, style: &ShapeStyle, width: u32, height: u32) -> Option<Rgba8> {
    let (w, h) = (width.max(1), height.max(1));
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;

    let inset = style.outline.map_or(0.0, |_| style.thickness / 2.0);
    let (iw, ih) = (
        (w as f32 - style.thickness).max(1.0),
        (h as f32 - style.thickness).max(1.0),
    );
    let path = geometry(kind, iw, ih)?;
    let path = path.transform(tiny_skia::Transform::from_translate(inset, inset))?;

    if let Some(colour) = style.fill {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(colour_of(colour));
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    if let Some(colour) = style.outline {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(colour_of(colour));
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width: style.thickness.max(MIN_THICKNESS),
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(
            &path,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    unpremultiply(pixmap, w, h)
}

pub fn render_placed(
    kind: ShapeKind,
    style: &ShapeStyle,
    width: u32,
    height: u32,
    size: (f32, f32),
    place: tiny_skia::Transform,
) -> Option<Rgba8> {
    let (w, h) = (width.max(1), height.max(1));
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let path = geometry(kind, size.0, size.1)?.transform(place)?;

    if let Some(colour) = style.fill {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(colour_of(colour));
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    if let Some(colour) = style.outline {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(colour_of(colour));
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width: style.thickness.max(MIN_THICKNESS),
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(
            &path,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    unpremultiply(pixmap, w, h)
}

pub(super) fn unpremultiply(pixmap: tiny_skia::Pixmap, w: u32, h: u32) -> Option<Rgba8> {
    let mut bytes = pixmap.take();
    for px in bytes.as_chunks_mut::<4>().0 {
        match px[3] {
            0 => *px = [0, 0, 0, 0],
            255 => {}
            a => {
                let a = a as u32;
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
    }
    Rgba8::from_raw(w, h, bytes)
}

pub fn outline_of(kind: ShapeKind, w: f32, h: f32) -> Option<tiny_skia::Path> {
    geometry(kind, w, h)
}

fn geometry(kind: ShapeKind, w: f32, h: f32) -> Option<tiny_skia::Path> {
    if kind == ShapeKind::RoundedRectangle {
        return rounded_rectangle(w, h);
    }
    let path = outline_path(kind)?;
    path.transform(tiny_skia::Transform::from_scale(
        w / OUTLINE_BOX,
        h / OUTLINE_BOX,
    ))
}

const CORNER: f32 = 0.18934;

fn rounded_rectangle(w: f32, h: f32) -> Option<tiny_skia::Path> {
    let r = (CORNER * w.min(h)).min(w / 2.0).min(h / 2.0);
    let mut b = tiny_skia::PathBuilder::new();
    let k = r * (4.0 / 3.0) * (std::f32::consts::SQRT_2 - 1.0);
    b.move_to(r, 0.0);
    b.line_to(w - r, 0.0);
    b.cubic_to(w - r + k, 0.0, w, r - k, w, r);
    b.line_to(w, h - r);
    b.cubic_to(w, h - r + k, w - r + k, h, w - r, h);
    b.line_to(r, h);
    b.cubic_to(r - k, h, 0.0, h - r + k, 0.0, h - r);
    b.line_to(0.0, r);
    b.cubic_to(0.0, r - k, r - k, 0.0, r, 0.0);
    b.close();
    b.finish()
}

static OUTLINES: std::sync::LazyLock<Vec<Option<tiny_skia::Path>>> =
    std::sync::LazyLock::new(|| ALL.iter().map(|kind| parse_outline(*kind)).collect());

fn outline_path(kind: ShapeKind) -> Option<tiny_skia::Path> {
    OUTLINES.get(kind.index())?.clone()
}

fn parse_outline(kind: ShapeKind) -> Option<tiny_skia::Path> {
    let tree = usvg::Tree::from_data(kind.outline(), &usvg::Options::default()).ok()?;
    first_path(tree.root()).cloned()
}

fn first_path(group: &usvg::Group) -> Option<&tiny_skia::Path> {
    group.children().iter().find_map(|node| match node {
        usvg::Node::Path(path) => Some(path.data()),
        usvg::Node::Group(inner) => first_path(inner),
        _ => None,
    })
}

fn colour_of([r, g, b, a]: [u8; 4]) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(pixels: &Rgba8) -> usize {
        pixels
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 8)
            .count()
    }

    #[test]
    fn every_shape_has_geometry_and_draws_something() {
        let style = ShapeStyle {
            fill: Some([255, 0, 0, 255]),
            ..Default::default()
        };
        for kind in ALL {
            let drawn = render(*kind, &style, 64, 64)
                .unwrap_or_else(|| panic!("{} did not render", kind.name()));
            assert_eq!(drawn.size(), (64, 64));
            assert!(opaque(&drawn) > 64, "{} came out nearly empty", kind.name());
        }
    }

    #[test]
    fn a_filled_rectangle_covers_more_than_an_outlined_one() {
        let outline = ShapeStyle {
            fill: None,
            outline: Some([0, 0, 0, 255]),
            thickness: 4.0,
        };
        let filled = ShapeStyle {
            fill: Some([0, 0, 0, 255]),
            ..outline
        };
        let a = render(ShapeKind::Rectangle, &outline, 80, 80).unwrap();
        let b = render(ShapeKind::Rectangle, &filled, 80, 80).unwrap();
        assert!(
            opaque(&b) > opaque(&a) * 2,
            "a fill should cover far more than its outline"
        );
    }

    #[test]
    fn a_shape_stays_inside_its_box_however_thick_the_outline() {
        let style = ShapeStyle {
            fill: None,
            outline: Some([0, 0, 0, 255]),
            thickness: 20.0,
        };
        let drawn = render(ShapeKind::Circle, &style, 100, 100).unwrap();
        let bytes = drawn.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * 100 + x) * 4 + 3];
        assert!(alpha(50, 4) > 8, "the stroke should reach the top edge");
        assert!(alpha(50, 50) == 0, "and the middle should still be hollow");
    }

    #[test]
    fn a_placed_outline_is_the_same_thickness_all_the_way_round() {
        let style = ShapeStyle {
            fill: None,
            outline: Some([0, 0, 0, 255]),
            thickness: 8.0,
        };
        let (w, h) = (240u32, 60u32);
        let inset = style.thickness / 2.0;
        let size = (w as f32 - style.thickness, h as f32 - style.thickness);
        let place = tiny_skia::Transform::from_translate(inset, inset);
        let drawn = render_placed(ShapeKind::Rectangle, &style, w, h, size, place).unwrap();

        let bytes = drawn.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * w as usize + x) * 4 + 3];
        let across = (0..w as usize)
            .filter(|x| alpha(*x, h as usize / 2) > 8)
            .count();
        let down = (0..h as usize)
            .filter(|y| alpha(w as usize / 2, *y) > 8)
            .count();
        assert!(
            across.abs_diff(down) <= 2,
            "the sides come to {across} px and the top and bottom to {down}, both should be 16"
        );
        assert!(
            (14..=18).contains(&across),
            "and that thickness should be the 8 asked for"
        );
    }

    #[test]
    fn what_is_committed_is_what_was_previewed() {
        let style = ShapeStyle {
            fill: None,
            outline: Some([0, 0, 0, 255]),
            thickness: 9.0,
        };
        let (w, h) = (300u32, 70u32);
        let inset = style.thickness / 2.0;
        let size = (w as f32 - style.thickness, h as f32 - style.thickness);
        let place = tiny_skia::Transform::from_translate(inset, inset);

        for kind in [
            ShapeKind::Rectangle,
            ShapeKind::RoundedRectangle,
            ShapeKind::Circle,
            ShapeKind::Heart,
        ] {
            let preview = render(kind, &style, w, h).unwrap();
            let committed = render_placed(kind, &style, w, h, size, place).unwrap();
            let differing = preview
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(committed.as_bytes().as_chunks::<4>().0)
                .filter(|(a, b)| a[3].abs_diff(b[3]) > 32)
                .count();
            let total = (w * h) as usize;
            assert!(
                differing * 200 < total,
                "{} differs on {differing} of {total} pixels",
                kind.name()
            );
        }
    }

    #[test]
    fn a_placed_rounded_rectangle_keeps_its_corners_round() {
        let style = ShapeStyle {
            fill: Some([0, 0, 0, 255]),
            outline: None,
            thickness: 1.0,
        };
        let (w, h) = (400u32, 100u32);
        let place = tiny_skia::Transform::identity();
        let drawn = render_placed(
            ShapeKind::RoundedRectangle,
            &style,
            w,
            h,
            (w as f32, h as f32),
            place,
        )
        .unwrap();

        let bytes = drawn.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * w as usize + x) * 4 + 3];
        let r = (CORNER * h as f32) as usize;
        assert_eq!(alpha(0, 0), 0, "the very corner is outside the shape");
        assert!(
            alpha(r + 2, 2) > 8,
            "and it is round again within the radius"
        );
        assert!(
            alpha(80, 1) > 8,
            "the top edge is straight away from the corner"
        );
    }

    #[test]
    fn a_rounded_rectangle_keeps_its_corners_round_in_a_wide_box() {
        let style = ShapeStyle {
            fill: Some([0, 0, 0, 255]),
            outline: None,
            thickness: 1.0,
        };
        let wide = render(ShapeKind::RoundedRectangle, &style, 400, 100).unwrap();
        let bytes = wide.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * 400 + x) * 4 + 3];

        let r = (CORNER * 100.0) as usize;
        assert_eq!(alpha(0, 0), 0, "the very corner is outside the shape");
        assert!(
            alpha(r + 2, 2) > 8,
            "and it is round again within the radius"
        );
        assert!(
            alpha(80, 1) > 8,
            "the top edge is straight away from the corner"
        );
    }

    #[test]
    fn the_corner_radius_does_not_follow_the_long_side() {
        let style = ShapeStyle {
            fill: Some([0, 0, 0, 255]),
            outline: None,
            thickness: 1.0,
        };
        let bite = |w: u32, h: u32| {
            let drawn = render(ShapeKind::RoundedRectangle, &style, w, h).unwrap();
            let bytes = drawn.as_bytes();
            (0..w)
                .find(|x| bytes[(*x as usize) * 4 + 3] > 8)
                .unwrap_or(w)
        };
        assert!(bite(200, 200).abs_diff(bite(800, 200)) <= 2);
    }

    #[test]
    fn the_panel_offers_all_twenty_four() {
        assert_eq!(ALL.len(), 24);
    }
}
