pub mod gmm;
pub mod maxflow;
pub mod model;
pub mod refine;
mod runtime;
pub mod workflow;

use crate::doc::{Rect, Rgba8, image::CHANNELS};
use gmm::Gmm;
use maxflow::{DIRS, Grid};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Known {
    Background,
    Foreground,
    Either,
}

const GAMMA: f32 = 50.0;

const LAMBDA: f32 = GAMMA * 9.0;

const RIM: f32 = 0.09;

pub const WORKING: usize = 560;

#[derive(Clone)]
pub struct Cutout {
    width: usize,
    height: usize,
    colours: Arc<Vec<[f32; 3]>>,
    known: Vec<Known>,
    label: Vec<bool>,
    links: Arc<Vec<f32>>,
    models: Option<(Gmm, Gmm)>,
    scale: f32,
    source: (u32, u32),
    region: Rect,
    rect: Rect,
    strokes: Vec<refine::Dab>,
}

impl Cutout {
    pub fn new(pixels: &Rgba8, rect: Rect) -> Self {
        Self::with_limit(pixels, rect, WORKING)
    }

    pub(super) fn with_limit(pixels: &Rgba8, rect: Rect, limit: usize) -> Self {
        let (source_w, source_h) = pixels.size();
        let rect = Rect::new(
            rect.x0.min(source_w),
            rect.y0.min(source_h),
            rect.x1.min(source_w),
            rect.y1.min(source_h),
        );
        let margin = (rect.width().max(rect.height()) / 12).max(8);
        let region = Rect::new(
            rect.x0.saturating_sub(margin),
            rect.y0.saturating_sub(margin),
            rect.x1.saturating_add(margin).min(source_w),
            rect.y1.saturating_add(margin).min(source_h),
        );
        let longest = region.width().max(region.height()).max(1) as usize;
        let scale = (limit as f32 / longest as f32).min(1.0);
        let width = ((region.width() as f32 * scale).round() as usize).max(1);
        let height = ((region.height() as f32 * scale).round() as usize).max(1);

        let mut colours = Vec::with_capacity(width * height);
        let bytes = pixels.as_bytes();
        for y in 0..height {
            for x in 0..width {
                let sx = region.x0 + ((x as f32 + 0.5) / scale) as u32;
                let sy = region.y0 + ((y as f32 + 0.5) / scale) as u32;
                let i = (sy.min(source_h - 1) as usize * source_w as usize
                    + sx.min(source_w - 1) as usize)
                    * CHANNELS;
                colours.push([bytes[i] as f32, bytes[i + 1] as f32, bytes[i + 2] as f32]);
            }
        }

        let mut cutout = Self {
            width,
            height,
            colours: Arc::new(colours),
            known: vec![Known::Background; width * height],
            label: vec![false; width * height],
            links: Arc::new(Vec::new()),
            models: None,
            scale,
            source: (source_w, source_h),
            region,
            rect,
            strokes: Vec::new(),
        };

        let box_in_working = |v: u32, s: f32| (v as f32 * s).round() as usize;
        let x0 = box_in_working(rect.x0 - region.x0, scale).min(width);
        let y0 = box_in_working(rect.y0 - region.y0, scale).min(height);
        let x1 = box_in_working(rect.x1 - region.x0, scale).min(width);
        let y1 = box_in_working(rect.y1 - region.y0, scale).min(height);
        let outside = (width * height - (x1 - x0) * (y1 - y0)) as f32;
        let roomy = outside > (width * height) as f32 * 0.05;
        let band = (((x1 - x0).min(y1 - y0) as f32 * RIM).round() as usize).max(1);
        for y in y0..y1 {
            for x in x0..x1 {
                cutout.known[y * width + x] = Known::Either;
                let rim = x < x0 + band || y < y0 + band || x + band >= x1 || y + band >= y1;
                cutout.label[y * width + x] = roomy || !rim;
            }
        }

        cutout.links = Arc::new(cutout.build_links());
        cutout
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn paint(&mut self, at: (f32, f32), radius: f32, foreground: bool) {
        self.strokes.push(refine::Dab {
            at,
            radius,
            foreground,
            stroke: 0,
        });
        let radius = (radius * self.scale).max(0.75);
        let centre = (
            (at.0 - self.region.x0 as f32) * self.scale,
            (at.1 - self.region.y0 as f32) * self.scale,
        );
        let known = if foreground {
            Known::Foreground
        } else {
            Known::Background
        };

        let x0 = ((centre.0 - radius).floor().max(0.0)) as usize;
        let y0 = ((centre.1 - radius).floor().max(0.0)) as usize;
        let x1 = ((centre.0 + radius).ceil() as usize).min(self.width);
        let y1 = ((centre.1 + radius).ceil() as usize).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = (x as f32 + 0.5 - centre.0).powi(2) + (y as f32 + 0.5 - centre.1).powi(2);
                if d <= radius * radius {
                    self.known[y * self.width + x] = known;
                    self.label[y * self.width + x] = foreground;
                }
            }
        }
    }

    pub fn run(&mut self, passes: usize) {
        let mut models: Option<(Gmm, Gmm)> = None;
        for _pass in 0..passes {
            if std::env::var("CUTOUT_TRACE").is_ok() {
                let kept = self.label.iter().filter(|l| **l).count();
                eprintln!("    pass {_pass}: {kept} of {} kept", self.label.len());
            }
            let Some(fitted) = self.fit(models.as_ref()) else {
                return;
            };
            let before = self.label.clone();
            self.cut(&fitted.0, &fitted.1);
            models = Some(fitted);

            let moved = before
                .iter()
                .zip(&self.label)
                .filter(|(was, now)| was != now)
                .count();
            let decided = self.known.iter().filter(|k| **k == Known::Either).count();
            if std::env::var("CUTOUT_TRACE").is_ok() {
                eprintln!("      moved {moved} of {decided} decided");
            }
            if moved * 200 < decided * 3 {
                break;
            }
        }
        self.models = models;
    }

    pub fn recut(&mut self) {
        let Some(models) = self.models.take() else {
            self.run(1);
            return;
        };
        self.cut(&models.0, &models.1);

        let kept = self.label.iter().filter(|l| **l).count();
        let forced = self
            .known
            .iter()
            .filter(|k| **k == Known::Foreground)
            .count();
        if kept < forced * 2 {
            let mut fg = Vec::new();
            let mut bg = Vec::new();
            for (i, colour) in self.colours.iter().enumerate() {
                match self.known[i] {
                    Known::Foreground => fg.push(*colour),
                    Known::Background => bg.push(*colour),
                    Known::Either => {}
                }
            }
            if !fg.is_empty() && !bg.is_empty() {
                let models = (
                    Gmm::fit(&fg, &gmm::cluster(&fg)),
                    Gmm::fit(&bg, &gmm::cluster(&bg)),
                );
                self.cut(&models.0, &models.1);
                self.models = Some(models);
            } else {
                self.models = Some(models);
            }
            return;
        }
        self.models = Some(models);
    }

    fn fit(&self, previous: Option<&(Gmm, Gmm)>) -> Option<(Gmm, Gmm)> {
        let mut fg = Vec::new();
        let mut bg = Vec::new();
        for (i, colour) in self.colours.iter().enumerate() {
            if self.label[i] {
                fg.push(*colour);
            } else {
                bg.push(*colour);
            }
        }
        if fg.is_empty() || bg.is_empty() {
            return None;
        }

        let (fg_parts, bg_parts) = match previous {
            Some((old_fg, old_bg)) => (
                fg.iter().map(|c| old_fg.nearest(*c)).collect(),
                bg.iter().map(|c| old_bg.nearest(*c)).collect(),
            ),
            None => (gmm::cluster(&fg), gmm::cluster(&bg)),
        };
        Some((Gmm::fit(&fg, &fg_parts), Gmm::fit(&bg, &bg_parts)))
    }

    fn cut(&mut self, foreground: &Gmm, background: &Gmm) {
        let mut grid = Grid::new(self.width, self.height);
        let region = self.region();

        for node in 0..self.colours.len() {
            let (x, y) = (node % self.width, node / self.width);
            if x < region.0 || y < region.1 || x >= region.2 || y >= region.3 {
                continue;
            }
            let (source, sink) = match self.known[node] {
                Known::Background => (0.0, LAMBDA),
                Known::Foreground => (LAMBDA, 0.0),
                Known::Either => (
                    cost(background.likelihood(self.colours[node])),
                    cost(foreground.likelihood(self.colours[node])),
                ),
            };
            grid.set_terminal(node, source, sink);

            for dir in 0..8 {
                if dir % 2 == 1 {
                    continue;
                }
                if grid.neighbour(node, dir).is_some() {
                    grid.set_neighbour(node, dir, self.links[node * 8 + dir]);
                }
            }
        }

        grid.max_flow();
        for node in 0..self.label.len() {
            self.label[node] = match self.known[node] {
                Known::Background => false,
                Known::Foreground => true,
                Known::Either => grid.is_source(node),
            };
        }
    }

    fn region(&self) -> (usize, usize, usize, usize) {
        const MARGIN: usize = 8;
        let (mut x0, mut y0) = (self.width, self.height);
        let (mut x1, mut y1) = (0usize, 0usize);
        for (node, known) in self.known.iter().enumerate() {
            if *known == Known::Background {
                continue;
            }
            let (x, y) = (node % self.width, node / self.width);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
        if x1 <= x0 || y1 <= y0 {
            return (0, 0, self.width, self.height);
        }
        (
            x0.saturating_sub(MARGIN),
            y0.saturating_sub(MARGIN),
            (x1 + MARGIN).min(self.width),
            (y1 + MARGIN).min(self.height),
        )
    }

    fn build_links(&self) -> Vec<f32> {
        let mut total = 0.0;
        let mut pairs = 0.0;
        for node in 0..self.colours.len() {
            for dir in (0..8).step_by(2) {
                let Some(other) = self.neighbour(node, dir) else {
                    continue;
                };
                total += distance(self.colours[node], self.colours[other]);
                pairs += 1.0;
            }
        }
        let beta = if total <= 0.0 {
            0.0
        } else {
            pairs / (2.0 * total)
        };

        let mut links = vec![0.0; self.colours.len() * 8];
        for node in 0..self.colours.len() {
            for dir in 0..8 {
                let Some(other) = self.neighbour(node, dir) else {
                    continue;
                };
                let (dx, dy) = DIRS[dir];
                let apart = if dx != 0 && dy != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                let d = distance(self.colours[node], self.colours[other]);
                links[node * 8 + dir] = GAMMA / apart * (-beta * d).exp();
            }
        }
        links
    }

    fn neighbour(&self, node: usize, dir: usize) -> Option<usize> {
        let (dx, dy) = DIRS[dir];
        let x = (node % self.width) as i32 + dx;
        let y = (node / self.width) as i32 + dy;
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
    }

    pub fn working_mask(&self) -> Vec<u8> {
        self.label
            .iter()
            .map(|fg| if *fg { 255 } else { 0 })
            .collect()
    }

    pub fn mask(&self) -> Vec<u8> {
        let (w, h) = (self.source.0 as usize, self.source.1 as usize);
        let mut out = vec![0u8; w * h];
        for y in self.rect.y0 as usize..self.rect.y1 as usize {
            let sy =
                (((y - self.region.y0 as usize) as f32 * self.scale) as usize).min(self.height - 1);
            for x in self.rect.x0 as usize..self.rect.x1 as usize {
                let sx = (((x - self.region.x0 as usize) as f32 * self.scale) as usize)
                    .min(self.width - 1);
                out[y * w + x] = if self.label[sy * self.width + sx] {
                    255
                } else {
                    0
                };
            }
        }
        out
    }
}

