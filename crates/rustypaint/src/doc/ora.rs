use super::image::{CHANNELS, Rgba8};
use super::layers::{Layer, OPAQUE, Stack};
use super::transform;
use crate::i18n;
use std::io::{Read, Seek, Write};
use std::path::Path;

pub const EXTENSION: &str = "ora";

const MIMETYPE: &str = "image/openraster";
const VERSION: &str = "0.0.6";
const NAMESPACE: &str = "https://github.com/ItzELECTR0/RustyPaint";
const THUMBNAIL: u32 = 256;

// Past this a single layer would not fit the decoder's allocation limit anyway.
const MOST_PIXELS: u64 = 1 << 27;

pub enum Opened {
    Layers { stack: Stack, trimmed: bool },
    // Uses something the stack cannot hold, so only the merged picture is safe to edit.
    Flat(Rgba8),
}

pub fn load(path: &Path) -> Result<Opened, String> {
    let name = path.file_name().unwrap_or_default().display().to_string();
    let file = std::fs::File::open(path)
        .map_err(|e| i18n::error_cannot_open(&path.display().to_string(), &e.to_string()))?;
    read(std::io::BufReader::new(file)).map_err(|e| i18n::error_cannot_open(&name, &e))
}

pub fn read(source: impl Read + Seek) -> Result<Opened, String> {
    let mut archive = zip::ZipArchive::new(source).map_err(|e| e.to_string())?;
    let stack_xml = String::from_utf8(entry(&mut archive, "stack.xml")?)
        .map_err(|_| "stack.xml is not UTF-8".to_owned())?;
    let document = roxmltree::Document::parse(&stack_xml).map_err(|e| e.to_string())?;
    let image = document.root_element();
    if image.tag_name().name() != "image" {
        return Err("stack.xml has no image element".into());
    }
    let side = |attribute: &str| -> Result<u32, String> {
        image
            .attribute(attribute)
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("the image has no usable {attribute}"))
    };
    let size = (side("w")?, side("h")?);
    if u64::from(size.0) * u64::from(size.1) > MOST_PIXELS {
        return Err(i18n::error_impossible_size().to_owned());
    }
    let root = image
        .children()
        .find(|node| node.has_tag_name("stack"))
        .ok_or_else(|| "stack.xml has no stack".to_owned())?;

    let mut found = Vec::new();
    if !gather(root, true, &mut found) {
        let merged = entry(&mut archive, "mergedimage.png")?;
        return decode(&merged).map(Opened::Flat);
    }

    // stack.xml lists the top layer first; the document keeps the bottom first.
    found.reverse();
    let mut trimmed = false;
    let mut transparent = true;
    let mut layers = Vec::with_capacity(found.len());
    let mut active = None;
    for (index, layer) in found.into_iter().enumerate() {
        let pixels = decode(&entry(&mut archive, &layer.src)?)?;
        let (placed, cut) = place(&pixels, layer.offset, size);
        trimmed |= cut;
        if index == 0 && layer.backing {
            transparent = false;
            continue;
        }
        if layer.selected {
            active = Some(layers.len());
        }
        layers.push(Layer {
            id: layers.len() as u64 + 1,
            name: layer.name,
            visible: layer.visible,
            opacity: layer.opacity,
            pixels: placed,
        });
    }
    if layers.is_empty() {
        layers.push(Layer {
            id: 1,
            name: i18n::layer_background().to_owned(),
            visible: true,
            opacity: OPAQUE,
            pixels: Rgba8::transparent(size.0, size.1),
        });
    }
    let active = active.unwrap_or(layers.len() - 1);
    Ok(Opened::Layers {
        stack: Stack {
            layers,
            active,
            transparent,
        },
        trimmed,
    })
}

struct Found {
    src: String,
    name: String,
    visible: bool,
    opacity: u8,
    offset: (i64, i64),
    selected: bool,
    backing: bool,
}

