use super::refine::Dab;
use crate::doc::{Rect, Rgba8};
use anyhow::{Result as ModelResult, anyhow as format_err, ensure};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
const SIDE: usize = 1024;
// Box coverage where a mask starts looking like the backdrop, and where it certainly is.
const SUSPECT: f32 = 0.75;
const BACKDROP: f32 = 0.9;
// What claiming the box border costs, the score below which the model doubts itself, and the
// agreement between proposals that makes a disagreeing guide the odd one out.
const EDGE: f32 = 0.8;
const UNSURE: f32 = 0.70;
const CONSENSUS: f32 = 0.75;
const ENCODER: &[u8] = include_bytes!("../../../../../res/models/mobile-sam/encoder.onnx");
const DECODER: &[u8] = include_bytes!("../../../../../res/models/mobile-sam/decoder.onnx");
pub const NOTICES: &str = concat!(
    include_str!("../../../../../res/models/mobile-sam/NOTICE.md"),
    "\n\n",
    include_str!("../../../../../res/models/mobile-sam/LICENSE-MIT"),
    "\n\n",
    include_str!("../../../../../res/models/mobile-sam/LICENSE-APACHE"),
    "\n\nONNX Runtime\n",
    include_str!("../../../../../res/licenses/onnxruntime/LICENSE"),
    "\n\n",
    include_str!("../../../../../res/licenses/onnxruntime/ThirdPartyNotices.txt"),
    "\n\nort Rust bindings\n",
    include_str!("../../../../../res/licenses/onnxruntime/ort-LICENSE-MIT"),
);

pub struct Model {
    runtime: super::runtime::Model,
}

pub struct Encoded {
    model: Arc<Model>,
    embedding: Vec<f32>,
    pub region: Rect,
    resized: (u32, u32),
    guide: super::Cutout,
    pixels: Rgba8,
    fallback: OnceLock<super::Cutout>,
    hint: Option<Dab>,
}

impl std::fmt::Debug for Encoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encoded")
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

impl Model {
    pub fn bundled() -> Result<Arc<Self>, String> {
        static BUNDLED: OnceLock<Mutex<Option<Arc<Model>>>> = OnceLock::new();
        let mut cached = BUNDLED
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|e| e.to_string())?;
        if let Some(model) = cached.as_ref() {
            return Ok(model.clone());
        }
        let model = Arc::new(Self::load(ENCODER, DECODER).map_err(|e| format!("{e:#}"))?);
        *cached = Some(model.clone());
        Ok(model)
    }

    pub fn from_directory(path: &Path) -> Result<Arc<Self>, String> {
        let read = |name| -> Result<Vec<u8>, String> {
            let path = path.join(name);
            let size = path.metadata().map_err(|e| e.to_string())?.len();
            if size > 512 * 1024 * 1024 {
                return Err(format!("{} exceeds 512 MB", path.display()));
            }
            std::fs::read(path).map_err(|e| e.to_string())
        };
        Self::load(&read("encoder.onnx")?, &read("decoder.onnx")?)
            .map(Arc::new)
            .map_err(|e| format!("{e:#}"))
    }

    fn load(encoder: &[u8], decoder: &[u8]) -> ModelResult<Self> {
        Ok(Self {
            runtime: super::runtime::Model::load(encoder, decoder)?,
        })
    }

    pub fn encode(self: &Arc<Self>, pixels: &Rgba8, rect: Rect) -> Result<Arc<Encoded>, String> {
        self.encode_inner(pixels, rect)
            .map(Arc::new)
            .map_err(|e| format!("{e:#}"))
    }

    fn encode_inner(self: &Arc<Self>, pixels: &Rgba8, rect: Rect) -> ModelResult<Encoded> {
        ensure!(!rect.is_empty(), "The selection box is empty");
        ensure!(
            rect == rect.clamped(pixels.width(), pixels.height()),
            "Selection box is outside the image"
        );
        let margin = rect.width().max(rect.height()) / 8;
        let region = Rect::new(
            rect.x0.saturating_sub(margin),
            rect.y0.saturating_sub(margin),
            rect.x1.saturating_add(margin).min(pixels.width()),
            rect.y1.saturating_add(margin).min(pixels.height()),
        );
        let crop = crate::doc::transform::crop(pixels, region);
        let rgb = image::RgbaImage::from_raw(crop.width(), crop.height(), crop.as_bytes().to_vec())
            .ok_or_else(|| format_err!("Invalid image dimensions"))?;
        let scale = SIDE as f64 / region.width().max(region.height()) as f64;
        let resized = (
            (region.width() as f64 * scale).round().max(1.0) as u32,
            (region.height() as f64 * scale).round().max(1.0) as u32,
        );
        let rgb = image::imageops::resize(
            &rgb,
            resized.0,
            resized.1,
            image::imageops::FilterType::Triangle,
        );
        let mut input = vec![0.0f32; SIDE * SIDE * 3];
        for px in input.as_chunks_mut::<3>().0 {
            px.copy_from_slice(&[123.675, 116.28, 103.53]);
        }
        for y in 0..resized.1 as usize {
            for x in 0..resized.0 as usize {
                let pixel = rgb.get_pixel(x as u32, y as u32).0;
                let alpha = pixel[3] as f32 / 255.0;
                for c in 0..3 {
                    input[(y * SIDE + x) * 3 + c] = pixel[c] as f32 * alpha + 255.0 * (1.0 - alpha);
                }
            }
        }
        let embedding = self.runtime.encode(&input)?;
        let mut coarse = super::Cutout::with_limit(pixels, rect, 192);
        coarse.run(3);
        let hint =
            super::refine::interior_prompt(&coarse.working_mask(), coarse.size()).map(|(x, y)| {
                Dab {
                    at: (
                        coarse.region.x0 as f32 + (x as f32 + 0.5) / coarse.scale,
                        coarse.region.y0 as f32 + (y as f32 + 0.5) / coarse.scale,
                    ),
                    radius: 1.0,
                    foreground: true,
                    stroke: 0,
                }
            });
        Ok(Encoded {
            model: self.clone(),
            embedding,
            region,
            resized,
            guide: coarse,
            pixels: pixels.clone(),
            fallback: OnceLock::new(),
            hint,
        })
    }
}

