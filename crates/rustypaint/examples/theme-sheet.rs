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
#[path = "../src/ui/theme/mod.rs"]
mod theme;

use doc::Rgba8;
use text::{TextBox, TextStyle};
use theme::{Mode, Palette, Scheme};

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

const PANEL: (f32, f32) = (520.0, 340.0);
const GAP: f32 = 24.0;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("theme.png"));

    let width = (PANEL.0 * 2.0 + GAP * 3.0) as u32;
    let height = (PANEL.1 * 2.0 + GAP * 3.0 + 28.0) as u32;
    let mut sheet = Pixmap::new(width, height).expect("sheet");
    sheet.fill(Color::from_rgba8(120, 120, 120, 255));

    for (row, mode) in [Mode::Light, Mode::Dark].into_iter().enumerate() {
        for (column, scheme) in Scheme::ALL.into_iter().enumerate() {
            let x = GAP + column as f32 * (PANEL.0 + GAP);
            let y = GAP + row as f32 * (PANEL.1 + GAP);
            panel(
                &mut sheet,
                theme::palette_for(mode, scheme),
                mode,
                scheme,
                x,
                y,
            );
            report(theme::palette_for(mode, scheme), mode, scheme);
        }
    }

    sheet.save_png(&out).expect("write the sheet");
    println!("\nwrote {}", out.display());
}

fn panel(sheet: &mut Pixmap, c: &Palette, mode: Mode, scheme: Scheme, x: f32, y: f32) {
    let (w, h) = PANEL;

    for row in 0..h as u32 {
        let t = row as f32 / h;
        fill(
            sheet,
            x,
            y + row as f32,
            w,
            1.0,
            mix(c.workspace_top, c.workspace_bottom, t),
        );
    }

    let strip = 48.0;
    fill(sheet, x, y, w, strip, c.top_bar);
    label(sheet, "Brushes", x + 16.0, y + 16.0, 13.0, c.text_on_dark);
    let tab = (x + 92.0, 68.0);
    wash(
        sheet,
        tab.0,
        y,
        tab.1,
        strip,
        c.selection_from,
        c.selection_to,
    );
    label(
        sheet,
        "Shapes",
        tab.0 + 8.0,
        y + 16.0,
        13.0,
        c.selection_text,
    );
    label(
        sheet,
        "Text",
        tab.0 + tab.1 + 16.0,
        y + 16.0,
        13.0,
        c.text_on_dark,
    );

    let canvas = (x + 40.0, y + strip + 40.0, 210.0, 150.0);
    let shade = Color::from_rgba(0.0, 0.0, 0.0, c.shadow).unwrap_or(Color::BLACK);
    fill_colour(
        sheet,
        canvas.0 + 3.0,
        canvas.1 + 5.0,
        canvas.2,
        canvas.3,
        shade,
    );
    checkerboard(sheet, canvas, c);

    let side = (x + w - 200.0, y + strip, 200.0, h - strip);
    fill(sheet, side.0, side.1, side.2, side.3, c.side_panel);
    label(
        sheet,
        "Marker",
        side.0 + 18.0,
        side.1 + 20.0,
        20.0,
        c.accent_text,
    );
    label(
        sheet,
        "Thickness",
        side.0 + 18.0,
        side.1 + 56.0,
        13.0,
        c.text,
    );
    label(
        sheet,
        "24 px",
        side.0 + 140.0,
        side.1 + 56.0,
        13.0,
        c.text_dim,
    );

    let track = (side.0 + 18.0, side.1 + 84.0, 164.0, 4.0);
    fill(sheet, track.0, track.1, track.2, track.3, c.border);
    fill(sheet, track.0, track.1, track.2 * 0.6, track.3, c.accent);
    disc(sheet, track.0 + track.2 * 0.6, track.1 + 2.0, 7.0, c.accent);

    fill(sheet, side.0 + 18.0, side.1 + 112.0, 76.0, 30.0, c.control);
    label(sheet, "Fill", side.0 + 34.0, side.1 + 120.0, 13.0, c.text);
    wash(
        sheet,
        side.0 + 106.0,
        side.1 + 112.0,
        76.0,
        30.0,
        c.selection_from,
        c.selection_to,
    );
    label(
        sheet,
        "Line",
        side.0 + 122.0,
        side.1 + 120.0,
        13.0,
        c.selection_text,
    );

    label(
        sheet,
        "The quick brown fox.",
        side.0 + 18.0,
        side.1 + 164.0,
        13.0,
        c.text,
    );
    label(
        sheet,
        "Jumps over the lazy dog.",
        side.0 + 18.0,
        side.1 + 186.0,
        13.0,
        c.text_dim,
    );

    let title = format!("{mode:?} / {scheme:?}");
    label(sheet, &title, x + 40.0, y + h - 40.0, 15.0, c.text_on_dark);
}

