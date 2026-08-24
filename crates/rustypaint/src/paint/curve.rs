use super::shapes::{MIN_THICKNESS, ShapeStyle};
use crate::doc::Rgba8;

type Point = (f32, f32);
type CubicSpan = (Point, Point, Point, Point);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    Line,
    Curve3,
    Curve4,
    Curve5,
}

pub const ALL: &[CurveKind] = &[
    CurveKind::Line,
    CurveKind::Curve3,
    CurveKind::Curve4,
    CurveKind::Curve5,
];

impl CurveKind {
    pub fn name(self) -> &'static str {
        match self {
            CurveKind::Line => "Line",
            CurveKind::Curve3 => "3-point curve",
            CurveKind::Curve4 => "4-point curve",
            CurveKind::Curve5 => "5-point curve",
        }
    }

    pub fn points(self) -> usize {
        match self {
            CurveKind::Line => 2,
            CurveKind::Curve3 => 3,
            CurveKind::Curve4 => 4,
            CurveKind::Curve5 => 5,
        }
    }
}

pub const MAX_POINTS: usize = 24;

pub const SHAPE_BONES: usize = 16;

pub fn lay_out(kind: CurveKind, from: (f32, f32), to: (f32, f32)) -> Vec<(f32, f32)> {
    let n = kind.points();
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        })
        .collect()
}

pub fn bounds(points: &[(f32, f32)], thickness: f32, closed: bool) -> (f32, f32, f32, f32) {
    let pad = thickness.max(MIN_THICKNESS) / 2.0 + 1.0;
    let (mut x0, mut y0) = (f32::MAX, f32::MAX);
    let (mut x1, mut y1) = (f32::MIN, f32::MIN);
    for (x, y) in points {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }

    if let Some(path) = path(points, closed) {
        let b = path.bounds();
        x0 = x0.min(b.left());
        y0 = y0.min(b.top());
        x1 = x1.max(b.right());
        y1 = y1.max(b.bottom());
    }
    (x0 - pad, y0 - pad, x1 + pad, y1 + pad)
}

pub fn path(points: &[(f32, f32)], closed: bool) -> Option<tiny_skia::Path> {
    if points.len() < 2 {
        return None;
    }
    let mut b = tiny_skia::PathBuilder::new();
    b.move_to(points[0].0, points[0].1);

    if points.len() == 2 && !closed {
        b.line_to(points[1].0, points[1].1);
        return b.finish();
    }

    let spans = span_count(points, closed);
    for i in 0..spans {
        let (_, c1, c2, p2) = cubic_span(points, i, closed);
        b.cubic_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
    }
    if closed {
        b.close();
    }
    b.finish()
}