impl Encoded {
    pub fn colour_fallback(&self) -> bool {
        self.fallback.get().is_some()
    }

    pub fn select(&self, size: (u32, u32), rect: Rect, strokes: &[Dab]) -> Result<Vec<u8>, String> {
        if size != self.pixels.size() || rect != self.guide.rect {
            return Err("The cached image or box changed".into());
        }
        if let Some(base) = self.fallback.get() {
            if strokes.is_empty() {
                return Ok(base.mask());
            }
            let mut cutout = base.clone();
            for dab in strokes {
                cutout.paint(dab.at, dab.radius, dab.foreground);
            }
            cutout.recut();
            return Ok(cutout.mask());
        }
        self.select_inner(size, rect, strokes, strokes.iter().any(|s| s.foreground))
            .map_err(|e| format!("{e:#}"))
    }

    fn select_inner(
        &self,
        size: (u32, u32),
        rect: Rect,
        strokes: &[Dab],
        point_only: bool,
    ) -> ModelResult<Vec<u8>> {
        ensure!(
            size == self.pixels.size() && rect == self.guide.rect,
            "The cached image or box changed"
        );
        let mut prompts = prompts(strokes, rect);
        if point_only && prompts.is_empty() {
            prompts.extend(self.hint);
        }
        let count = prompts.len();
        let points = count + if point_only { 1 } else { 2 };
        let mut coords = vec![0.0f32; points * 2];
        let mut labels = vec![-1.0f32; points];
        let map = |point: (f32, f32)| self.map(point);
        if !point_only {
            coords[..2].copy_from_slice(&map((rect.x0 as f32, rect.y0 as f32)));
            coords[2..4].copy_from_slice(&map((rect.x1 as f32, rect.y1 as f32)));
            labels[0] = 2.0;
            labels[1] = 3.0;
        }
        for (n, dab) in prompts.iter().enumerate() {
            let i = n + if point_only { 0 } else { 2 };
            coords[i * 2..(i + 1) * 2].copy_from_slice(&map(dab.at));
            labels[i] = if dab.foreground { 1.0 } else { 0.0 };
        }
        let (scores, masks) = self
            .model
            .runtime
            .decode(&self.embedding, &coords, &labels)?;
        ensure!(!scores.is_empty(), "The model returned no masks");
        ensure!(
            scores.iter().all(|v| v.is_finite()),
            "Non-finite model scores"
        );
        ensure!(masks.iter().all(|v| v.is_finite()), "Non-finite model mask");
        let samples: Vec<(usize, bool)> = self
            .guide
            .label
            .iter()
            .enumerate()
            .filter_map(|(i, &foreground)| {
                let point = (
                    self.guide.region.x0 as f32 + (i % self.guide.width) as f32 / self.guide.scale,
                    self.guide.region.y0 as f32 + (i / self.guide.width) as f32 / self.guide.scale,
                );
                if point.0 < rect.x0 as f32
                    || point.0 >= rect.x1 as f32
                    || point.1 < rect.y0 as f32
                    || point.1 >= rect.y1 as f32
                {
                    return None;
                }
                let p = map(point);
                let at = (p[1] / 4.0).clamp(0.0, 255.0) as usize * 256
                    + (p[0] / 4.0).clamp(0.0, 255.0) as usize;
                Some((at, foreground))
            })
            .collect();
        let sampled = samples.len().max(1) as f32;
        let guide_in_box = samples.iter().filter(|(_, f)| *f).count() as f32 / sampled;
        let (agreements, coverage): (Vec<f32>, Vec<f32>) = masks
            .as_chunks::<{ 256 * 256 }>()
            .0
            .iter()
            .map(|logits| {
                let (mut intersection, mut union, mut selected_total) = (0u32, 0u32, 0u32);
                for &(at, foreground) in &samples {
                    let selected = logits[at] > 0.0;
                    intersection += u32::from(selected && foreground);
                    union += u32::from(selected || foreground);
                    selected_total += u32::from(selected);
                }
                (
                    intersection as f32 / union.max(1) as f32,
                    selected_total as f32 / sampled,
                )
            })
            .unzip();
        // An almost empty or almost full colour guide agrees with a wrong mask as readily as a
        // right one, so it only arbitrates when it actually separated something inside the box.
        let guided = (0.10..=0.85).contains(&guide_in_box);
        let border = self.border(rect, &masks);
        let best = choose(&scores, &agreements, &coverage, &border, guided, point_only);
        // The guide overrules only what the model's own proposals do not back up, and a proposal
        // the model itself doubts is worth a second opinion whatever the guide says.
        let doubted = guided
            && if point_only {
                agreements[best] < 0.70
            } else {
                scores[best] < UNSURE
                    || (agreements[best] < 0.45 && consensus(&masks, best) < CONSENSUS)
            };
        if strokes.is_empty() && (coverage[best] > BACKDROP || doubted) {
            if !point_only && guided && self.hint.is_some() {
                return self.select_inner(size, rect, strokes, true);
            }
            return Ok(self
                .fallback
                .get_or_init(|| {
                    let mut cutout = super::Cutout::new(&self.pixels, rect);
                    cutout.run(5);
                    cutout
                })
                .mask());
        }
        Ok(self.expand(&masks[best * 256 * 256..(best + 1) * 256 * 256], size, rect))
    }

