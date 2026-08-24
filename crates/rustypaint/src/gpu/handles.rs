use crate::doc::transform::Anchor;
use iced::{Point, Rectangle, mouse};

pub const HALF: f32 = 5.0;

const SLOP: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

pub const ALL: [Handle; 8] = [
    Handle::TopLeft,
    Handle::TopRight,
    Handle::BottomLeft,
    Handle::BottomRight,
    Handle::Top,
    Handle::Bottom,
    Handle::Left,
    Handle::Right,
];

type Pull = (i32, i32);

impl Handle {
    pub fn index(self) -> usize {
        ALL.iter().position(|h| *h == self).unwrap_or(0)
    }

    fn pull(self) -> Pull {
        match self {
            Handle::TopLeft => (-1, -1),
            Handle::TopRight => (1, -1),
            Handle::BottomLeft => (-1, 1),
            Handle::BottomRight => (1, 1),
            Handle::Top => (0, -1),
            Handle::Bottom => (0, 1),
            Handle::Left => (-1, 0),
            Handle::Right => (1, 0),
        }
    }

    pub fn anchor(self) -> Anchor {
        match self.pull() {
            (-1, -1) => Anchor::BottomRight,
            (1, -1) => Anchor::BottomLeft,
            (-1, 1) => Anchor::TopRight,
            (1, 1) => Anchor::TopLeft,
            (0, -1) => Anchor::Bottom,
            (0, 1) => Anchor::Top,
            (-1, 0) => Anchor::Right,
            _ => Anchor::Left,
        }
    }

    pub fn centre(self, canvas: Rectangle) -> Point {
        let (dx, dy) = self.pull();
        let x = match dx {
            -1 => canvas.x,
            1 => canvas.x + canvas.width,
            _ => canvas.x + canvas.width / 2.0,
        };
        let y = match dy {
            -1 => canvas.y,
            1 => canvas.y + canvas.height,
            _ => canvas.y + canvas.height / 2.0,
        };
        Point::new(x, y)
    }

    pub fn cursor(self) -> mouse::Interaction {
        match self.pull() {
            (0, _) => mouse::Interaction::ResizingVertically,
            (_, 0) => mouse::Interaction::ResizingHorizontally,
            (-1, -1) | (1, 1) => mouse::Interaction::ResizingDiagonallyDown,
            _ => mouse::Interaction::ResizingDiagonallyUp,
        }
    }

    pub fn resize(self, from: (u32, u32), delta: (f32, f32)) -> (u32, u32) {
        let (dx, dy) = self.pull();
        let apply = |size: u32, pull: i32, moved: f32| -> u32 {
            let changed = size as f32 + moved * pull as f32;
            changed.round().clamp(1.0, u32::MAX as f32) as u32
        };
        (apply(from.0, dx, delta.0), apply(from.1, dy, delta.1))
    }
}

pub fn hit(canvas: Rectangle, point: Point) -> Option<Handle> {
    let reach = HALF + SLOP;
    ALL.iter().copied().find(|handle| {
        let c = handle.centre(canvas);
        (point.x - c.x).abs() <= reach && (point.y - c.y).abs() <= reach
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rectangle {
        Rectangle {
            x: 100.0,
            y: 50.0,
            width: 200.0,
            height: 100.0,
        }
    }

    #[test]
    fn every_grip_sits_on_the_canvas_edge() {
        let r = canvas();
        for handle in ALL {
            let c = handle.centre(r);
            let on_x = c.x == r.x || c.x == r.x + r.width || c.x == r.x + r.width / 2.0;
            let on_y = c.y == r.y || c.y == r.y + r.height || c.y == r.y + r.height / 2.0;
            assert!(on_x && on_y, "{handle:?} landed at {c:?}");
        }
    }

    #[test]
    fn a_grip_hits_itself_and_nothing_else() {
        let r = canvas();
        for handle in ALL {
            assert_eq!(hit(r, handle.centre(r)), Some(handle));
        }
        assert_eq!(
            hit(r, Point::new(150.0, 80.0)),
            None,
            "the middle is not a grip"
        );
    }

    #[test]
    fn dragging_the_right_edge_only_changes_width() {
        let out = Handle::Right.resize((200, 100), (40.0, 25.0));
        assert_eq!(out, (240, 100), "vertical movement should be ignored");
    }

    #[test]
    fn dragging_the_left_edge_grows_the_other_way() {
        assert_eq!(Handle::Left.resize((200, 100), (-30.0, 0.0)), (230, 100));
        assert_eq!(Handle::Left.resize((200, 100), (30.0, 0.0)), (170, 100));
    }

    #[test]
    fn a_corner_moves_both_axes() {
        assert_eq!(
            Handle::BottomRight.resize((200, 100), (10.0, 20.0)),
            (210, 120)
        );
        assert_eq!(Handle::TopLeft.resize((200, 100), (10.0, 20.0)), (190, 80));
    }

    #[test]
    fn the_anchor_is_the_side_that_is_not_moving() {
        assert_eq!(Handle::Right.anchor(), Anchor::Left);
        assert_eq!(Handle::Left.anchor(), Anchor::Right);
        assert_eq!(Handle::BottomRight.anchor(), Anchor::TopLeft);
        assert_eq!(Handle::TopLeft.anchor(), Anchor::BottomRight);
    }

    #[test]
    fn a_canvas_never_shrinks_below_one_pixel() {
        assert_eq!(Handle::Right.resize((10, 10), (-9999.0, 0.0)), (1, 10));
    }
}
