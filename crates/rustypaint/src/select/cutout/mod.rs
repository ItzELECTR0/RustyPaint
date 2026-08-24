pub mod gmm;
pub mod maxflow;

use crate::doc::{Rect, Rgba8, image::CHANNELS};
use gmm::Gmm;
use maxflow::{DIRS, Grid};

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

pub struct Cutout {
    width: usize,
    height: usize,
    colours: Vec<[f32; 3]>,
    known: Vec<Known>,
    label: Vec<bool>,
    links: Vec<f32>,
    models: Option<(Gmm, Gmm)>,
    scale: f32,
    source: (u32, u32),
}

impl Cutout {
    pub fn new(pixels: &Rgba8, rect: Rect) -> Self {
        let (source_w, source_h) = pixels.size();
        let longest = source_w.max(source_h) as usize;
        let scale = (WORKING as f32 / longest as f32).min(1.0);
        let width = ((source_w as f32 * scale).round() as usize).max(1);
        let height = ((source_h as f32 * scale).round() as usize).max(1);

        let mut colours = Vec::with_capacity(width * height);
        let bytes = pixels.as_bytes();
        for y in 0..height {
            for x in 0..width {
                let sx = ((x as f32 + 0.5) / scale) as u32;
                let sy = ((y as f32 + 0.5) / scale) as u32;
                let i = (sy.min(source_h - 1) as usize * source_w as usize
                    + sx.min(source_w - 1) as usize)
                    * CHANNELS;
                colours.push([bytes[i] as f32, bytes[i + 1] as f32, bytes[i + 2] as f32]);
            }
        }

        let mut cutout = Self {
            width,
            height,
            colours,
            known: vec![Known::Background; width * height],
            label: vec![false; width * height],
            links: Vec::new(),
            models: None,
            scale,
            source: (source_w, source_h),
        };

        let box_in_working = |v: u32, s: f32| (v as f32 * s).round() as usize;
        let x0 = box_in_working(rect.x0, scale);
        let y0 = box_in_working(rect.y0, scale);
        let x1 = box_in_working(rect.x1, scale).min(width);
        let y1 = box_in_working(rect.y1, scale).min(height);
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

        cutout.links = cutout.build_links();
        cutout
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn paint(&mut self, at: (f32, f32), radius: f32, foreground: bool) {
        let radius = radius * self.scale;
        let centre = (at.0 * self.scale, at.1 * self.scale);
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
        self.tidy();
        self.models = models;
    }

    pub fn recut(&mut self) {
        let Some(models) = self.models.take() else {
            self.run(1);
            return;
        };
        self.cut(&models.0, &models.1);
        self.tidy();

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
                self.tidy();
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

    pub fn tidy(&mut self) {
        let pieces = self.pieces(true);
        let biggest = pieces.iter().map(|p| p.len()).max().unwrap_or(0);
        if biggest == 0 {
            return;
        }
        for piece in &pieces {
            if piece.len() * 4 < biggest {
                for node in piece {
                    if self.known[*node] != Known::Foreground {
                        self.label[*node] = false;
                    }
                }
            }
        }

        let holes = self.pieces(false);
        for hole in &holes {
            let touches_edge = hole.iter().any(|node| {
                let (x, y) = (node % self.width, node / self.width);
                x == 0 || y == 0 || x + 1 == self.width || y + 1 == self.height
            });
            if !touches_edge && hole.len() * 200 < biggest {
                for node in hole {
                    if self.known[*node] != Known::Background {
                        self.label[*node] = true;
                    }
                }
            }
        }
    }

    fn pieces(&self, wanted: bool) -> Vec<Vec<usize>> {
        let mut seen = vec![false; self.label.len()];
        let mut out = Vec::new();
        let mut stack = Vec::new();

        for start in 0..self.label.len() {
            if seen[start] || self.label[start] != wanted {
                continue;
            }
            let mut piece = Vec::new();
            stack.push(start);
            seen[start] = true;
            while let Some(node) = stack.pop() {
                piece.push(node);
                for dir in [2, 3, 6, 7] {
                    let Some(other) = self.neighbour(node, dir) else {
                        continue;
                    };
                    if !seen[other] && self.label[other] == wanted {
                        seen[other] = true;
                        stack.push(other);
                    }
                }
            }
            out.push(piece);
        }
        out
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
        for y in 0..h {
            let sy = ((y as f32 * self.scale) as usize).min(self.height - 1);
            for x in 0..w {
                let sx = ((x as f32 * self.scale) as usize).min(self.width - 1);
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
        let mut mask = self.mask();
        let (w, h) = (self.source.0 as usize, self.source.1 as usize);
        if pixels.size() != self.source {
            return mask;
        }

        let reach = ((1.0 / self.scale).ceil() as usize + 1).clamp(2, 6);
        let band = self.band(&mask, w, h, reach);
        let mut in_band = vec![false; w * h];
        for &node in &band {
            in_band[node] = true;
        }

        let bytes = pixels.as_bytes();
        let colour_at = |i: usize| {
            [
                bytes[i * CHANNELS] as f32,
                bytes[i * CHANNELS + 1] as f32,
                bytes[i * CHANNELS + 2] as f32,
            ]
        };

        let window = (reach * 2 + 2) as i32;
        let mut decided = mask.clone();
        for &node in &band {
            let (x, y) = ((node % w) as i32, (node / w) as i32);
            let here = colour_at(node);
            let (mut best_fg, mut best_bg) = (f32::MAX, f32::MAX);

            for dy in -window..=window {
                for dx in -window..=window {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let other = ny as usize * w + nx as usize;
                    if in_band[other] {
                        continue;
                    }
                    let d = distance(here, colour_at(other));
                    if mask[other] > 128 {
                        best_fg = best_fg.min(d);
                    } else {
                        best_bg = best_bg.min(d);
                    }
                }
            }

            if best_fg == f32::MAX || best_bg == f32::MAX {
                continue;
            }
            decided[node] = if best_fg <= best_bg { 255 } else { 0 };
        }

        for &node in &band {
            let (x, y) = ((node % w) as i32, (node / w) as i32);
            let mut fg = 0;
            let mut total = 0;
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    total += 1;
                    if decided[ny as usize * w + nx as usize] > 128 {
                        fg += 1;
                    }
                }
            }
            mask[node] = if fg * 2 > total { 255 } else { 0 };
        }
        mask
    }

    fn band(&self, mask: &[u8], w: usize, h: usize, reach: usize) -> Vec<usize> {
        let mut edge = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let node = y * w + x;
                let mine = mask[node] > 128;
                let boundary = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        nx >= 0
                            && ny >= 0
                            && nx < w as i32
                            && ny < h as i32
                            && (mask[ny as usize * w + nx as usize] > 128) != mine
                    });
                if boundary {
                    edge.push((x, y));
                }
            }
        }

        let mut band = Vec::new();
        let reach = reach as i32;
        for (x, y) in edge {
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 {
                        band.push(ny as usize * w + nx as usize);
                    }
                }
            }
        }
        band.sort_unstable();
        band.dedup();
        band
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
    let mut waiting: Vec<usize> = (0..hole.len()).filter(|i| hole[*i]).collect();
    let mut known: Vec<bool> = hole.iter().map(|h| !h).collect();
    while !waiting.is_empty() {
        let mut filled_any = false;
        let mut still = Vec::with_capacity(waiting.len());
        let mut writes = Vec::new();

        for node in waiting {
            let (x, y) = (node % stride, node / stride);
            let mut total = [0u32; CHANNELS];
            let mut count = 0u32;
            for (dx, dy) in [
                (1i32, 0i32),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
            ] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let other = ny as usize * stride + nx as usize;
                if !known[other] {
                    continue;
                }
                for c in 0..CHANNELS {
                    total[c] += bytes[other * CHANNELS + c] as u32;
                }
                count += 1;
            }

            if count == 0 {
                still.push(node);
                continue;
            }
            let mut colour = [0u8; CHANNELS];
            for c in 0..CHANNELS {
                colour[c] = (total[c] / count) as u8;
            }
            writes.push((node, colour));
            filled_any = true;
        }

        for (node, colour) in writes {
            bytes[node * CHANNELS..node * CHANNELS + CHANNELS].copy_from_slice(&colour);
            known[node] = true;
        }
        if !filled_any {
            break;
        }
        waiting = still;
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
}
