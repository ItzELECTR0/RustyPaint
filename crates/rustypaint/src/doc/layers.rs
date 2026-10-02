use super::image::{CHANNELS, Rgba8};

pub type LayerId = u64;

pub const OPAQUE: u8 = 255;

// Below this many pixels a composite is quicker on one thread than spread across several.
const PARALLEL_FROM: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Properties {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub pixels: Rgba8,
}

impl Layer {
    pub fn properties(&self) -> Properties {
        Properties {
            id: self.id,
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
        }
    }

    // What this layer adds to a composite, 0 when it adds nothing at all.
    pub fn strength(&self) -> u8 {
        if self.visible { self.opacity } else { 0 }
    }
}

// The whole stack described without its pixels, bottom layer first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub layers: Vec<Properties>,
    pub active: LayerId,
}

// Everything undo can reach: the layers, which one is being worked on, and the backing.
#[derive(Clone, Debug)]
pub struct Stack {
    pub layers: Vec<Layer>,
    pub active: usize,
    pub transparent: bool,
}

impl Stack {
    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id == id)
    }

    pub fn active_id(&self) -> LayerId {
        self.layers[self.active].id
    }

    pub fn size(&self) -> (u32, u32) {
        self.layers[0].pixels.size()
    }

    // True when a flat picture would lose something: a second layer, or a hidden or faded one.
    pub fn layered(&self) -> bool {
        match self.layers.as_slice() {
            [only] => !only.visible || only.opacity != OPAQUE,
            _ => true,
        }
    }

    // What the layers show together, without the backing.
    pub fn composite(&self) -> Rgba8 {
        let (width, height) = self.size();
        composite(&self.layers, (width, height))
            .unwrap_or_else(|| Rgba8::transparent(width, height))
    }

    // The picture as it is seen and saved flat, backing included.
    pub fn flattened(&self) -> Rgba8 {
        let composite = self.composite();
        let opaque = composite
            .as_bytes()
            .iter()
            .skip(3)
            .step_by(CHANNELS)
            .all(|&a| a == 255);
        if self.transparent || opaque {
            composite
        } else {
            composite.flattened_onto([255, 255, 255, 255])
        }
    }

    pub fn arrangement(&self) -> Arrangement {
        Arrangement {
            layers: self.layers.iter().map(Layer::properties).collect(),
            active: self.active_id(),
        }
    }

    // Takes pixels from the layers already here, then from `kept`, and starts anything else
    // blank, which is what a layer that was only ever added holds.
    pub fn rearrange(&mut self, target: &Arrangement, kept: &[(LayerId, Rgba8)]) {
        let (width, height) = self.size();
        let mut pool = std::mem::take(&mut self.layers);
        self.layers = target
            .layers
            .iter()
            .map(|properties| {
                let pixels = match pool.iter().position(|layer| layer.id == properties.id) {
                    Some(at) => pool.swap_remove(at).pixels,
                    None => kept
                        .iter()
                        .find(|(id, _)| *id == properties.id)
                        .map(|(_, pixels)| pixels.clone())
                        .unwrap_or_else(|| Rgba8::transparent(width, height)),
                };
                Layer {
                    id: properties.id,
                    name: properties.name.clone(),
                    visible: properties.visible,
                    opacity: properties.opacity,
                    pixels,
                }
            })
            .collect();
        self.active = self.index_of(target.active).unwrap_or(0);
    }
}

pub fn scale(alpha: u8, by: u8) -> u8 {
    ((alpha as u32 * by as u32 + 127) / 255) as u8
}

// Straight-alpha source-over in 8 bits. Every flat picture and both viewport composites go
// through here, so what is saved and what is shown cannot disagree on a layer's edge.
pub fn blend(under: &mut [u8], over: &[u8], opacity: u8) {
    if opacity == 0 {
        return;
    }
    let pairs = under
        .as_chunks_mut::<CHANNELS>()
        .0
        .iter_mut()
        .zip(over.as_chunks::<CHANNELS>().0);
    for (dst, src) in pairs {
        let sa = scale(src[3], opacity) as u32;
        if sa == 0 {
            continue;
        }
        if sa == 255 {
            *dst = [src[0], src[1], src[2], 255];
            continue;
        }
        let da = dst[3] as u32;
        let keep = da * (255 - sa);
        let total = sa * 255 + keep;
        for c in 0..3 {
            dst[c] = ((src[c] as u32 * sa * 255 + dst[c] as u32 * keep + total / 2) / total) as u8;
        }
        dst[3] = ((total + 127) / 255) as u8;
    }
}

