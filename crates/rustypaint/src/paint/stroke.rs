use super::brush::{Brush, Build, Mode, Tip, to_level};
use crate::doc::rect::{Bounds, Rect};
use crate::doc::{Document, image::CHANNELS};

const DOTS_PER_PUFF: usize = 12;
const SETTLE_STEPS: usize = 200;

// Coverage and undo are kept in tiles this many pixels a side, made on first touch.
const TILE: u32 = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mirror {
    pub horizontal: bool,
    pub vertical: bool,
}

type Point = (f32, f32);
type Cell = (i64, i64);

// Part of a row whose coverage changed and has not reached the canvas yet.
#[derive(Debug, Clone, Copy)]
struct Run {
    y: u32,
    x0: u32,
    x1: u32,
}

enum Cover {
    Strongest(Tiles<u8>),
    // Optical density, so passes of any length add up the same way.
    Density(Tiles<f32>),
}

impl Cover {
    fn alpha(&self, x: u32, y: u32) -> f32 {
        match self {
            Cover::Strongest(cover) => cover.get(x, y) as f32 / 255.0,
            Cover::Density(density) => 1.0 - (-density.get(x, y)).exp(),
        }
    }
}

pub struct Stroke {
    brush: Brush,
    tip: Tip,
    mirror: Mirror,
    size: (u32, u32),
    backup: Backup,
    cover: Cover,
    pending: Vec<Run>,
    dirty: [Bounds; 4],
    touched: Bounds,
    last: Option<Point>,
    swept: Option<(Point, Point)>,
    puffs: u64,
    trail: (Option<Cell>, Option<Cell>),
    steady: Point,
    aim: Option<Point>,
}

impl Stroke {
    pub fn begin_with_mirror(brush: Brush, doc: &Document, x: f32, y: f32, mirror: Mirror) -> Self {
        let size = doc.size();
        let cover = match brush.profile().build {
            Build::Max => Cover::Strongest(Tiles::new(size)),
            Build::Accumulate => Cover::Density(Tiles::new(size)),
        };
        let mut stroke = Self {
            brush,
            tip: brush.tip(),
            mirror,
            size,
            backup: Backup::new(size),
            cover,
            pending: Vec::new(),
            dirty: Default::default(),
            touched: Bounds::default(),
            last: None,
            swept: None,
            puffs: 0,
            trail: (None, None),
            steady: (x, y),
            aim: None,
        };
        stroke.reach(x, y);
        stroke
    }

    pub fn extend(&mut self, x: f32, y: f32) {
        if self.brush.stabilizer() <= 0.0 {
            self.reach(x, y);
            return;
        }
        self.aim = Some((x, y));
        self.steady = eased(self.steady, (x, y), self.brush.follow());
        self.reach(self.steady.0, self.steady.1);
    }

    // Nothing more is coming, so the brush walks the rest of the way to where the hand left off.
    pub fn settle(&mut self) {
        let Some(aim) = self.aim.take() else {
            return;
        };
        let follow = self.brush.follow();
        for _ in 0..SETTLE_STEPS {
            if (aim.0 - self.steady.0).hypot(aim.1 - self.steady.1) <= 0.5 {
                break;
            }
            self.steady = eased(self.steady, aim, follow);
            self.reach(self.steady.0, self.steady.1);
        }
        self.steady = aim;
        self.reach(aim.0, aim.1);
    }

    fn reach(&mut self, x: f32, y: f32) {
        let to = self.placed(x, y);
        let Some(from) = self.last else {
            self.dab(to);
            self.last = Some(to);
            return;
        };
        if from == to {
            return;
        }
        if self.brush.tool.snaps_to_pixels() {
            for cell in cells_between(cell_of(from), cell_of(to)) {
                self.press(cell);
            }
        } else {
            match self.cover {
                Cover::Strongest(_) => self.sweep(from, to),
                Cover::Density(_) => {
                    // A stamp's worth per spacing travelled, each laid at the end of its piece so
                    // the last one lands under the pointer.
                    let length = (to.0 - from.0).hypot(to.1 - from.1);
                    let spacing = self.brush.step();
                    let pieces = (length / spacing).ceil().max(1.0);
                    let weight = length / pieces / spacing;
                    for piece in 1..=pieces as usize {
                        let t = piece as f32 / pieces;
                        let at = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
                        self.deposit(at, weight);
                    }
                }
            }
        }
        self.last = Some(to);
    }

    fn placed(&self, x: f32, y: f32) -> Point {
        if self.brush.tool.snaps_to_pixels() {
            (x.floor() + 0.5, y.floor() + 0.5)
        } else {
            (x, y)
        }
    }

    fn dab(&mut self, at: Point) {
        if self.brush.tool.snaps_to_pixels() {
            self.press(cell_of(at));
            return;
        }
        match self.cover {
            Cover::Strongest(_) => self.sweep(at, at),
            Cover::Density(_) => self.deposit(at, 1.0),
        }
    }

    // A one pixel pen goes through pixel-perfect; wider pens stamp the whole tip.
    fn press(&mut self, cell: Cell) {
        if self.brush.stamp_radius() <= 0.5 {
            self.step_onto(cell);
        } else {
            let centre = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
            self.sweep(centre, centre);
        }
    }

    fn step_onto(&mut self, cell: Cell) {
        if !self.brush.drops_corners() {
            self.mark(cell, true);
            return;
        }
        if self.trail.1 == Some(cell) {
            return;
        }
        self.mark(cell, true);

        // A corner is only known once the pixel after it arrives, so it is laid down and taken
        // back rather than held while the pointer waits somewhere else.
        if let (Some(before), Some(middle)) = self.trail
            && corners(before, middle, cell)
        {
            self.mark(middle, false);
            self.trail.1 = Some(cell);
            return;
        }
        self.trail = (self.trail.1, Some(cell));
    }

