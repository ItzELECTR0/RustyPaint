#![allow(
    dead_code,
    unused_imports,
    reason = "whole modules are pulled in for one function each"
)]

#[path = "../src/assets.rs"]
mod assets;
#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/i18n/mod.rs"]
mod i18n;
#[path = "../src/paint/mod.rs"]
mod paint;

use doc::{Rgba8, image::CHANNELS};
use paint::curve::{self, CurveKind};
use paint::shapes::{self, ShapeKind, ShapeStyle};

const CELL: u32 = 220;
const PAD: u32 = 20;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "curves.png".into());
    let ink = ShapeStyle {
        fill: None,
        outline: Some([31, 90, 155, 255]),
        thickness: 6.0,
    };

    let mut sheet = Rgba8::white(CELL * 4, CELL * 2 + 230);

    for (col, kind) in curve::ALL.iter().enumerate() {
        let span = (CELL - PAD * 2) as f32;
        let from = (PAD as f32, span * 0.7);
        let to = (PAD as f32 + span, span * 0.3);

        let straight = curve::lay_out(*kind, from, to);
        blit(&mut sheet, &drawn(&straight, &ink), col as u32 * CELL, 0);

        let mut bent = straight.clone();
        for (i, point) in bent.iter_mut().enumerate() {
            if i > 0 && i < straight.len() - 1 {
                point.1 += if i % 2 == 0 { 60.0 } else { -60.0 };
            }
        }
        blit(&mut sheet, &drawn(&bent, &ink), col as u32 * CELL, CELL);
    }

    let filled = ShapeStyle {
        fill: None,
        outline: Some([31, 90, 155, 255]),
        thickness: 5.0,
    };
    let mut x = PAD;
    for (w, h) in [(160u32, 160u32), (400, 110), (110, 190)] {
        let drawn = shapes::render(ShapeKind::RoundedRectangle, &filled, w, h).unwrap();
        blit(&mut sheet, &drawn, x, CELL * 2 + PAD);
        x += w + PAD;
    }

    write(&sheet, &out);
    println!("four tools straight and bent, and three rounded rectangles, to {out}");
}

fn drawn(points: &[(f32, f32)], style: &ShapeStyle) -> Rgba8 {
    curve::render(points, style, (0.0, 0.0), CELL, CELL, false)
        .unwrap_or_else(|| Rgba8::transparent(CELL, CELL))
}

fn blit(sheet: &mut Rgba8, src: &Rgba8, x: u32, y: u32) {
    let (sw, sh) = src.size();
    let (dw, _) = sheet.size();
    let source = src.as_bytes().to_vec();
    let dest = sheet.pixels_mut();
    for row in 0..sh {
        for col in 0..sw {
            let s = ((row * sw + col) as usize) * CHANNELS;
            let alpha = source[s + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let d = (((y + row) * dw + x + col) as usize) * CHANNELS;
            for c in 0..3 {
                dest[d + c] = ((source[s + c] as u32 * alpha + dest[d + c] as u32 * (255 - alpha))
                    / 255) as u8;
            }
        }
    }
}

fn write(sheet: &Rgba8, path: &str) {
    let (w, h) = sheet.size();
    image::RgbaImage::from_raw(w, h, sheet.as_bytes().to_vec())
        .expect("sheet is the size it says")
        .save(path)
        .expect("could not write the sheet");
}
