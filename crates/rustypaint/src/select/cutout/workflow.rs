use super::{
    Cutout,
    model::{Encoded, Model},
    refine::{self, Dab, Settings},
};
use crate::doc::{Rect, Rgba8};
use std::sync::Arc;

#[derive(Clone)]
enum Analysis {
    Colour(Arc<Cutout>),
    Object(Arc<Encoded>),
    Tone,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Target {
    #[default]
    Subject,
    Background,
    Colour,
}

impl Target {
    pub const ALL: [Self; 3] = [Self::Subject, Self::Background, Self::Colour];

    pub fn name(self) -> &'static str {
        match self {
            Self::Subject => crate::i18n::cutout_subject(),
            Self::Background => crate::i18n::cutout_background(),
            Self::Colour => crate::i18n::cutout_colour(),
        }
    }

    fn analysed_alike(self, other: Self) -> bool {
        (self == Self::Colour) == (other == Self::Colour)
    }
}

pub struct ResultMask {
    analysis: Analysis,
    aim: Aim,
    pub base: Arc<Vec<u8>>,
    pub raw: Arc<Vec<u8>>,
    pub mask: Vec<u8>,
    pub strokes: Vec<Dab>,
    pub foreground: Option<Rgba8>,
    pub colour_fallback: bool,
}

// What the cut is after, and the colour it is after when that is a colour.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Aim {
    pub target: Target,
    pub tone: [u8; 4],
    pub tolerance: f32,
}

impl Default for Aim {
    fn default() -> Self {
        Self {
            target: Target::default(),
            tone: [0, 0, 0, 255],
            tolerance: 0.2,
        }
    }
}

pub struct Request {
    pub rect: Rect,
    pub object: bool,
    pub aim: Aim,
    pub path: Option<std::path::PathBuf>,
    pub strokes: Vec<Dab>,
    pub settings: Settings,
}

impl std::fmt::Debug for ResultMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultMask")
            .field("pixels", &self.mask.len())
            .finish_non_exhaustive()
    }
}

pub fn compute(
    pixels: &Rgba8,
    request: &Request,
    previous: Option<&ResultMask>,
) -> Result<ResultMask, String> {
    let Request {
        rect,
        object,
        aim,
        path,
        strokes,
        settings,
    } = request;
    let (rect, object, aim, settings) = (*rect, *object, *aim, *settings);
    let analysis = match previous.filter(|p| p.aim.target.analysed_alike(aim.target)) {
        Some(previous) => previous.analysis.clone(),
        None if aim.target == Target::Colour => Analysis::Tone,
        None if object => {
            let model = match path.as_deref() {
                Some(path) => Model::from_directory(path)?,
                None => Model::bundled()?,
            };
            Analysis::Object(model.encode(pixels, rect)?)
        }
        None => {
            let mut cutout = Cutout::new(pixels, rect);
            cutout.run(5);
            Analysis::Colour(Arc::new(cutout))
        }
    };
    let base = match previous.filter(|p| p.aim == aim) {
        Some(previous) => previous.base.clone(),
        None => {
            let mut base = propose(&analysis, pixels, rect, aim, &[])?;
            if aim.target == Target::Background {
                for y in rect.rows() {
                    for x in rect.cols() {
                        let at = y as usize * pixels.width() as usize + x as usize;
                        base[at] = 255 - base[at];
                    }
                }
            }
            Arc::new(base)
        }
    };
    let raw = match previous {
        Some(previous) if previous.strokes == *strokes && previous.aim == aim => {
            previous.raw.clone()
        }
        _ if strokes.is_empty() => base.clone(),
        _ => {
            // Drags already accounted for stay accounted for, so adding one costs one, not all.
            let carried = previous
                .filter(|p| p.aim == aim)
                .filter(|p| matches!(&p.analysis, Analysis::Object(e) if !e.colour_fallback()))
                .filter(|p| {
                    let done = p.strokes.len();
                    (1..strokes.len()).contains(&done)
                        && strokes.starts_with(&p.strokes)
                        && strokes[done].stroke != strokes[done - 1].stroke
                });
            let (from, todo) = match carried {
                Some(previous) => (previous.raw.as_slice(), &strokes[previous.strokes.len()..]),
                None => (base.as_slice(), strokes.as_slice()),
            };
            Arc::new(correct(&analysis, pixels, rect, aim, from, todo)?)
        }
    };
    let mask = refine::finish(pixels, &raw, rect, strokes, settings);
    let foreground = settings
        .decontaminate
        .then(|| refine::decontaminate(pixels, &mask));
    let colour_fallback =
        matches!(&analysis, Analysis::Object(encoded) if encoded.colour_fallback());
    Ok(ResultMask {
        analysis,
        aim,
        base,
        raw,
        mask,
        strokes: strokes.to_vec(),
        foreground,
        colour_fallback,
    })
}

