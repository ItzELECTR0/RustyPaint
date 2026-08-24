#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Rect {
    pub fn new(x0: u32, y0: u32, x1: u32, y1: u32) -> Self {
        Self {
            x0,
            y0,
            x1: x1.max(x0),
            y1: y1.max(y0),
        }
    }

    pub fn around(cx: f32, cy: f32, radius: f32, width: u32, height: u32) -> Option<Self> {
        let x0 = (cx - radius).floor().max(0.0) as u32;
        let y0 = (cy - radius).floor().max(0.0) as u32;
        let x1 = ((cx + radius).ceil().max(0.0) as u32).min(width);
        let y1 = ((cy + radius).ceil().max(0.0) as u32).min(height);
        (x0 < x1 && y0 < y1).then_some(Self { x0, y0, x1, y1 })
    }

    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }

    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    pub fn area(&self) -> usize {
        self.width() as usize * self.height() as usize
    }

    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    pub fn clamped(self, width: u32, height: u32) -> Self {
        Self {
            x0: self.x0.min(width),
            y0: self.y0.min(height),
            x1: self.x1.min(width),
            y1: self.y1.min(height),
        }
    }

    pub fn rows(&self) -> std::ops::Range<u32> {
        self.y0..self.y1
    }

    pub fn cols(&self) -> std::ops::Range<u32> {
        self.x0..self.x1
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Bounds(Option<Rect>);

impl Bounds {
    pub fn add(&mut self, rect: Rect) {
        self.0 = Some(match self.0 {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    pub fn take(&mut self) -> Option<Rect> {
        self.0.take()
    }

    pub fn get(&self) -> Option<Rect> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_ignores_empties() {
        let a = Rect::new(2, 2, 6, 6);
        let empty = Rect::new(4, 4, 4, 4);
        assert_eq!(a.union(empty), a);
        assert_eq!(empty.union(a), a);
    }

    #[test]
    fn a_stamp_box_rounds_outwards_and_clamps() {
        let r = Rect::around(1.5, 1.5, 3.0, 100, 100).unwrap();
        assert_eq!((r.x0, r.y0), (0, 0));
        assert_eq!((r.x1, r.y1), (5, 5));

        assert!(Rect::around(-50.0, 10.0, 3.0, 100, 100).is_none());
    }

    #[test]
    fn bounds_grow_over_a_sequence() {
        let mut b = Bounds::default();
        assert!(b.get().is_none());
        b.add(Rect::new(10, 10, 20, 20));
        b.add(Rect::new(5, 30, 8, 40));
        assert_eq!(b.get().unwrap(), Rect::new(5, 10, 20, 40));
    }
}