pub fn nearest_span(points: &[(f32, f32)], at: (f32, f32), closed: bool) -> usize {
    (0..span_count(points, closed))
        .map(|i| (i, distance_to_curve_span(points, i, at, closed)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(points.len(), |(i, _)| i + 1)
}

pub fn distance_to(points: &[(f32, f32)], at: (f32, f32), closed: bool) -> f32 {
    (0..span_count(points, closed))
        .map(|i| distance_to_curve_span(points, i, at, closed))
        .fold(f32::MAX, f32::min)
}

fn span_count(points: &[(f32, f32)], closed: bool) -> usize {
    if closed {
        points.len()
    } else {
        points.len().saturating_sub(1)
    }
}

fn distance_to_curve_span(points: &[(f32, f32)], span: usize, at: (f32, f32), closed: bool) -> f32 {
    if points.len() == 2 && !closed {
        return distance_to_span(points[0], points[1], at);
    }

    let (p1, c1, c2, p2) = cubic_span(points, span, closed);
    let mut previous = p1;
    let mut nearest = f32::MAX;
    for step in 1..=STEPS {
        let next = cubic(p1, c1, c2, p2, step as f32 / STEPS as f32);
        nearest = nearest.min(distance_to_span(previous, next, at));
        previous = next;
    }
    nearest
}

fn cubic_span(points: &[(f32, f32)], span: usize, closed: bool) -> CubicSpan {
    let n = points.len() as isize;
    let point = |i: isize| {
        if closed {
            points[i.rem_euclid(n) as usize]
        } else {
            points[i.clamp(0, n - 1) as usize]
        }
    };
    let i = span as isize;
    let (p0, p1, p2, p3) = (point(i - 1), point(i), point(i + 1), point(i + 2));
    let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
    let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
    (p1, c1, c2, p2)
}

fn distance_to_span(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    let t = if len <= f32::EPSILON {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len).clamp(0.0, 1.0)
    };
    let (nx, ny) = (a.0 + dx * t, a.1 + dy * t);
    ((p.0 - nx).powi(2) + (p.1 - ny).powi(2)).sqrt()
}

pub fn sample(path: &tiny_skia::Path, count: usize) -> Vec<(f32, f32)> {
    let mut line: Vec<(f32, f32)> = Vec::new();
    let mut cursor = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    let push = |p: (f32, f32), line: &mut Vec<(f32, f32)>| {
        if line
            .last()
            .is_none_or(|last| (last.0 - p.0).abs() + (last.1 - p.1).abs() > 0.01)
        {
            line.push(p);
        }
    };
    for segment in path.segments() {
        use tiny_skia::PathSegment as S;
        match segment {
            S::MoveTo(p) => {
                cursor = (p.x, p.y);
                start = cursor;
                push(cursor, &mut line);
            }
            S::LineTo(p) => {
                cursor = (p.x, p.y);
                push(cursor, &mut line);
            }
            S::QuadTo(c, p) => {
                for step in 1..=STEPS {
                    let t = step as f32 / STEPS as f32;
                    push(quadratic(cursor, (c.x, c.y), (p.x, p.y), t), &mut line);
                }
                cursor = (p.x, p.y);
            }
            S::CubicTo(c1, c2, p) => {
                for step in 1..=STEPS {
                    let t = step as f32 / STEPS as f32;
                    let at = cubic(cursor, (c1.x, c1.y), (c2.x, c2.y), (p.x, p.y), t);
                    push(at, &mut line);
                }
                cursor = (p.x, p.y);
            }
            S::Close => {
                cursor = start;
                push(cursor, &mut line);
            }
        }
    }
    if line.len() < 2 || count < 2 {
        return line;
    }

    let mut lengths = vec![0.0f32];
    for pair in line.windows(2) {
        let step = ((pair[1].0 - pair[0].0).powi(2) + (pair[1].1 - pair[0].1).powi(2)).sqrt();
        lengths.push(lengths.last().unwrap_or(&0.0) + step);
    }
    let total = *lengths.last().unwrap_or(&0.0);
    if total <= f32::EPSILON {
        return line;
    }

    let mut out = Vec::with_capacity(count);
    let mut span = 0;
    for i in 0..count {
        let want = total * i as f32 / count as f32;
        while span + 2 < line.len() && lengths[span + 1] < want {
            span += 1;
        }
        let run = lengths[span + 1] - lengths[span];
        let t = if run <= f32::EPSILON {
            0.0
        } else {
            (want - lengths[span]) / run
        };
        out.push((
            line[span].0 + (line[span + 1].0 - line[span].0) * t,
            line[span].1 + (line[span + 1].1 - line[span].1) * t,
        ));
    }
    out
}

const STEPS: usize = 16;

fn quadratic(a: (f32, f32), c: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    (
        u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
        u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
    )
}

fn cubic(a: (f32, f32), c1: (f32, f32), c2: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let (uu, tt) = (u * u, t * t);
    (
        uu * u * a.0 + 3.0 * uu * t * c1.0 + 3.0 * u * tt * c2.0 + tt * t * b.0,
        uu * u * a.1 + 3.0 * uu * t * c1.1 + 3.0 * u * tt * c2.1 + tt * t * b.1,
    )
}

pub fn render(
    points: &[(f32, f32)],
    style: &ShapeStyle,
    origin: (f32, f32),
    width: u32,
    height: u32,
    closed: bool,
) -> Option<Rgba8> {
    let (w, h) = (width.max(1), height.max(1));
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let path = path(points, closed)?;
    let place = tiny_skia::Transform::from_translate(-origin.0, -origin.1);

    if closed && let Some(fill) = style.fill {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(colour_of(fill));
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, place, None);
    }

    if let Some(colour) = style.outline.or((!closed).then_some([0, 0, 0, 255])) {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(colour_of(colour));
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width: style.thickness.max(MIN_THICKNESS),
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, place, None);
    }

    super::shapes::unpremultiply(pixmap, w, h)
}

