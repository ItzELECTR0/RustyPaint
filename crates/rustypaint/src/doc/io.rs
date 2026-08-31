use super::Rgba8;
use image::ImageDecoder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

pub const READABLE: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "dds", "exr", "ff", "hdr", "ico",
    "pam", "pbm", "pgm", "pnm", "ppm", "qoi", "tga", "icns", "ora", "otb", "pcx", "sgi", "iris",
    "rgb", "rgba", "bw", "wbmp", "xbm", "bm", "xpm",
];

pub const WRITABLE: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff"];

pub fn load(path: &Path) -> Result<Rgba8, String> {
    image_extras::register();
    if has_header(path, b"icns")? {
        return load_icns(path);
    }

    let reader = image::ImageReader::open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?
        .with_guessed_format()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let name = path.file_name().unwrap_or_default().display().to_string();
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("cannot open {name}: {e}"))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("cannot open {name}: {e}"))?;
    img.apply_orientation(orientation);

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Rgba8::from_raw(w, h, rgba.into_raw()).ok_or_else(|| "image has an impossible size".into())
}

pub fn save(pixels: &Rgba8, path: &Path) -> Result<(), String> {
    let (w, h) = pixels.size();
    let buffer = image::RgbaImage::from_raw(w, h, pixels.as_bytes().to_vec())
        .ok_or("image has an impossible size")?;

    if flattens_alpha(path) {
        let flattened = flatten_onto_white(&buffer);
        flattened.save(path)
    } else {
        buffer.save(path)
    }
    .map_err(|e| format!("cannot save {}: {e}", path.display()))
}

pub fn flattens_alpha(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some("jpg" | "jpeg"))
}

pub fn extension(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_lowercase())
}

pub fn with_default_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension("png")
    }
}

fn has_header(path: &Path, expected: &[u8]) -> Result<bool, String> {
    let mut file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut header = vec![0; expected.len()];
    let read = file
        .read(&mut header)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(read == expected.len() && header == expected)
}

fn load_icns(path: &Path) -> Result<Rgba8, String> {
    let name = path.file_name().unwrap_or_default().display().to_string();
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let family = icns::IconFamily::read(BufReader::new(file))
        .map_err(|e| format!("cannot open {name}: {e}"))?;
    let icon_type = family
        .available_icons()
        .into_iter()
        .max_by_key(|kind| kind.pixel_width() * kind.pixel_height())
        .ok_or_else(|| format!("cannot open {name}: icon file contains no images"))?;
    let image = family
        .get_icon_with_type(icon_type)
        .map_err(|e| format!("cannot open {name}: {e}"))?
        .convert_to(icns::PixelFormat::RGBA);
    Rgba8::from_raw(image.width(), image.height(), image.into_data().into_vec())
        .ok_or_else(|| "image has an impossible size".into())
}

fn flatten_onto_white(src: &image::RgbaImage) -> image::RgbImage {
    let mut out = image::RgbImage::new(src.width(), src.height());
    for (dst, px) in out.pixels_mut().zip(src.pixels()) {
        let a = px[3] as u32;
        let over = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        *dst = image::Rgb([over(px[0]), over(px[1]), over(px[2])]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("rustypaint-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn sample() -> Rgba8 {
        let (w, h) = (16u32, 8u32);
        let mut px = Vec::with_capacity((w * h) as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let a = if x < w / 2 { 255 } else { 0 };
                px.extend_from_slice(&[(x * 16) as u8, (y * 32) as u8, 64, a]);
            }
        }
        Rgba8::from_raw(w, h, px).unwrap()
    }

    #[test]
    fn png_round_trips_byte_for_byte() {
        let original = sample();
        let path = scratch("round-trip.png");
        save(&original, &path).unwrap();
        let reloaded = load(&path).unwrap();

        assert_eq!(reloaded.size(), original.size());
        assert_eq!(reloaded.as_bytes(), original.as_bytes());
    }

    #[test]
    fn jpeg_flattens_transparency_onto_white() {
        let path = scratch("flattened.jpg");
        assert!(flattens_alpha(&path));
        save(&sample(), &path).unwrap();

        let reloaded = load(&path).unwrap();
        assert!(
            reloaded
                .as_bytes()
                .iter()
                .skip(3)
                .step_by(4)
                .all(|&a| a == 255)
        );
        let last = reloaded.as_bytes().len() - 4;
        let right_edge = &reloaded.as_bytes()[last..last + 3];
        assert!(right_edge.iter().all(|&c| c > 200), "got {right_edge:?}");
    }

    #[test]
    fn the_format_comes_from_the_contents_not_the_extension() {
        let original = sample();
        let honest = scratch("liar.webp");
        let buffer = image::RgbaImage::from_raw(
            original.width(),
            original.height(),
            original.as_bytes().to_vec(),
        )
        .unwrap();
        buffer.save(&honest).unwrap();

        let liar = scratch("liar.jpg");
        std::fs::copy(&honest, &liar).unwrap();

        let loaded = load(&liar).expect("a mislabelled WebP should still open");
        assert_eq!(loaded.size(), original.size());
    }

    #[test]
    fn extra_formats_use_their_headers_when_available() {
        let path = scratch("xpm-without-extension.data");
        std::fs::write(
            &path,
            b"/* XPM */\nstatic char *icon[] = {\n\"2 1 2 1\",\n\". c #ff0000\",\n\"  c None\",\n\". \"\n};\n",
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.size(), (2, 1));
        assert_eq!(&loaded.as_bytes()[..4], &[255, 0, 0, 255]);
        assert_eq!(&loaded.as_bytes()[4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn a_photograph_is_opened_the_way_up_it_was_taken() {
        let original = sample();
        let plain = scratch("upright-plain.jpg");
        save(&original, &plain).unwrap();

        let app1: [u8; 36] = [
            0xFF, 0xE1, 0x00, 0x22, // APP1, 34 bytes of it
            b'E', b'x', b'i', b'f', 0x00, 0x00, b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00,
            0x00, // little endian, IFD0 at 8
            0x01, 0x00, // one entry
            0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, // no IFD after it
        ];
        let jpeg = std::fs::read(&plain).unwrap();
        let mut turned = Vec::with_capacity(jpeg.len() + app1.len());
        turned.extend_from_slice(&jpeg[..2]);
        turned.extend_from_slice(&app1);
        turned.extend_from_slice(&jpeg[2..]);
        let path = scratch("upright.jpg");
        std::fs::write(&path, &turned).unwrap();

        let (w, h) = original.size();
        assert_eq!(load(&plain).unwrap().size(), (w, h), "no tag, no turn");
        assert_eq!(
            load(&path).unwrap().size(),
            (h, w),
            "the tag says stand it up"
        );
    }

    #[test]
    fn a_missing_extension_becomes_png() {
        assert_eq!(
            with_default_extension("shot".into()),
            PathBuf::from("shot.png")
        );
        assert_eq!(
            with_default_extension("shot.bmp".into()),
            PathBuf::from("shot.bmp")
        );
    }
}
