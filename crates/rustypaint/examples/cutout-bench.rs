#![allow(dead_code, unused_imports, reason = "standalone cutout benchmark")]
#[path = "../src/select/cutout/mod.rs"]
mod cutout;
#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/i18n/mod.rs"]
mod i18n;

use doc::Rect;
use std::{path::Path, time::Instant};

fn report(name: &str, method: &str, mask: &[u8], truth: &[u8], elapsed: f64) {
    let (mut tp, mut fp, mut fn_, mut bg) = (0usize, 0usize, 0usize, 0usize);
    for (&m, &t) in mask.iter().zip(truth) {
        if t == 128 {
            continue;
        }
        let selected = m > 128;
        if t == 255 {
            if selected {
                tp += 1;
            } else {
                fn_ += 1;
            }
        } else {
            bg += 1;
            if selected {
                fp += 1;
            }
        }
    }
    println!(
        "{name}\t{method}\t{:.3}\t{:.2}\t{:.2}\t{:.2}",
        elapsed,
        100.0 * tp as f64 / (tp + fp + fn_).max(1) as f64,
        100.0 * fn_ as f64 / (tp + fn_).max(1) as f64,
        100.0 * fp as f64 / bg.max(1) as f64
    );
}

const CORRECTIONS: u32 = 3;