    // How much of the box's border band each proposal claims. A subject rarely lines the border,
    // so this is what tells it from the backdrop when the box itself says nothing.
    fn border(&self, rect: Rect, masks: &[f32]) -> Vec<f32> {
        let corner = |x: u32, y: u32| {
            let point = self.map((x as f32, y as f32));
            (
                (point[0] / 4.0).clamp(0.0, 255.0) as usize,
                (point[1] / 4.0).clamp(0.0, 255.0) as usize,
            )
        };
        let (x0, y0) = corner(rect.x0, rect.y0);
        let (x1, y1) = corner(rect.x1, rect.y1);
        let shortest = (x1.saturating_sub(x0)).min(y1.saturating_sub(y0)) as f32;
        let band = ((shortest * super::RIM).round() as usize).max(1);
        masks
            .as_chunks::<{ 256 * 256 }>()
            .0
            .iter()
            .map(|logits| {
                let (mut claimed, mut total) = (0u32, 0u32);
                for y in y0..y1 {
                    for x in x0..x1 {
                        if x >= x0 + band && y >= y0 + band && x + band < x1 && y + band < y1 {
                            continue;
                        }
                        total += 1;
                        claimed += u32::from(logits[y * 256 + x] > 0.0);
                    }
                }
                claimed as f32 / total.max(1) as f32
            })
            .collect()
    }

