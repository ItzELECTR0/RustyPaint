#![allow(dead_code, unused_imports, reason = "standalone cutout evaluation")]
#[path = "../src/select/cutout/mod.rs"]
mod cutout;
#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/i18n/mod.rs"]
mod i18n;

use cutout::refine::Dab;

fn components(delta: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; delta.len()];
    let mut out = Vec::new();
    for start in 0..delta.len() {
        if !delta[start] || seen[start] {
            continue;
        }
        let mut open = vec![start];
        let mut group = Vec::new();
        seen[start] = true;
        while let Some(i) = open.pop() {
            group.push(i);
            let (x, y) = (i % w, i / w);
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                let n = ny * w + nx;
                if nx < w && ny < h && delta[n] && !seen[n] {
                    seen[n] = true;
                    open.push(n);
                }
            }
        }
        out.push(group);
    }
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = &args[0];
    let n: Vec<f32> = args[1..].iter().map(|s| s.parse().unwrap()).collect();
    let pixels = doc::io::load(std::path::Path::new(path))?;
    let (w, h) = pixels.size();
    let rect = doc::Rect::new(n[0] as u32, n[1] as u32, n[2] as u32, n[3] as u32).clamped(w, h);
    // stroke: x0 y0 x1 y1 radius adding
    let (sx0, sy0, sx1, sy1, radius, adding) = (n[4], n[5], n[6], n[7], n[8], n[9] > 0.5);
    let steps = ((sx1 - sx0).hypot(sy1 - sy0) / (radius * 0.4))
        .ceil()
        .max(1.0) as usize;
    let strokes: Vec<Dab> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            Dab {
                at: (sx0 + t * (sx1 - sx0), sy0 + t * (sy1 - sy0)),
                radius,
                foreground: adding,
                stroke: 1,
            }
        })
        .collect();
    let request = cutout::workflow::Request {
        rect,
        object: true,
        aim: cutout::workflow::Aim::default(),
        path: None,
        strokes: Vec::new(),
        settings: cutout::refine::Settings::default(),
    };
    let started = std::time::Instant::now();
    let base = cutout::workflow::compute(&pixels, &request, None)?;
    println!("base: {:?}", started.elapsed());
    let request = cutout::workflow::Request { strokes, ..request };
    let started = std::time::Instant::now();
    let corrected = cutout::workflow::compute(&pixels, &request, Some(&base))?;
    println!("corrected: {:?}", started.elapsed());
    let strokes = &request.strokes;
    let kept = |m: &[u8]| m.iter().filter(|v| **v > 128).count();
    let (w, h) = (w as usize, h as usize);
    let near = |i: usize| {
        let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
        strokes
            .iter()
            .map(|d| (d.at.0 - x).hypot(d.at.1 - y))
            .fold(f32::MAX, f32::min)
    };
    let delta: Vec<bool> = base
        .raw
        .iter()
        .zip(corrected.raw.iter())
        .map(|(b, p)| (*b > 128) != (*p > 128))
        .collect();
    let groups = components(&delta, w, h);
    println!(
        "kept {} -> {} ({} change components)",
        kept(&base.raw),
        kept(&corrected.raw),
        groups.len()
    );
    for g in groups.iter().take(8) {
        let d = g.iter().map(|i| near(*i)).fold(f32::MAX, f32::min);
        let adds = g.iter().filter(|i| corrected.raw[**i] > 128).count();
        println!(
            "  component {:>8} px, {:>6.0} px from stroke, {} added / {} removed",
            g.len(),
            d,
            adds,
            g.len() - adds
        );
    }
    let far: usize = groups
        .iter()
        .filter(|g| g.iter().map(|i| near(*i)).fold(f32::MAX, f32::min) > radius)
        .map(|g| g.len())
        .sum();
    let painted = (0..w * h).filter(|i| near(*i) <= radius).count();
    let changed: usize = groups.iter().map(|g| g.len()).sum();
    let reach = groups
        .iter()
        .flatten()
        .map(|i| near(*i))
        .fold(0.0f32, f32::max);
    println!("  unrelated change: {far} px");
    println!(
        "  painted {painted} px, changed {changed} px ({:.1}x), reaching {reach:.0} px out",
        changed as f32 / painted.max(1) as f32
    );
    let out = std::path::Path::new("target/cutout");
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for (name, mask) in [("base", &base.mask), ("corrected", &corrected.mask)] {
        let mut shot = pixels.clone();
        for (i, pixel) in shot
            .pixels_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let a = mask[i] as u32;
            let ground = if ((i % w) / 12 + (i / w) / 12).is_multiple_of(2) {
                90
            } else {
                140
            };
            for value in &mut pixel[..3] {
                *value = ((*value as u32 * a + ground * (255 - a)) / 255) as u8;
            }
        }
        for dab in strokes {
            let radius = dab.radius;
            for y in (dab.at.1 - radius) as i64..(dab.at.1 + radius) as i64 {
                for x in (dab.at.0 - radius) as i64..(dab.at.0 + radius) as i64 {
                    let d = (x as f32 + 0.5 - dab.at.0).hypot(y as f32 + 0.5 - dab.at.1);
                    if !(radius - 1.5..=radius).contains(&d) || x < 0 || y < 0 {
                        continue;
                    }
                    let (x, y) = (x as usize, y as usize);
                    if x < w && y < h {
                        let at = (y * w + x) * 4;
                        shot.pixels_mut()[at..at + 3].copy_from_slice(&if dab.foreground {
                            [40, 220, 60]
                        } else {
                            [240, 40, 40]
                        });
                    }
                }
            }
        }
        doc::io::save(&shot, &out.join(format!("stroke-{name}.png")))?;
    }
    Ok(())
}