fn propose(
    analysis: &Analysis,
    pixels: &Rgba8,
    rect: Rect,
    aim: Aim,
    strokes: &[Dab],
) -> Result<Vec<u8>, String> {
    match analysis {
        Analysis::Tone => Ok(tone(pixels, rect, aim)),
        Analysis::Object(encoded) => encoded.select(pixels.size(), rect, strokes),
        Analysis::Colour(base) => {
            let mut cutout = (**base).clone();
            for dab in strokes {
                cutout.paint(dab.at, dab.radius, dab.foreground);
            }
            if !strokes.is_empty() {
                cutout.recut();
            }
            Ok(cutout.mask())
        }
    }
}

// Each drag asks the model what it lies on and takes or gives back that much. Without one the cut
// is simply re-proposed, which leaves the strokes to keep only what they reach.
fn correct(
    analysis: &Analysis,
    pixels: &Rgba8,
    rect: Rect,
    aim: Aim,
    from: &[u8],
    strokes: &[Dab],
) -> Result<Vec<u8>, String> {
    let object = match analysis {
        Analysis::Object(encoded) if !encoded.colour_fallback() => Some(encoded),
        _ => None,
    };
    let Some(encoded) = object else {
        let proposed = propose(analysis, pixels, rect, aim, strokes)?;
        let reach = reach(strokes, from, pixels.size());
        return Ok(localise(from, &proposed, strokes, pixels.size(), reach));
    };
    let mut out = from.to_vec();
    for group in strokes.chunk_by(|a, b| a.stroke == b.stroke) {
        let reach = reach(group, &out, pixels.size());
        let region = encoded.region(pixels.size(), rect, group, reach)?;
        let wanted = group.first().is_some_and(|dab| dab.foreground);
        let proposed: Vec<u8> = out
            .iter()
            .zip(&region)
            .map(|(kept, taken)| {
                if *taken > 128 {
                    u8::from(wanted) * 255
                } else {
                    *kept
                }
            })
            .collect();
        out = localise(&out, &proposed, group, pixels.size(), reach);
    }
    Ok(out)
}

// How far past the paint one drag may pull the edge with it. A drag that straddles the edge is
// tracing it and may only move it as far as it painted; one laid into what it is changing takes all.
fn reach(strokes: &[Dab], from: &[u8], size: (u32, u32)) -> f32 {
    let widest = strokes.iter().fold(0.0f32, |r, dab| r.max(dab.radius));
    let wanted = strokes.first().is_some_and(|dab| dab.foreground);
    let under = painted(strokes, size, 0.0);
    let (mut covered, mut settled) = (0usize, 0usize);
    for (i, touched) in under.iter().enumerate() {
        if *touched {
            covered += 1;
            settled += usize::from((from[i] > 128) == wanted);
        }
    }
    let tracing = covered > 0 && settled * 4 >= covered;
    widest * if tracing { 0.5 } else { 3.0 }
}

