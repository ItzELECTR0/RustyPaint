use crate::doc::{Rect, Rgba8};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub at: (f32, f32),
    pub radius: f32,
    pub foreground: bool,
    // Which drag painted it, so one correction is not read as a prompt for another.
    pub stroke: u32,
}

const MISSING: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub radius: f32,
    pub feather: f32,
    pub smooth: f32,
    pub shift: f32,
    pub decontaminate: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            radius: 4.0,
            feather: 0.0,
            smooth: 0.0,
            shift: 0.0,
            decontaminate: false,
        }
    }
}

pub fn finish(
    pixels: &Rgba8,
    raw: &[u8],
    rect: Rect,
    strokes: &[Dab],
    settings: Settings,
) -> Vec<u8> {
    // The matte places the edge better than the cut does, but only whole pixels say honestly what
    // is being lifted, so it is thresholded and may only add to what the cut already chose.
    let matted = matte(pixels, raw, rect, settings.radius as u32);
    let mut mask: Vec<u8> = matted
        .iter()
        .zip(raw)
        .map(|(soft, cut)| u8::from(*soft > 128 || *cut > 128) * 255)
        .collect();
    let (w, h) = (pixels.width() as usize, pixels.height() as usize);
    if settings.smooth > 0.0 {
        mask = blur(&mask, w, h, settings.smooth.round() as usize);
        for value in &mut mask {
            *value = if *value > 128 { 255 } else { 0 };
        }
    }
    if settings.shift.abs() >= 0.5 {
        let grow = settings.shift > 0.0;
        let mut seeds: Vec<u32> = mask
            .iter()
            .enumerate()
            .map(|(i, m)| {
                if (*m > 128) == grow {
                    i as u32
                } else {
                    MISSING
                }
            })
            .collect();
        propagate(&mut seeds, w, h);
        let radius2 = settings.shift.powi(2) as u64;
        for (i, value) in mask.iter_mut().enumerate() {
            if distance(i, seeds[i], w) <= radius2 {
                *value = if grow { 255 } else { 0 };
            }
        }
    }
    if settings.feather > 0.0 {
        mask = blur(&mask, w, h, settings.feather.ceil() as usize);
    }
    constrain(&mut mask, pixels, rect, strokes);
    mask
}

pub fn decontaminate(pixels: &Rgba8, mask: &[u8]) -> Rgba8 {
    let (w, h) = (pixels.width() as usize, pixels.height() as usize);
    let mut seeds: Vec<u32> = mask
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if *m == 0 && pixels.as_bytes()[i * 4 + 3] > 0 {
                i as u32
            } else {
                MISSING
            }
        })
        .collect();
    propagate(&mut seeds, w, h);
    let mut result = pixels.clone();
    for (i, pixel) in result
        .pixels_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        if mask[i] == 0 || mask[i] == 255 || distance(i, seeds[i], w) > 64 * 64 {
            continue;
        }
        let alpha = mask[i] as f32 / 255.0;
        let bg = seeds[i] as usize * 4;
        for (c, value) in pixel[..3].iter_mut().enumerate() {
            *value = ((pixels.as_bytes()[i * 4 + c] as f32
                - (1.0 - alpha) * pixels.as_bytes()[bg + c] as f32)
                / alpha.max(0.05))
            .round()
            .clamp(0.0, 255.0) as u8;
        }
    }
    result
}

fn blur(mask: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    let mut horizontal = vec![0u8; mask.len()];
    let mut out = vec![0u8; mask.len()];
    for y in 0..height {
        let mut sum = 0u32;
        let mut right = 0;
        let mut left = 0;
        for x in 0..width {
            while right < (x + radius + 1).min(width) {
                sum += mask[y * width + right] as u32;
                right += 1;
            }
            while left < x.saturating_sub(radius) {
                sum -= mask[y * width + left] as u32;
                left += 1;
            }
            horizontal[y * width + x] = (sum / (right - left) as u32) as u8;
        }
    }
    for x in 0..width {
        let mut sum = 0u32;
        let mut bottom = 0;
        let mut top = 0;
        for y in 0..height {
            while bottom < (y + radius + 1).min(height) {
                sum += horizontal[bottom * width + x] as u32;
                bottom += 1;
            }
            while top < y.saturating_sub(radius) {
                sum -= horizontal[top * width + x] as u32;
                top += 1;
            }
            out[y * width + x] = (sum / (bottom - top) as u32) as u8;
        }
    }
    out
}