    fn mark(&mut self, cell: Cell, on: bool) {
        let centre = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
        let (copies, count) = self.copies(centre, centre);
        let Self {
            tip,
            size,
            cover,
            pending,
            dirty,
            touched,
            ..
        } = self;
        let Cover::Strongest(cover) = cover else {
            return;
        };
        for &(slot, at, _) in &copies[..count] {
            let (x, y) = (at.0.floor(), at.1.floor());
            if x < 0.0 || y < 0.0 || x >= size.0 as f32 || y >= size.1 as f32 {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let here = cover.at(x, y);
            let value = if on {
                (*here).max(tip.coverage(0.0, x as i64, y as i64))
            } else {
                0
            };
            if value != *here {
                *here = value;
                let run = Run {
                    y,
                    x0: x,
                    x1: x + 1,
                };
                pending.push(run);
                dirty[slot].add(run.rect());
                if on {
                    touched.add(run.rect());
                }
            }
        }
    }

    fn sweep(&mut self, a: Point, b: Point) {
        let before = self.swept.replace((a, b));
        let (copies, count) = self.copies(a, b);
        for &(slot, ca, cb) in &copies[..count] {
            let skip = before.map(|(pa, pb)| (self.flipped(pa, slot), self.flipped(pb, slot)));
            self.sweep_copy(slot, ca, cb, skip);
        }
    }

    // The previous sweep's solid core is already as strong as the brush gets, so rows skip it and a
    // short move only visits the new crescent.
    fn sweep_copy(&mut self, slot: usize, a: Point, b: Point, skip: Option<(Point, Point)>) {
        let Self {
            tip,
            size,
            cover,
            pending,
            dirty,
            touched,
            ..
        } = self;
        let Cover::Strongest(cover) = cover else {
            return;
        };
        let Some((rows, cols)) = span(*size, a, b, tip.extent() + 1.0) else {
            return;
        };
        let sweep = tip.sweep(a, b);
        let round = tip.is_round();
        let core = tip.solid() - 0.01;
        let skip = skip.filter(|_| round && core > 0.0);

        for y in rows {
            let py = y as f32 + 0.5;
            let (mut x0, mut x1) = (cols.start, cols.end);
            if round {
                let Some(reached) = crossing(a, b, tip.reach(), py) else {
                    continue;
                };
                (x0, x1) = widened(reached, x0..x1);
            }
            let (ex0, ex1) = skip
                .and_then(|(sa, sb)| crossing(sa, sb, core, py))
                .map_or((x1, x1), |inside| narrowed(inside, x0..x1));

            let mut run: Option<Run> = None;
            for x in (x0..ex0.max(x0)).chain(ex1.max(x0)..x1) {
                let at = cover.at(x, y);
                if *at == u8::MAX {
                    continue;
                }
                let c = tip.coverage(sweep.distance(x, y), x as i64, y as i64);
                if c > *at {
                    *at = c;
                    extend_run(&mut run, y, x, pending, &mut dirty[slot], touched);
                }
            }
            if let Some(run) = run {
                finish_run(run, pending, &mut dirty[slot], touched);
            }
        }
    }

    fn deposit(&mut self, centre: Point, weight: f32) {
        let (copies, count) = self.copies(centre, centre);
        for &(slot, at, _) in &copies[..count] {
            self.deposit_copy(slot, at, weight);
        }
    }

    fn deposit_copy(&mut self, slot: usize, centre: Point, weight: f32) {
        let Self {
            tip,
            size,
            cover,
            pending,
            dirty,
            touched,
            ..
        } = self;
        let Cover::Density(density) = cover else {
            return;
        };
        let Some((rows, cols)) = span(*size, centre, centre, tip.extent() + 1.0) else {
            return;
        };
        let round = tip.is_round();
        let sweep = tip.sweep(centre, centre);
        for y in rows {
            let py = y as f32 + 0.5;
            let (mut x0, mut x1) = (cols.start, cols.end);
            if round {
                let Some(reached) = crossing(centre, centre, tip.reach(), py) else {
                    continue;
                };
                (x0, x1) = widened(reached, x0..x1);
            }
            if x0 >= x1 {
                continue;
            }
            let rise = (py - centre.1) * (py - centre.1);
            let mut x = x0;
            while x < x1 {
                let end = ((x / TILE + 1) * TILE).min(x1);
                let (k, start) = density.locate(x, y);
                let tile = &mut density.tile(k)[start..start + (end - x) as usize];
                for (here, x) in tile.iter_mut().zip(x..end) {
                    let d = if round {
                        let run = x as f32 + 0.5 - centre.0;
                        (run * run + rise).sqrt()
                    } else {
                        sweep.distance(x, y)
                    };
                    let strength = tip.strength(d, x as i64, y as i64);
                    if strength > 0.0 {
                        *here += weight * absorbed(strength);
                    }
                }
                x = end;
            }
            finish_run(Run { y, x0, x1 }, pending, &mut dirty[slot], touched);
        }
    }

    // Each copy keeps its own slot so copies at opposite edges upload separately.
    fn copies(&self, a: Point, b: Point) -> ([(usize, Point, Point); 4], usize) {
        let mut out = [(0, a, b); 4];
        let mut count = 0;
        for slot in 0..4 {
            let (flip_x, flip_y) = (slot & 1 != 0, slot & 2 != 0);
            if (flip_x && !self.mirror.horizontal) || (flip_y && !self.mirror.vertical) {
                continue;
            }
            let copy = (slot, self.flipped(a, slot), self.flipped(b, slot));
            if out[..count]
                .iter()
                .any(|other| (other.1, other.2) == (copy.1, copy.2))
            {
                continue;
            }
            out[count] = copy;
            count += 1;
        }
        (out, count)
    }

    fn flipped(&self, (x, y): Point, slot: usize) -> Point {
        (
            if slot & 1 != 0 {
                self.size.0 as f32 - x
            } else {
                x
            },
            if slot & 2 != 0 {
                self.size.1 as f32 - y
            } else {
                y
            },
        )
    }

    pub fn puff(&mut self, x: f32, y: f32) {
        use std::f32::consts::TAU;

        let radius = self.brush.radius();
        for _ in 0..DOTS_PER_PUFF {
            self.puffs = self.puffs.wrapping_add(1);
            let angle = super::brush::hash01(self.puffs.wrapping_mul(2)) * TAU;
            let away = super::brush::hash01(self.puffs.wrapping_mul(2).wrapping_add(1)).sqrt();
            let r = away * radius;
            let at = self.placed(x + angle.cos() * r, y + angle.sin() * r);
            self.dab(at);
        }
        self.last = Some((x, y));
        self.swept = None;
    }

    // Returns one changed region per mirrored copy.
    pub fn flush(&mut self, doc: &mut Document) -> Vec<Rect> {
        if self.pending.is_empty() || doc.size() != self.size {
            return Vec::new();
        }
        let width = self.size.0 as usize;
        let opacity = self.brush.opacity().clamp(0.0, 1.0);
        let erase = self.brush.tool.mode() == Mode::Erase;
        let colour = self.brush.colour;
        let pixels = doc.edit().pixels_mut();

        // Overlapping passes queue the same pixels more than once.
        self.pending.sort_unstable_by_key(|run| (run.y, run.x0));
        let mut merged: Vec<Run> = Vec::with_capacity(self.pending.len());
        for run in self.pending.drain(..) {
            match merged.last_mut() {
                Some(last) if last.y == run.y && run.x0 <= last.x1 => last.x1 = last.x1.max(run.x1),
                _ => merged.push(run),
            }
        }

        for run in merged {
            self.backup.keep(pixels, run);
            let row = run.y as usize * width;
            for x in run.x0..run.x1 {
                let index = row + x as usize;
                let a = self.cover.alpha(x, run.y) * opacity;
                let i = index * CHANNELS;
                let under = self.backup.under(x, run.y);
                let out = if erase {
                    let mut px = under;
                    px[3] = (under[3] as f32 * (1.0 - a) + 0.5) as u8;
                    px
                } else {
                    over(under, colour, a)
                };
                pixels[i..i + CHANNELS].copy_from_slice(&out);
            }
        }
        self.dirty.iter_mut().filter_map(Bounds::take).collect()
    }

    pub fn touched(&self) -> Option<Rect> {
        self.touched.get()
    }

    pub fn commit(&self, doc: &mut Document) {
        let Some(touched) = self.touched() else {
            return;
        };
        doc.commit_parts(self.label(), self.backup.parts(touched));
    }

    pub fn label(&self) -> &'static str {
        self.brush.tool.name()
    }
}