// Where a person would most likely dab next: the middle of the biggest mistake, with a brush that
// fits inside it. Reference pixels marked uncertain are not mistakes.
fn worst(mask: &[u8], truth: &[u8], width: usize, round: u32) -> Option<cutout::refine::Dab> {
    let wrong: Vec<bool> = mask
        .iter()
        .zip(truth)
        .map(|(&m, &t)| t != 128 && (m > 128) != (t == 255))
        .collect();
    let height = mask.len() / width;
    let mut seen = vec![false; mask.len()];
    let mut best: Option<(usize, Vec<usize>)> = None;
    for start in 0..wrong.len() {
        if !wrong[start] || seen[start] {
            continue;
        }
        let mut open = vec![start];
        let mut group = Vec::new();
        seen[start] = true;
        while let Some(i) = open.pop() {
            group.push(i);
            let (x, y) = (i % width, i / width);
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx >= width || ny >= height {
                    continue;
                }
                let n = ny * width + nx;
                if wrong[n] && !seen[n] {
                    seen[n] = true;
                    open.push(n);
                }
            }
        }
        if best.as_ref().is_none_or(|(size, _)| group.len() > *size) {
            best = Some((group.len(), group));
        }
    }
    let (size, group) = best?;
    if size < 64 {
        return None;
    }
    let (sx, sy) = group.iter().fold((0usize, 0usize), |(sx, sy), i| {
        (sx + i % width, sy + i / width)
    });
    let (cx, cy) = (sx as f32 / size as f32, sy as f32 / size as f32);
    let at = group
        .iter()
        .min_by(|a, b| {
            let d = |i: usize| ((i % width) as f32 - cx).hypot((i / width) as f32 - cy);
            d(**a).total_cmp(&d(**b))
        })
        .copied()?;
    Some(cutout::refine::Dab {
        at: ((at % width) as f32 + 0.5, (at / width) as f32 + 0.5),
        radius: ((size as f32 / std::f32::consts::PI).sqrt() / 3.0).clamp(4.0, 48.0),
        foreground: truth[at] == 255,
        stroke: round,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        return Err("usage: cutout-bench DATASET BASELINE_EXECUTABLE BASELINE_MASK_DIRECTORY OUTPUT_DIRECTORY [IMAGE ...]".into());
    }
    let (dataset, baseline, old_masks, out) = (
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    );
    std::fs::create_dir_all(out)?;
    let model = cutout::model::Model::bundled()?;
    println!(
        "image\tmethod\tseconds\tIoU_percent\tmissed_subject_percent\tretained_background_percent"
    );
    let mut files = if args.len() > 4 {
        args[4..].to_vec()
    } else {
        std::fs::read_dir(dataset.join("data_GT"))?
            .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()?
    };
    files.sort();
    for file in files {
        let name = Path::new(&file).file_stem().unwrap().to_str().unwrap();
        let path = dataset.join("data_GT").join(&file);
        let pixels = doc::io::load(&path)?;
        let (w, h) = pixels.size();
        let truth =
            image::open(dataset.join("boundary_GT").join(format!("{name}.bmp")))?.to_luma8();
        if truth.dimensions() != (w, h) {
            return Err("Reference dimensions differ".into());
        }
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for (x, y, pixel) in truth.enumerate_pixels() {
            if pixel.0[0] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
        let margin = (w.max(h) / 20).max(5);
        let rect = Rect::new(
            x0.saturating_sub(margin),
            y0.saturating_sub(margin),
            x1.saturating_add(margin).min(w),
            y1.saturating_add(margin).min(h),
        );
        eprintln!("{name}: {w}x{h}, {rect:?}");
        let old = std::process::Command::new(baseline)
            .arg(&path)
            .args([
                rect.x0.to_string(),
                rect.y0.to_string(),
                rect.x1.to_string(),
                rect.y1.to_string(),
            ])
            .output()?;
        if !old.status.success() {
            return Err(String::from_utf8_lossy(&old.stderr).into_owned().into());
        }
        let old_time: f64 = String::from_utf8_lossy(&old.stdout)
            .lines()
            .find_map(|line| {
                line.split_once("all in ")
                    .and_then(|(_, s)| s.trim_end_matches('s').parse().ok())
            })
            .ok_or("No baseline timing")?;
        let old_mask = image::open(old_masks.join(format!("{name}-mask.png")))?.to_luma8();
        report(name, "old", old_mask.as_raw(), truth.as_raw(), old_time);
        let now = Instant::now();
        let mut colour = cutout::Cutout::new(&pixels, rect);
        colour.run(5);
        let colour_mask = cutout::refine::finish(
            &pixels,
            &colour.mask(),
            rect,
            &[],
            cutout::refine::Settings::default(),
        );
        report(
            name,
            "colour",
            &colour_mask,
            truth.as_raw(),
            now.elapsed().as_secs_f64(),
        );
        let now = Instant::now();
        let encoded = model.encode(&pixels, rect)?;
        let raw = encoded.select((w, h), rect, &[])?;
        let object_time = now.elapsed().as_secs_f64();
        report(name, "object_raw", &raw, truth.as_raw(), object_time);
        let settings = cutout::refine::Settings::default();
        let refined = cutout::refine::finish(&pixels, &raw, rect, &[], settings);
        report(
            name,
            "object_refined",
            &refined,
            truth.as_raw(),
            now.elapsed().as_secs_f64(),
        );
        let now = Instant::now();
        let repeated = encoded.select((w, h), rect, &[])?;
        assert_eq!(raw, repeated, "cached decoding must be deterministic");
        eprintln!("{name}: cached decoder {:.3}s", now.elapsed().as_secs_f64());
        let mut request = cutout::workflow::Request {
            rect,
            object: true,
            aim: cutout::workflow::Aim::default(),
            path: None,
            strokes: Vec::new(),
            settings,
        };
        let mut result = cutout::workflow::compute(&pixels, &request, None)?;
        let mut corrected = refined.clone();
        for round in 1..=CORRECTIONS {
            let Some(dab) = worst(&result.mask, truth.as_raw(), w as usize, round) else {
                break;
            };
            request.strokes.push(dab);
            let now = Instant::now();
            result = cutout::workflow::compute(&pixels, &request, Some(&result))?;
            corrected = result.mask.clone();
            report(
                name,
                &format!("object_corrected{round}"),
                &corrected,
                truth.as_raw(),
                now.elapsed().as_secs_f64(),
            );
        }
        for (method, mask) in [
            ("old", old_mask.as_raw()),
            ("colour", &colour_mask),
            ("object", &refined),
            ("corrected", &corrected),
        ] {
            image::GrayImage::from_raw(w, h, mask.clone())
                .unwrap()
                .save(out.join(format!("{name}-{method}-mask.png")))?;
            let mut composite = image::RgbImage::new(w, h);
            for (i, pixel) in composite.pixels_mut().enumerate() {
                let alpha = u32::from(mask[i]);
                for (c, bg) in [240u32, 100, 210].into_iter().enumerate() {
                    pixel.0[c] = ((u32::from(pixels.as_bytes()[i * 4 + c]) * alpha
                        + bg * (255 - alpha))
                        / 255) as u8;
                }
            }
            composite.save(out.join(format!("{name}-{method}.png")))?;
        }
    }
    Ok(())
}