fn painted(strokes: &[Dab], size: (u32, u32), reach: f32) -> Vec<bool> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut out = vec![false; w * h];
    for dab in strokes {
        let Some(area) = Rect::around(dab.at.0, dab.at.1, dab.radius + reach, size.0, size.1)
        else {
            continue;
        };
        for y in area.rows() {
            for x in area.cols() {
                let i = y as usize * w + x as usize;
                if !out[i]
                    && (x as f32 + 0.5 - dab.at.0).hypot(y as f32 + 0.5 - dab.at.1)
                        <= dab.radius + reach
                {
                    out[i] = true;
                }
            }
        }
    }
    out
}

// Everything close enough to the chosen colour, which the strokes then correct by hand.
fn tone(pixels: &Rgba8, rect: Rect, aim: Aim) -> Vec<u8> {
    let limit = (aim.tolerance * 441.673).powi(2);
    let bytes = pixels.as_bytes();
    let mut mask = vec![0; pixels.width() as usize * pixels.height() as usize];
    for y in rect.rows() {
        for x in rect.cols() {
            let at = y as usize * pixels.width() as usize + x as usize;
            let apart: f32 = (0..3)
                .map(|c| (bytes[at * 4 + c] as f32 - aim.tone[c] as f32).powi(2))
                .sum();
            mask[at] = if apart <= limit { 255 } else { 0 };
        }
    }
    mask
}

// Only a change in the painted direction reaching back to a stroke counts. It is taken whole while
// it stays near the paint, and clipped to the drag's reach when it runs, boundary of its own and all.
fn localise(
    base: &[u8],
    proposed: &[u8],
    strokes: &[Dab],
    size: (u32, u32),
    reach: f32,
) -> Vec<u8> {
    let within = painted(strokes, size, reach);
    let free = flood(base, proposed, strokes, size, None);
    match (0..base.len()).all(|i| within[i] || free[i] == base[i]) {
        true => free,
        false => flood(base, proposed, strokes, size, Some(&within)),
    }
}

