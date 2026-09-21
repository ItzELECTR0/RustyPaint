#![allow(dead_code, unused_imports, reason = "standalone cutout evaluation")]
#[path = "../src/select/cutout/mod.rs"]
mod cutout;
#[path = "../src/doc/mod.rs"]
mod doc;
#[path = "../src/i18n/mod.rs"]
mod i18n;

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("usage: cutout-model IMAGE [x0 y0 x1 y1]")?;
    let pixels = doc::io::load(std::path::Path::new(path))?;
    let (w, h) = pixels.size();
    let rect = if args.len() >= 5 {
        let n: Vec<u32> = args[1..5].iter().map(|s| s.parse().unwrap()).collect();
        doc::Rect::new(n[0], n[1], n[2], n[3]).clamped(w, h)
    } else {
        doc::Rect::new(0, 0, w, h)
    };
    let started = std::time::Instant::now();
    let model = cutout::model::Model::bundled()?;
    println!("Model loaded: {:?}", started.elapsed());
    let started = std::time::Instant::now();
    let encoded = model.encode(&pixels, rect)?;
    println!("Encoded: {:?}", started.elapsed());
    let started = std::time::Instant::now();
    let raw = encoded.select(pixels.size(), rect, &[])?;
    println!("Decoded: {:?}", started.elapsed());
    let started = std::time::Instant::now();
    let mask = cutout::refine::finish(
        &pixels,
        &raw,
        rect,
        &[],
        cutout::refine::Settings::default(),
    );
    println!("Refined: {:?}", started.elapsed());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/cutout");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = std::path::Path::new(path)
        .file_stem()
        .unwrap()
        .to_string_lossy();
    image::GrayImage::from_raw(w, h, mask.clone())
        .unwrap()
        .save(dir.join(format!("{name}-model-mask.png")))
        .map_err(|e| e.to_string())?;
    let mut output = image::RgbaImage::from_raw(w, h, pixels.as_bytes().to_vec()).unwrap();
    for (i, pixel) in output.pixels_mut().enumerate() {
        pixel.0[3] = (u16::from(pixel.0[3]) * u16::from(mask[i]) / 255) as u8;
    }
    output
        .save(dir.join(format!("{name}-model.png")))
        .map_err(|e| e.to_string())?;
    Ok(())
}
