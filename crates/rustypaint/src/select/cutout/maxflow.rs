#![allow(
    clippy::needless_range_loop,
    reason = "these loops walk a matrix or a grid, and the index is the point"
)]

pub const DIRS: [(i32, i32); 8] = [
    (-1, -1),
    (1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (-1, 1),
    (-1, 0),
    (1, 0),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tree {
    Free,
    Source,
    Sink,
}

const TERMINAL: i8 = -1;
const NONE: i8 = -2;

pub struct Grid {
    width: usize,
    height: usize,
    arcs: Vec<f32>,
    terminal: Vec<f32>,
    through: f32,

    tree: Vec<Tree>,
    parent: Vec<i8>,
    active: std::collections::VecDeque<usize>,
    in_active: Vec<bool>,
    orphans: Vec<usize>,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        let n = width * height;
        Self {
            width,
            height,
            arcs: vec![0.0; n * 8],
            terminal: vec![0.0; n],
            through: 0.0,
            tree: vec![Tree::Free; n],
            parent: vec![NONE; n],
            active: std::collections::VecDeque::new(),
            in_active: vec![false; n],
            orphans: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.terminal.len()
    }

    pub fn set_terminal(&mut self, node: usize, source: f32, sink: f32) {
        self.through += source.min(sink);
        self.terminal[node] = source - sink;
    }

    pub fn set_neighbour(&mut self, node: usize, dir: usize, capacity: f32) {
        let Some(other) = self.neighbour(node, dir) else {
            return;
        };
        self.arcs[node * 8 + dir] = capacity;
        self.arcs[other * 8 + (dir ^ 1)] = capacity;
    }

    pub fn neighbour(&self, node: usize, dir: usize) -> Option<usize> {
        let (dx, dy) = DIRS[dir];
        let x = (node % self.width) as i32 + dx;
        let y = (node / self.width) as i32 + dy;
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
    }

    pub fn is_source(&self, node: usize) -> bool {
        self.tree[node] == Tree::Source
    }

    pub fn max_flow(&mut self) -> f32 {
        self.start();
        let mut flow = self.through;

        while let Some(node) = self.next_active() {
            while self.parent[node] != NONE {
                let Some((from, dir)) = self.grow(node) else {
                    break;
                };
                let pushed = self.augment(from, dir);
                self.adopt();
                if pushed <= 0.0 {
                    break;
                }
                flow += pushed;
            }
        }
        flow
    }

    fn start(&mut self) {
        for node in 0..self.len() {
            if self.terminal[node] > 0.0 {
                self.tree[node] = Tree::Source;
                self.parent[node] = TERMINAL;
                self.activate(node);
            } else if self.terminal[node] < 0.0 {
                self.tree[node] = Tree::Sink;
                self.parent[node] = TERMINAL;
                self.activate(node);
            }
        }
    }

    fn activate(&mut self, node: usize) {
        if !self.in_active[node] {
            self.in_active[node] = true;
            self.active.push_back(node);
        }
    }

    fn next_active(&mut self) -> Option<usize> {
        let node = self.active.pop_front()?;
        self.in_active[node] = false;
        Some(node)
    }

    fn grow(&mut self, node: usize) -> Option<(usize, usize)> {
        let mine = self.tree[node];
        for dir in 0..8 {
            let residual = match mine {
                Tree::Source => self.arcs[node * 8 + dir],
                Tree::Sink => {
                    let Some(other) = self.neighbour(node, dir) else {
                        continue;
                    };
                    self.arcs[other * 8 + (dir ^ 1)]
                }
                Tree::Free => return None,
            };
            if residual <= 0.0 {
                continue;
            }
            let Some(other) = self.neighbour(node, dir) else {
                continue;
            };

            match self.tree[other] {
                Tree::Free => {
                    self.tree[other] = mine;
                    self.parent[other] = (dir ^ 1) as i8;
                    self.activate(other);
                }
                found if found != mine => {
                    return Some(match mine {
                        Tree::Source => (node, dir),
                        _ => (other, dir ^ 1),
                    });
                }
                _ => {}
            }
        }
        None
    }

    fn augment(&mut self, from: usize, dir: usize) -> f32 {
        let to = self
            .neighbour(from, dir)
            .expect("the arc we just grew along");

        let mut bottleneck = self.arcs[from * 8 + dir];
        let mut node = from;
        while self.parent[node] != TERMINAL {
            let up = self.parent[node] as usize;
            let parent = self.neighbour(node, up).expect("a parent");
            bottleneck = bottleneck.min(self.arcs[parent * 8 + (up ^ 1)]);
            node = parent;
        }
        bottleneck = bottleneck.min(self.terminal[node]);

        let mut node = to;
        while self.parent[node] != TERMINAL {
            let up = self.parent[node] as usize;
            let parent = self.neighbour(node, up).expect("a parent");
            bottleneck = bottleneck.min(self.arcs[node * 8 + up]);
            node = parent;
        }
        bottleneck = bottleneck.min(-self.terminal[node]);

        if bottleneck <= 0.0 {
            return 0.0;
        }

        self.arcs[from * 8 + dir] -= bottleneck;
        self.arcs[to * 8 + (dir ^ 1)] += bottleneck;

        let mut node = from;
        while self.parent[node] != TERMINAL {
            let up = self.parent[node] as usize;
            let parent = self.neighbour(node, up).expect("a parent");
            self.arcs[parent * 8 + (up ^ 1)] -= bottleneck;
            self.arcs[node * 8 + up] += bottleneck;
            if self.arcs[parent * 8 + (up ^ 1)] <= 0.0 {
                self.parent[node] = NONE;
                self.orphans.push(node);
            }
            node = parent;
        }
        self.terminal[node] -= bottleneck;
        if self.terminal[node] <= 0.0 {
            self.parent[node] = NONE;
            self.orphans.push(node);
        }

        let mut node = to;
        while self.parent[node] != TERMINAL {
            let up = self.parent[node] as usize;
            let parent = self.neighbour(node, up).expect("a parent");
            self.arcs[node * 8 + up] -= bottleneck;
            self.arcs[parent * 8 + (up ^ 1)] += bottleneck;
            if self.arcs[node * 8 + up] <= 0.0 {
                self.parent[node] = NONE;
                self.orphans.push(node);
            }
            node = parent;
        }
        self.terminal[node] += bottleneck;
        if self.terminal[node] >= 0.0 {
            self.parent[node] = NONE;
            self.orphans.push(node);
        }

        bottleneck
    }

    fn adopt(&mut self) {
        while let Some(node) = self.orphans.pop() {
            let mine = self.tree[node];
            if mine == Tree::Free {
                continue;
            }

            if let Some(dir) = self.find_parent(node, mine) {
                self.parent[node] = dir as i8;
                continue;
            }

            for dir in 0..8 {
                let Some(other) = self.neighbour(node, dir) else {
                    continue;
                };
                if self.tree[other] != mine {
                    continue;
                }
                if self.can_parent(node, other, dir, mine) {
                    self.activate(other);
                }
                if self.parent[other] == (dir ^ 1) as i8 {
                    self.parent[other] = NONE;
                    self.orphans.push(other);
                }
            }
            self.tree[node] = Tree::Free;
            self.parent[node] = NONE;
        }
    }

    fn find_parent(&self, node: usize, mine: Tree) -> Option<usize> {
        for dir in 0..8 {
            let Some(other) = self.neighbour(node, dir) else {
                continue;
            };
            if self.tree[other] != mine || self.parent[other] == NONE {
                continue;
            }
            if !self.can_parent(node, other, dir, mine) {
                continue;
            }
            if self.rooted(other, mine) {
                return Some(dir);
            }
        }
        None
    }

    fn can_parent(&self, node: usize, other: usize, dir: usize, mine: Tree) -> bool {
        let residual = match mine {
            Tree::Source => self.arcs[other * 8 + (dir ^ 1)],
            _ => self.arcs[node * 8 + dir],
        };
        residual > 0.0
    }

    fn rooted(&self, mut node: usize, mine: Tree) -> bool {
        let mut steps = 0;
        while self.parent[node] != TERMINAL {
            if self.parent[node] == NONE || self.tree[node] != mine {
                return false;
            }
            let up = self.parent[node] as usize;
            let Some(parent) = self.neighbour(node, up) else {
                return false;
            };
            node = parent;
            steps += 1;
            if steps > self.len() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flow_is_the_bottleneck_along_the_only_path() {
        let mut grid = Grid::new(2, 1);
        grid.set_terminal(0, 3.0, 0.0);
        grid.set_terminal(1, 0.0, 4.0);
        grid.set_neighbour(0, 7, 2.0);

        assert!((grid.max_flow() - 2.0).abs() < 1e-4);
        assert!(grid.is_source(0), "the first pixel stays with the source");
        assert!(!grid.is_source(1));
    }

    #[test]
    fn the_cut_falls_where_the_arcs_are_cheapest() {
        let (w, h) = (6, 4);
        let mut grid = Grid::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let node = y * w + x;
                if x == 0 {
                    grid.set_terminal(node, 100.0, 0.0);
                }
                if x == w - 1 {
                    grid.set_terminal(node, 0.0, 100.0);
                }
                if x + 1 < w {
                    grid.set_neighbour(node, 7, if x == 2 { 1.0 } else { 50.0 });
                }
                if y + 1 < h {
                    grid.set_neighbour(node, 3, 50.0);
                }
            }
        }

        let flow = grid.max_flow();
        assert!(
            (flow - h as f32).abs() < 1e-3,
            "the seam costs one per row, got {flow}"
        );
        for y in 0..h {
            for x in 0..w {
                assert_eq!(grid.is_source(y * w + x), x <= 2, "at {x},{y}");
            }
        }
    }

    #[test]
    fn it_agrees_with_a_textbook_max_flow_on_random_grids() {
        let mut seed = 0x5eed_1234_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };

        for case in 0..40 {
            let (w, h) = (3 + case % 7, 3 + case % 5);
            let n = w * h;
            let mut grid = Grid::new(w, h);
            let mut caps = vec![vec![0.0f32; n + 2]; n + 2];

            for node in 0..n {
                let (from_source, to_sink) = (next() * 4.0, next() * 4.0);
                grid.set_terminal(node, from_source, to_sink);
                caps[n][node] = from_source;
                caps[node][n + 1] = to_sink;

                for dir in [1, 3, 7] {
                    let Some(other) = grid.neighbour(node, dir) else {
                        continue;
                    };
                    let capacity = next() * 3.0;
                    grid.set_neighbour(node, dir, capacity);
                    caps[node][other] = capacity;
                    caps[other][node] = capacity;
                }
            }

            let pristine = caps.clone();
            let ours = grid.max_flow();
            let theirs = edmonds_karp(&mut caps, n, n + 1);
            assert!(
                (ours - theirs).abs() < 1e-3,
                "case {case}: {ours} against {theirs}"
            );

            let side = |node: usize| match node {
                s if s == n => true,
                t if t == n + 1 => false,
                pixel => grid.is_source(pixel),
            };
            let mut cut = 0.0;
            for from in 0..n + 2 {
                for to in 0..n + 2 {
                    if side(from) && !side(to) {
                        cut += pristine[from][to];
                    }
                }
            }
            assert!(
                (cut - ours).abs() < 1e-3,
                "case {case}: cut {cut} against flow {ours}"
            );
        }
    }

    #[test]
    fn the_cut_it_leaves_costs_what_the_flow_carried() {
        let mut seed = 0x9e37_79b9_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };

        for (w, h) in [(40usize, 30usize), (120, 90), (300, 200)] {
            let mut grid = Grid::new(w, h);
            let mut terminals = vec![(0.0f32, 0.0f32); w * h];
            let mut links = vec![[0.0f32; 8]; w * h];

            for node in 0..w * h {
                let x = node % w;
                let thing = x > w / 4 && x < w * 3 / 4;
                let noise = next() < 0.05;
                let (source, sink) = match thing != noise {
                    true => (6.0 + next(), 0.5),
                    false => (0.5, 6.0 + next()),
                };
                terminals[node] = (source, sink);
                grid.set_terminal(node, source, sink);

                for dir in (0..8).step_by(2) {
                    if grid.neighbour(node, dir).is_none() {
                        continue;
                    }
                    let capacity = 1.0 + next() * 30.0;
                    grid.set_neighbour(node, dir, capacity);
                    links[node][dir] = capacity;
                }
            }

            let flow = grid.max_flow();

            let mut cut = 0.0;
            for node in 0..w * h {
                if grid.is_source(node) {
                    cut += terminals[node].1;
                } else {
                    cut += terminals[node].0;
                }
                for dir in (0..8).step_by(2) {
                    let Some(other) = grid.neighbour(node, dir) else {
                        continue;
                    };
                    if grid.is_source(node) != grid.is_source(other) {
                        cut += links[node][dir];
                    }
                }
            }

            let kept = (0..w * h).filter(|n| grid.is_source(*n)).count();
            assert!(
                (cut - flow).abs() < flow.max(1.0) * 1e-3,
                "{w} by {h}: the cut costs {cut} and the flow carried {flow}, keeping {kept}"
            );
        }
    }

    fn edmonds_karp(caps: &mut [Vec<f32>], source: usize, sink: usize) -> f32 {
        let n = caps.len();
        let mut flow = 0.0;
        loop {
            let mut parent = vec![usize::MAX; n];
            parent[source] = source;
            let mut queue = std::collections::VecDeque::from([source]);
            while let Some(node) = queue.pop_front() {
                for next in 0..n {
                    if parent[next] == usize::MAX && caps[node][next] > 1e-9 {
                        parent[next] = node;
                        queue.push_back(next);
                    }
                }
            }
            if parent[sink] == usize::MAX {
                return flow;
            }

            let mut bottleneck = f32::MAX;
            let mut node = sink;
            while node != source {
                bottleneck = bottleneck.min(caps[parent[node]][node]);
                node = parent[node];
            }
            let mut node = sink;
            while node != source {
                caps[parent[node]][node] -= bottleneck;
                caps[node][parent[node]] += bottleneck;
                node = parent[node];
            }
            flow += bottleneck;
        }
    }
}