fn flood(
    base: &[u8],
    proposed: &[u8],
    strokes: &[Dab],
    size: (u32, u32),
    within: Option<&[bool]>,
) -> Vec<u8> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut open = Vec::new();
    let mut taken = vec![false; base.len()];
    let changed = |i: usize, adding: bool| {
        within.is_none_or(|within| within[i])
            && (proposed[i] > 128) == adding
            && (base[i] > 128) != (proposed[i] > 128)
    };
    for dab in strokes {
        let Some(area) = Rect::around(dab.at.0, dab.at.1, dab.radius, size.0, size.1) else {
            continue;
        };
        for y in area.rows() {
            for x in area.cols() {
                let i = y as usize * w + x as usize;
                if taken[i]
                    || !changed(i, dab.foreground)
                    || (x as f32 + 0.5 - dab.at.0).hypot(y as f32 + 0.5 - dab.at.1) > dab.radius
                {
                    continue;
                }
                taken[i] = true;
                open.push(i);
            }
        }
    }
    let mut out = base.to_vec();
    while let Some(i) = open.pop() {
        out[i] = proposed[i];
        let adding = proposed[i] > 128;
        let (x, y) = (i % w, i / w);
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            let n = ny * w + nx;
            if nx < w && ny < h && !taken[n] && changed(n, adding) {
                taken[n] = true;
                open.push(n);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dab(at: (f32, f32), radius: f32, foreground: bool, stroke: u32) -> Dab {
        Dab {
            at,
            radius,
            foreground,
            stroke,
        }
    }

    #[test]
    fn a_correction_only_changes_what_its_stroke_reaches() {
        let width = 60u32;
        let base = vec![255u8; (width * width) as usize];
        let blob = |cx: f32, cy: f32| {
            let mut out = base.clone();
            for (i, value) in out.iter_mut().enumerate() {
                let (x, y) = (
                    (i as u32 % width) as f32 + 0.5,
                    (i as u32 / width) as f32 + 0.5,
                );
                if (x - cx).hypot(y - cy) <= 6.0 {
                    *value = 0;
                }
            }
            out
        };
        let mut proposed = blob(15.0, 15.0);
        for (i, value) in proposed.iter_mut().enumerate() {
            let (x, y) = (
                (i as u32 % width) as f32 + 0.5,
                (i as u32 / width) as f32 + 0.5,
            );
            if (x - 45.0).hypot(y - 45.0) <= 6.0 {
                *value = 0;
            }
        }
        let stroke = [dab((15.5, 15.5), 2.0, false, 1)];
        let out = localise(&base, &proposed, &stroke, (width, width), 6.0);
        assert_eq!(out, blob(15.0, 15.0), "only the painted blob goes");
    }

    fn truck() -> Rgba8 {
        let decoded =
            image::load_from_memory(include_bytes!("../../../tests/fixtures/cutout/truck.jpg"))
                .unwrap()
                .to_rgba8();
        Rgba8::from_raw(decoded.width(), decoded.height(), decoded.into_raw()).unwrap()
    }

    fn kept(mask: &[u8]) -> usize {
        mask.iter().filter(|v| **v > 128).count()
    }

    #[test]
    fn a_bigger_brush_takes_more_of_what_it_finds() {
        let pixels = truck();
        let mut request = Request {
            rect: Rect::new(60, 240, 1750, 880),
            object: true,
            aim: Aim::default(),
            path: None,
            strokes: vec![],
            settings: Settings::default(),
        };
        let base = compute(&pixels, &request, None).unwrap();
        request.strokes = vec![dab((800.5, 560.5), 16.0, false, 1)];
        let small = compute(&pixels, &request, Some(&base)).unwrap();
        request.strokes = vec![dab((800.5, 560.5), 64.0, false, 2)];
        let large = compute(&pixels, &request, Some(&base)).unwrap();
        assert!(
            kept(&small.mask) < kept(&base.mask),
            "the stroke takes something"
        );
        assert!(
            kept(&large.mask) < kept(&small.mask),
            "and a bigger brush takes more of it"
        );
        let far = (0..pixels.width() as usize * pixels.height() as usize).all(|i| {
            let (x, y) = (
                (i % pixels.width() as usize) as f32 + 0.5,
                (i / pixels.width() as usize) as f32 + 0.5,
            );
            (x - 800.5).hypot(y - 560.5) <= 16.0 * 4.0 || small.raw[i] == base.raw[i]
        });
        assert!(far, "and none of it lands out of the brush's reach");
    }

    // The nearest thing to a stroke the model does not already agree with: a lit pixel next to an
    // unlit one, taken from the middle of the list so the choice does not depend on scan order.
    fn on_the_edge(mask: &[u8], width: usize) -> (f32, f32) {
        let edges: Vec<usize> = (0..mask.len())
            .filter(|&i| {
                mask[i] > 128
                    && [i.wrapping_sub(1), i + 1, i.wrapping_sub(width), i + width]
                        .iter()
                        .any(|&n| mask.get(n).is_none_or(|m| *m <= 128))
            })
            .collect();
        let at = edges[edges.len() / 2];
        ((at % width) as f32 + 0.5, (at / width) as f32 + 0.5)
    }

    #[test]
    fn a_drag_along_the_edge_nudges_it_while_one_laid_inside_takes_the_area() {
        let pixels = truck();
        let width = pixels.width() as usize;
        let mut request = Request {
            rect: Rect::new(60, 240, 1750, 880),
            object: true,
            aim: Aim::default(),
            path: None,
            strokes: vec![],
            settings: Settings::default(),
        };
        let base = compute(&pixels, &request, None).unwrap();
        request.strokes = vec![dab(on_the_edge(&base.raw, width), 16.0, false, 1)];
        let traced = compute(&pixels, &request, Some(&base)).unwrap();
        request.strokes = vec![dab((1000.5, 550.5), 16.0, false, 2)];
        let laid = compute(&pixels, &request, Some(&base)).unwrap();
        let taken = |m: &ResultMask| kept(&base.mask).saturating_sub(kept(&m.mask));
        assert!(taken(&traced) > 0, "tracing the edge still moves it");
        assert!(
            taken(&traced) * 4 < taken(&laid),
            "but a drag laid inside takes far more: {} against {}",
            taken(&traced),
            taken(&laid)
        );
    }

    #[test]
    fn each_drag_corrects_its_own_area_and_nothing_it_never_touched() {
        let pixels = truck();
        let width = pixels.width() as usize;
        let mut request = Request {
            rect: Rect::new(60, 240, 1750, 880),
            object: true,
            aim: Aim::default(),
            path: None,
            strokes: vec![],
            settings: Settings::default(),
        };
        let base = compute(&pixels, &request, None).unwrap();
        let first: Vec<Dab> = (0..8)
            .map(|i| dab((800.0 + i as f32 * 6.0, 560.5), 16.0, false, 1))
            .collect();
        request.strokes = first.clone();
        let once = compute(&pixels, &request, Some(&base)).unwrap();
        assert_eq!(once.mask[560 * width + 810], 0, "the stroke is taken out");
        assert!(reachable(
            &base.raw,
            &once.raw,
            &request.strokes,
            pixels.size()
        ));
        request
            .strokes
            .extend((0..8).map(|i| dab((1500.0 + i as f32 * 6.0, 700.5), 16.0, false, 2)));
        let twice = compute(&pixels, &request, Some(&once)).unwrap();
        assert_eq!(
            twice.mask[560 * width + 810],
            0,
            "the first drag still holds"
        );
        assert_eq!(twice.mask[700 * width + 1510], 0, "and so does the second");
        assert!(reachable(
            &base.raw,
            &twice.raw,
            &request.strokes,
            pixels.size()
        ));
    }

    // Every pixel a correction changed connects back to a stroke through other changed pixels.
    fn reachable(base: &[u8], corrected: &[u8], strokes: &[Dab], size: (u32, u32)) -> bool {
        let (w, h) = (size.0 as usize, size.1 as usize);
        let changed = |i: usize| (base[i] > 128) != (corrected[i] > 128);
        let mut seen = vec![false; base.len()];
        let mut open = Vec::new();
        for stroke in strokes {
            let (x, y) = (stroke.at.0 as usize, stroke.at.1 as usize);
            let i = y * w + x;
            if changed(i) && !seen[i] {
                seen[i] = true;
                open.push(i);
            }
        }
        while let Some(i) = open.pop() {
            let (x, y) = (i % w, i / w);
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                let n = ny * w + nx;
                if nx < w && ny < h && changed(n) && !seen[n] {
                    seen[n] = true;
                    open.push(n);
                }
            }
        }
        (0..base.len()).all(|i| !changed(i) || seen[i])
    }

    #[test]
    fn the_background_target_keeps_everything_the_subject_one_does_not() {
        let mut pixels = Rgba8::new(60, 40, [30, 60, 180, 255]);
        for y in 10..30 {
            for x in 15..45 {
                pixels.pixels_mut()[(y * 60 + x) * 4..(y * 60 + x) * 4 + 4]
                    .copy_from_slice(&[200, 30, 20, 255]);
            }
        }
        let rect = Rect::new(5, 5, 55, 35);
        let mut request = Request {
            rect,
            object: false,
            aim: Aim::default(),
            path: None,
            strokes: vec![],
            settings: Settings {
                radius: 0.0,
                ..Settings::default()
            },
        };
        let subject = compute(&pixels, &request, None).unwrap();
        request.aim.target = Target::Background;
        let background = compute(&pixels, &request, Some(&subject)).unwrap();
        assert_eq!(
            subject.mask[20 * 60 + 30],
            255,
            "the red patch is the subject"
        );
        assert_eq!(background.mask[20 * 60 + 30], 0);
        assert_eq!(background.mask[8 * 60 + 8], 255, "the blue ground is kept");
        assert_eq!(background.mask[2 * 60 + 2], 0, "outside the box stays out");
    }

    #[test]
    fn a_colour_target_takes_what_matches_it_and_widens_with_tolerance() {
        let mut pixels = Rgba8::new(60, 40, [30, 60, 180, 255]);
        for y in 10..30 {
            for x in 15..45 {
                pixels.pixels_mut()[(y * 60 + x) * 4..(y * 60 + x) * 4 + 4]
                    .copy_from_slice(&[200, 30, 20, 255]);
            }
        }
        pixels.pixels_mut()[(20 * 60 + 40) * 4..(20 * 60 + 40) * 4 + 4]
            .copy_from_slice(&[170, 60, 50, 255]);
        let mut request = Request {
            rect: Rect::new(5, 5, 55, 35),
            object: false,
            aim: Aim {
                target: Target::Colour,
                tone: [200, 30, 20, 255],
                tolerance: 0.05,
            },
            path: None,
            strokes: vec![],
            settings: Settings {
                radius: 0.0,
                ..Settings::default()
            },
        };
        let tight = compute(&pixels, &request, None).unwrap();
        assert_eq!(tight.mask[20 * 60 + 30], 255, "the patch it was given");
        assert_eq!(tight.mask[20 * 60 + 40], 0, "a near miss is still a miss");
        assert_eq!(tight.mask[8 * 60 + 8], 0, "and the blue ground is not it");
        request.aim.tolerance = 0.2;
        let loose = compute(&pixels, &request, Some(&tight)).unwrap();
        assert_eq!(loose.mask[20 * 60 + 40], 255, "tolerance reaches it");
        assert_eq!(loose.mask[8 * 60 + 8], 0);
        request.strokes = vec![dab((30.5, 20.5), 2.0, false, 1)];
        let rubbed = compute(&pixels, &request, Some(&loose)).unwrap();
        assert_eq!(
            rubbed.mask[20 * 60 + 30],
            0,
            "the stroke takes what it covers"
        );
        assert_eq!(
            rubbed.mask[20 * 60 + 40],
            255,
            "and leaves the rest of the colour"
        );
    }

    #[test]
    fn carrying_earlier_drags_forward_gives_what_replaying_them_would() {
        let pixels = truck();
        let mut request = Request {
            rect: Rect::new(60, 240, 1750, 880),
            object: true,
            aim: Aim::default(),
            path: None,
            strokes: vec![],
            settings: Settings::default(),
        };
        let base = compute(&pixels, &request, None).unwrap();
        request.strokes = vec![dab((800.5, 560.5), 16.0, false, 1)];
        let once = compute(&pixels, &request, Some(&base)).unwrap();
        request.strokes.push(dab((1510.5, 710.5), 24.0, false, 2));
        let carried = compute(&pixels, &request, Some(&once)).unwrap();
        let replayed = compute(&pixels, &request, Some(&base)).unwrap();
        assert_eq!(carried.raw, replayed.raw);
        assert!(kept(&carried.mask) < kept(&once.mask));
    }

    #[test]
    fn edge_edits_reuse_analysis_and_raw_mask_and_strokes_are_reversible() {
        let mut pixels = Rgba8::new(60, 40, [30, 60, 180, 255]);
        for y in 10..30 {
            for x in 15..45 {
                pixels.pixels_mut()[(y * 60 + x) * 4..(y * 60 + x) * 4 + 4]
                    .copy_from_slice(&[200, 30, 20, 255]);
            }
        }
        let mut request = Request {
            rect: Rect::new(5, 5, 55, 35),
            object: false,
            aim: Aim::default(),
            path: None,
            strokes: vec![],
            settings: Settings::default(),
        };
        let initial = compute(&pixels, &request, None).unwrap();
        request.settings.feather = 2.0;
        let feathered = compute(&pixels, &request, Some(&initial)).unwrap();
        assert!(Arc::ptr_eq(&initial.raw, &feathered.raw));
        assert_ne!(initial.mask, feathered.mask);
        request.strokes.push(dab((30.5, 20.5), 2.0, false, 1));
        let removed = compute(&pixels, &request, Some(&feathered)).unwrap();
        assert_eq!(removed.mask[20 * 60 + 30], 0);
        request.strokes.clear();
        let restored = compute(&pixels, &request, Some(&removed)).unwrap();
        assert_eq!(restored.mask, feathered.mask);
    }
}