fn checkerboard(sheet: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), c: &Palette) {
    let square: f32 = 8.0;
    let mut row = 0;
    let mut cy = y;
    while cy < y + h {
        let mut column = 0;
        let mut cx = x;
        while cx < x + w {
            let dark = (row + column) % 2 == 1;
            let tile = if dark {
                c.checker_dark
            } else {
                c.checker_light
            };
            fill(
                sheet,
                cx,
                cy,
                square.min(x + w - cx),
                square.min(y + h - cy),
                tile,
            );
            cx += square;
            column += 1;
        }
        cy += square;
        row += 1;
    }
    fill(sheet, x, y, w / 2.0, h, c.canvas);
}

fn report(c: &Palette, mode: Mode, scheme: Scheme) {
    println!("\n{mode:?} / {scheme:?}");
    for (name, fg, bg) in [
        ("text on panel", c.text, c.side_panel),
        ("dim on panel", c.text_dim, c.side_panel),
        ("accent label", c.accent_text, c.side_panel),
        ("top bar text", c.text_on_dark, c.top_bar),
        ("wash text, left", c.selection_text, c.selection_from),
        ("wash text, right", c.selection_text, c.selection_to),
    ] {
        let ratio = contrast(fg, bg);
        let verdict = if ratio >= 4.5 { "ok" } else { "TOO LOW" };
        println!("  {name:<18} {ratio:>5.2}:1  {verdict}");
    }
}

fn contrast(a: iced::Color, b: iced::Color) -> f32 {
    fn luminance(c: iced::Color) -> f32 {
        fn channel(v: f32) -> f32 {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn label(sheet: &mut Pixmap, content: &str, x: f32, y: f32, size: f32, colour: iced::Color) {
    let style = TextStyle {
        size,
        colour: bytes(colour),
        background: false,
        ..TextStyle::default()
    };
    let mut boxed = TextBox::restyled_from(content, style, 400.0);
    boxed.set_width(400.0);
    let height = (size * 1.6).ceil() as u32;
    let Some(drawn) = boxed.render(400, height, false, false) else {
        return;
    };
    blit(sheet, &drawn, x, y);
}

fn blit(sheet: &mut Pixmap, source: &Rgba8, x: f32, y: f32) {
    let (sw, sh) = source.size();
    let bytes = source.as_bytes();
    let (ox, oy) = (x.round() as i32, y.round() as i32);
    for row in 0..sh {
        for column in 0..sw {
            let i = ((row * sw + column) * 4) as usize;
            let a = bytes[i + 3] as f32 / 255.0;
            if a <= 0.0 {
                continue;
            }
            let (px, py) = (ox + column as i32, oy + row as i32);
            if px < 0 || py < 0 || px >= sheet.width() as i32 || py >= sheet.height() as i32 {
                continue;
            }
            let target = ((py as u32 * sheet.width() + px as u32) * 4) as usize;
            let data = sheet.data_mut();
            for channel in 0..3 {
                let over = bytes[i + channel] as f32 * a;
                let under = data[target + channel] as f32 * (1.0 - a);
                data[target + channel] = (over + under).round().clamp(0.0, 255.0) as u8;
            }
            data[target + 3] = 255;
        }
    }
}

fn fill(sheet: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, colour: iced::Color) {
    fill_colour(sheet, x, y, w, h, skia(colour));
}

fn fill_colour(sheet: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, colour: Color) {
    let Some(rect) = Rect::from_xywh(x, y, w.max(0.0), h.max(0.0)) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(colour);
    paint.anti_alias = false;
    sheet.fill_rect(rect, &paint, Transform::identity(), None);
}

fn wash(sheet: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, from: iced::Color, to: iced::Color) {
    for column in 0..w as u32 {
        let t = column as f32 / w;
        fill(sheet, x + column as f32, y, 1.0, h, mix(from, to, t));
    }
}

fn disc(sheet: &mut Pixmap, x: f32, y: f32, r: f32, colour: iced::Color) {
    let mut builder = PathBuilder::new();
    builder.push_circle(x, y, r);
    let Some(path) = builder.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color(skia(colour));
    paint.anti_alias = true;
    sheet.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

fn skia(c: iced::Color) -> Color {
    Color::from_rgba(c.r, c.g, c.b, c.a).unwrap_or(Color::BLACK)
}

fn bytes(c: iced::Color) -> [u8; 4] {
    [
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
        (c.a * 255.0).round() as u8,
    ]
}
