use std::sync::Arc;

pub const CHANNELS: usize = 4;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rgba8 {
    width: u32,
    height: u32,
    pixels: Arc<Vec<u8>>,
}

#[allow(
    dead_code,
    reason = "buffer API is complete ahead of the tools that use it"
)]
impl Rgba8 {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let count = width as usize * height as usize;
        let pixels = fill.repeat(count);
        Self {
            width,
            height,
            pixels: Arc::new(pixels),
        }
    }

    pub fn transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, [0, 0, 0, 0])
    }

    pub fn white(width: u32, height: u32) -> Self {
        Self::new(width, height, [255, 255, 255, 255])
    }

    pub fn from_raw(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (pixels.len() == width as usize * height as usize * CHANNELS).then(|| Self {
            width,
            height,
            pixels: Arc::new(pixels),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    pub fn bytes_arc(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.pixels)
    }

    pub fn pixels_mut(&mut self) -> &mut Vec<u8> {
        Arc::make_mut(&mut self.pixels)
    }

    pub fn flattened_onto(&self, background: [u8; 4]) -> Self {
        let mut out = self.clone();
        for px in out.pixels_mut().as_chunks_mut::<CHANNELS>().0 {
            let a = px[3] as u32;
            for c in 0..3 {
                px[c] = ((px[c] as u32 * a + background[c] as u32 * (255 - a)) / 255) as u8;
            }
            px[3] = 255;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn pixel_at(img: &Rgba8, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * img.width() as usize + x as usize) * CHANNELS;
        img.as_bytes()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn flattening_composites_onto_the_background_and_drops_alpha() {
        let mut img = Rgba8::new(2, 1, [255, 0, 0, 128]);
        img.pixels_mut()[4..8].copy_from_slice(&CLEAR);
        let out = img.flattened_onto([255, 255, 255, 255]);

        assert_eq!(pixel_at(&out, 0, 0)[3], 255, "nothing stays translucent");
        assert!((pixel_at(&out, 0, 0)[1] as i32 - 127).abs() <= 2);
        assert_eq!(
            pixel_at(&out, 1, 0),
            [255, 255, 255, 255],
            "clear becomes background"
        );
    }

    #[test]
    fn buffers_share_storage_until_written() {
        let a = Rgba8::new(8, 8, RED);
        let mut b = a.clone();
        assert!(Arc::ptr_eq(&a.bytes_arc(), &b.bytes_arc()));
        b.pixels_mut()[0] = 1;
        assert!(!Arc::ptr_eq(&a.bytes_arc(), &b.bytes_arc()));
    }
}
