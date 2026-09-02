#![allow(
    dead_code,
    unused_imports,
    reason = "whole modules are pulled in for one function each"
)]

#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/i18n/mod.rs"]
mod i18n;
#[path = "../src/text/mod.rs"]
mod text;

use doc::{Rgba8, image::CHANNELS};
use text::{Align, TextBox, TextStyle};

const WIDTH: u32 = 760;
const ROW: u32 = 76;
const PAD: u32 = 16;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "text.png".into());
    let base = TextStyle {
        size: 40.0,
        ..Default::default()
    };

    let rows: Vec<(String, TextStyle, &str, bool)> = vec![
        ("Regular".into(), base.clone(), "Writing Test", false),
        (
            "Bold".into(),
            TextStyle {
                bold: true,
                ..base.clone()
            },
            "Writing Test",
            false,
        ),
        (
            "Italic".into(),
            TextStyle {
                italic: true,
                ..base.clone()
            },
            "Writing Test",
            false,
        ),
        (
            "Underline".into(),
            TextStyle {
                underline: true,
                ..base.clone()
            },
            "Writing Test",
            false,
        ),
        (
            "Background fill".into(),
            TextStyle {
                background: true,
                ..base.clone()
            },
            "Writing Test",
            false,
        ),
        (
            "Left".into(),
            TextStyle {
                align: Align::Left,
                ..base.clone()
            },
            "aligned",
            false,
        ),
        (
            "Centre".into(),
            TextStyle {
                align: Align::Centre,
                ..base.clone()
            },
            "aligned",
            false,
        ),
        (
            "Right".into(),
            TextStyle {
                align: Align::Right,
                ..base.clone()
            },
            "aligned",
            false,
        ),
        ("Caret".into(), base.clone(), "typing", true),
    ];

    let wrapped = TextBox::restyled_from(
        "the quick brown fox jumps over the lazy dog",
        TextStyle {
            size: 28.0,
            ..base.clone()
        },
        360.0,
    );

    let height = ROW * rows.len() as u32 + wrapped.height().ceil() as u32 + PAD * 3;
    let mut sheet = Rgba8::white(WIDTH, height);

    for (i, (label, style, body, caret)) in rows.iter().enumerate() {
        let boxed = TextBox::restyled_from(body, style.clone(), (WIDTH - PAD * 2) as f32);
        let drawn = boxed
            .render(WIDTH - PAD * 2, ROW - 8, *caret, *caret)
            .expect("the row draws");
        blit(&mut sheet, &drawn, PAD, i as u32 * ROW + PAD / 2);
        eprintln!("{label}");
    }

    let y = ROW * rows.len() as u32 + PAD;
    let drawn = wrapped
        .render(360, wrapped.height().ceil() as u32, false, false)
        .unwrap();
    blit(&mut sheet, &drawn, PAD, y);

    write(&sheet, &out);
    println!("every text style, and a wrapped paragraph, to {out}");
}

fn blit(sheet: &mut Rgba8, src: &Rgba8, x: u32, y: u32) {
    let (sw, sh) = src.size();
    let (dw, dh) = sheet.size();
    let source = src.as_bytes().to_vec();
    let dest = sheet.pixels_mut();
    for row in 0..sh {
        for col in 0..sw {
            if x + col >= dw || y + row >= dh {
                continue;
            }
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
