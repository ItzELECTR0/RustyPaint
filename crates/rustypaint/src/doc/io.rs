use super::Rgba8;
use image::ImageDecoder;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};

pub const READABLE: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "dds", "exr", "ff", "hdr", "ico",
    "pam", "pbm", "pgm", "pnm", "ppm", "qoi", "tga", "icns", "ora", "otb", "pcx", "sgi", "iris",
    "rgb", "rgba", "bw", "wbmp", "xbm", "bm", "xpm",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SaveFormat {
    #[default]
    Png,
    Jpeg,
    WebP,
    Bmp,
    Gif,
    Tiff,
    Tga,
    Ico,
    Icns,
    Qoi,
    Pnm,
    Farbfeld,
}

impl SaveFormat {
    pub const ALL: &'static [Self] = &[
        Self::Png,
        Self::Jpeg,
        Self::WebP,
        Self::Bmp,
        Self::Gif,
        Self::Tiff,
        Self::Tga,
        Self::Ico,
        Self::Icns,
        Self::Qoi,
        Self::Pnm,
        Self::Farbfeld,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG image",
            Self::Jpeg => "JPEG image",
            Self::WebP => "WebP image",
            Self::Bmp => "Bitmap image",
            Self::Gif => "GIF image",
            Self::Tiff => "TIFF image",
            Self::Tga => "Targa image",
            Self::Ico => "Windows icon",
            Self::Icns => "Apple icon",
            Self::Qoi => "Quite OK Image",
            Self::Pnm => "Portable anymap",
            Self::Farbfeld => "Farbfeld image",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Tiff => "tiff",
            Self::Tga => "tga",
            Self::Ico => "ico",
            Self::Icns => "icns",
            Self::Qoi => "qoi",
            Self::Pnm => "pam",
            Self::Farbfeld => "ff",
        }
    }

    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::WebP => &["webp"],
            Self::Bmp => &["bmp"],
            Self::Gif => &["gif"],
            Self::Tiff => &["tif", "tiff"],
            Self::Tga => &["tga"],
            Self::Ico => &["ico"],
            Self::Icns => &["icns"],
            Self::Qoi => &["qoi"],
            Self::Pnm => &["pam", "pbm", "pgm", "pnm", "ppm"],
            Self::Farbfeld => &["ff"],
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = extension(path)?;
        Self::ALL
            .iter()
            .copied()
            .find(|format| format.extensions().contains(&extension.as_str()))
    }

    fn image_format(self) -> Option<image::ImageFormat> {
        Some(match self {
            Self::Png => image::ImageFormat::Png,
            Self::Jpeg => image::ImageFormat::Jpeg,
            Self::WebP => image::ImageFormat::WebP,
            Self::Bmp => image::ImageFormat::Bmp,
            Self::Gif => image::ImageFormat::Gif,
            Self::Tiff => image::ImageFormat::Tiff,
            Self::Tga => image::ImageFormat::Tga,
            Self::Ico => image::ImageFormat::Ico,
            Self::Icns => return None,
            Self::Qoi => image::ImageFormat::Qoi,
            Self::Pnm => image::ImageFormat::Pnm,
            Self::Farbfeld => image::ImageFormat::Farbfeld,
        })
    }
}

impl fmt::Display for SaveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (.{})", self.label(), self.extension())
    }
}

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
    let format = SaveFormat::from_path(path)
        .ok_or_else(|| format!("cannot save {}: unsupported file format", path.display()))?;
    save_as(pixels, path, format)
}

pub fn save_as(pixels: &Rgba8, path: &Path, format: SaveFormat) -> Result<(), String> {
    if format == SaveFormat::Icns {
        return save_icns(pixels, path);
    }

    let (w, h) = pixels.size();
    if format == SaveFormat::Ico && (w > 256 || h > 256) {
        return Err(format!(
            "cannot save {}: ICO dimensions must be at most 256 x 256 pixels",
            path.display()
        ));
    }
    let buffer = image::RgbaImage::from_raw(w, h, pixels.as_bytes().to_vec())
        .ok_or("image has an impossible size")?;
    let image_format = format.image_format().expect("non-ICNS format");

    if format == SaveFormat::Farbfeld {
        image::DynamicImage::ImageRgba8(buffer)
            .to_rgba16()
            .save_with_format(path, image_format)
    } else if format == SaveFormat::Jpeg {
        let flattened = flatten_onto_white(&buffer);
        flattened.save_with_format(path, image_format)
    } else {
        buffer.save_with_format(path, image_format)
    }
    .map_err(|e| format!("cannot save {}: {e}", path.display()))
}

pub fn extension(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_lowercase())
}