impl Run {
    fn rect(self) -> Rect {
        Rect::new(self.x0, self.y, self.x1, self.y + 1)
    }
}

fn extend_run(
    run: &mut Option<Run>,
    y: u32,
    x: u32,
    pending: &mut Vec<Run>,
    dirty: &mut Bounds,
    touched: &mut Bounds,
) {
    match run {
        Some(open) if open.x1 == x => open.x1 = x + 1,
        _ => {
            if let Some(done) = run.replace(Run {
                y,
                x0: x,
                x1: x + 1,
            }) {
                finish_run(done, pending, dirty, touched);
            }
        }
    }
}

fn finish_run(run: Run, pending: &mut Vec<Run>, dirty: &mut Bounds, touched: &mut Bounds) {
    pending.push(run);
    dirty.add(run.rect());
    touched.add(run.rect());
}

// A canvas-sized grid that only allocates the tiles something writes to.
struct Tiles<T> {
    tiles: Vec<Option<Box<[T]>>>,
    across: usize,
    size: (u32, u32),
}

impl<T: Copy + Default> Tiles<T> {
    fn new(size: (u32, u32)) -> Self {
        let across = size.0.div_ceil(TILE) as usize;
        let down = size.1.div_ceil(TILE) as usize;
        Self {
            tiles: (0..across * down).map(|_| None).collect(),
            across,
            size,
        }
    }

    fn locate(&self, x: u32, y: u32) -> (usize, usize) {
        let k = (y / TILE) as usize * self.across + (x / TILE) as usize;
        (k, ((y % TILE) * TILE + x % TILE) as usize)
    }

    fn get(&self, x: u32, y: u32) -> T {
        let (k, i) = self.locate(x, y);
        self.tiles[k]
            .as_ref()
            .map_or_else(T::default, |tile| tile[i])
    }

    fn at(&mut self, x: u32, y: u32) -> &mut T {
        let (k, i) = self.locate(x, y);
        &mut self.tile(k)[i]
    }

    fn tile(&mut self, k: usize) -> &mut [T] {
        self.tiles[k].get_or_insert_with(|| vec![T::default(); (TILE * TILE) as usize].into())
    }

    fn bounds(&self, k: usize) -> Rect {
        let (tx, ty) = ((k % self.across) as u32, (k / self.across) as u32);
        Rect::new(tx * TILE, ty * TILE, (tx + 1) * TILE, (ty + 1) * TILE)
            .clamped(self.size.0, self.size.1)
    }
}

// What was under the stroke, saved a tile at a time as the stroke first reaches it.
struct Backup(Tiles<[u8; CHANNELS]>);

impl Backup {
    fn new(size: (u32, u32)) -> Self {
        Self(Tiles::new(size))
    }

    fn keep(&mut self, canvas: &[u8], run: Run) {
        let tiles = &mut self.0;
        let stride = tiles.size.0 as usize * CHANNELS;
        let ty = (run.y / TILE) as usize;
        for tx in (run.x0 / TILE) as usize..=((run.x1 - 1) / TILE) as usize {
            let k = ty * tiles.across + tx;
            if tiles.tiles[k].is_some() {
                continue;
            }
            let bounds = tiles.bounds(k);
            let (x0, x1) = (bounds.x0 as usize * CHANNELS, bounds.x1 as usize * CHANNELS);
            let tile = tiles.tile(k);
            for (r, y) in bounds.rows().enumerate() {
                let row = y as usize * stride;
                let to = &mut tile[r * TILE as usize..][..bounds.width() as usize];
                to.as_flattened_mut()
                    .copy_from_slice(&canvas[row + x0..row + x1]);
            }
        }
    }