fn distance(a: usize, b: u32, width: usize) -> u64 {
    if b == MISSING {
        return u64::MAX;
    }
    let b = b as usize;
    let dx = (a % width).abs_diff(b % width) as u64;
    let dy = (a / width).abs_diff(b / width) as u64;
    dx * dx + dy * dy
}

fn propagate(seeds: &mut [u32], width: usize, height: usize) {
    for reverse in [false, true] {
        for step in 0..seeds.len() {
            let i = if reverse {
                seeds.len() - step - 1
            } else {
                step
            };
            let (x, y) = ((i % width) as i32, (i / width) as i32);
            let sign = if reverse { -1 } else { 1 };
            for (dx, dy) in [(-1, 0), (-1, -1), (0, -1), (1, -1)] {
                let (nx, ny) = (x + dx * sign, y + dy * sign);
                if nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32 {
                    let seed = seeds[ny as usize * width + nx as usize];
                    if distance(i, seed, width) < distance(i, seeds[i], width) {
                        seeds[i] = seed;
                    }
                }
            }
        }
    }
}

pub(super) fn interior_prompt(mask: &[u8], size: (usize, usize)) -> Option<(usize, usize)> {
    let mut seeds: Vec<u32> = mask
        .iter()
        .enumerate()
        .map(|(i, &m)| if m == 0 { i as u32 } else { MISSING })
        .collect();
    propagate(&mut seeds, size.0, size.1);
    let (i, d) = mask
        .iter()
        .enumerate()
        .filter(|(_, m)| **m > 128)
        .map(|(i, _)| (i, distance(i, seeds[i], size.0)))
        .filter(|(_, d)| *d < u64::MAX)
        .max_by_key(|(_, d)| *d)?;
    (d >= 4).then_some((i % size.0, i / size.0))
}

// Where the edge really falls, read from the colours either side of it within the detail radius.
pub fn matte(pixels: &Rgba8, mask: &[u8], rect: Rect, radius: u32) -> Vec<u8> {
    let mut out = mask.to_vec();
    if radius == 0 || rect.area() == 0 {
        return out;
    }
    let reach = radius * 2 + 1;
    let bounds = Rect::new(
        rect.x0.saturating_sub(reach),
        rect.y0.saturating_sub(reach),
        rect.x1.saturating_add(reach).min(pixels.width()),
        rect.y1.saturating_add(reach).min(pixels.height()),
    );
    let (w, h) = (bounds.width() as usize, bounds.height() as usize);
    let source = |i: usize| {
        (bounds.y0 as usize + i / w) * pixels.width() as usize + bounds.x0 as usize + i % w
    };
    let colour = |i: usize| {
        let offset = source(i) * 4;
        let bytes = pixels.as_bytes();
        [
            bytes[offset] as f32,
            bytes[offset + 1] as f32,
            bytes[offset + 2] as f32,
        ]
    };
    let mut fg: Vec<u32> = (0..w * h)
        .map(|i| {
            if mask[source(i)] > 128 {
                i as u32
            } else {
                MISSING
            }
        })
        .collect();
    let mut bg: Vec<u32> = (0..w * h)
        .map(|i| {
            if mask[source(i)] <= 128 {
                i as u32
            } else {
                MISSING
            }
        })
        .collect();
    propagate(&mut fg, w, h);
    propagate(&mut bg, w, h);
    let radius2 = u64::from(radius).pow(2);
    let band: Vec<bool> = (0..w * h)
        .map(|i| distance(i, if mask[source(i)] > 128 { bg[i] } else { fg[i] }, w) <= radius2)
        .collect();
    for i in 0..w * h {
        fg[i] = if !band[i] && mask[source(i)] > 128 {
            i as u32
        } else {
            MISSING
        };
        bg[i] = if !band[i] && mask[source(i)] <= 128 {
            i as u32
        } else {
            MISSING
        };
    }
    propagate(&mut fg, w, h);
    propagate(&mut bg, w, h);
    let limit = u64::from(radius * 4 + 2).pow(2);
    for i in 0..w * h {
        if !band[i] || distance(i, fg[i], w) > limit || distance(i, bg[i], w) > limit {
            continue;
        }
        let (f, b, c) = (colour(fg[i] as usize), colour(bg[i] as usize), colour(i));
        let span: f32 = (0..3).map(|k| (f[k] - b[k]).powi(2)).sum();
        if span < 100.0 {
            continue;
        }
        let alpha =
            ((0..3).map(|k| (c[k] - b[k]) * (f[k] - b[k])).sum::<f32>() / span).clamp(0.0, 1.0);
        let residual: f32 = (0..3)
            .map(|k| (c[k] - (b[k] + alpha * (f[k] - b[k]))).powi(2))
            .sum();
        // A colour pair that cannot explain this pixel must not override the object mask.
        if residual <= 900.0 {
            out[source(i)] = (alpha * 255.0).round() as u8;
        }
    }
    out
}

