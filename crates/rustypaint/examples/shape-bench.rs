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

use paint::shapes::{self, ShapeKind, ShapeStyle};
use std::time::Instant;

fn main() {
    let style = ShapeStyle {
        fill: Some([255, 0, 0, 255]),
        outline: Some([0, 0, 0, 255]),
        thickness: 8.0,
    };
    for kind in [
        ShapeKind::Rectangle,
        ShapeKind::Heart,
        ShapeKind::ThoughtBubble,
    ] {
        for size in [128u32, 512, 1024] {
            let n = 20;
            let t = Instant::now();
            for i in 0..n {
                std::hint::black_box(shapes::render(kind, &style, size + i, size).unwrap());
            }
            println!(
                "{:>16} {size:>5}px  {:>10?}/frame",
                kind.name(),
                t.elapsed() / n
            );
        }
    }
}