    // What the model sees under one correction stroke, at the granularity that stroke covers.
    pub fn region(
        &self,
        size: (u32, u32),
        rect: Rect,
        stroke: &[Dab],
        reach: f32,
    ) -> Result<Vec<u8>, String> {
        self.region_inner(size, rect, stroke, reach)
            .map_err(|e| format!("{e:#}"))
    }

    fn region_inner(
        &self,
        size: (u32, u32),
        rect: Rect,
        stroke: &[Dab],
        reach: f32,
    ) -> ModelResult<Vec<u8>> {
        ensure!(
            size == self.pixels.size() && rect == self.guide.rect,
            "The cached image or box changed"
        );
        let points = prompts(stroke, rect);
        ensure!(!points.is_empty(), "The stroke is outside the box");
        // The box is looser than what will be accepted, so the model still sees the edge to snap to.
        let reach = reach * 1.5 + points.iter().fold(0.0f32, |r, d| r.max(d.radius));
        let around = points
            .iter()
            .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, d: &Dab| {
                [
                    b[0].min(d.at.0 - reach),
                    b[1].min(d.at.1 - reach),
                    b[2].max(d.at.0 + reach),
                    b[3].max(d.at.1 + reach),
                ]
            });
        let mut coords = vec![0.0f32; (points.len() + 2) * 2];
        let mut labels = vec![1.0f32; points.len() + 2];
        coords[..2].copy_from_slice(
            &self.map((around[0].max(rect.x0 as f32), around[1].max(rect.y0 as f32))),
        );
        coords[2..4].copy_from_slice(
            &self.map((around[2].min(rect.x1 as f32), around[3].min(rect.y1 as f32))),
        );
        labels[0] = 2.0;
        labels[1] = 3.0;
        for (n, dab) in points.iter().enumerate() {
            coords[(n + 2) * 2..(n + 3) * 2].copy_from_slice(&self.map(dab.at));
        }
        let (_, masks) = self
            .model
            .runtime
            .decode(&self.embedding, &coords, &labels)?;
        ensure!(masks.iter().all(|v| v.is_finite()), "Non-finite model mask");
        let at = |point: (f32, f32)| {
            let p = self.map(point);
            (p[1] / 4.0).clamp(0.0, 255.0) as usize * 256 + (p[0] / 4.0).clamp(0.0, 255.0) as usize
        };
        let painted: Vec<usize> = points.iter().map(|d| at(d.at)).collect();
        let candidates = masks.as_chunks::<{ 256 * 256 }>().0;
        let measure = |logits: &[f32; 256 * 256]| {
            (
                painted.iter().filter(|i| logits[**i] > 0.0).count() as f32 / painted.len() as f32,
                logits.iter().filter(|v| **v > 0.0).count(),
            )
        };
        // The stroke says how much to take: the smallest part it fits inside, not the whole object.
        let best = (0..candidates.len())
            .map(|i| (i, measure(&candidates[i])))
            .min_by(|a, b| {
                (a.1.0 < 0.9, a.1.1)
                    .cmp(&(b.1.0 < 0.9, b.1.1))
                    .then_with(|| b.1.0.total_cmp(&a.1.0))
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        let (low, high) = (
            self.map((rect.x0 as f32, rect.y0 as f32)),
            self.map((rect.x1 as f32, rect.y1 as f32)),
        );
        let inside = (high[0] - low[0]) * (high[1] - low[1]) / 16.0;
        if measure(&candidates[best]).1 as f32 > inside * BACKDROP {
            return Ok(vec![0; size.0 as usize * size.1 as usize]);
        }
        Ok(self.expand(&candidates[best], size, rect))
    }

    fn map(&self, point: (f32, f32)) -> [f32; 2] {
        [
            (point.0 - self.region.x0 as f32) * self.resized.0 as f32 / self.region.width() as f32,
            (point.1 - self.region.y0 as f32) * self.resized.1 as f32 / self.region.height() as f32,
        ]
    }

    fn expand(&self, logits: &[f32], size: (u32, u32), rect: Rect) -> Vec<u8> {
        let mut mask = vec![0; size.0 as usize * size.1 as usize];
        for y in rect.rows() {
            for x in rect.cols() {
                let point = self.map((x as f32 + 0.5, y as f32 + 0.5));
                let (fx, fy) = (
                    (point[0] / 4.0 - 0.5).clamp(0.0, 255.0),
                    (point[1] / 4.0 - 0.5).clamp(0.0, 255.0),
                );
                let (x0, y0) = (fx as usize, fy as usize);
                let (x1, y1) = ((x0 + 1).min(255), (y0 + 1).min(255));
                let (dx, dy) = (fx.fract(), fy.fract());
                let value = (logits[y0 * 256 + x0] * (1.0 - dx) + logits[y0 * 256 + x1] * dx)
                    * (1.0 - dy)
                    + (logits[y1 * 256 + x0] * (1.0 - dx) + logits[y1 * 256 + x1] * dx) * dy;
                mask[y as usize * size.0 as usize + x as usize] = if value > 0.0 { 255 } else { 0 };
            }
        }
        mask
    }
}

// Four variations on one answer means the model is sure of it. Mutually exclusive ones mean it is
// guessing, and then the guide is worth listening to.
fn consensus(masks: &[f32], best: usize) -> f32 {
    let chunks = masks.as_chunks::<{ 256 * 256 }>().0;
    let Some(pick) = chunks.get(best).filter(|_| chunks.len() > 1) else {
        return 0.0;
    };
    let mut worst = 1.0f32;
    for (i, other) in chunks.iter().enumerate() {
        if i == best {
            continue;
        }
        let (mut shared, mut either) = (0u32, 0u32);
        for (a, b) in pick.iter().zip(other) {
            shared += u32::from(*a > 0.0 && *b > 0.0);
            either += u32::from(*a > 0.0 || *b > 0.0);
        }
        worst = worst.min(shared as f32 / either.max(1) as f32);
    }
    worst
}

fn choose(
    scores: &[f32],
    agreements: &[f32],
    coverage: &[f32],
    border: &[f32],
    guided: bool,
    point_only: bool,
) -> usize {
    let weight = if !guided {
        0.0
    } else if point_only {
        0.75
    } else {
        0.25
    };
    let rank = |i: usize| {
        scores[i] + weight * agreements[i] + if i == 0 { 0.025 } else { 0.0 }
            - 4.0 * (coverage[i] - SUSPECT).max(0.0)
            - EDGE * border[i]
    };
    (0..scores.len())
        .max_by(|a, b| rank(*a).total_cmp(&rank(*b)))
        .unwrap_or(0)
}

fn prompts(strokes: &[Dab], rect: Rect) -> Vec<Dab> {
    let mut prompts: Vec<Dab> = Vec::new();
    for &dab in strokes.iter().rev() {
        if dab.at.0 < rect.x0 as f32
            || dab.at.0 >= rect.x1 as f32
            || dab.at.1 < rect.y0 as f32
            || dab.at.1 >= rect.y1 as f32
        {
            continue;
        }
        if prompts
            .iter()
            .any(|newer| (newer.at.0 - dab.at.0).hypot(newer.at.1 - dab.at.1) <= newer.radius)
        {
            continue;
        }
        prompts.push(dab);
        if prompts.len() == 64 {
            break;
        }
    }
    prompts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_sampling_keeps_recent_corrections_and_excludes_outside_strokes() {
        let mut strokes: Vec<_> = (0..100)
            .map(|x| Dab {
                at: (x as f32 + 0.5, 5.5),
                radius: 0.6,
                foreground: true,
                stroke: 1,
            })
            .collect();
        strokes.push(Dab {
            at: (50.5, 5.5),
            radius: 3.0,
            foreground: false,
            stroke: 2,
        });
        let sampled = prompts(&strokes, Rect::new(10, 0, 90, 10));
        assert!(sampled.len() <= 64);
        assert!(!sampled[0].foreground);
        assert!(sampled.iter().all(|s| s.at.0 >= 10.0 && s.at.0 < 90.0));
        assert!(
            !sampled
                .iter()
                .any(|s| s.foreground && (s.at.0 - 50.5).abs() <= 3.0)
        );
    }

    #[test]
    fn a_proposal_that_lines_the_box_border_loses_to_the_one_that_does_not() {
        // Measured on a kangaroo in grass, where the guide called 88% of the box foreground and
        // its agreement alone ranked the backdrop above the animal.
        let scores = [0.653, 0.681, 0.867, 0.895];
        let agreements = [0.026, 0.309, 0.489, 0.322];
        let coverage = [0.023, 0.281, 0.585, 0.292];
        let border = [0.03, 0.05, 0.88, 0.06];
        assert_eq!(
            choose(&scores, &agreements, &coverage, &border, true, false),
            3
        );
        assert_eq!(
            choose(&scores, &agreements, &coverage, &border, false, false),
            3
        );
    }

    #[test]
    fn a_proposal_that_swallows_the_box_is_ranked_out() {
        // Measured on open scissors, where every confident proposal covered the whole box.
        let scores = [0.900, 0.520, 0.856, 0.990];
        let agreements = [0.107, 0.150, 0.024, 0.125];
        let coverage = [0.961, 0.249, 0.854, 0.981];
        let border = [1.000, 0.150, 0.980, 1.000];
        assert_eq!(
            choose(&scores, &agreements, &coverage, &border, true, false),
            1
        );
    }

    #[test]
    fn proposals_that_repeat_one_answer_agree_and_mutually_exclusive_ones_do_not() {
        let mut alike = vec![-1.0f32; 4 * 256 * 256];
        for candidate in 0..4 {
            for at in 0..40_000 + candidate * 100 {
                alike[candidate * 256 * 256 + at] = 1.0;
            }
        }
        assert!(consensus(&alike, 0) > CONSENSUS);
        let mut opposed = vec![-1.0f32; 2 * 256 * 256];
        for at in 0..256 * 256 {
            opposed[at] = if at < 40_000 { 1.0 } else { -1.0 };
            opposed[256 * 256 + at] = -opposed[at];
        }
        assert_eq!(consensus(&opposed, 0), 0.0);
    }

    #[test]
    fn bundled_model_selects_a_photographed_truck_and_reuses_its_embedding() {
        let decoded =
            image::load_from_memory(include_bytes!("../../../tests/fixtures/cutout/truck.jpg"))
                .unwrap()
                .to_rgba8();
        let pixels =
            Rgba8::from_raw(decoded.width(), decoded.height(), decoded.into_raw()).unwrap();
        let rect = Rect::new(60, 240, 1750, 880);
        let model = Model::bundled().unwrap();
        assert!(Arc::ptr_eq(&model, &Model::bundled().unwrap()));
        let encoded = model.encode(&pixels, rect).unwrap();
        let mask = encoded.select(pixels.size(), rect, &[]).unwrap();
        let w = pixels.width() as usize;
        assert_eq!(mask[550 * w + 1000], 255, "truck body");
        assert_eq!(mask[100 * w + 1000], 0, "sky outside box");
        assert_eq!(mask[870 * w + 1000], 0, "road inside box");
        assert_eq!(mask, encoded.select(pixels.size(), rect, &[]).unwrap());
        let stroke = Dab {
            at: (1000.5, 550.5),
            radius: 8.0,
            foreground: false,
            stroke: 1,
        };
        let mut corrected = encoded.select(pixels.size(), rect, &[stroke]).unwrap();
        super::super::refine::constrain(&mut corrected, &pixels, rect, &[stroke]);
        assert_eq!(corrected[550 * w + 1000], 0);
        assert!(
            encoded
                .select(pixels.size(), Rect::new(0, 0, 20, 20), &[])
                .is_err()
        );
        assert!(model.encode(&pixels, Rect::new(0, 0, 0, 0)).is_err());
        assert!(Model::load(b"not an ONNX graph", DECODER).is_err());
    }
}