pub fn constrain(mask: &mut [u8], pixels: &Rgba8, rect: Rect, strokes: &[Dab]) {
    let w = pixels.width() as usize;
    for dab in strokes {
        let x0 = (dab.at.0 - dab.radius).floor().max(rect.x0 as f32) as u32;
        let y0 = (dab.at.1 - dab.radius).floor().max(rect.y0 as f32) as u32;
        let x1 = ((dab.at.0 + dab.radius).ceil().max(0.0) as u32).min(rect.x1);
        let y1 = ((dab.at.1 + dab.radius).ceil().max(0.0) as u32).min(rect.y1);
        for y in y0..y1 {
            for x in x0..x1 {
                if (x as f32 + 0.5 - dab.at.0).powi(2) + (y as f32 + 0.5 - dab.at.1).powi(2)
                    <= dab.radius.powi(2)
                {
                    mask[y as usize * w + x as usize] = if dab.foreground { 255 } else { 0 };
                }
            }
        }
    }
    for (i, value) in mask.iter_mut().enumerate() {
        let (x, y) = ((i % w) as u32, (i / w) as u32);
        if x < rect.x0
            || x >= rect.x1
            || y < rect.y0
            || y >= rect.y1
            || pixels.as_bytes()[i * 4 + 3] == 0
        {
            *value = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matting_recovers_fractional_coverage_from_local_colours() {
        let mut image = Rgba8::new(40, 16, [230, 230, 230, 255]);
        let mut mask = vec![0; 40 * 16];
        for y in 0..16 {
            for x in 0..40 {
                let a = ((24.0 - x as f32) / 8.0).clamp(0.0, 1.0);
                let i = y * 40 + x;
                for (k, f) in [30.0, 80.0, 160.0].into_iter().enumerate() {
                    image.pixels_mut()[i * 4 + k] = (230.0 + a * (f - 230.0)).round() as u8;
                }
                mask[i] = if x < 20 { 255 } else { 0 };
            }
        }
        let output = matte(&image, &mask, Rect::new(0, 0, 40, 16), 5);
        for x in 16..=24 {
            let expected = ((24.0 - x as f32) / 8.0 * 255.0).round() as i32;
            assert!((output[8 * 40 + x] as i32 - expected).abs() <= 2);
        }
    }

    #[test]
    fn strokes_survive_matting_at_source_resolution_and_stay_inside_the_box() {
        let image = Rgba8::new(80, 60, [100, 100, 100, 255]);
        let mut mask = vec![128; 80 * 60];
        let rect = Rect::new(10, 10, 70, 50);
        constrain(
            &mut mask,
            &image,
            rect,
            &[
                Dab {
                    at: (20.5, 20.5),
                    radius: 0.6,
                    foreground: true,
                    stroke: 1,
                },
                Dab {
                    at: (21.5, 20.5),
                    radius: 0.6,
                    foreground: false,
                    stroke: 1,
                },
                Dab {
                    at: (2.0, 2.0),
                    radius: 3.0,
                    foreground: true,
                    stroke: 1,
                },
            ],
        );
        assert_eq!(mask[20 * 80 + 20], 255);
        assert_eq!(mask[20 * 80 + 21], 0);
        assert_eq!(mask[2 * 80 + 2], 0);
    }

    #[test]
    fn a_wider_detail_radius_reaches_further_across_a_soft_edge() {
        let (w, h) = (60usize, 20usize);
        let mut image = Rgba8::new(w as u32, h as u32, [240, 240, 240, 255]);
        let mut raw = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let a = ((34.0 - x as f32) / 20.0).clamp(0.0, 1.0);
                let i = y * w + x;
                for (k, f) in [20.0, 40.0, 60.0].into_iter().enumerate() {
                    image.pixels_mut()[i * 4 + k] = (240.0 + a * (f - 240.0)).round() as u8;
                }
                raw[i] = if a > 0.5 { 255 } else { 0 };
            }
        }
        let rect = Rect::new(0, 0, w as u32, h as u32);
        let settings = Settings {
            radius: 2.0,
            ..Settings::default()
        };
        let narrow = matte(&image, &raw, rect, settings.radius as u32);
        let wide = matte(&image, &raw, rect, 12);
        let soft = |mask: &[u8]| mask.iter().filter(|a| **a > 0 && **a < 255).count();
        assert!(
            soft(&narrow) * 2 < soft(&wide),
            "the wider radius mattes more of the ramp"
        );
        let expected = (16.0f32 / 20.0 * 255.0).round() as i32;
        assert!((wide[10 * w + 18] as i32 - expected).abs() <= 24);
        // Whatever the matte reads, the cut it hands back shows whole pixels.
        assert!(
            finish(&image, &raw, rect, &[], settings)
                .iter()
                .all(|a| *a == 0 || *a == 255)
        );
    }

    #[test]
    fn decontamination_removes_the_background_without_changing_source_alpha() {
        let image = Rgba8::from_raw(
            3,
            1,
            vec![20, 60, 100, 170, 130, 150, 170, 170, 240, 240, 240, 255],
        )
        .unwrap();
        let corrected = decontaminate(&image, &[255, 128, 0]);
        for (actual, expected) in corrected.as_bytes()[4..7].iter().zip([20, 60, 100]) {
            assert!((*actual as i32 - expected).abs() <= 1);
        }
        assert_eq!(corrected.as_bytes()[7], 170);
        assert_eq!(&corrected.as_bytes()[..4], &image.as_bytes()[..4]);
        assert_eq!(image.as_bytes()[4], 130);
    }

    #[test]
    fn a_cut_is_hard_until_feathered_and_respects_transparency_box_and_last_stroke() {
        let mut image = Rgba8::white(30, 20);
        image.pixels_mut()[(10 * 30 + 15) * 4 + 3] = 0;
        let rect = Rect::new(5, 5, 25, 15);
        let mut raw = vec![0; 30 * 20];
        for y in 6..14 {
            for x in 8..22 {
                raw[y * 30 + x] = 255;
            }
        }
        let strokes = [
            Dab {
                at: (10.5, 10.5),
                radius: 1.0,
                foreground: true,
                stroke: 1,
            },
            Dab {
                at: (10.5, 10.5),
                radius: 1.0,
                foreground: false,
                stroke: 1,
            },
        ];
        let settings = Settings {
            feather: 3.0,
            ..Settings::default()
        };
        let soft = finish(&image, &raw, rect, &strokes, settings);
        assert!(soft.iter().any(|&a| a > 0 && a < 255));
        assert_eq!(soft[10 * 30 + 10], 0);
        assert_eq!(soft[10 * 30 + 15], 0);
        assert_eq!(soft[4 * 30 + 12], 0);
        let hard = finish(&image, &raw, rect, &strokes, Settings::default());
        assert!(
            hard.iter().all(|&a| a == 0 || a == 255),
            "nothing softens the edge unless feather is asked for"
        );
    }

    #[test]
    fn shift_grows_and_shrinks_without_filling_the_whole_image() {
        let image = Rgba8::white(30, 20);
        let mut raw = vec![0; 600];
        for y in 5..15 {
            for x in 10..20 {
                raw[y * 30 + x] = 255;
            }
        }
        let run = |shift| {
            finish(
                &image,
                &raw,
                Rect::new(0, 0, 30, 20),
                &[],
                Settings {
                    radius: 0.0,
                    shift,
                    ..Settings::default()
                },
            )
        };
        assert_eq!(run(2.0)[10 * 30 + 8], 255);
        assert_eq!(run(-2.0)[10 * 30 + 11], 0);
        assert_eq!(run(-2.0)[10 * 30 + 15], 255);
        assert_eq!(run(2.0)[0], 0);
    }
}
