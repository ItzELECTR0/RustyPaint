#![allow(
    dead_code,
    unused_imports,
    reason = "whole modules are pulled in for one function each"
)]

#[path = "../src/select/cutout/mod.rs"]
mod cutout;
#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/i18n/mod.rs"]
mod i18n;

use cutout::Cutout;
use doc::{Rect, Rgba8};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: cutout <image> [x0 y0 x1 y1]");
        std::process::exit(2);
    };

    let image = doc::io::load(std::path::Path::new(&path)).expect("an image");
    let (w, h) = image.size();
    let pixels = image;

    let rest: Vec<String> = args.collect();
    let numbers: Vec<u32> = rest.iter().filter_map(|a| a.parse().ok()).collect();
    let dabs: Vec<(bool, f32, f32, f32)> = rest
        .iter()
        .filter(|a| a.starts_with('+') || a.starts_with('-'))
        .filter_map(|a| {
            let adding = a.starts_with('+');
            let mut parts = a[1..].split(',').filter_map(|p| p.parse::<f32>().ok());
            Some((adding, parts.next()?, parts.next()?, parts.next()?))
        })
        .collect();
    let rect = if numbers.len() == 4 {
        Rect::new(numbers[0], numbers[1], numbers[2], numbers[3])
    } else {
        Rect::new(w / 20, h / 20, w - w / 20, h - h / 20)
    };
    println!("{path}: {w} by {h}, box {rect:?}");

    let started = Instant::now();
    let mut cutout = Cutout::new(&pixels, rect);
    let setup = started.elapsed();
    let (ww, wh) = cutout.size();

    let cutting = Instant::now();
    cutout.run(3);
    let cut = cutting.elapsed();

    if !dabs.is_empty() {
        let brushing = Instant::now();
        for (adding, x, y, r) in &dabs {
            cutout.paint((*x, *y), *r, *adding);
        }
        cutout.recut();
        println!(
            "  {} dabs, another pass in {:.0}ms",
            dabs.len(),
            brushing.elapsed().as_secs_f32() * 1000.0
        );
    }

    let lifting = Instant::now();
    let mask = cutout.refined_mask(&pixels);
    let lift = lifting.elapsed();

    let kept = mask.iter().filter(|m| **m > 128).count();
    println!(
        "  worked at {ww} by {wh}: setup {:.0}ms, three passes {:.0}ms, edge {:.0}ms",
        setup.as_secs_f32() * 1000.0,
        cut.as_secs_f32() * 1000.0,
        lift.as_secs_f32() * 1000.0,
    );
    println!(
        "  kept {:.1}% of the picture, all in {:.2}s",
        kept as f32 / mask.len() as f32 * 100.0,
        started.elapsed().as_secs_f32()
    );

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cutout");
    std::fs::create_dir_all(&dir).expect("somewhere to write");
    let name = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "cut".into());

    let grey = image::GrayImage::from_raw(w, h, mask.clone()).expect("a mask");
    grey.save(dir.join(format!("{name}-mask.png")))
        .expect("the mask");
    let raw = image::GrayImage::from_raw(w, h, cutout.mask()).expect("a mask");
    raw.save(dir.join(format!("{name}-mask-raw.png")))
        .expect("the raw mask");

    let mut out = image::RgbaImage::new(w, h);
    let source = pixels.as_bytes();
    for (i, pixel) in out.pixels_mut().enumerate() {
        let (x, y) = (i % w as usize, i / w as usize);
        let ground = if ((x / 8) + (y / 8)) % 2 == 0 {
            190u8
        } else {
            150
        };
        *pixel = if mask[i] > 128 {
            image::Rgba([source[i * 4], source[i * 4 + 1], source[i * 4 + 2], 255])
        } else {
            image::Rgba([ground, ground, ground, 255])
        };
    }
    out.save(dir.join(format!("{name}-cut.png")))
        .expect("the cut");
    println!("  wrote {}", dir.join(format!("{name}-cut.png")).display());
}