// False when the stack holds something a plain list of normal layers cannot show the same way.
fn gather(stack: roxmltree::Node, visible: bool, found: &mut Vec<Found>) -> bool {
    for node in stack.children().filter(roxmltree::Node::is_element) {
        if !normal(node) {
            return false;
        }
        let shown = visible && node.attribute("visibility") != Some("hidden");
        match node.tag_name().name() {
            // A group can be lifted out as it is only while its opacity leaves its layers alone.
            "stack" => {
                if opacity(node) != OPAQUE || !gather(node, shown, found) {
                    return false;
                }
            }
            "layer" => {
                let Some(src) = node.attribute("src") else {
                    return false;
                };
                if !src.to_ascii_lowercase().ends_with(".png") {
                    return false;
                }
                let at = |name: &str| {
                    node.attribute(name)
                        .and_then(|value| value.trim().parse::<i64>().ok())
                        .unwrap_or(0)
                };
                found.push(Found {
                    src: src.to_owned(),
                    name: node.attribute("name").unwrap_or_default().to_owned(),
                    visible: shown,
                    opacity: opacity(node),
                    offset: (at("x"), at("y")),
                    selected: node.attribute("selected") == Some("true"),
                    backing: node.attribute((NAMESPACE, "backing")) == Some("white"),
                });
            }
            _ => return false,
        }
    }
    true
}

fn normal(node: roxmltree::Node) -> bool {
    matches!(node.attribute("composite-op"), None | Some("svg:src-over"))
}

fn opacity(node: roxmltree::Node) -> u8 {
    node.attribute("opacity")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map_or(OPAQUE, |value| {
            (value.clamp(0.0, 1.0) * 255.0).round() as u8
        })
}

fn entry<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| format!("{name} is missing"))?;
    let mut bytes = Vec::with_capacity(file.size().min(MOST_PIXELS * 4) as usize);
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<Rgba8, String> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Rgba8::from_raw(width, height, image.into_raw())
        .ok_or_else(|| i18n::error_impossible_size().to_owned())
}

// Lays a layer onto a canvas-sized one. True when something visible fell outside the canvas.
fn place(pixels: &Rgba8, (x, y): (i64, i64), size: (u32, u32)) -> (Rgba8, bool) {
    let (width, height) = pixels.size();
    if (x, y) == (0, 0) && (width, height) == size {
        return (pixels.clone(), false);
    }
    let mut out = Rgba8::transparent(size.0, size.1);
    let target = out.pixels_mut();
    let source = pixels.as_bytes();
    let mut cut = false;
    for row in 0..height as i64 {
        let line = &source[row as usize * width as usize * CHANNELS..][..width as usize * CHANNELS];
        let ty = y + row;
        let inside = |tx: i64| ty >= 0 && ty < size.1 as i64 && tx >= 0 && tx < size.0 as i64;
        for (column, pixel) in line.as_chunks::<CHANNELS>().0.iter().enumerate() {
            let tx = x + column as i64;
            if !inside(tx) {
                cut |= pixel[3] != 0;
                continue;
            }
            let at = (ty as usize * size.0 as usize + tx as usize) * CHANNELS;
            target[at..at + CHANNELS].copy_from_slice(pixel);
        }
    }
    (out, cut)
}

pub fn save(stack: &Stack, path: &Path) -> Result<(), String> {
    let fail = |e: String| i18n::error_cannot_save(&path.display().to_string(), &e);
    // Written beside the target and moved over it, so a failed save leaves the old project whole.
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = std::path::PathBuf::from(partial);
    let file = std::fs::File::create(&partial).map_err(|e| fail(e.to_string()))?;
    let written = write(stack, std::io::BufWriter::new(file))
        .and_then(|mut out| out.flush().map_err(|e| e.to_string()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(fail(e));
    }
    std::fs::rename(&partial, path).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        fail(e.to_string())
    })
}