pub fn with_extension(mut path: PathBuf, format: SaveFormat) -> PathBuf {
    if SaveFormat::from_path(&path) != Some(format) {
        path.set_extension(format.extension());
    }
    path
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

fn save_icns(pixels: &Rgba8, path: &Path) -> Result<(), String> {
    let (w, h) = pixels.size();
    let image = icns::Image::from_data(icns::PixelFormat::RGBA, w, h, pixels.as_bytes().to_vec())
        .map_err(|e| format!("cannot save {}: {e}", path.display()))?;
    let mut family = icns::IconFamily::new();
    family
        .add_icon(&image)
        .map_err(|e| format!("cannot save {}: {e}", path.display()))?;
    let file = File::create(path).map_err(|e| format!("cannot save {}: {e}", path.display()))?;
    family
        .write(BufWriter::new(file))
        .map_err(|e| format!("cannot save {}: {e}", path.display()))
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

    fn sample_sized(w: u32, h: u32) -> Rgba8 {
        let mut px = Vec::with_capacity((w * h) as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let a = if x < w / 2 { 255 } else { 0 };
                px.extend_from_slice(&[(x * 16) as u8, (y * 32) as u8, 64, a]);
            }
        }
        Rgba8::from_raw(w, h, px).unwrap()
    }

    fn sample() -> Rgba8 {
        sample_sized(16, 8)
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
    fn the_selected_save_format_wins_over_the_old_extension() {
        let path = scratch("converted.jpg");
        save_as(&sample(), &path, SaveFormat::Png).unwrap();

        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            image::guess_format(&bytes).unwrap(),
            image::ImageFormat::Png
        );
    }

    #[test]
    fn icon_formats_are_detected_from_their_headers() {
        let original = sample_sized(16, 16);
        for (name, format) in [
            ("windows-icon.data", SaveFormat::Ico),
            ("apple-icon.data", SaveFormat::Icns),
        ] {
            let path = scratch(name);
            save_as(&original, &path, format).unwrap();
            let reloaded = load(&path).unwrap();
            assert_eq!(reloaded.size(), original.size(), "{format}");
            assert_eq!(reloaded.as_bytes(), original.as_bytes(), "{format}");
        }
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
    fn every_listed_save_format_can_be_opened_again() {
        let original = sample_sized(16, 16);
        for &format in SaveFormat::ALL {
            let path = scratch(&format!("save-format.{}", format.extension()));
            save_as(&original, &path, format).unwrap_or_else(|e| panic!("{format}: {e}"));
            let reloaded = load(&path).unwrap_or_else(|e| panic!("{format}: {e}"));
            assert_eq!(reloaded.size(), original.size(), "{format}");
        }
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
    fn the_selected_format_supplies_a_matching_extension() {
        assert_eq!(
            with_extension("shot".into(), SaveFormat::Png),
            PathBuf::from("shot.png")
        );
        assert_eq!(
            with_extension("shot.jpeg".into(), SaveFormat::Jpeg),
            PathBuf::from("shot.jpeg")
        );
        assert_eq!(
            with_extension("shot.jpg".into(), SaveFormat::Png),
            PathBuf::from("shot.png")
        );
    }

    // The desktop entry, the packager metadata and the WiX fragment each name the
    // formats independently, so they are checked against the one list that decides
    // what the app can actually open.
    const MANIFEST: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    const FRAGMENT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packaging/windows/file-associations.wxs"
    ));
    const DESKTOP: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packaging/net.electris.RustyPaint.desktop"
    ));

    fn associations() -> Vec<toml::Table> {
        toml::from_str::<toml::Table>(MANIFEST).expect("the manifest parses")["package"]["metadata"]
            ["packager"]["file-associations"]
            .as_array()
            .expect("the associations are a list")
            .iter()
            .map(|entry| entry.as_table().expect("an association is a table").clone())
            .collect()
    }

    #[test]
    fn every_readable_format_is_offered_to_the_desktop() {
        let mut claimed: Vec<String> = associations()
            .iter()
            .flat_map(|entry| entry["extensions"].as_array().expect("extensions").clone())
            .map(|value| value.as_str().expect("an extension is a string").to_owned())
            .collect();
        claimed.sort();

        let mut readable: Vec<String> = READABLE.iter().map(|e| (*e).to_owned()).collect();
        readable.sort();

        assert_eq!(claimed, readable, "packager metadata and READABLE disagree");
    }

    #[test]
    fn windows_registers_every_extension_the_packager_names() {
        for extension in READABLE {
            assert!(
                FRAGMENT.contains(&format!(
                    "Key=\"Software\\Classes\\.{extension}\\OpenWithProgids\""
                )),
                "the WiX fragment never offers .{extension}"
            );
        }
    }

    #[test]
    fn the_desktop_entry_names_the_same_types_the_packager_does() {
        let listed: Vec<&str> = DESKTOP
            .lines()
            .find_map(|line| line.strip_prefix("MimeType="))
            .expect("a MimeType line")
            .split(';')
            .filter(|entry| !entry.is_empty())
            .collect();

        for entry in associations() {
            let mime = entry["mime-type"].as_str().expect("a mime type");
            assert!(
                listed.contains(&mime),
                "the desktop entry never offers {mime}"
            );
        }
    }
}