fn colour_of([r, g, b, a]: [u8; 4]) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> ShapeStyle {
        ShapeStyle {
            fill: None,
            outline: Some([0, 0, 0, 255]),
            thickness: 4.0,
        }
    }

    #[test]
    fn a_line_has_two_points_and_the_curves_have_more() {
        assert_eq!(CurveKind::Line.points(), 2);
        assert_eq!(CurveKind::Curve3.points(), 3);
        assert_eq!(CurveKind::Curve4.points(), 4);
        assert_eq!(CurveKind::Curve5.points(), 5);
        assert_eq!(ALL.len(), 4);
    }

    #[test]
    fn a_drag_puts_the_ends_where_it_started_and_finished() {
        let points = lay_out(CurveKind::Line, (90.0, 10.0), (10.0, 70.0));
        assert_eq!(points.first(), Some(&(90.0, 10.0)));
        assert_eq!(points.last(), Some(&(10.0, 70.0)));
    }

    #[test]
    fn the_points_of_a_curve_are_spread_along_the_drag() {
        let points = lay_out(CurveKind::Curve5, (0.0, 0.0), (40.0, 0.0));
        assert_eq!(
            points,
            vec![
                (0.0, 0.0),
                (10.0, 0.0),
                (20.0, 0.0),
                (30.0, 0.0),
                (40.0, 0.0)
            ]
        );
    }

    #[test]
    fn the_box_holds_the_bow_and_not_only_the_points() {
        let points = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let (x0, y0, x1, y1) = bounds(&points, 4.0, false);
        assert!(x1 > 105.0, "the box stops at the rightmost point, at {x1}");

        let (w, h) = (
            (x1 - x0.floor()).ceil() as u32,
            (y1 - y0.floor()).ceil() as u32,
        );
        let drawn = render(&points, &style(), (x0.floor(), y0.floor()), w, h, false).unwrap();
        let bytes = drawn.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * w as usize + x) * 4 + 3];
        let right = w as usize - 1;
        assert!(
            (0..h as usize).all(|y| alpha(right, y) == 0),
            "the drawing runs into the right edge of its own box"
        );
    }

    #[test]
    fn the_box_holds_the_points_and_the_stroke_around_them() {
        let (x0, y0, x1, y1) = bounds(&[(10.0, 10.0), (30.0, 20.0)], 8.0, false);
        assert!(x0 < 10.0 && y0 < 10.0 && x1 > 30.0 && y1 > 20.0);
        assert!((x0 - 5.0).abs() < 0.01, "got {x0}");
    }

    #[test]
    fn a_closed_curve_comes_back_to_where_it_started() {
        let square = vec![(0.0, 0.0), (60.0, 0.0), (60.0, 60.0), (0.0, 60.0)];
        let open = path(&square, false).unwrap();
        let closed = path(&square, true).unwrap();

        assert!(closed.len() > open.len());
        let (_, _, x1, y1) = bounds(&square, 2.0, true);
        assert!(x1 > 60.0 && y1 > 60.0);
    }

    #[test]
    fn a_new_point_goes_in_the_span_it_was_dropped_on() {
        let points = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)];
        assert_eq!(nearest_span(&points, (104.0, 50.0), false), 2);
        assert_eq!(nearest_span(&points, (50.0, -6.25), false), 1);

        assert!(distance_to(&points, (50.0, -6.25), false) < 0.1);
        assert!(distance_to(&points, (50.0, 60.0), false) > 40.0);
    }

    #[test]
    fn a_path_is_sampled_evenly_along_its_length() {
        let mut b = tiny_skia::PathBuilder::new();
        b.move_to(0.0, 0.0);
        b.line_to(40.0, 0.0);
        b.line_to(40.0, 40.0);
        b.line_to(0.0, 40.0);
        b.close();
        let path = b.finish().unwrap();

        let points = sample(&path, 8);
        assert_eq!(points.len(), 8);
        let step =
            |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        for pair in points.windows(2) {
            assert!((step(pair[0], pair[1]) - 20.0).abs() < 0.5, "{:?}", pair);
        }
        assert_eq!(points[0], (0.0, 0.0));
    }

    #[test]
    fn a_curve_passes_through_every_one_of_its_points() {
        let points = vec![(0.0, 50.0), (50.0, 0.0), (100.0, 50.0)];
        let drawn = render(&points, &style(), (0.0, 0.0), 101, 101, false).unwrap();
        let bytes = drawn.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * 101 + x) * 4 + 3];
        assert!(alpha(50, 1) > 8, "the middle point should be at the top");
        assert!(
            alpha(50, 50) == 0,
            "and the line should be nowhere near the centre"
        );
    }

    #[test]
    fn a_straight_line_stays_straight() {
        let points = vec![(0.0, 0.0), (60.0, 60.0)];
        let drawn = render(&points, &style(), (0.0, 0.0), 61, 61, false).unwrap();
        let bytes = drawn.as_bytes();
        let alpha = |x: usize, y: usize| bytes[(y * 61 + x) * 4 + 3];
        assert!(alpha(30, 30) > 8, "on the diagonal");
        assert!(alpha(50, 10) == 0, "not on the other one");
    }

    #[test]
    fn the_origin_shifts_the_drawing_into_the_buffer() {
        let points = vec![(200.0, 200.0), (240.0, 200.0)];
        let drawn = render(&points, &style(), (198.0, 198.0), 44, 8, false).unwrap();
        let bytes = drawn.as_bytes();
        assert!(
            bytes[(2 * 44 + 20) * 4 + 3] > 8,
            "the line should be near the buffer's top"
        );
    }
}