pub fn write<W: Write + Seek>(stack: &Stack, out: W) -> Result<W, String> {
    let (width, height) = stack.size();
    let merged = &stack.flattened();
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(out);
    let put = |zip: &mut zip::ZipWriter<W>,
               name: &str,
               bytes: &[u8],
               options: zip::write::SimpleFileOptions| {
        zip.start_file(
            name,
            options.large_file(bytes.len() as u64 >= u32::MAX as u64),
        )
        .and_then(|()| zip.write_all(bytes).map_err(Into::into))
        .map_err(|e| e.to_string())
    };

    put(&mut zip, "mimetype", MIMETYPE.as_bytes(), stored)?;

    let pngs = encode_layers(stack)?;
    let mut entries = String::new();
    for (index, layer) in stack.layers.iter().enumerate().rev() {
        let (offset, _) = &pngs[index];
        entries.push_str(&format!(
            "  <layer name=\"{}\" src=\"data/layer{index}.png\" x=\"{}\" y=\"{}\" opacity=\"{:.6}\" \
             visibility=\"{}\" composite-op=\"svg:src-over\"{}/>\n",
            escape(&layer.name),
            offset.0,
            offset.1,
            layer.opacity as f32 / 255.0,
            if layer.visible { "visible" } else { "hidden" },
            if index == stack.active {
                " selected=\"true\""
            } else {
                ""
            },
        ));
    }
    // Other editors have no backing, so it travels as a locked white layer they will show as is.
    if !stack.transparent {
        entries.push_str(&format!(
            "  <layer name=\"{}\" src=\"data/backing.png\" edit-locked=\"true\" rustypaint:backing=\"white\"/>\n",
            escape(i18n::canvas_heading()),
        ));
    }
    let xml = format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n\
         <image version=\"{VERSION}\" w=\"{width}\" h=\"{height}\" xmlns:rustypaint=\"{NAMESPACE}\">\n \
         <stack>\n{entries} </stack>\n</image>\n"
    );
    put(&mut zip, "stack.xml", xml.as_bytes(), deflated)?;

    for (index, (_, png)) in pngs.iter().enumerate() {
        put(&mut zip, &format!("data/layer{index}.png"), png, stored)?;
    }
    if !stack.transparent {
        let white = Rgba8::white(width, height);
        put(&mut zip, "data/backing.png", &png(&white)?, stored)?;
    }
    put(&mut zip, "mergedimage.png", &png(merged)?, stored)?;
    put(
        &mut zip,
        "Thumbnails/thumbnail.png",
        &png(&thumbnail(merged))?,
        stored,
    )?;
    zip.finish().map_err(|e| e.to_string())
}

// Each layer is cropped to what it holds, as Krita writes them, and encoded on its own thread.
fn encode_layers(stack: &Stack) -> Result<Vec<((u32, u32), Vec<u8>)>, String> {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = stack
            .layers
            .iter()
            .map(|layer| {
                scope.spawn(move || {
                    let bounds = content(&layer.pixels);
                    let cropped = transform::crop(&layer.pixels, bounds);
                    png(&cropped).map(|bytes| ((bounds.x0, bounds.y0), bytes))
                })
            })
            .collect();
        jobs.into_iter()
            .map(|job| {
                job.join()
                    .unwrap_or_else(|_| Err("a layer failed to encode".into()))
            })
            .collect()
    })
}

// The smallest box holding every pixel that shows, or one pixel for a layer that is empty.
fn content(pixels: &Rgba8) -> super::Rect {
    let (width, _) = pixels.size();
    let rows: Vec<&[[u8; CHANNELS]]> = pixels
        .as_bytes()
        .chunks(width as usize * CHANNELS)
        .map(|row| row.as_chunks::<CHANNELS>().0)
        .collect();
    let shows = |row: &[[u8; CHANNELS]]| row.iter().any(|pixel| pixel[3] != 0);
    let Some(top) = rows.iter().position(|row| shows(row)) else {
        return super::Rect::new(0, 0, 1, 1);
    };
    let bottom = rows.iter().rposition(|row| shows(row)).unwrap_or(top);
    let mut left = width as usize;
    let mut right = 0;
    for row in &rows[top..=bottom] {
        if let Some(first) = row.iter().position(|pixel| pixel[3] != 0) {
            left = left.min(first);
            right = right.max(row.iter().rposition(|pixel| pixel[3] != 0).unwrap_or(first));
        }
    }
    super::Rect::new(left as u32, top as u32, right as u32 + 1, bottom as u32 + 1)
}