impl Cutout {
    pub fn refined_mask(&self, pixels: &Rgba8) -> Vec<u8> {
        let mask = self.mask();
        if pixels.size() != self.source {
            return mask;
        }
        let reach = ((1.0 / self.scale).ceil() as u32 + 1).clamp(2, 24);
        let mut out = refine::matte(pixels, &mask, self.rect, reach);
        refine::constrain(&mut out, pixels, self.rect, &self.strokes);
        out
    }
}

fn cost(likelihood: f32) -> f32 {
    -(likelihood.max(1e-12)).ln()
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

pub fn fill_behind(pixels: &Rgba8, mask: &[u8], rect: Rect) -> Rgba8 {
    let (w, h) = pixels.size();
    let mut out = pixels.clone();
    let stride = w as usize;

    let mut hole = vec![false; stride * h as usize];
    for (row, y) in rect.rows().enumerate() {
        for (column, x) in rect.cols().enumerate() {
            if mask[row * rect.width() as usize + column] > 128 {
                hole[y as usize * stride + x as usize] = true;
            }
        }
    }

    let bytes = out.pixels_mut();
    let mut known: Vec<bool> = hole.iter().map(|h| !h).collect();
    let neighbours = |node: usize| {
        let (x, y) = ((node % stride) as i32, (node / stride) as i32);
        DIRS.into_iter().filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32)
                .then(|| ny as usize * stride + nx as usize)
        })
    };
    let mut queued = vec![false; hole.len()];
    let mut waiting: Vec<usize> = (0..hole.len())
        .filter(|&i| hole[i] && neighbours(i).any(|j| known[j]))
        .collect();
    for &node in &waiting {
        queued[node] = true;
    }
    while !waiting.is_empty() {
        let mut writes = Vec::new();

        for &node in &waiting {
            let mut total = [0u32; CHANNELS];
            let mut count = 0u32;
            for other in neighbours(node) {
                if !known[other] {
                    continue;
                }
                for c in 0..CHANNELS {
                    total[c] += bytes[other * CHANNELS + c] as u32;
                }
                count += 1;
            }

            if count == 0 {
                continue;
            }
            let mut colour = [0u8; CHANNELS];
            for c in 0..CHANNELS {
                colour[c] = (total[c] / count) as u8;
            }
            writes.push((node, colour));
        }

        for (node, colour) in writes {
            bytes[node * CHANNELS..node * CHANNELS + CHANNELS].copy_from_slice(&colour);
            known[node] = true;
        }
        let mut next = Vec::new();
        for node in waiting {
            for other in neighbours(node) {
                if hole[other] && !queued[other] {
                    queued[other] = true;
                    next.push(other);
                }
            }
        }
        waiting = next;
    }

    let inside: Vec<usize> = (0..hole.len()).filter(|i| hole[*i]).collect();
    for _ in 0..SMOOTHING {
        let mut writes = Vec::with_capacity(inside.len());
        for &node in &inside {
            let (x, y) = (node % stride, node / stride);
            let mut total = [0u32; CHANNELS];
            let mut count = 0u32;
            for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let other = ny as usize * stride + nx as usize;
                for c in 0..CHANNELS {
                    total[c] += bytes[other * CHANNELS + c] as u32;
                }
                count += 1;
            }
            if count == 0 {
                continue;
            }
            let mut colour = [0u8; CHANNELS];
            for c in 0..CHANNELS {
                colour[c] = (total[c] / count) as u8;
            }
            writes.push((node, colour));
        }
        for (node, colour) in writes {
            bytes[node * CHANNELS..node * CHANNELS + CHANNELS].copy_from_slice(&colour);
        }
    }

    out
}

