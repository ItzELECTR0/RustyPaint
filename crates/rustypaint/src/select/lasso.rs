use crate::doc::Rect;

const SPACING: f32 = 1.5;

const ENOUGH: usize = 3;

#[derive(Debug, Default, Clone)]
pub struct Lasso {
    points: Vec<(f32, f32)>,
}

impl Lasso {
    pub fn started_at(x: f32, y: f32) -> Self {
        Self {
            points: vec![(x, y)],
        }
    }

    pub fn push(&mut self, x: f32, y: f32) {
        match self.points.last() {
            Some(&(px, py)) if (x - px).abs() < SPACING && (y - py).abs() < SPACING => {}
            _ => self.points.push((x, y)),
        }
    }

    pub fn points(&self) -> &[(f32, f32)] {
        &self.points
    }

    pub fn bounds(&self, canvas: (u32, u32)) -> Option<Rect> {
        if self.points.len() < ENOUGH {
            return None;
        }
        let (mut x0, mut y0) = (f32::MAX, f32::MAX);
        let (mut x1, mut y1) = (f32::MIN, f32::MIN);
        for (x, y) in &self.points {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        let rect = Rect {
            x0: x0.floor().max(0.0) as u32,
            y0: y0.floor().max(0.0) as u32,
            x1: x1.ceil().max(0.0) as u32,
            y1: y1.ceil().max(0.0) as u32,
        };
        let rect = rect.clamped(canvas.0, canvas.1);
        (!rect.is_empty()).then_some(rect)
    }

    pub fn mask(&self, rect: Rect) -> Option<Vec<u8>> {
        if self.points.len() < ENOUGH {
            return None;
        }
        let (w, h) = (rect.width(), rect.height());
        let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;

        let mut builder = tiny_skia::PathBuilder::new();
        let first = self.points[0];
        builder.move_to(first.0 - rect.x0 as f32, first.1 - rect.y0 as f32);
        for (x, y) in &self.points[1..] {
            builder.line_to(x - rect.x0 as f32, y - rect.y0 as f32);
        }
        builder.close();
        let path = builder.finish()?;

        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::WHITE);
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );

        Some(
            pixmap
                .take()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|px| px[3])
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: f32) -> Lasso {
        let mut lasso = Lasso::started_at(10.0, 10.0);
        for (x, y) in [
            (10.0 + size, 10.0),
            (10.0 + size, 10.0 + size),
            (10.0, 10.0 + size),
        ] {
            lasso.push(x, y);
        }
        lasso
    }

    #[test]
    fn points_on_top_of_each_other_are_not_kept() {
        let mut lasso = Lasso::started_at(5.0, 5.0);
        for _ in 0..50 {
            lasso.push(5.2, 5.1);
        }
        assert_eq!(lasso.points().len(), 1);
        lasso.push(20.0, 20.0);
        assert_eq!(lasso.points().len(), 2);
    }

    #[test]
    fn a_click_is_not_a_loop() {
        let lasso = Lasso::started_at(5.0, 5.0);
        assert!(lasso.bounds((100, 100)).is_none());
        assert!(
            lasso
                .mask(Rect {
                    x0: 0,
                    y0: 0,
                    x1: 10,
                    y1: 10
                })
                .is_none()
        );
    }

    #[test]
    fn the_box_holds_the_loop() {
        let rect = square(40.0).bounds((100, 100)).expect("a loop");
        assert_eq!((rect.x0, rect.y0, rect.x1, rect.y1), (10, 10, 50, 50));
    }

    #[test]
    fn a_loop_drawn_off_the_edge_keeps_the_part_that_is_on_it() {
        let mut lasso = Lasso::started_at(-40.0, -40.0);
        for (x, y) in [(30.0, -40.0), (30.0, 30.0), (-40.0, 30.0)] {
            lasso.push(x, y);
        }
        let rect = lasso.bounds((100, 100)).expect("a loop");
        assert_eq!((rect.x0, rect.y0), (0, 0));
        assert_eq!((rect.x1, rect.y1), (30, 30));
    }

    #[test]
    fn the_mask_is_solid_inside_and_empty_outside() {
        let lasso = square(40.0);
        let rect = lasso.bounds((100, 100)).unwrap();
        let mask = lasso.mask(rect).expect("a mask");
        assert_eq!(mask.len(), (rect.width() * rect.height()) as usize);

        let at = |x: u32, y: u32| mask[(y * rect.width() + x) as usize];
        assert_eq!(at(20, 20), 255, "the middle is inside");
        assert_eq!(at(0, 0), 255, "and so is the corner it was drawn from");
    }

    #[test]
    fn a_triangle_leaves_the_corners_out() {
        let mut lasso = Lasso::started_at(0.0, 0.0);
        lasso.push(60.0, 0.0);
        lasso.push(0.0, 60.0);
        let rect = lasso.bounds((100, 100)).unwrap();
        let mask = lasso.mask(rect).unwrap();
        let at = |x: u32, y: u32| mask[(y * rect.width() + x) as usize];
        assert!(at(5, 5) > 200, "inside the triangle");
        assert_eq!(at(55, 55), 0, "the corner the diagonal cuts off");
    }
}