    fn under(&self, x: u32, y: u32) -> [u8; CHANNELS] {
        self.0.get(x, y)
    }

    fn parts(&self, touched: Rect) -> Vec<(Rect, Vec<u8>)> {
        let tiles = &self.0;
        let mut out = Vec::new();
        for (k, tile) in tiles.tiles.iter().enumerate() {
            let Some(tile) = tile else {
                continue;
            };
            let bounds = tiles.bounds(k);
            let rect = bounds.intersection(touched);
            if rect.is_empty() {
                continue;
            }
            let mut before = Vec::with_capacity(rect.area() * CHANNELS);
            for y in rect.rows() {
                let row = (y - bounds.y0) as usize * TILE as usize;
                let from = row + (rect.x0 - bounds.x0) as usize;
                before.extend_from_slice(tile[from..from + rect.width() as usize].as_flattened());
            }
            out.push((rect, before));
        }
        out
    }
}

fn span(
    size: (u32, u32),
    a: Point,
    b: Point,
    extent: f32,
) -> Option<(std::ops::Range<u32>, std::ops::Range<u32>)> {
    let clamp = |v: f32, limit: u32| (v.max(0.0) as u32).min(limit);
    let x0 = clamp((a.0.min(b.0) - extent).floor(), size.0);
    let x1 = clamp((a.0.max(b.0) + extent).ceil(), size.0);
    let y0 = clamp((a.1.min(b.1) - extent).floor(), size.1);
    let y1 = clamp((a.1.max(b.1) + extent).ceil(), size.1);
    (x0 < x1 && y0 < y1).then_some((y0..y1, x0..x1))
}

