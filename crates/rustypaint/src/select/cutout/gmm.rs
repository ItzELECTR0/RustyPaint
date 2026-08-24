#![allow(
    clippy::needless_range_loop,
    reason = "these loops walk a matrix or a grid, and the index is the point"
)]

pub const COMPONENTS: usize = 5;

const REGULARISE: f32 = 12.0;

#[derive(Clone, Copy, Default)]
struct Component {
    weight: f32,
    mean: [f32; 3],
    inverse: [[f32; 3]; 3],
    determinant: f32,
}

#[derive(Clone, Default)]
pub struct Gmm {
    parts: [Component; COMPONENTS],
}

#[derive(Clone, Copy, Default)]
struct Tally {
    count: f64,
    sum: [f64; 3],
    products: [[f64; 3]; 3],
}

impl Tally {
    fn add(&mut self, colour: [f32; 3]) {
        self.count += 1.0;
        for i in 0..3 {
            self.sum[i] += colour[i] as f64;
            for j in 0..3 {
                self.products[i][j] += colour[i] as f64 * colour[j] as f64;
            }
        }
    }
}

impl Gmm {
    pub fn fit(colours: &[[f32; 3]], assignment: &[u8]) -> Self {
        let mut tallies = [Tally::default(); COMPONENTS];
        for (colour, which) in colours.iter().zip(assignment) {
            tallies[*which as usize % COMPONENTS].add(*colour);
        }

        let total: f64 = tallies.iter().map(|t| t.count).sum::<f64>().max(1.0);
        let mut parts = [Component::default(); COMPONENTS];
        for (part, tally) in parts.iter_mut().zip(&tallies) {
            if tally.count < 1.0 {
                continue;
            }
            part.weight = (tally.count / total) as f32;
            let mean = [
                tally.sum[0] / tally.count,
                tally.sum[1] / tally.count,
                tally.sum[2] / tally.count,
            ];
            part.mean = [mean[0] as f32, mean[1] as f32, mean[2] as f32];

            let mut covariance = [[0.0f32; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    covariance[i][j] =
                        (tally.products[i][j] / tally.count - mean[i] * mean[j]) as f32;
                }
                covariance[i][i] += REGULARISE;
            }
            let (inverse, determinant) = invert(covariance);
            part.inverse = inverse;
            part.determinant = determinant;
        }
        Self { parts }
    }

    pub fn likelihood(&self, colour: [f32; 3]) -> f32 {
        self.parts.iter().map(|part| part.likelihood(colour)).sum()
    }

    pub fn nearest(&self, colour: [f32; 3]) -> u8 {
        let mut best = (0usize, f32::MIN);
        for (i, part) in self.parts.iter().enumerate() {
            let score = part.likelihood(colour);
            if score > best.1 {
                best = (i, score);
            }
        }
        best.0 as u8
    }
}

impl Component {
    fn likelihood(&self, colour: [f32; 3]) -> f32 {
        if self.weight <= 0.0 || self.determinant <= 0.0 {
            return 0.0;
        }
        let d = [
            colour[0] - self.mean[0],
            colour[1] - self.mean[1],
            colour[2] - self.mean[2],
        ];
        let mut mahalanobis = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                mahalanobis += d[i] * self.inverse[i][j] * d[j];
            }
        }
        self.weight / self.determinant.sqrt() * (-0.5 * mahalanobis).exp()
    }
}

fn invert(m: [[f32; 3]; 3]) -> ([[f32; 3]; 3], f32) {
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if determinant.abs() < 1e-6 {
        return ([[0.0; 3]; 3], 0.0);
    }
    let inv = 1.0 / determinant;
    let mut out = [[0.0f32; 3]; 3];
    #[allow(
        clippy::needless_range_loop,
        reason = "both indices are into the other matrix"
    )]
    for i in 0..3 {
        for j in 0..3 {
            let (a, b) = ((i + 1) % 3, (i + 2) % 3);
            let (c, d) = ((j + 1) % 3, (j + 2) % 3);
            out[j][i] = (m[a][c] * m[b][d] - m[a][d] * m[b][c]) * inv;
        }
    }
    (out, determinant)
}