fn png(pixels: &Rgba8) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            pixels.as_bytes(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn thumbnail(merged: &Rgba8) -> Rgba8 {
    let (width, height) = merged.size();
    let fit = (THUMBNAIL as f32 / width.max(height) as f32).min(1.0);
    if fit >= 1.0 {
        return merged.clone();
    }
    let size = (
        ((width as f32 * fit).round() as u32).max(1),
        ((height as f32 * fit).round() as u32).max(1),
    );
    transform::scale(merged, size.0, size.1, transform::Resampling::default())
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: u64, name: &str, pixels: Rgba8, visible: bool, opacity: u8) -> Layer {
        Layer {
            id,
            name: name.into(),
            visible,
            opacity,
            pixels,
        }
    }

    fn dot(size: (u32, u32), at: (u32, u32), colour: [u8; 4]) -> Rgba8 {
        let mut pixels = Rgba8::transparent(size.0, size.1);
        let i = (at.1 as usize * size.0 as usize + at.0 as usize) * CHANNELS;
        pixels.pixels_mut()[i..i + CHANNELS].copy_from_slice(&colour);
        pixels
    }

    fn sample() -> Stack {
        let size = (40, 30);
        Stack {
            layers: vec![
                layer(
                    1,
                    "Paper",
                    Rgba8::new(40, 30, [240, 230, 200, 255]),
                    true,
                    OPAQUE,
                ),
                layer(
                    2,
                    "Ink & <notes>",
                    dot(size, (5, 7), [10, 20, 30, 255]),
                    true,
                    128,
                ),
                layer(
                    3,
                    "Sketch",
                    dot(size, (39, 29), [200, 0, 0, 90]),
                    false,
                    OPAQUE,
                ),
            ],
            active: 1,
            transparent: true,
        }
    }

    fn round_trip(stack: &Stack) -> (Opened, Vec<u8>) {
        let bytes = write(stack, std::io::Cursor::new(Vec::new()))
            .unwrap()
            .into_inner();
        (read(std::io::Cursor::new(bytes.clone())).unwrap(), bytes)
    }

    #[test]
    fn a_saved_project_opens_with_the_same_layers() {
        let stack = sample();
        let (
            Opened::Layers {
                stack: back,
                trimmed,
            },
            _,
        ) = round_trip(&stack)
        else {
            panic!("a plain stack opens as layers");
        };
        assert!(!trimmed);
        assert_eq!(back.active, 1);
        assert!(back.transparent);
        for (a, b) in stack.layers.iter().zip(&back.layers) {
            assert_eq!(
                (&a.name, a.visible, a.opacity),
                (&b.name, b.visible, b.opacity)
            );
            assert_eq!(a.pixels, b.pixels, "{} came back different", a.name);
        }
    }

    #[test]
    fn the_archive_follows_the_file_layout() {
        let (_, bytes) = round_trip(&sample());
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let first = archive.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        drop(first);
        assert_eq!(
            entry(&mut archive, "mimetype").unwrap(),
            MIMETYPE.as_bytes()
        );
        for name in ["stack.xml", "mergedimage.png", "Thumbnails/thumbnail.png"] {
            assert!(archive.by_name(name).is_ok(), "{name} is missing");
        }
        let layer = decode(&entry(&mut archive, "data/layer1.png").unwrap()).unwrap();
        assert_eq!(layer.size(), (1, 1), "a layer is cropped to what it holds");
    }

    #[test]
    fn the_backing_travels_as_a_white_layer_and_comes_back_as_the_backing() {
        let mut stack = sample();
        stack.transparent = false;
        let (Opened::Layers { stack: back, .. }, bytes) = round_trip(&stack) else {
            panic!("a plain stack opens as layers");
        };
        assert!(!back.transparent);
        assert_eq!(back.layers.len(), 3, "the backing is not a layer here");

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let xml = String::from_utf8(entry(&mut archive, "stack.xml").unwrap()).unwrap();
        assert!(xml.contains("rustypaint:backing=\"white\""));
    }

    #[test]
    fn a_layer_another_editor_would_lose_opens_flat() {
        for op in ["svg:multiply", "svg:dst-out"] {
            let xml = format!(
                "<image version=\"0.0.6\" w=\"2\" h=\"1\"><stack>\
                 <layer src=\"data/a.png\" composite-op=\"{op}\"/></stack></image>"
            );
            let merged = Rgba8::new(2, 1, [1, 2, 3, 255]);
            match read(archive_with(&xml, &[("data/a.png", &merged)], &merged)).unwrap() {
                Opened::Flat(pixels) => assert_eq!(pixels, merged),
                Opened::Layers { .. } => panic!("{op} is not a normal layer"),
            }
        }
    }

    #[test]
    fn groups_are_lifted_out_while_that_changes_nothing() {
        let red = Rgba8::new(2, 1, [255, 0, 0, 255]);
        let xml = "<image version=\"0.0.6\" w=\"2\" h=\"1\"><stack>\
                   <stack name=\"g\" visibility=\"hidden\"><layer src=\"data/a.png\" name=\"a\"/></stack>\
                   <layer src=\"data/a.png\" name=\"b\" x=\"1\"/></stack></image>";
        let Opened::Layers { stack, trimmed } =
            read(archive_with(xml, &[("data/a.png", &red)], &red)).unwrap()
        else {
            panic!("a hidden group of normal layers is a list of hidden layers");
        };
        assert!(trimmed, "the offset layer hangs off the canvas");
        let names: Vec<_> = stack.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["b", "a"]);
        assert!(!stack.layers[1].visible, "the group hid it");
        assert_eq!(&stack.layers[0].pixels.as_bytes()[0..4], &[0, 0, 0, 0]);

        let faded = xml.replace("visibility=\"hidden\"", "opacity=\"0.5\"");
        assert!(matches!(
            read(archive_with(&faded, &[("data/a.png", &red)], &red)).unwrap(),
            Opened::Flat(_)
        ));
    }

    #[test]
    fn opacity_survives_an_eight_bit_round_trip_through_krita() {
        for opacity in [0u8, 1, 127, 128, 200, 255] {
            let written = format!("{:.6}", opacity as f32 / 255.0);
            let krita = format!("{:.6}", written.parse::<f32>().unwrap());
            let xml = format!(
                "<image version=\"0.0.6\" w=\"1\" h=\"1\"><stack>\
                 <layer src=\"data/a.png\" opacity=\"{krita}\"/></stack></image>"
            );
            let dot = Rgba8::new(1, 1, [0, 0, 0, 255]);
            let Opened::Layers { stack, .. } =
                read(archive_with(&xml, &[("data/a.png", &dot)], &dot)).unwrap()
            else {
                panic!("a normal layer opens as a layer");
            };
            assert_eq!(stack.layers[0].opacity, opacity);
        }
    }

    fn archive_with(
        xml: &str,
        layers: &[(&str, &Rgba8)],
        merged: &Rgba8,
    ) -> std::io::Cursor<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("mimetype", options).unwrap();
        zip.write_all(MIMETYPE.as_bytes()).unwrap();
        zip.start_file("stack.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        for (name, pixels) in layers {
            zip.start_file(*name, options).unwrap();
            zip.write_all(&png(pixels).unwrap()).unwrap();
        }
        zip.start_file("mergedimage.png", options).unwrap();
        zip.write_all(&png(merged).unwrap()).unwrap();
        let mut out = zip.finish().unwrap();
        out.set_position(0);
        out
    }
}
