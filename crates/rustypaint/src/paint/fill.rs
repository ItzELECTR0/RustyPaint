use crate::doc::rect::{Bounds, Rect};
use crate::doc::{Rgba8, image::CHANNELS};

pub fn pick(pixels: &Rgba8, x: i64, y: i64) -> Option<[u8; 4]> {
    let (w, h) = pixels.size();
    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
        return None;
    }
    let i = (y as usize * w as usize + x as usize) * CHANNELS;
    Some(pixels.as_bytes()[i..i + CHANNELS].try_into().unwrap())
}

pub fn flood(pixels: &mut Rgba8, x: i64, y: i64, colour: [u8; 4], tolerance: f32) -> Option<Rect> {
    let (w, h) = pixels.size();
    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 || w == 0 || h == 0 {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let (sx, sy) = (x as usize, y as usize);

    let target = pick(pixels, x, y)?;
    if target == colour {
        return None;
    }

    let cutoff = (tolerance.clamp(0.0, 1.0) * 255.0) as i32;
    let bytes = pixels.pixels_mut();
    let matches = |bytes: &[u8], i: usize| {
        (0..CHANNELS).all(|c| (bytes[i + c] as i32 - target[c] as i32).abs() <= cutoff)
    };

    let mut filled = vec![false; w * h];
    let mut touched = Bounds::default();
    let mut queue = vec![(sx, sy)];

    while let Some((qx, qy)) = queue.pop() {
        if filled[qy * w + qx] || !matches(bytes, (qy * w + qx) * CHANNELS) {
            continue;
        }

        let mut left = qx;
        while left > 0
            && !filled[qy * w + left - 1]
            && matches(bytes, (qy * w + left - 1) * CHANNELS)
        {
            left -= 1;
        }
        let mut right = qx;
        while right + 1 < w
            && !filled[qy * w + right + 1]
            && matches(bytes, (qy * w + right + 1) * CHANNELS)
        {
            right += 1;
        }

        for x in left..=right {
            filled[qy * w + x] = true;
            let i = (qy * w + x) * CHANNELS;
            bytes[i..i + CHANNELS].copy_from_slice(&colour);
        }
        touched.add(Rect::new(
            left as u32,
            qy as u32,
            right as u32 + 1,
            qy as u32 + 1,
        ));

        for ny in [qy.checked_sub(1), (qy + 1 < h).then_some(qy + 1)]
            .into_iter()
            .flatten()
        {
            for x in left..=right {
                if !filled[ny * w + x] && matches(bytes, (ny * w + x) * CHANNELS) {
                    queue.push((x, ny));
                }
            }
        }
    }

    touched.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn at(img: &Rgba8, x: u32, y: u32) -> [u8; 4] {
        pick(img, x as i64, y as i64).unwrap()
    }

    fn set(img: &mut Rgba8, x: u32, y: u32, c: [u8; 4]) {
        let w = img.width() as usize;
        let i = (y as usize * w + x as usize) * CHANNELS;
        img.pixels_mut()[i..i + CHANNELS].copy_from_slice(&c);
    }

    #[test]
    fn an_empty_canvas_fills_entirely() {
        let mut img = Rgba8::new(8, 8, WHITE);
        let touched = flood(&mut img, 4, 4, RED, 0.0).unwrap();
        assert_eq!(touched, Rect::new(0, 0, 8, 8));
        assert!(
            img.as_bytes()
                .as_chunks::<CHANNELS>()
                .0
                .iter()
                .all(|p| *p == RED)
        );
    }

    #[test]
    fn a_wall_stops_the_fill() {
        let mut img = Rgba8::new(8, 4, WHITE);
        for y in 0..4 {
            set(&mut img, 4, y, BLUE);
        }
        flood(&mut img, 0, 0, RED, 0.0).unwrap();

        assert_eq!(at(&img, 3, 2), RED, "the near side is filled");
        assert_eq!(at(&img, 4, 2), BLUE, "the wall itself is not");
        assert_eq!(at(&img, 5, 2), WHITE, "and nothing leaks past it");
    }

    #[test]
    fn the_fill_goes_round_corners() {
        let mut img = Rgba8::new(5, 5, WHITE);
        for y in 0..4 {
            set(&mut img, 2, y, BLUE);
        }
        flood(&mut img, 0, 0, RED, 0.0).unwrap();
        assert_eq!(at(&img, 4, 0), RED, "the far side is reachable underneath");
    }

    #[test]
    fn tolerance_lets_near_colours_through() {
        let mut img = Rgba8::new(4, 1, WHITE);
        set(&mut img, 2, 0, [250, 250, 250, 255]);

        let mut strict = img.clone();
        flood(&mut strict, 0, 0, RED, 0.0);
        assert_eq!(at(&strict, 3, 0), WHITE);

        flood(&mut img, 0, 0, RED, 0.05);
        assert_eq!(at(&img, 3, 0), RED);
    }

    #[test]
    fn filling_with_the_colour_already_there_does_nothing() {
        let mut img = Rgba8::new(4, 4, RED);
        assert_eq!(flood(&mut img, 1, 1, RED, 0.0), None);
    }

    #[test]
    fn a_click_outside_the_canvas_does_nothing() {
        let mut img = Rgba8::new(4, 4, WHITE);
        assert_eq!(flood(&mut img, -1, 2, RED, 0.0), None);
        assert_eq!(flood(&mut img, 2, 99, RED, 0.0), None);
        assert!(
            img.as_bytes()
                .as_chunks::<CHANNELS>()
                .0
                .iter()
                .all(|p| *p == WHITE)
        );
    }

    #[test]
    fn transparent_regions_fill_like_any_other() {
        let mut img = Rgba8::transparent(4, 4);
        let touched = flood(&mut img, 0, 0, RED, 0.0).unwrap();
        assert_eq!(touched, Rect::new(0, 0, 4, 4));
    }

    #[test]
    fn the_pipette_reads_what_is_there_and_nothing_outside() {
        let mut img = Rgba8::new(4, 4, WHITE);
        set(&mut img, 1, 2, BLUE);
        assert_eq!(pick(&img, 1, 2), Some(BLUE));
        assert_eq!(pick(&img, 0, 0), Some(WHITE));
        assert_eq!(pick(&img, 4, 0), None);
        assert_eq!(pick(&img, -1, 0), None);
    }
}