// The x range where row `y` lies within `radius` of segment `a`-`b`: the union of where it crosses
// the two end discs and the band between them, which is one stretch because the shape is convex.
fn crossing(a: Point, b: Point, radius: f32, y: f32) -> Option<(f32, f32)> {
    if radius < 0.0 {
        return None;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for end in [a, b] {
        let across = radius * radius - (y - end.1) * (y - end.1);
        if across >= 0.0 {
            let half = across.sqrt();
            lo = lo.min(end.0 - half);
            hi = hi.max(end.0 + half);
        }
    }

    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    if length > 0.0 {
        // Distance from the line and position along it are both linear in x.
        let mut band = (f32::NEG_INFINITY, f32::INFINITY);
        let side = (y - a.1) * dx / length;
        let along = (y - a.1) * dy / length;
        if within(&mut band, -dy / length, side, -radius, radius, a.0)
            && within(&mut band, dx / length, along, 0.0, length, a.0)
            && band.0 <= band.1
        {
            lo = lo.min(band.0);
            hi = hi.max(band.1);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

// Narrows `range` to where `slope * (x - origin) + at` is within `min..=max`.
fn within(range: &mut (f32, f32), slope: f32, at: f32, min: f32, max: f32, origin: f32) -> bool {
    if slope.abs() < 1e-6 {
        return (min..=max).contains(&at);
    }
    let (p, q) = ((min - at) / slope + origin, (max - at) / slope + origin);
    range.0 = range.0.max(p.min(q));
    range.1 = range.1.min(p.max(q));
    true
}

fn widened((lo, hi): (f32, f32), cols: std::ops::Range<u32>) -> (u32, u32) {
    let x0 = ((lo - 1.5).floor().max(0.0) as u32).max(cols.start);
    let x1 = ((hi + 1.5).ceil().max(0.0) as u32).min(cols.end);
    (x0, x1.max(x0))
}

fn narrowed((lo, hi): (f32, f32), cols: std::ops::Range<u32>) -> (u32, u32) {
    let x0 = ((lo - 0.5).ceil().max(0.0) as u32).clamp(cols.start, cols.end);
    let x1 = (((hi - 0.5).floor() + 1.0).max(0.0) as u32).clamp(cols.start, cols.end);
    if x1 > x0 {
        (x0, x1)
    } else {
        (cols.end, cols.end)
    }
}

// -ln(1 - strength), the optical density one full pass adds.
fn absorbed(strength: f32) -> f32 {
    if strength < 0.25 {
        let s2 = strength * strength;
        strength + s2 / 2.0 + s2 * strength / 3.0 + s2 * s2 / 4.0
    } else {
        -(1.0 - strength).max(1e-6).ln()
    }
}

fn cell_of(at: Point) -> Cell {
    (at.0.floor() as i64, at.1.floor() as i64)
}

// Bresenham from `from` to `to`, excluding `from`.
fn cells_between(from: Cell, to: Cell) -> impl Iterator<Item = Cell> {
    let (dx, dy) = ((to.0 - from.0).abs(), -(to.1 - from.1).abs());
    let (sx, sy) = ((to.0 - from.0).signum(), (to.1 - from.1).signum());
    let mut error = dx + dy;
    let mut at = from;
    std::iter::from_fn(move || {
        if at == to {
            return None;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            at.0 += sx;
        }
        if twice <= dx {
            error += dx;
            at.1 += sy;
        }
        Some(at)
    })
}

// A pixel between two that already touch diagonally is the doubled corner of a thin line.
fn corners(before: Cell, middle: Cell, after: Cell) -> bool {
    let touches = |a: Cell, b: Cell| (a.0 - b.0).abs() + (a.1 - b.1).abs() == 1;
    touches(before, middle)
        && touches(after, middle)
        && (before.0 - after.0).abs() == 1
        && (before.1 - after.1).abs() == 1
}

fn eased(from: Point, to: Point, follow: f32) -> Point {
    (
        from.0 + (to.0 - from.0) * follow,
        from.1 + (to.1 - from.1) * follow,
    )
}

fn over(under: [u8; 4], src: [u8; 4], alpha: f32) -> [u8; 4] {
    let sa = alpha * (src[3] as f32 / 255.0);
    if sa <= 0.0 {
        return under;
    }
    let ua = under[3] as f32 / 255.0;
    let out_a = sa + ua * (1.0 - sa);
    if out_a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mix = |s: u8, u: u8| {
        let v = (s as f32 * sa + u as f32 * ua * (1.0 - sa)) / out_a;
        (v + 0.5) as u8
    };
    [
        mix(src[0], under[0]),
        mix(src[1], under[1]),
        mix(src[2], under[2]),
        to_level(out_a),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Rgba8;
    use crate::paint::brush::Tool;

    fn doc(transparent: bool) -> Document {
        Document::blank_sized(16, 16, transparent)
    }

    fn at(doc: &Document, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * doc.size().0 as usize + x as usize) * CHANNELS;
        doc.pixels().as_bytes()[i..i + 4].try_into().unwrap()
    }

    fn red() -> Brush {
        let mut b = Brush {
            tool: Tool::PixelPen,
            colour: [255, 0, 0, 255],
            ..Default::default()
        };
        b.set_thickness(1.0);
        b
    }

    #[test]
    fn a_stamp_lands_where_it_was_put() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 8.5, 4.5, Mirror::default());
        s.flush(&mut d);
        assert_eq!(at(&d, 8, 4), [255, 0, 0, 255]);
        assert_eq!(at(&d, 0, 0), [0, 0, 0, 0], "elsewhere is still empty");
    }

    #[test]
    fn mirror_axes_repeat_a_stamp_without_changing_its_undo_shape() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(
            red(),
            &d,
            3.5,
            4.5,
            Mirror {
                horizontal: true,
                vertical: true,
            },
        );
        s.flush(&mut d);

        for (x, y) in [(3, 4), (12, 4), (3, 11), (12, 11)] {
            assert_eq!(at(&d, x, y), [255, 0, 0, 255], "missing mirrored pixel");
        }
        assert_eq!(at(&d, 4, 4), [0, 0, 0, 0]);
        assert_eq!(s.touched().unwrap().width(), 10);
        assert_eq!(s.touched().unwrap().height(), 8);
    }

    #[test]
    fn a_dragged_stroke_leaves_no_gaps() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 1.5, 8.5, Mirror::default());
        s.extend(14.5, 8.5);
        s.flush(&mut d);
        for x in 1..=14 {
            assert_eq!(at(&d, x, 8), [255, 0, 0, 255], "gap at x={x}");
        }
    }

    #[test]
    fn a_staircase_keeps_one_pixel_at_every_corner() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 2.5, 2.5, Mirror::default());
        for step in 1..=5 {
            s.extend(2.5 + step as f32, 1.5 + step as f32);
            s.extend(2.5 + step as f32, 2.5 + step as f32);
        }
        s.flush(&mut d);

        let painted = |x: u32, y: u32| at(&d, x, y) == [255, 0, 0, 255];
        for step in 0..=5 {
            assert!(
                painted(2 + step, 2 + step),
                "the diagonal itself is missing"
            );
        }
        for step in 1..=5 {
            assert!(
                !painted(2 + step, 1 + step),
                "the corner beside the diagonal was kept"
            );
        }
    }

    #[test]
    fn a_corner_stays_when_pixel_perfect_is_turned_off() {
        let mut blunt = red();
        blunt.set_pixel_perfect(false);

        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(blunt, &d, 2.5, 2.5, Mirror::default());
        s.extend(3.5, 2.5);
        s.extend(3.5, 3.5);
        s.flush(&mut d);

        assert_eq!(at(&d, 3, 2), [255, 0, 0, 255], "the corner should be kept");
    }

    #[test]
    fn a_dropped_corner_goes_back_to_what_was_under_it() {
        let mut d = doc(false);
        let mut under = Brush {
            tool: Tool::Marker,
            colour: [0, 0, 255, 255],
            ..red()
        };
        under.set_thickness(1.0);
        let mut first = Stroke::begin_with_mirror(under, &d, 3.5, 2.5, Mirror::default());
        first.flush(&mut d);
        let blue = at(&d, 3, 2);

        let mut s = Stroke::begin_with_mirror(red(), &d, 2.5, 2.5, Mirror::default());
        s.extend(3.5, 2.5);
        s.extend(3.5, 3.5);
        s.flush(&mut d);

        assert_eq!(at(&d, 3, 2), blue, "the corner was not put back");
    }

    #[test]
    fn the_pixel_pen_lays_down_a_square_when_asked_to() {
        let mut d = doc(false);
        let mut square = red();
        square.set_thickness(5.0);
        square.set_square_tip(true);
        let mut s = Stroke::begin_with_mirror(square, &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);

        for (x, y) in [(6, 6), (10, 10), (6, 10), (10, 6)] {
            assert_eq!(at(&d, x, y), [255, 0, 0, 255], "the corner {x},{y} is bare");
        }
        assert_eq!(at(&d, 5, 8), [0, 0, 0, 0], "and it stops at its own width");
    }

    #[test]
    fn the_pixel_pen_is_round_until_then() {
        let mut d = doc(false);
        let mut round = red();
        round.set_thickness(5.0);
        let mut s = Stroke::begin_with_mirror(round, &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);

        assert_eq!(at(&d, 8, 6), [255, 0, 0, 255], "the top of the dot is bare");
        assert_eq!(at(&d, 6, 6), [0, 0, 0, 0], "a round tip has no corners");
    }

    #[test]
    fn a_stabilised_stroke_lags_the_hand_and_then_catches_up() {
        let mut steady = Brush {
            tool: Tool::Marker,
            colour: [255, 0, 0, 255],
            ..Default::default()
        };
        steady.set_thickness(1.0);
        steady.set_stabilizer(0.8);

        let mut d = doc(true);
        let mut s = Stroke::begin_with_mirror(steady, &d, 2.0, 8.0, Mirror::default());
        s.extend(14.0, 8.0);
        s.flush(&mut d);
        let reached = |d: &Document| (0..16).filter(|x| at(d, *x, 8)[3] > 0).max().unwrap();
        let lagged = reached(&d);
        assert!(lagged < 13, "a jump straight to the end is no stabiliser");

        s.settle();
        s.flush(&mut d);
        assert!(
            reached(&d) >= 13,
            "the stroke never caught up with the hand"
        );
    }

    #[test]
    fn a_shaky_hand_comes_out_straighter_than_it_went_in() {
        let waver = |stabilizer: f32| {
            let mut brush = Brush {
                tool: Tool::Marker,
                colour: [255, 0, 0, 255],
                ..Default::default()
            };
            brush.set_thickness(1.0);
            brush.set_stabilizer(stabilizer);

            let mut d = doc(true);
            let mut s = Stroke::begin_with_mirror(brush, &d, 1.0, 8.0, Mirror::default());
            for step in 1..=14 {
                let shake = if step % 2 == 0 { 2.0 } else { -2.0 };
                s.extend(1.0 + step as f32, 8.0 + shake);
            }
            s.settle();
            s.flush(&mut d);

            (0..16)
                .filter(|y| (0..16).any(|x| at(&d, x, *y)[3] > 0))
                .count()
        };

        assert!(
            waver(0.85) < waver(0.0),
            "stabilising left the wobble as wide as it was"
        );
    }

    #[test]
    fn overlapping_passes_do_not_darken_within_one_stroke() {
        let mut d = doc(false);
        let mut half = red();
        half.set_opacity(0.5);
        let mut s = Stroke::begin_with_mirror(half, &d, 8.5, 8.5, Mirror::default());
        for _ in 0..12 {
            s.extend(8.5, 8.5);
            s.extend(8.6, 8.5);
        }
        s.flush(&mut d);

        let once = {
            let mut d2 = doc(false);
            let mut s2 = Stroke::begin_with_mirror(half, &d2, 8.5, 8.5, Mirror::default());
            s2.flush(&mut d2);
            at(&d2, 8, 8)
        };
        assert_eq!(at(&d, 8, 8), once, "a stroke crossing itself changed shade");
    }

    #[test]
    fn the_eraser_clears_alpha_on_a_transparent_canvas() {
        let mut d = doc(true);
        let mut paint = Stroke::begin_with_mirror(red(), &d, 8.5, 8.5, Mirror::default());
        paint.flush(&mut d);
        assert_eq!(at(&d, 8, 8)[3], 255);

        let mut rubber = Brush {
            tool: Tool::Eraser,
            ..red()
        };
        rubber.set_thickness(4.0);
        let mut s = Stroke::begin_with_mirror(rubber, &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);
        assert_eq!(at(&d, 8, 8)[3], 0, "pixel should be fully transparent");
    }

    #[test]
    fn the_eraser_never_paints_white_it_only_removes() {
        let mut d = doc(false);
        let mut paint = Stroke::begin_with_mirror(red(), &d, 8.5, 8.5, Mirror::default());
        paint.flush(&mut d);

        let mut rubber = Brush {
            tool: Tool::Eraser,
            ..red()
        };
        rubber.set_thickness(4.0);
        let mut s = Stroke::begin_with_mirror(rubber, &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);
        assert_eq!(
            at(&d, 8, 8),
            [255, 0, 0, 0],
            "alpha gone, no white painted in"
        );

        d.set_transparent(true);
        assert_eq!(at(&d, 8, 8)[3], 0, "and it is genuinely see-through now");
    }

    #[test]
    fn a_soft_eraser_leaves_a_fading_edge() {
        let mut d = doc(true);
        let mut ink = Brush {
            tool: Tool::Marker,
            colour: [255, 0, 0, 255],
            ..Default::default()
        };
        ink.set_thickness(100.0);
        let mut paint = Stroke::begin_with_mirror(ink, &d, 8.0, 8.0, Mirror::default());
        paint.flush(&mut d);

        let mut rubber = Brush {
            tool: Tool::Eraser,
            ..Default::default()
        };
        rubber.set_thickness(12.0);
        rubber.set_antialiased(true);
        rubber.set_hardness(0.0);
        let mut s = Stroke::begin_with_mirror(rubber, &d, 8.0, 8.0, Mirror::default());
        s.flush(&mut d);

        let alpha: Vec<u8> = (8..14).map(|x| at(&d, x, 8)[3]).collect();
        assert!(
            alpha[0] < 16,
            "the centre should be all but gone: {alpha:?}"
        );
        assert!(
            alpha.windows(2).all(|w| w[0] < w[1]),
            "alpha should climb outwards: {alpha:?}"
        );
        assert!(alpha[5] > 200, "and barely touch the rim: {alpha:?}");
    }

    fn sized(tool: Tool, thickness: f32) -> Brush {
        let mut b = Brush {
            tool,
            colour: [255, 0, 0, 255],
            ..Default::default()
        };
        b.set_thickness(thickness);
        b
    }

    fn opaque(width: u32, height: u32) -> Document {
        Document::from_image(Rgba8::new(width, height, [40, 90, 160, 255]), None)
    }

    // Unreached pixels at least `inside` in from the edge of the tip swept from `a` to `b`.
    fn untouched(d: &Document, erase: bool, a: (f32, f32), b: (f32, f32), inside: f32) -> usize {
        let (w, h) = d.size();
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let along = dx * dx + dy * dy;
        let mut count = 0;
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if along > 0.0 {
                    (((px - a.0) * dx + (py - a.1) * dy) / along).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let near = (px - a.0 - dx * t).hypot(py - a.1 - dy * t) <= inside;
                let alpha = at(d, x, y)[3];
                let reached = if erase { alpha < 255 } else { alpha > 0 };
                if near && !reached {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn a_one_pixel_move_paints_what_it_newly_reaches_at_once() {
        for (tool, thickness) in [
            (Tool::Marker, 200.0),
            (Tool::Marker, 12.0),
            (Tool::Eraser, 200.0),
            (Tool::OilBrush, 120.0),
            (Tool::Calligraphy, 60.0),
            (Tool::Watercolour, 120.0),
            (Tool::Pencil, 40.0),
            (Tool::PixelPen, 30.0),
        ] {
            let brush = sized(tool, thickness);
            let erase = tool == Tool::Eraser;
            let mut d = if erase {
                opaque(300, 300)
            } else {
                Document::blank_sized(300, 300, true)
            };
            let inside = brush.stamp_radius() * brush.profile().aspect - 1.5;
            let start = (120.3, 150.3);
            let mut s = Stroke::begin_with_mirror(brush, &d, start.0, start.1, Mirror::default());
            s.flush(&mut d);
            for step in 1..=12 {
                let to = (start.0 + step as f32, start.1);
                s.extend(to.0, to.1);
                s.flush(&mut d);
                assert_eq!(
                    untouched(&d, erase, to, to, inside),
                    0,
                    "{tool:?} {thickness} left part of where it now is untouched after {step} px"
                );
            }
        }
    }

    #[test]
    fn a_one_pixel_pen_marks_each_pixel_the_moment_the_pointer_enters_it() {
        for pixel_perfect in [true, false] {
            let mut pen = red();
            pen.set_pixel_perfect(pixel_perfect);
            let mut d = doc(true);
            let mut s = Stroke::begin_with_mirror(pen, &d, 2.9, 8.5, Mirror::default());
            s.flush(&mut d);
            for x in [3.1, 4.05, 5.0, 6.95, 7.2] {
                s.extend(x, 8.5);
                s.flush(&mut d);
                assert_eq!(
                    at(&d, x as u32, 8),
                    [255, 0, 0, 255],
                    "the pixel under {x} waited for the pointer to travel further"
                );
            }
        }
    }

    #[test]
    fn no_gaps_however_fast_or_sharply_the_pointer_turns() {
        let path = [
            (20.5, 20.5),
            (180.5, 60.5),
            (25.5, 70.5),
            (175.5, 180.5),
            (170.0, 20.0),
            (30.0, 190.0),
            (29.0, 189.0),
            (180.0, 100.0),
        ];
        for (tool, thickness) in [
            (Tool::Marker, 12.0),
            (Tool::Eraser, 30.0),
            (Tool::Marker, 3.0),
            (Tool::PixelPen, 5.0),
        ] {
            let brush = sized(tool, thickness);
            let erase = tool == Tool::Eraser;
            let mut d = if erase {
                opaque(200, 200)
            } else {
                Document::blank_sized(200, 200, true)
            };
            let inside = (brush.stamp_radius() - 1.5).max(0.0);
            let mut s =
                Stroke::begin_with_mirror(brush, &d, path[0].0, path[0].1, Mirror::default());
            for &(x, y) in &path[1..] {
                s.extend(x, y);
            }
            s.flush(&mut d);
            for pair in path.windows(2) {
                assert_eq!(
                    untouched(&d, erase, pair[0], pair[1], inside),
                    0,
                    "{tool:?} {thickness} left a gap between {:?} and {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn a_one_pixel_line_stays_joined_up_however_fast_it_is_drawn() {
        for pixel_perfect in [true, false] {
            let mut pen = red();
            pen.set_pixel_perfect(pixel_perfect);
            let mut d = Document::blank_sized(64, 64, true);
            let mut s = Stroke::begin_with_mirror(pen, &d, 2.5, 2.5, Mirror::default());
            for (x, y) in [(60.5, 10.5), (5.5, 40.5), (50.5, 60.5), (50.5, 5.5)] {
                s.extend(x, y);
            }
            s.flush(&mut d);

            let painted: Vec<(i64, i64)> = (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|&(x, y)| at(&d, x as u32, y as u32)[3] > 0)
                .collect();
            let mut reached = vec![(2i64, 2i64)];
            let mut seen = std::collections::HashSet::from([(2i64, 2i64)]);
            while let Some((x, y)) = reached.pop() {
                for (nx, ny) in painted.iter().copied() {
                    if (nx - x).abs() <= 1 && (ny - y).abs() <= 1 && seen.insert((nx, ny)) {
                        reached.push((nx, ny));
                    }
                }
            }
            assert_eq!(
                seen.len(),
                painted.len(),
                "the line came apart (pixel-perfect {pixel_perfect})"
            );
            assert!(
                seen.contains(&(50, 5)),
                "and it reaches where the pointer went"
            );
        }
    }

    #[test]
    fn releasing_has_nothing_left_to_finish() {
        let brush = sized(Tool::Marker, 80.0);
        let mut d = Document::blank_sized(200, 200, true);
        let mut s = Stroke::begin_with_mirror(brush, &d, 50.0, 100.0, Mirror::default());
        s.extend(57.0, 100.0);
        s.flush(&mut d);
        let before = d.version();

        s.settle();
        assert!(
            s.flush(&mut d).is_empty(),
            "the release painted the tail late"
        );
        assert_eq!(d.version(), before);
        assert_eq!(untouched(&d, false, (57.0, 100.0), (57.0, 100.0), 38.5), 0);
    }

    #[test]
    fn paint_builds_up_the_same_however_the_path_is_cut_up() {
        for tool in [Tool::Watercolour, Tool::Pencil, Tool::Crayon] {
            let brush = sized(tool, 40.0);
            let draw = |cuts: usize| {
                let mut d = Document::blank_sized(200, 60, true);
                let mut s = Stroke::begin_with_mirror(brush, &d, 30.0, 30.0, Mirror::default());
                for i in 1..=cuts {
                    s.extend(30.0 + 140.0 * i as f32 / cuts as f32, 30.0);
                }
                s.flush(&mut d);
                d
            };
            let (whole, pieces) = (draw(1), draw(140));
            // Spacing can leave a pixel one stamp off; more means paint was lost or doubled. The
            // end cap depends on where the last sample landed, so it only has to be painted.
            let body = (0..60).flat_map(|y| (0..148).map(move |x| (x, y)));
            let stamp = (brush.profile().flow * 255.0).ceil() as u8;
            let worst = body
                .clone()
                .map(|(x, y)| at(&whole, x, y)[3].abs_diff(at(&pieces, x, y)[3]))
                .max()
                .unwrap();
            assert!(
                worst <= stamp,
                "{tool:?} came out {worst} levels different drawn in one go and in pieces"
            );
            let ink =
                |d: &Document| -> f64 { body.clone().map(|(x, y)| at(d, x, y)[3] as f64).sum() };
            let ratio = ink(&pieces) / ink(&whole);
            assert!(
                (0.99..1.01).contains(&ratio),
                "{tool:?} laid down {ratio:.3} times the paint in pieces"
            );
            for d in [&whole, &pieces] {
                assert!(at(d, 168, 30)[3] > 0, "{tool:?} stopped short of the end");
            }
        }
    }

    #[test]
    fn starting_a_stroke_does_not_copy_the_canvas() {
        let mut d = Document::blank_sized(2000, 1500, true);
        let buffer = std::sync::Arc::as_ptr(&d.pixels().bytes_arc());
        let mut s = Stroke::begin_with_mirror(red(), &d, 8.5, 4.5, Mirror::default());
        s.extend(40.5, 30.5);
        s.flush(&mut d);
        assert_eq!(
            std::sync::Arc::as_ptr(&d.pixels().bytes_arc()),
            buffer,
            "the whole canvas was copied to remember what was under one stroke"
        );
    }

    #[test]
    fn undo_puts_back_exactly_what_was_under_the_stroke() {
        let (w, h) = (300, 200);
        let mut pattern = Vec::with_capacity(w * h * CHANNELS);
        for i in 0..w * h {
            pattern.extend_from_slice(&[(i % 251) as u8, (i % 13) as u8 * 19, 7, (i % 199) as u8]);
        }
        let original = Rgba8::from_raw(w as u32, h as u32, pattern).unwrap();
        let mut d = Document::from_image(original.clone(), None);

        let mut eraser = sized(Tool::Eraser, 50.0);
        eraser.set_antialiased(true);
        let both = Mirror {
            horizontal: true,
            vertical: true,
        };
        for brush in [
            sized(Tool::Watercolour, 70.0),
            eraser,
            sized(Tool::Marker, 9.0),
        ] {
            let mut s = Stroke::begin_with_mirror(brush, &d, 10.0, 15.0, both);
            for step in 1..=30 {
                s.extend(10.0 + step as f32 * 4.5, 15.0 + step as f32 * 2.0);
                s.flush(&mut d);
            }
            s.commit(&mut d);
        }
        assert_ne!(d.pixels(), &original);

        while d.undo().is_some() {}
        assert!(d.pixels() == &original, "undo left part of a stroke behind");
    }

    #[test]
    fn a_long_thin_stroke_is_remembered_by_what_it_touched_not_its_box() {
        let mut d = Document::blank_sized(2000, 2000, true);
        let mut s =
            Stroke::begin_with_mirror(sized(Tool::Marker, 10.0), &d, 5.0, 5.0, Mirror::default());
        s.extend(1995.0, 1995.0);
        s.flush(&mut d);
        s.commit(&mut d);

        let boxed = s.touched().unwrap().area() * CHANNELS * 2;
        assert!(
            d.history_bytes() * 10 < boxed,
            "{} bytes kept for a stroke whose box is {boxed}",
            d.history_bytes()
        );
        d.undo();
        assert!(
            d.pixels().as_bytes().iter().all(|&b| b == 0),
            "undo missed a tile"
        );
    }

    #[test]
    fn each_mirrored_copy_reports_its_own_region() {
        let mut d = Document::blank_sized(400, 300, true);
        let both = Mirror {
            horizontal: true,
            vertical: true,
        };
        let mut s = Stroke::begin_with_mirror(sized(Tool::Marker, 10.0), &d, 20.0, 20.0, both);
        s.flush(&mut d);
        s.extend(30.0, 25.0);
        let regions = s.flush(&mut d);
        assert_eq!(regions.len(), 4);
        for rect in regions {
            assert!(
                rect.width() < 40 && rect.height() < 40,
                "{rect:?} spans more than one copy"
            );
        }
    }

    #[test]
    fn a_wide_pixel_pen_presses_its_tip_on_every_pixel_of_the_line() {
        let mut pen = sized(Tool::PixelPen, 3.0);
        pen.colour = [255, 0, 0, 255];
        let mut d = Document::blank_sized(40, 20, true);
        let mut s = Stroke::begin_with_mirror(pen, &d, 3.5, 3.5, Mirror::default());
        s.extend(33.5, 13.5);
        s.flush(&mut d);
        for (cx, cy) in std::iter::once((3, 3)).chain(cells_between((3, 3), (33, 13))) {
            for (x, y) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (cx + dx, cy + dy))) {
                assert_eq!(at(&d, x as u32, y as u32)[3], 255, "the tip missed {x},{y}");
            }
        }
    }

    #[test]
    fn only_the_touched_region_is_reported() {
        let mut d = doc(false);
        let mut s = Stroke::begin_with_mirror(red(), &d, 8.5, 8.5, Mirror::default());
        s.flush(&mut d);
        let touched = s.touched().unwrap();
        assert!(
            touched.width() <= 3 && touched.height() <= 3,
            "{touched:?} is too broad"
        );
    }
}