pub fn cluster(colours: &[[f32; 3]]) -> Vec<u8> {
    if colours.is_empty() {
        return Vec::new();
    }
    let mut centres = vec![colours[0]];
    while centres.len() < COMPONENTS.min(colours.len()) {
        let mut furthest = (0usize, -1.0f32);
        for (i, colour) in colours.iter().enumerate() {
            let nearest = centres
                .iter()
                .map(|centre| distance(*centre, *colour))
                .fold(f32::MAX, f32::min);
            if nearest > furthest.1 {
                furthest = (i, nearest);
            }
        }
        centres.push(colours[furthest.0]);
    }

    let mut assignment = vec![0u8; colours.len()];
    for _ in 0..8 {
        let mut moved = false;
        for (colour, slot) in colours.iter().zip(assignment.iter_mut()) {
            let mut best = (0usize, f32::MAX);
            for (i, centre) in centres.iter().enumerate() {
                let d = distance(*centre, *colour);
                if d < best.1 {
                    best = (i, d);
                }
            }
            if *slot != best.0 as u8 {
                *slot = best.0 as u8;
                moved = true;
            }
        }
        if !moved {
            break;
        }

        let mut sums = vec![([0.0f32; 3], 0.0f32); centres.len()];
        for (colour, which) in colours.iter().zip(&assignment) {
            let (sum, count) = &mut sums[*which as usize];
            for i in 0..3 {
                sum[i] += colour[i];
            }
            *count += 1.0;
        }
        for (centre, (sum, count)) in centres.iter_mut().zip(&sums) {
            if *count > 0.0 {
                *centre = [sum[0] / count, sum[1] / count, sum[2] / count];
            }
        }
    }
    assignment
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mixture_prefers_the_colours_it_was_fitted_to() {
        let reds: Vec<[f32; 3]> = (0..50)
            .map(|i| [200.0 + i as f32 * 0.4, 30.0, 30.0])
            .collect();
        let blues: Vec<[f32; 3]> = (0..50)
            .map(|i| [30.0, 30.0, 200.0 + i as f32 * 0.4])
            .collect();

        let red = Gmm::fit(&reds, &cluster(&reds));
        let blue = Gmm::fit(&blues, &cluster(&blues));

        assert!(red.likelihood([210.0, 30.0, 30.0]) > blue.likelihood([210.0, 30.0, 30.0]));
        assert!(blue.likelihood([30.0, 30.0, 210.0]) > red.likelihood([30.0, 30.0, 210.0]));
    }

    #[test]
    fn clustering_separates_colours_that_are_far_apart() {
        let mut colours = Vec::new();
        for tone in [
            [10.0, 10.0, 10.0],
            [250.0, 250.0, 250.0],
            [250.0, 10.0, 10.0],
        ] {
            for _ in 0..20 {
                colours.push(tone);
            }
        }
        let assignment = cluster(&colours);
        assert_eq!(assignment[0], assignment[19], "one tone, one cluster");
        assert_ne!(
            assignment[0], assignment[20],
            "and different tones, different clusters"
        );
        assert_ne!(assignment[20], assignment[40]);
    }

    #[test]
    fn a_model_fitted_to_a_million_pixels_still_says_something() {
        for count in [1_000, 100_000, 1_000_000] {
            let colours: Vec<[f32; 3]> = (0..count).map(|_| [235.0, 235.0, 240.0]).collect();
            let model = Gmm::fit(&colours, &vec![0u8; colours.len()]);

            let at_home = model.likelihood([235.0, 235.0, 240.0]);
            assert!(
                at_home > 1e-3,
                "{count} pixels of one colour, and the model puts {at_home} on that colour"
            );
            assert!(
                model.likelihood([20.0, 200.0, 40.0]) < at_home / 100.0,
                "{count}"
            );
        }
    }

    #[test]
    fn the_inverse_is_one() {
        let m = [[4.0, 1.0, 0.5], [1.0, 3.0, 0.25], [0.5, 0.25, 2.0]];
        let (inverse, determinant) = invert(m);
        assert!(determinant > 0.0);
        for i in 0..3 {
            for j in 0..3 {
                let entry: f32 = (0..3).map(|k| m[i][k] * inverse[k][j]).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((entry - want).abs() < 1e-4, "at {i},{j}: {entry}");
            }
        }
    }
}