const SMOOTHING: usize = 24;

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(width: u32, height: u32, thing: Rect, ink: [u8; 4], ground: [u8; 4]) -> Rgba8 {
        let mut image = Rgba8::new(width, height, ground);
        let pixels = image.pixels_mut();
        for y in thing.rows() {
            for x in thing.cols() {
                let i = (y as usize * width as usize + x as usize) * CHANNELS;
                pixels[i..i + CHANNELS].copy_from_slice(&ink);
            }
        }
        image
    }

    fn accuracy(mask: &[u8], want: &dyn Fn(usize) -> bool) -> f32 {
        let right = mask
            .iter()
            .enumerate()
            .filter(|(i, m)| (**m > 128) == want(*i))
            .count();
        right as f32 / mask.len() as f32
    }

    #[test]
    fn it_cuts_the_thing_out_of_the_ground() {
        let (w, h) = (80u32, 60u32);
        let thing = Rect::new(20, 15, 60, 45);
        let image = blob(w, h, thing, [200, 40, 40, 255], [40, 60, 200, 255]);

        let mut cutout = Cutout::new(&image, Rect::new(14, 9, 66, 51));
        cutout.run(3);

        let mask = cutout.mask();
        let inside = |i: usize| {
            let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
            x >= thing.x0 && x < thing.x1 && y >= thing.y0 && y < thing.y1
        };
        let hit = accuracy(&mask, &inside);
        assert!(
            hit > 0.99,
            "the cut got {:.1}% of the pixels right",
            hit * 100.0
        );
    }

    #[test]
    fn nothing_outside_the_box_is_ever_kept() {
        let (w, h) = (60u32, 40u32);
        let image = blob(
            w,
            h,
            Rect::new(5, 5, 55, 35),
            [220, 220, 60, 255],
            [20, 20, 20, 255],
        );

        let mut cutout = Cutout::new(&image, Rect::new(20, 10, 40, 30));
        cutout.run(2);

        let mask = cutout.mask();
        for y in 0..h as usize {
            for x in 0..w as usize {
                if !(20..40).contains(&x) || !(10..30).contains(&y) {
                    assert_eq!(
                        mask[y * w as usize + x],
                        0,
                        "kept {x},{y}, which is outside"
                    );
                }
            }
        }
    }

    #[test]
    fn a_box_with_no_room_round_it_still_finds_the_thing() {
        let (w, h) = (80u32, 60u32);
        let thing = Rect::new(16, 12, 64, 48);
        let image = blob(w, h, thing, [200, 40, 40, 255], [40, 60, 200, 255]);

        let mut cutout = Cutout::new(&image, Rect::new(0, 0, w, h));
        cutout.run(3);

        let mask = cutout.mask();
        let inside = |i: usize| {
            let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
            x >= thing.x0 && x < thing.x1 && y >= thing.y0 && y < thing.y1
        };
        let hit = accuracy(&mask, &inside);
        assert!(
            hit > 0.95,
            "the cut got {:.1}% of the pixels right",
            hit * 100.0
        );
    }

    #[test]
    fn a_brush_stroke_overrules_the_colours() {
        let (w, h) = (80u32, 60u32);
        let thing = Rect::new(20, 15, 60, 45);
        let image = blob(w, h, thing, [200, 40, 40, 255], [40, 60, 200, 255]);

        let mut cutout = Cutout::new(&image, Rect::new(14, 9, 66, 51));
        cutout.run(2);
        assert!(
            cutout.mask()[30 * w as usize + 30] > 128,
            "it starts inside the cut"
        );

        cutout.paint((30.0, 30.0), 6.0, false);
        cutout.run(1);
        assert_eq!(
            cutout.mask()[30 * w as usize + 30],
            0,
            "and the brush took it out"
        );
    }

    #[test]
    fn rubbing_out_a_leak_leaves_the_part_it_matches() {
        let (w, h) = (110u32, 60u32);
        let mut image = Rgba8::new(w, h, [30, 30, 40, 255]);
        {
            let pixels = image.pixels_mut();
            let mut paint = |x0: usize, x1: usize, colour: [u8; 4]| {
                for y in 20..45usize {
                    for x in x0..x1 {
                        let i = (y * w as usize + x) * CHANNELS;
                        pixels[i..i + CHANNELS].copy_from_slice(&colour);
                    }
                }
            };
            paint(20, 60, [235, 225, 70, 255]);
            paint(60, 76, [120, 85, 45, 255]);
            paint(78, 96, [120, 85, 45, 255]);
        }

        let mut cutout = Cutout::new(&image, Rect::new(8, 12, 102, 53));
        cutout.run(3);
        let tail = 32 * w as usize + 68;
        let leak = 32 * w as usize + 86;
        assert!(cutout.mask()[tail] > 128, "the tail starts inside the cut");
        assert!(
            cutout.mask()[leak] > 128,
            "and so does the leak, being the same brown"
        );

        cutout.paint((86.0, 32.0), 8.0, false);
        cutout.recut();
        assert_eq!(cutout.mask()[leak], 0, "the leak brushed out went");
        assert!(cutout.mask()[tail] > 128, "and the tail it matched stayed");
    }

    #[test]
    fn refining_the_edge_beats_blowing_the_small_mask_up() {
        let (w, h) = (1400u32, 1000u32);
        let mut image = Rgba8::new(w, h, [235, 235, 240, 255]);
        let inside = |x: u32, y: u32| {
            let (dx, dy) = ((x as f32 - 700.0).abs(), (y as f32 - 500.0).abs());
            dx / 460.0 + dy / 380.0 <= 1.0
        };
        {
            let pixels = image.pixels_mut();
            for y in 0..h {
                for x in 0..w {
                    if inside(x, y) {
                        let i = (y as usize * w as usize + x as usize) * CHANNELS;
                        pixels[i..i + CHANNELS].copy_from_slice(&[40, 90, 190, 255]);
                    }
                }
            }
        }

        let mut cutout = Cutout::new(&image, Rect::new(180, 80, 1220, 920));
        cutout.run(3);

        let wrong = |mask: &[u8]| {
            mask.iter()
                .enumerate()
                .filter(|(i, m)| {
                    let (x, y) = ((*i % w as usize) as u32, (*i / w as usize) as u32);
                    (**m > 128) != inside(x, y)
                })
                .count()
        };
        let blown_up = wrong(&cutout.mask());
        let refined = wrong(&cutout.refined_mask(&image));

        assert!(
            refined * 2 < blown_up,
            "the refined edge got {refined} pixels wrong against the blown up one's {blown_up}"
        );
    }

    #[test]
    fn a_big_picture_is_worked_out_small_and_answered_full_size() {
        let (w, h) = (2400u32, 1600u32);
        let image = blob(
            w,
            h,
            Rect::new(600, 400, 1800, 1200),
            [30, 160, 90, 255],
            [230, 230, 240, 255],
        );

        let cutout = Cutout::new(&image, Rect::new(500, 300, 1900, 1300));
        let (ww, wh) = cutout.size();
        assert!(ww.max(wh) <= WORKING, "the working copy is {ww} by {wh}");
        assert_eq!(cutout.mask().len(), (w * h) as usize);
    }

    #[test]
    fn a_small_subject_in_a_large_photo_keeps_its_source_detail() {
        let thing = Rect::new(2000, 900, 2020, 920);
        let image = blob(4000, 2000, thing, [200, 30, 20, 255], [30, 60, 180, 255]);
        let mut cutout = Cutout::new(&image, Rect::new(1990, 890, 2030, 930));
        assert_eq!(cutout.scale, 1.0);
        assert_eq!(cutout.size(), (56, 56));
        cutout.run(3);
        let mask = cutout.refined_mask(&image);
        assert_eq!(mask[910 * 4000 + 2010], 255);
        assert_eq!(mask[910 * 4000 + 1992], 0);
    }

    #[test]
    fn holes_and_detached_details_are_not_discarded() {
        let mut image = blob(
            100,
            80,
            Rect::new(20, 20, 70, 60),
            [200, 30, 20, 255],
            [30, 60, 180, 255],
        );
        for y in 30..50 {
            for x in 30..60 {
                image.pixels_mut()[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4]
                    .copy_from_slice(&[30, 60, 180, 255]);
            }
        }
        for y in 25..29 {
            for x in 78..82 {
                image.pixels_mut()[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4]
                    .copy_from_slice(&[200, 30, 20, 255]);
            }
        }
        let mut cutout = Cutout::new(&image, Rect::new(10, 10, 90, 70));
        cutout.run(3);
        let mask = cutout.refined_mask(&image);
        assert_eq!(mask[40 * 100 + 45], 0);
        assert_eq!(mask[25 * 100 + 25], 255);
        assert_eq!(mask[27 * 100 + 80], 255);
    }

    #[test]
    fn background_fill_reaches_the_centre_and_leaves_the_surroundings_alone() {
        let image = blob(
            200,
            140,
            Rect::new(20, 20, 180, 120),
            [200, 30, 20, 0],
            [30, 60, 180, 255],
        );
        let filled = fill_behind(&image, &vec![255; 160 * 100], Rect::new(20, 20, 180, 120));
        assert_eq!(
            &filled.as_bytes()[(70 * 200 + 100) * 4..(70 * 200 + 100) * 4 + 4],
            &[30, 60, 180, 255]
        );
        assert_eq!(&filled.as_bytes()[..80], &image.as_bytes()[..80]);
        assert_eq!(
            fill_behind(&image, &vec![0; 200 * 140], Rect::new(0, 0, 200, 140)),
            image
        );
    }
}
