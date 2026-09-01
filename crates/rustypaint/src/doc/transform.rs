use super::Rect;
use super::image::{CHANNELS, Rgba8};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Centre,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub fn growth(self) -> (f32, f32) {
        let (fx, fy) = self.factors();
        (fx as f32 / 2.0, fy as f32 / 2.0)
    }

    fn factors(self) -> (i64, i64) {
        match self {
            Anchor::TopLeft => (0, 0),
            Anchor::Top => (1, 0),
            Anchor::TopRight => (2, 0),
            Anchor::Left => (0, 1),
            Anchor::Centre => (1, 1),
            Anchor::Right => (2, 1),
            Anchor::BottomLeft => (0, 2),
            Anchor::Bottom => (1, 2),
            Anchor::BottomRight => (2, 2),
        }
    }

    fn offset(self, old: (u32, u32), new: (u32, u32)) -> (i64, i64) {
        let dx = new.0 as i64 - old.0 as i64;
        let dy = new.1 as i64 - old.1 as i64;
        let (fx, fy) = self.factors();
        (dx * fx / 2, dy * fy / 2)
    }
}

pub fn place(src: &Rgba8, width: u32, height: u32, offset: (i64, i64), fill: [u8; 4]) -> Rgba8 {
    let mut out = Rgba8::new(width.max(1), height.max(1), fill);
    let (ox, oy) = offset;

    let y0 = oy.max(0);
    let y1 = (oy + src.height() as i64).min(height as i64);
    let x0 = ox.max(0);
    let x1 = (ox + src.width() as i64).min(width as i64);
    if y0 >= y1 || x0 >= x1 {
        return out;
    }

    let span = (x1 - x0) as usize * CHANNELS;
    let src_stride = src.width() as usize * CHANNELS;
    let dst_stride = width as usize * CHANNELS;
    let src_bytes = src.as_bytes();
    let dst = out.pixels_mut();

    for y in y0..y1 {
        let s = (y - oy) as usize * src_stride + (x0 - ox) as usize * CHANNELS;
        let d = y as usize * dst_stride + x0 as usize * CHANNELS;
        dst[d..d + span].copy_from_slice(&src_bytes[s..s + span]);
    }
    out
}

pub fn resize_canvas(src: &Rgba8, width: u32, height: u32, anchor: Anchor, fill: [u8; 4]) -> Rgba8 {
    let offset = anchor.offset(src.size(), (width, height));
    place(src, width, height, offset, fill)
}

#[allow(dead_code, reason = "the crop tool that drives this is Phase 5")]
pub fn crop(src: &Rgba8, rect: Rect) -> Rgba8 {
    let rect = rect.clamped(src.width(), src.height());
    place(
        src,
        rect.width(),
        rect.height(),
        (-(rect.x0 as i64), -(rect.y0 as i64)),
        [0, 0, 0, 0],
    )
}

pub fn scale(src: &Rgba8, width: u32, height: u32) -> Rgba8 {
    let (width, height) = (width.max(1), height.max(1));
    if (width, height) == src.size() {
        return src.clone();
    }
    // Resampling non-premultiplied pixels averages the colour of invisible neighbours into visible
    // edges, so weight every channel by its alpha first and divide it back out afterwards.
    let mut premultiplied = src.as_bytes().to_vec();
    for px in premultiplied.as_chunks_mut::<CHANNELS>().0 {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }

    let buffer = image::RgbaImage::from_raw(src.width(), src.height(), premultiplied)
        .expect("buffer size always matches its dimensions");
    let mut scaled = image::imageops::resize(
        &buffer,
        width,
        height,
        image::imageops::FilterType::Triangle,
    )
    .into_raw();

    for px in scaled.as_chunks_mut::<CHANNELS>().0 {
        let a = px[3] as u32;
        if a == 0 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
        }
    }
    Rgba8::from_raw(width, height, scaled).expect("resize produces a matching buffer")
}

pub fn flip_horizontal(src: &Rgba8) -> Rgba8 {
    let (w, h) = src.size();
    let mut out = src.clone();
    let stride = w as usize * CHANNELS;
    let bytes = src.as_bytes();
    let dst = out.pixels_mut();
    for y in 0..h as usize {
        for x in 0..w as usize {
            let from = y * stride + x * CHANNELS;
            let to = y * stride + (w as usize - 1 - x) * CHANNELS;
            dst[to..to + CHANNELS].copy_from_slice(&bytes[from..from + CHANNELS]);
        }
    }
    out
}

pub fn flip_vertical(src: &Rgba8) -> Rgba8 {
    let (w, h) = src.size();
    let stride = w as usize * CHANNELS;
    let mut out = src.clone();
    let bytes = src.as_bytes();
    let dst = out.pixels_mut();
    for y in 0..h as usize {
        let from = y * stride;
        let to = (h as usize - 1 - y) * stride;
        dst[to..to + stride].copy_from_slice(&bytes[from..from + stride]);
    }
    out
}

