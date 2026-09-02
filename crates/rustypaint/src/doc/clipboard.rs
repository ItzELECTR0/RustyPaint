use super::{Rgba8, io};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Clip {
    Image(Rgba8),
    Text(String),
}

pub fn copy(pixels: &Rgba8) -> Result<(), String> {
    let (width, height) = pixels.size();
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| crate::i18n::error_no_clipboard(&e.to_string()))?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: pixels.as_bytes().into(),
        })
        .map_err(|e| crate::i18n::error_cannot_copy(&e.to_string()))
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| crate::i18n::error_no_clipboard(&e.to_string()))?;
    clipboard
        .set_text(text)
        .map_err(|e| crate::i18n::error_cannot_copy(&e.to_string()))
}

pub fn paste() -> Option<Clip> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    image_on(&mut clipboard)
        .or_else(|| files_on(&mut clipboard))
        .or_else(|| text_on(&mut clipboard))
}

pub fn paste_into_text() -> Option<Clip> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    text_on(&mut clipboard)
        .or_else(|| image_on(&mut clipboard))
        .or_else(|| files_on(&mut clipboard))
}

fn image_on(clipboard: &mut arboard::Clipboard) -> Option<Clip> {
    let image = clipboard.get_image().ok()?;
    let pixels = Rgba8::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )?;
    Some(Clip::Image(pixels))
}

fn files_on(clipboard: &mut arboard::Clipboard) -> Option<Clip> {
    first_image(&clipboard.get().file_list().ok()?).map(Clip::Image)
}

fn text_on(clipboard: &mut arboard::Clipboard) -> Option<Clip> {
    let text = clipboard.get_text().ok()?;
    (!text.is_empty()).then_some(Clip::Text(text))
}

pub fn first_image(paths: &[PathBuf]) -> Option<Rgba8> {
    paths.iter().find_map(|path| io::load(&tidy(path)).ok())
}

// A text/uri-list is CRLF separated and the carriage return survives arboard's parse.
fn tidy(path: &Path) -> PathBuf {
    match path.to_str() {
        Some(text) if text.ends_with(['\r', '\n']) => {
            PathBuf::from(text.trim_end_matches(['\r', '\n']))
        }
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("rustypaint-clipboard-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    fn write_png(name: &str) -> PathBuf {
        let path = scratch(name);
        io::save(&Rgba8::new(4, 3, [255, 0, 0, 255]), &path).unwrap();
        path
    }

    #[test]
    fn a_copied_image_file_is_read_as_pixels() {
        let path = write_png("copied.png");
        let pixels = first_image(&[path]).expect("the file should open");
        assert_eq!(pixels.size(), (4, 3));
    }

    #[test]
    fn a_trailing_carriage_return_still_finds_the_file() {
        let path = write_png("crlf.png");
        let with_cr = PathBuf::from(format!("{}\r\n", path.display()));
        assert!(!with_cr.exists(), "the untrimmed path should not resolve");
        assert!(first_image(&[with_cr]).is_some());
    }

    #[test]
    fn the_first_readable_image_wins() {
        let note = scratch("note.txt");
        std::fs::write(&note, b"not an image").unwrap();
        let png = write_png("second.png");
        assert!(first_image(&[note.clone(), png]).is_some());
        assert!(first_image(&[note]).is_none());
    }
}
