use tiny_skia::{Pixmap, Transform};

const SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

const SIMPLE_BELOW: u32 = 32;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../res");
    let full = load(&root.join("icon.svg"));
    let small = load(&root.join("icon-small.svg"));

    std::fs::create_dir_all(root.join("icons")).expect("make res/icons");
    let mut drawn = Vec::new();
    for &size in SIZES {
        let tree = if size < SIMPLE_BELOW { &small } else { &full };
        let pixmap = render(tree, size);
        let path = root.join("icons").join(format!("icon-{size}.png"));
        pixmap.save_png(&path).expect("write a size");
        println!("{} ({size} px)", path.display());
        drawn.push((size, pixmap));
    }

    let mut args = std::env::args().skip(1);
    if let Some(flag) = args.next()
        && flag == "--sheet"
        && let Some(path) = args.next()
    {
        sheet(&drawn, &path);
        println!("{path} (every size, and each one doubled)");
    }
}

fn load(path: &std::path::Path) -> usvg::Tree {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    usvg::Tree::from_data(&bytes, &usvg::Options::default()).expect("parse the icon")
}

fn render(tree: &usvg::Tree, size: u32) -> Pixmap {
    let mut pixmap = Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 / tree.size().width();
    resvg_render(tree, &mut pixmap, Transform::from_scale(scale, scale));
    pixmap
}

fn resvg_render(tree: &usvg::Tree, pixmap: &mut Pixmap, transform: Transform) {
    fn walk(group: &usvg::Group, pixmap: &mut Pixmap, transform: Transform) {
        let transform = transform.pre_concat(group.transform());
        let clip = group
            .clip_path()
            .and_then(|clip| mask(clip, pixmap, transform));
        for node in group.children() {
            match node {
                usvg::Node::Path(path) => draw(path, pixmap, transform, clip.as_ref()),
                usvg::Node::Group(inner) => walk(inner, pixmap, transform),
                _ => {}
            }
        }
    }

    fn mask(
        clip: &usvg::ClipPath,
        pixmap: &Pixmap,
        transform: Transform,
    ) -> Option<tiny_skia::Mask> {
        let mut mask = tiny_skia::Mask::new(pixmap.width(), pixmap.height())?;
        let transform = transform.pre_concat(clip.transform());
        for node in clip.root().children() {
            if let usvg::Node::Path(path) = node {
                mask.fill_path(path.data(), tiny_skia::FillRule::Winding, true, transform);
            }
        }
        Some(mask)
    }

    fn draw(
        path: &usvg::Path,
        pixmap: &mut Pixmap,
        transform: Transform,
        clip: Option<&tiny_skia::Mask>,
    ) {
        let Some(fill) = path.fill() else { return };
        let mut paint = tiny_skia::Paint {
            anti_alias: true,
            ..Default::default()
        };
        match fill.paint() {
            usvg::Paint::Color(c) => {
                paint.set_color_rgba8(c.red, c.green, c.blue, fill.opacity().to_u8());
            }
            usvg::Paint::LinearGradient(g) => {
                let stops = g
                    .stops()
                    .iter()
                    .map(|s| {
                        let c = s.color();
                        tiny_skia::GradientStop::new(
                            s.offset().get(),
                            tiny_skia::Color::from_rgba8(c.red, c.green, c.blue, 255),
                        )
                    })
                    .collect();
                let Some(shader) = tiny_skia::LinearGradient::new(
                    tiny_skia::Point::from_xy(g.x1(), g.y1()),
                    tiny_skia::Point::from_xy(g.x2(), g.y2()),
                    stops,
                    tiny_skia::SpreadMode::Pad,
                    g.transform(),
                ) else {
                    return;
                };
                paint.shader = shader;
            }
            _ => return,
        }
        pixmap.fill_path(
            path.data(),
            &paint,
            tiny_skia::FillRule::Winding,
            transform,
            clip,
        );
    }

    walk(tree.root(), pixmap, transform);
}

fn sheet(drawn: &[(u32, Pixmap)], path: &str) {
    const PAD: u32 = 12;
    let width: u32 = drawn.iter().map(|(s, _)| s * 2 + PAD).sum::<u32>() + PAD;
    let height = drawn.iter().map(|(s, _)| s).max().copied().unwrap_or(256) * 3 + PAD * 3;
    let mut sheet = Pixmap::new(width, height).expect("sheet");
    sheet.fill(tiny_skia::Color::from_rgba8(238, 238, 238, 255));

    let mut x = PAD;
    for (size, pixmap) in drawn {
        blit(&mut sheet, pixmap, x, PAD);
        let doubled = Pixmap::from_vec(
            pixmap.data().to_vec(),
            tiny_skia::IntSize::from_wh(*size, *size).expect("size"),
        );
        if let Some(doubled) = doubled {
            let mut big = Pixmap::new(size * 2, size * 2).expect("doubled");
            big.draw_pixmap(
                0,
                0,
                doubled.as_ref(),
                &tiny_skia::PixmapPaint {
                    quality: tiny_skia::FilterQuality::Nearest,
                    ..Default::default()
                },
                Transform::from_scale(2.0, 2.0),
                None,
            );
            blit(&mut sheet, &big, x, PAD * 2 + 256);
        }
        x += size * 2 + PAD;
    }
    sheet.save_png(path).expect("write the sheet");
}

fn blit(sheet: &mut Pixmap, source: &Pixmap, x: u32, y: u32) {
    sheet.draw_pixmap(
        x as i32,
        y as i32,
        source.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}