// Visible layers flattened in order, or None when nothing among them shows.
pub fn composite(layers: &[Layer], size: (u32, u32)) -> Option<Rgba8> {
    let mut shown = layers.iter().filter(|layer| layer.strength() > 0);
    let first = shown.next()?;
    let mut out = first.pixels.clone();
    if first.strength() != OPAQUE {
        let mut faded = Rgba8::transparent(size.0, size.1);
        blend_rows(
            faded.pixels_mut(),
            first.pixels.as_bytes(),
            first.strength(),
        );
        out = faded;
    }
    for layer in shown {
        blend_rows(out.pixels_mut(), layer.pixels.as_bytes(), layer.strength());
    }
    Some(out)
}

fn blend_rows(under: &mut [u8], over: &[u8], opacity: u8) {
    let pixels = under.len() / CHANNELS;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if pixels < PARALLEL_FROM || threads < 2 {
        blend(under, over, opacity);
        return;
    }
    let chunk = pixels.div_ceil(threads) * CHANNELS;
    std::thread::scope(|scope| {
        for (dst, src) in under.chunks_mut(chunk).zip(over.chunks(chunk)) {
            scope.spawn(move || blend(dst, src, opacity));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: LayerId, fill: [u8; 4], opacity: u8, visible: bool) -> Layer {
        Layer {
            id,
            name: format!("Layer {id}"),
            visible,
            opacity,
            pixels: Rgba8::new(2, 1, fill),
        }
    }

    fn first(pixels: &Rgba8) -> [u8; 4] {
        pixels.as_bytes()[0..4].try_into().unwrap()
    }

    #[test]
    fn an_opaque_layer_hides_whatever_is_under_it() {
        let stack = [
            layer(1, [255, 0, 0, 255], OPAQUE, true),
            layer(2, [0, 0, 255, 255], OPAQUE, true),
        ];
        assert_eq!(first(&composite(&stack, (2, 1)).unwrap()), [0, 0, 255, 255]);
    }

    #[test]
    fn hidden_and_fully_faded_layers_add_nothing() {
        let stack = [
            layer(1, [255, 0, 0, 255], OPAQUE, true),
            layer(2, [0, 0, 255, 255], OPAQUE, false),
            layer(3, [0, 255, 0, 255], 0, true),
        ];
        assert_eq!(first(&composite(&stack, (2, 1)).unwrap()), [255, 0, 0, 255]);
        assert!(composite(&stack[1..], (2, 1)).is_none());
    }

    #[test]
    fn half_opacity_mixes_halfway() {
        let stack = [
            layer(1, [0, 0, 0, 255], OPAQUE, true),
            layer(2, [255, 255, 255, 255], 128, true),
        ];
        let [r, g, b, a] = first(&composite(&stack, (2, 1)).unwrap());
        assert_eq!(a, 255);
        assert!(r.abs_diff(128) <= 1 && r == g && g == b);
    }

    #[test]
    fn colour_survives_on_an_empty_backdrop() {
        let stack = [layer(1, [10, 200, 30, 90], 200, true)];
        let [r, g, b, a] = first(&composite(&stack, (2, 1)).unwrap());
        assert_eq!((r, g, b), (10, 200, 30));
        assert_eq!(a, scale(90, 200));
    }

    // Splitting the stack around the active layer is what the viewport does, and the parts have
    // to land where the whole does or the screen would not match the saved picture.
    #[test]
    fn compositing_in_parts_matches_compositing_at_once() {
        let stack = [
            layer(1, [200, 40, 40, 255], OPAQUE, true),
            layer(2, [40, 200, 40, 180], 220, true),
            layer(3, [40, 40, 200, 120], 255, true),
            layer(4, [250, 250, 20, 60], 128, true),
        ];
        let whole = composite(&stack, (2, 1)).unwrap();
        let mut parts = composite(&stack[..2], (2, 1)).unwrap();
        blend(parts.pixels_mut(), stack[2].pixels.as_bytes(), OPAQUE);
        let above = composite(&stack[3..], (2, 1)).unwrap();
        blend(parts.pixels_mut(), above.as_bytes(), OPAQUE);
        for (a, b) in whole.as_bytes().iter().zip(parts.as_bytes()) {
            assert!(a.abs_diff(*b) <= 1, "{whole:?} against {parts:?}");
        }
    }

    #[test]
    fn a_large_composite_splits_across_threads_without_seams() {
        let (w, h) = (1500, 800);
        let mut bottom = Rgba8::new(w, h, [30, 60, 90, 255]);
        bottom.pixels_mut()[0..4].copy_from_slice(&[0, 0, 0, 0]);
        let top = Rgba8::new(w, h, [200, 100, 50, 77]);
        let stack = [
            Layer {
                pixels: bottom.clone(),
                ..layer(1, [0; 4], OPAQUE, true)
            },
            Layer {
                pixels: top.clone(),
                ..layer(2, [0; 4], 200, true)
            },
        ];
        let threaded = composite(&stack, (w, h)).unwrap();
        let mut single = bottom;
        blend(single.pixels_mut(), top.as_bytes(), 200);
        assert_eq!(threaded, single);
    }
}
