use crate::doc::{Rect, Version};
use std::collections::VecDeque;

// A texture further behind than this many regions is uploaded whole.
const REMEMBERED: usize = 256;

// Extra pixels allowed when merging two regions, on top of half again their area. A fixed
// allowance alone would merge mirrored copies on the same rows into a band across the canvas.
const SLACK: usize = 32 * 32;

const MOST_UPLOADS: usize = 8;

// Regions changed by recent versions of the canvas, so the renderer can catch up from whichever
// version it last drew. See `.agents/rendering.md`.
#[derive(Debug, Clone, Default)]
pub struct Damage {
    from: Version,
    edits: VecDeque<(Version, Rect)>,
}

impl Damage {
    pub fn record(
        &mut self,
        before: Version,
        after: Version,
        rects: impl IntoIterator<Item = Rect>,
    ) {
        if before == after {
            return;
        }
        match self.edits.back() {
            // One edit can arrive as several regions, one per mirrored copy.
            Some(&(last, _)) if last == before || last == after => {}
            _ => {
                self.edits.clear();
                self.from = before;
            }
        }
        self.edits.extend(
            rects
                .into_iter()
                .filter(|rect| !rect.is_empty())
                .map(|rect| (after, rect)),
        );
        while self.edits.len() > REMEMBERED {
            if let Some((version, _)) = self.edits.pop_front() {
                self.from = version;
            }
        }
    }

    pub fn clear(&mut self) {
        self.edits.clear();
    }

    // `None` when the log does not reach back to `uploaded`.
    pub fn since(&self, uploaded: Version, version: Version) -> Option<Vec<Rect>> {
        let &(last, _) = self.edits.back()?;
        let known = uploaded == self.from || self.edits.iter().any(|(v, _)| *v == uploaded);
        if last != version || !known {
            return None;
        }
        Some(coalesce(
            self.edits
                .iter()
                .filter(|(v, _)| *v > uploaded)
                .map(|(_, rect)| *rect),
        ))
    }
}

fn coalesce(rects: impl Iterator<Item = Rect>) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for rect in rects {
        let mut rect = rect;
        while let Some(i) = out.iter().position(|other| {
            other.union(rect).area() <= (other.area() + rect.area()) * 3 / 2 + SLACK
        }) {
            rect = out.swap_remove(i).union(rect);
        }
        out.push(rect);
    }
    while out.len() > MOST_UPLOADS {
        let mut best = (0, 1, usize::MAX);
        for i in 0..out.len() {
            for j in i + 1..out.len() {
                let growth = out[i].union(out[j]).area() - out[i].area() - out[j].area();
                if growth < best.2 {
                    best = (i, j, growth);
                }
            }
        }
        let merged = out[best.0].union(out[best.1]);
        out.swap_remove(best.1);
        out[best.0] = merged;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: u32, y0: u32, x1: u32, y1: u32) -> Rect {
        Rect::new(x0, y0, x1, y1)
    }

    #[test]
    fn a_texture_one_frame_behind_gets_only_what_changed_since() {
        let mut damage = Damage::default();
        damage.record(10, 11, [r(0, 0, 4, 4)]);
        damage.record(11, 12, [r(1000, 1000, 1004, 1004)]);
        damage.record(12, 13, [r(2000, 0, 2004, 4)]);

        assert_eq!(damage.since(12, 13), Some(vec![r(2000, 0, 2004, 4)]));
        assert_eq!(damage.since(10, 13).map(|v| v.len()), Some(3));
        assert_eq!(damage.since(13, 13), Some(vec![]));
    }

    #[test]
    fn a_version_the_log_never_saw_is_uploaded_whole() {
        let mut damage = Damage::default();
        damage.record(10, 11, [r(0, 0, 4, 4)]);
        damage.record(11, 14, [r(0, 0, 4, 4)]);

        assert_eq!(damage.since(9, 14), None, "older than the log");
        assert_eq!(damage.since(12, 14), None, "never one of this document's");
        assert_eq!(damage.since(11, 15), None, "the log stops short");
    }

    #[test]
    fn a_gap_in_the_chain_starts_the_log_again() {
        let mut damage = Damage::default();
        damage.record(10, 11, [r(0, 0, 4, 4)]);
        damage.record(20, 21, [r(8, 8, 9, 9)]);

        assert_eq!(damage.since(11, 21), None);
        assert_eq!(damage.since(20, 21), Some(vec![r(8, 8, 9, 9)]));
    }

    #[test]
    fn the_regions_of_one_edit_stay_together() {
        let mut damage = Damage::default();
        damage.record(10, 11, [r(0, 0, 4, 4), r(500, 500, 504, 504)]);
        damage.record(11, 12, [r(0, 4, 4, 8), r(500, 504, 504, 508)]);

        let mut since = damage.since(10, 12).unwrap();
        since.sort_by_key(|rect| rect.x0);
        assert_eq!(since, vec![r(0, 0, 4, 8), r(500, 500, 504, 508)]);
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut damage = Damage::default();
        damage.record(10, 11, [r(0, 0, 4, 4)]);
        damage.clear();
        assert_eq!(damage.since(10, 11), None);
    }

    #[test]
    fn the_log_forgets_its_oldest_regions_first() {
        let mut damage = Damage::default();
        for v in 0..REMEMBERED as u64 + 10 {
            damage.record(v, v + 1, [r(0, 0, 1, 1)]);
        }
        let last = REMEMBERED as u64 + 10;
        assert_eq!(damage.since(0, last), None);
        assert!(damage.since(last - 5, last).is_some());
    }

    #[test]
    fn far_apart_regions_are_not_merged_into_one_huge_one() {
        let rects = coalesce([r(0, 0, 10, 10), r(4000, 4000, 4010, 4010)].into_iter());
        assert_eq!(rects.len(), 2);
    }

    #[test]
    fn mirrored_copies_on_the_same_rows_stay_apart() {
        let rects = coalesce([r(16, 16, 30, 30), r(370, 16, 384, 30)].into_iter());
        assert_eq!(
            rects.len(),
            2,
            "merging them would upload a band across the canvas"
        );
    }

    #[test]
    fn too_many_regions_merge_where_it_costs_least() {
        let rects = coalesce((0..20).map(|i| r(i * 1000, 0, i * 1000 + 10, 10)));
        assert_eq!(rects.len(), MOST_UPLOADS);
        let covered: usize = rects.iter().map(Rect::area).sum();
        assert!(
            covered < 20 * 1000 * 10,
            "merged far more than it needed to"
        );
    }
}
