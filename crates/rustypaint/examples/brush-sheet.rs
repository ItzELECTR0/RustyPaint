#![allow(
    dead_code,
    unused_imports,
    reason = "whole modules are pulled in for one function each"
)]

#[path = "../src/assets.rs"]
mod assets;
#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/paint/mod.rs"]
mod paint;

use doc::Document;
use paint::brush::{MAX_THICKNESS, PANEL_ORDER};
use paint::{Brush, Stroke, Tool};

const ROW: u32 = 92;
const WIDTH: u32 = 620;
const MARGIN: f32 = 40.0;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "brushes.png".into());

    let strokes: Vec<Tool> = PANEL_ORDER
        .iter()
        .copied()
        .filter(|t| t.profile().is_some())
        .collect();
    let height = ROW * strokes.len() as u32;

    let mut doc = Document::blank_sized(WIDTH, height, false);

    if let Some(row) = strokes.iter().position(|t| *t == Tool::Eraser) {
        let band = Brush {
            tool: Tool::Marker,
            thickness: MAX_THICKNESS,
            opacity: 1.0,
            colour: [150, 170, 190, 255],
            ..Default::default()
        };
        let y = ROW as f32 * (row as f32 + 0.5);
        let mut stroke = Stroke::begin(band, &doc, 0.0, y);
        stroke.extend(WIDTH as f32, y);
        stroke.flush(&mut doc);
    }

    for (i, tool) in strokes.iter().copied().enumerate() {
        let y = ROW as f32 * (i as f32 + 0.5);
        let brush = Brush {
            tool,
            thickness: if tool == Tool::PixelPen { 3.0 } else { 34.0 },
            opacity: 1.0,
            colour: [20, 20, 20, 255],
            ..Default::default()
        };

        let mut stroke = Stroke::begin(brush, &doc, MARGIN, y);
        for step in 0..=220 {
            let t = step as f32 / 220.0;
            let x = MARGIN + t * (WIDTH as f32 - MARGIN * 2.0);
            stroke.extend(x, y + (t * 9.0).sin() * (ROW as f32 * 0.22));
        }
        if tool.sprays() {
            for step in 0..=60 {
                let t = step as f32 / 60.0;
                stroke.puff(MARGIN + t * (WIDTH as f32 - MARGIN * 2.0), y);
            }
        }
        stroke.flush(&mut doc);
    }

    let flattened = doc.flattened();
    doc::io::save(&flattened, std::path::Path::new(&path)).expect("could not write the sheet");
    println!("{} strokes -> {path}", strokes.len());
}
