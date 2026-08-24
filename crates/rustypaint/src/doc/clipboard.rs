use super::Rgba8;

#[derive(Debug, Clone)]
pub enum Clip {
    Image(Rgba8),
    Text(String),
}

pub fn copy(pixels: &Rgba8) -> Result<(), String> {
    let (width, height) = pixels.size();
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("no clipboard: {e}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: pixels.as_bytes().into(),
        })
        .map_err(|e| format!("cannot copy: {e}"))
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("no clipboard: {e}"))?;
    clipboard
        .set_text(text)
        .map_err(|e| format!("cannot copy: {e}"))
}

pub fn paste() -> Option<Clip> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    image_on(&mut clipboard).or_else(|| text_on(&mut clipboard))
}

pub fn paste_into_text() -> Option<Clip> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    text_on(&mut clipboard).or_else(|| image_on(&mut clipboard))
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

fn text_on(clipboard: &mut arboard::Clipboard) -> Option<Clip> {
    let text = clipboard.get_text().ok()?;
    (!text.is_empty()).then_some(Clip::Text(text))
}
