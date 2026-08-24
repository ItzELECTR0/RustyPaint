use crate::doc::Rect;
use crate::gpu::Handle;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
}

const MIN_SIDE: f32 = 2.0;

impl Xform {
    pub fn from_rect(rect: Rect) -> Self {
        Self {
            x: rect.x0 as f32,
            y: rect.y0 as f32,
            width: rect.width() as f32,
            height: rect.height() as f32,
            rotation: 0.0,
        }
    }

    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn to_local(self, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = self.centre();
        let (sin, cos) = (-self.rotation).sin_cos();
        let (dx, dy) = (x - cx, y - cy);
        let rx = dx * cos - dy * sin;
        let ry = dx * sin + dy * cos;
        (
            rx / self.width.max(f32::EPSILON) + 0.5,
            ry / self.height.max(f32::EPSILON) + 0.5,
        )
    }

    pub fn to_canvas(self, u: f32, v: f32) -> (f32, f32) {
        let (cx, cy) = self.centre();
        let (dx, dy) = ((u - 0.5) * self.width, (v - 0.5) * self.height);
        let (sin, cos) = self.rotation.sin_cos();
        (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos)
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (u, v) = self.to_local(x, y);
        (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)
    }

    pub fn handle_at(&self, handle: Handle) -> (f32, f32) {
        let (u, v) = handle_uv(handle);
        self.to_canvas(u, v)
    }

    pub fn rotation_grip(&self, reach: f32) -> (f32, f32) {
        let (cx, cy) = self.centre();
        let (sin, cos) = self.rotation.sin_cos();
        let dy = -(self.height / 2.0 + reach);
        (cx - dy * sin, cy + dy * cos)
    }