pub fn rotate_90(src: &Rgba8, clockwise: bool) -> Rgba8 {
    let (w, h) = src.size();
    let mut out = Rgba8::new(h, w, [0, 0, 0, 0]);
    let src_stride = w as usize * CHANNELS;
    let dst_stride = h as usize * CHANNELS;
    let bytes = src.as_bytes();
    let dst = out.pixels_mut();

    for y in 0..h as usize {
        for x in 0..w as usize {
            let from = y * src_stride + x * CHANNELS;
            let (nx, ny) = if clockwise {
                (h as usize - 1 - y, x)
            } else {
                (y, w as usize - 1 - x)
            };
            let to = ny * dst_stride + nx * CHANNELS;
            dst[to..to + CHANNELS].copy_from_slice(&bytes[from..from + CHANNELS]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn numbered() -> Rgba8 {
        let mut px = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                px.extend_from_slice(&[y * 3 + x, 0, 0, 255]);
            }
        }
        Rgba8::from_raw(3, 2, px).unwrap()
    }

    fn ids(img: &Rgba8) -> Vec<u8> {
        img.as_bytes().iter().step_by(CHANNELS).copied().collect()
    }

    fn at(img: &Rgba8, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * img.width() as usize + x as usize) * CHANNELS;
        img.as_bytes()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn extending_from_the_top_left_pushes_content_down_and_right() {
        let src = Rgba8::new(2, 2, RED);
        let out = resize_canvas(&src, 4, 4, Anchor::BottomRight, CLEAR);
        assert_eq!(at(&out, 0, 0), CLEAR);
        assert_eq!(at(&out, 2, 2), RED);
        assert_eq!(at(&out, 3, 3), RED);
    }

    #[test]
    fn extending_from_the_bottom_right_leaves_content_put() {
        let src = Rgba8::new(2, 2, RED);
        let out = resize_canvas(&src, 4, 4, Anchor::TopLeft, CLEAR);
        assert_eq!(at(&out, 0, 0), RED);
        assert_eq!(at(&out, 1, 1), RED);
        assert_eq!(at(&out, 2, 2), CLEAR);
    }

    #[test]
    fn centring_splits_the_new_space_evenly() {
        let src = Rgba8::new(2, 2, RED);
        let out = resize_canvas(&src, 6, 6, Anchor::Centre, CLEAR);
        assert_eq!(at(&out, 2, 2), RED);
        assert_eq!(at(&out, 3, 3), RED);
        assert_eq!(at(&out, 1, 1), CLEAR);
        assert_eq!(at(&out, 4, 4), CLEAR);
    }

    #[test]
    fn shrinking_crops_and_never_resamples() {
        let src = numbered();
        let out = resize_canvas(&src, 2, 1, Anchor::TopLeft, CLEAR);
        assert_eq!(ids(&out), vec![0, 1]);
    }

    #[test]
    fn cropping_takes_the_requested_window() {
        let src = numbered();
        let out = crop(&src, Rect::new(1, 0, 3, 2));
        assert_eq!(out.size(), (2, 2));
        assert_eq!(ids(&out), vec![1, 2, 4, 5]);
    }

    #[test]
    fn a_new_area_takes_the_fill_colour() {
        let src = Rgba8::new(1, 1, RED);
        let opaque = resize_canvas(&src, 2, 2, Anchor::TopLeft, [255, 255, 255, 255]);
        assert_eq!(at(&opaque, 1, 1), [255, 255, 255, 255]);

        let transparent = resize_canvas(&src, 2, 2, Anchor::TopLeft, CLEAR);
        assert_eq!(
            at(&transparent, 1, 1),
            CLEAR,
            "a transparent canvas stays transparent"
        );
    }

    #[test]
    fn flips_are_their_own_inverse() {
        let src = numbered();
        assert_eq!(ids(&flip_horizontal(&src)), vec![2, 1, 0, 5, 4, 3]);
        assert_eq!(ids(&flip_vertical(&src)), vec![3, 4, 5, 0, 1, 2]);
        assert_eq!(ids(&flip_horizontal(&flip_horizontal(&src))), ids(&src));
        assert_eq!(ids(&flip_vertical(&flip_vertical(&src))), ids(&src));
    }

    #[test]
    fn rotating_swaps_the_canvas_dimensions() {
        let src = numbered();
        let out = rotate_90(&src, true);
        assert_eq!(out.size(), (2, 3));
        assert_eq!(ids(&out), vec![3, 0, 4, 1, 5, 2]);
    }

    #[test]
    fn four_quarter_turns_come_back_to_the_start() {
        let src = numbered();
        for clockwise in [true, false] {
            let mut img = src.clone();
            for _ in 0..4 {
                img = rotate_90(&img, clockwise);
            }
            assert_eq!(img.size(), src.size());
            assert_eq!(ids(&img), ids(&src));
        }
    }

    #[test]
    fn opposite_rotations_cancel() {
        let src = numbered();
        assert_eq!(ids(&rotate_90(&rotate_90(&src, true), false)), ids(&src));
    }

    #[test]
    fn scaling_keeps_transparent_neighbours_out_of_visible_edges() {
        let mut src = Rgba8::new(2, 1, RED);
        src.pixels_mut()[CHANNELS..].copy_from_slice(&CLEAR);
        let out = scale(&src, 101, 1);

        let edges = out.as_bytes().as_chunks::<CHANNELS>().0;
        let mixed = edges.iter().filter(|px| (128..255).contains(&px[3]));
        assert!(
            mixed.clone().count() > 0,
            "the fade should cross half alpha"
        );
        for px in mixed {
            assert!(
                px[0] >= 250 && px[1] == 0 && px[2] == 0,
                "a transparent neighbour must not darken the edge it borders, got {px:?}"
            );
        }
    }

    #[test]
    fn scaling_changes_size_and_keeps_a_flat_colour_flat() {
        let src = Rgba8::new(4, 4, RED);
        let out = scale(&src, 9, 7);
        assert_eq!(out.size(), (9, 7));
        assert!(
            out.as_bytes()
                .as_chunks::<CHANNELS>()
                .0
                .iter()
                .all(|p| *p == RED),
            "resampling a flat colour should not invent new ones"
        );
    }
}
