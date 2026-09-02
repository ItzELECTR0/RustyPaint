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
use paint::shapes::{self, ShapeStyle};

const CELL: u32 = 104;
const SHAPE: u32 = 84;
const COLS: u32 = 10;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "shapes.png".into());

    let styles = [
        ShapeStyle {
            fill: Some([0x2b, 0x6c, 0xb0, 0xff]),
            outline: None,
            thickness: 6.0,
        },
        ShapeStyle {
            fill: None,
            outline: Some([0x22, 0x22, 0x55, 0xff]),
            thickness: 6.0,
        },
    ];
    let cells = shapes::ALL.len() as u32 * styles.len() as u32;
    let rows = cells.div_ceil(COLS);

    let (w, h) = (COLS * CELL, rows * CELL);
    let mut sheet = Rgba8::new(w, h, [255, 255, 255, 255]);

    for (i, (kind, style)) in shapes::ALL
        .iter()
        .flat_map(|k| styles.iter().map(move |s| (*k, s)))
        .enumerate()
    {
        let Some(drawn) = shapes::render(kind, style, SHAPE, SHAPE) else {
            eprintln!("{} did not render", kind.name());
            continue;
        };
        let i = i as u32;
        let (ox, oy) = (
            (i % COLS) * CELL + (CELL - SHAPE) / 2,
            (i / COLS) * CELL + (CELL - SHAPE) / 2,
        );
        blit(&mut sheet, &drawn, ox, oy);
    }

    let (sw, sh) = sheet.size();
    image::save_buffer(&out, sheet.as_bytes(), sw, sh, image::ColorType::Rgba8)
        .expect("the sheet is written");
    println!("{} shapes, two ways each, to {out}", shapes::ALL.len());
}

fn blit(dest: &mut Rgba8, src: &Rgba8, ox: u32, oy: u32) {
    let (dw, _) = dest.size();
    let (sw, sh) = src.size();
    let source = src.as_bytes().to_vec();
    let bytes = dest.pixels_mut();
    for y in 0..sh {
        for x in 0..sw {
            let s = ((y * sw + x) as usize) * CHANNELS;
            let a = source[s + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = (((y + oy) * dw + x + ox) as usize) * CHANNELS;
            for c in 0..3 {
                let over = source[s + c] as u32 * a + bytes[d + c] as u32 * (255 - a);
                bytes[d + c] = (over / 255) as u8;
            }
        }
    }
}