    pub fn moved_by(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    pub fn resized(self, handle: Handle, x: f32, y: f32, keep_ratio: bool) -> Self {
        let (hu, hv) = handle_uv(handle);
        let (au, av) = (1.0 - hu, 1.0 - hv);
        let anchor = self.to_canvas(au, av);

        let (u, v) = self.to_local(x, y);

        let mut width = if hu == 0.5 {
            self.width
        } else {
            ((u - au) * self.width).abs().max(MIN_SIDE)
        };
        let mut height = if hv == 0.5 {
            self.height
        } else {
            ((v - av) * self.height).abs().max(MIN_SIDE)
        };

        if keep_ratio {
            let ratio = self.width.max(MIN_SIDE) / self.height.max(MIN_SIDE);
            if hu == 0.5 {
                width = height * ratio;
            } else if hv == 0.5 {
                height = width / ratio;
            } else {
                let scale =
                    (width / self.width.max(MIN_SIDE)).max(height / self.height.max(MIN_SIDE));
                width = (self.width * scale).max(MIN_SIDE);
                height = (self.height * scale).max(MIN_SIDE);
            }
        }

        let mut out = Self {
            width,
            height,
            ..self
        };
        let (nx, ny) = out.to_canvas(au, av);
        out.x += anchor.0 - nx;
        out.y += anchor.1 - ny;
        out
    }

    pub fn rotated_towards(self, x: f32, y: f32) -> Self {
        let (cx, cy) = self.centre();
        let rotation = (x - cx).atan2(cy - y);
        Self { rotation, ..self }
    }

    pub fn bounds(&self, size: (u32, u32)) -> Option<Rect> {
        let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
        let points: Vec<(f32, f32)> = corners
            .iter()
            .map(|(u, v)| self.to_canvas(*u, *v))
            .collect();

        let min_x = points.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let min_y = points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let max_x = points.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let max_y = points.iter().map(|p| p.1).fold(f32::MIN, f32::max);

        let x0 = min_x.floor().clamp(0.0, size.0 as f32) as u32;
        let y0 = min_y.floor().clamp(0.0, size.1 as f32) as u32;
        let x1 = max_x.ceil().clamp(0.0, size.0 as f32) as u32;
        let y1 = max_y.ceil().clamp(0.0, size.1 as f32) as u32;
        (x0 < x1 && y0 < y1).then(|| Rect::new(x0, y0, x1, y1))
    }
}

fn handle_uv(handle: Handle) -> (f32, f32) {
    match handle {
        Handle::TopLeft => (0.0, 0.0),
        Handle::TopRight => (1.0, 0.0),
        Handle::BottomLeft => (0.0, 1.0),
        Handle::BottomRight => (1.0, 1.0),
        Handle::Top => (0.5, 0.0),
        Handle::Bottom => (0.5, 1.0),
        Handle::Left => (0.0, 0.5),
        Handle::Right => (1.0, 0.5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> Xform {
        Xform {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            rotation: 0.0,
        }
    }

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
    }

    #[test]
    fn local_and_canvas_are_inverses() {
        for rotation in [0.0, 0.3, 1.2, -2.0, std::f32::consts::PI] {
            let t = Xform {
                rotation,
                ..boxed()
            };
            for point in [(0.0, 0.0), (0.5, 0.5), (1.0, 0.25), (0.2, 0.9)] {
                let canvas = t.to_canvas(point.0, point.1);
                let back = t.to_local(canvas.0, canvas.1);
                assert!(
                    close(back, point),
                    "{point:?} -> {canvas:?} -> {back:?} at {rotation}"
                );
            }
        }
    }

    #[test]
    fn the_box_contains_its_own_middle_and_not_the_outside() {
        let t = Xform {
            rotation: 0.6,
            ..boxed()
        };
        let (cx, cy) = t.centre();
        assert!(t.contains(cx, cy));
        assert!(!t.contains(cx + 500.0, cy));
    }

    #[test]
    fn dragging_a_corner_leaves_the_opposite_one_alone() {
        for rotation in [0.0, 0.7, -1.4] {
            let t = Xform {
                rotation,
                ..boxed()
            };
            let fixed = t.handle_at(Handle::TopLeft);

            let out = t.resized(Handle::BottomRight, 200.0, 200.0, false);
            assert!(
                close(out.handle_at(Handle::TopLeft), fixed),
                "top left moved at rotation {rotation}"
            );
        }
    }

    #[test]
    fn an_edge_grip_only_moves_its_own_axis() {
        let t = boxed();
        let out = t.resized(Handle::Right, 200.0, 999.0, false);
        assert!(
            (out.height - t.height).abs() < 0.01,
            "height should not have changed"
        );
        assert!(out.width > t.width);

        let out = t.resized(Handle::Bottom, 999.0, 200.0, false);
        assert!(
            (out.width - t.width).abs() < 0.01,
            "width should not have changed"
        );
    }

    #[test]
    fn a_box_cannot_be_dragged_away_to_nothing() {
        let out = boxed().resized(Handle::BottomRight, -5000.0, -5000.0, false);
        assert!(out.width >= MIN_SIDE && out.height >= MIN_SIDE, "{out:?}");
    }

    #[test]
    fn rotation_follows_the_grip() {
        let t = boxed();
        let (cx, cy) = t.centre();
        assert!(t.rotated_towards(cx, cy - 100.0).rotation.abs() < 0.01);
        let right = t.rotated_towards(cx + 100.0, cy).rotation;
        assert!(
            (right - std::f32::consts::FRAC_PI_2).abs() < 0.01,
            "got {right}"
        );
    }

    #[test]
    fn the_rotation_grip_sits_above_the_box_and_turns_with_it() {
        let t = boxed();
        let (_, gy) = t.rotation_grip(20.0);
        assert!(gy < t.y, "the grip should be above the top edge");

        let flipped = Xform {
            rotation: std::f32::consts::PI,
            ..t
        };
        let (_, gy) = flipped.rotation_grip(20.0);
        assert!(gy > t.y + t.height, "it should follow the box round");
    }

    #[test]
    fn bounds_grow_when_the_box_is_turned() {
        let upright = boxed().bounds((1000, 1000)).unwrap();
        let turned = Xform {
            rotation: 0.7,
            ..boxed()
        }
        .bounds((1000, 1000))
        .unwrap();
        assert!(
            turned.width() > upright.width() && turned.height() > upright.height(),
            "a rotated box needs a bigger upright box round it"
        );
    }

    #[test]
    fn bounds_stay_inside_the_canvas() {
        let t = Xform {
            x: -50.0,
            y: -50.0,
            width: 500.0,
            height: 500.0,
            rotation: 0.0,
        };
        let r = t.bounds((100, 80)).unwrap();
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (0, 0, 100, 80));
    }

    #[test]
    fn a_box_entirely_off_the_canvas_has_no_bounds() {
        let t = Xform {
            x: -500.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            rotation: 0.0,
        };
        assert!(t.bounds((100, 100)).is_none());
    }
}
