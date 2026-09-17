use crate::doc::{Rect, Rgba8, image::CHANNELS};

pub const MIN_STRENGTH: f32 = 1.0;
pub const MAX_STRENGTH: f32 = 200.0;
pub const MIN_ANGLE: f32 = -180.0;
pub const MAX_ANGLE: f32 = 180.0;
pub const MIN_DETAIL: f32 = 0.0;
pub const MAX_DETAIL: f32 = 1.0;
pub const MIN_PASSES: f32 = 1.0;
pub const MAX_PASSES: f32 = 4.0;
pub const MIN_BLADES: f32 = 3.0;
pub const MAX_BLADES: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Algorithm {
    Box,
    Gaussian,
    Median,
    Motion,
    Bilateral,
    Directional,
    Defocus,
    Kawase,
}

pub const ALGORITHMS: [Algorithm; 8] = [
    Algorithm::Box,
    Algorithm::Gaussian,
    Algorithm::Median,
    Algorithm::Motion,
    Algorithm::Bilateral,
    Algorithm::Directional,
    Algorithm::Defocus,
    Algorithm::Kawase,
];

impl std::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Algorithm::Box => crate::i18n::blur_algorithm_box(),
            Algorithm::Gaussian => crate::i18n::blur_algorithm_gaussian(),
            Algorithm::Median => crate::i18n::blur_algorithm_median(),
            Algorithm::Motion => crate::i18n::blur_algorithm_motion(),
            Algorithm::Bilateral => crate::i18n::blur_algorithm_bilateral(),
            Algorithm::Directional => crate::i18n::blur_algorithm_directional(),
            Algorithm::Defocus => crate::i18n::blur_algorithm_defocus(),
            Algorithm::Kawase => crate::i18n::blur_algorithm_kawase(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub algorithm: Algorithm,
    pub strength: f32,
    pub angle: f32,
    pub detail: f32,
    pub passes: u32,
    pub blades: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::Gaussian,
            strength: 12.0,
            angle: 0.0,
            detail: 0.7,
            passes: 4,
            blades: 6,
        }
    }
}

impl Settings {
    pub fn for_algorithm(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Box => Self {
                algorithm,
                strength: 8.0,
                ..Self::default()
            },
            Algorithm::Gaussian => Self::default(),
            Algorithm::Median => Self {
                algorithm,
                strength: 2.0,
                passes: 1,
                ..Self::default()
            },
            Algorithm::Motion => Self {
                algorithm,
                strength: 20.0,
                ..Self::default()
            },
            Algorithm::Bilateral => Self {
                algorithm,
                strength: 8.0,
                detail: 0.5,
                ..Self::default()
            },
            Algorithm::Directional => Self {
                algorithm,
                strength: 16.0,
                detail: 0.75,
                ..Self::default()
            },
            Algorithm::Defocus => Self {
                algorithm,
                strength: 12.0,
                blades: 6,
                ..Self::default()
            },
            Algorithm::Kawase => Self {
                algorithm,
                strength: 8.0,
                passes: 4,
                ..Self::default()
            },
        }
    }

    pub fn normalised(mut self) -> Self {
        self.strength = self.strength.clamp(MIN_STRENGTH, MAX_STRENGTH);
        self.angle = self.angle.clamp(MIN_ANGLE, MAX_ANGLE);
        self.detail = self.detail.clamp(MIN_DETAIL, MAX_DETAIL);
        self.passes = self.passes.clamp(MIN_PASSES as u32, MAX_PASSES as u32);
        self.blades = self.blades.clamp(MIN_BLADES as u32, MAX_BLADES as u32);
        self
    }

    pub fn cache_key(self) -> [u32; 6] {
        let settings = self.normalised();
        [
            settings.algorithm as u32,
            settings.strength.to_bits(),
            settings.angle.to_bits(),
            settings.detail.to_bits(),
            settings.passes,
            settings.blades,
        ]
    }

    pub fn reach(self) -> u32 {
        let settings = self.normalised();
        match settings.algorithm {
            Algorithm::Box => settings.strength.round() as u32,
            Algorithm::Gaussian => radii(settings.strength).into_iter().sum(),
            Algorithm::Median => settings.strength.ceil() as u32 + settings.passes,
            Algorithm::Motion => (settings.strength / 2.0).ceil() as u32,
            Algorithm::Bilateral => {
                settings.strength.ceil() as u32 + bilateral_softening_radius(settings)
            }
            Algorithm::Directional | Algorithm::Defocus => settings.strength.ceil() as u32,
            Algorithm::Kawase => (0..settings.passes)
                .map(|pass| kawase_offset(settings, pass).ceil() as u32)
                .sum(),
        }
    }
}

pub(crate) fn radii(strength: f32) -> [u32; 3] {
    let sigma = strength.clamp(MIN_STRENGTH, MAX_STRENGTH);
    let ideal = ((12.0 * sigma.powi(2) / 3.0) + 1.0).sqrt();
    let mut lower = ideal.floor();
    if lower % 2.0 == 0.0 {
        lower -= 1.0;
    }
    let upper = lower + 2.0;
    let lower_count = (0.75 * (lower + 3.0) - 3.0 * sigma.powi(2) / (lower + 1.0)).round() as usize;

    std::array::from_fn(|i| {
        let width = if i < lower_count { lower } else { upper } as u32;
        let radius = width.saturating_sub(1) / 2;
        if radius.is_multiple_of(2) {
            radius + 1
        } else {
            radius
        }
    })
}

pub(crate) fn kawase_offset(settings: Settings, pass: u32) -> f32 {
    settings.strength * (pass + 1) as f32 / settings.passes.max(1) as f32
}

pub(crate) fn bilateral_softening_radius(settings: Settings) -> u32 {
    (settings.strength * (1.0 - settings.detail)).round() as u32
}

pub fn render(pixels: &Rgba8, rect: Rect, settings: Settings) -> Option<Rgba8> {
    let settings = settings.normalised();
    let rect = rect.clamped(pixels.width(), pixels.height());
    if rect.is_empty() {
        return None;
    }

    let reach = settings.reach();
    let sample = Rect::new(
        rect.x0.saturating_sub(reach),
        rect.y0.saturating_sub(reach),
        rect.x1.saturating_add(reach).min(pixels.width()),
        rect.y1.saturating_add(reach).min(pixels.height()),
    );
    let mut source = Vec::with_capacity(sample.area() * CHANNELS);
    let stride = pixels.width() as usize * CHANNELS;
    for y in sample.rows() {
        let start = y as usize * stride + sample.x0 as usize * CHANNELS;
        let end = start + sample.width() as usize * CHANNELS;
        source.extend_from_slice(&pixels.as_bytes()[start..end]);
    }

    let source = image::RgbaImage::from_raw(sample.width(), sample.height(), source)?;
    let blurred = apply(source, settings);
    let source_stride = sample.width() as usize * CHANNELS;
    let mut target = Vec::with_capacity(rect.area() * CHANNELS);
    for y in rect.rows() {
        let source_start =
            (y - sample.y0) as usize * source_stride + (rect.x0 - sample.x0) as usize * CHANNELS;
        let span = rect.width() as usize * CHANNELS;
        target.extend_from_slice(&blurred.as_raw()[source_start..source_start + span]);
    }
    Rgba8::from_raw(rect.width(), rect.height(), target)
}

fn apply(source: image::RgbaImage, settings: Settings) -> image::RgbaImage {
    match settings.algorithm {
        Algorithm::Box => {
            let radius = settings.strength.round() as i32;
            box_pass(&box_pass(&source, radius, true), radius, false)
        }
        Algorithm::Gaussian => image::imageops::fast_blur(&source, settings.strength),
        Algorithm::Median => {
            let filtered = (0..settings.passes).fold(source, |image, _| median_pass(&image));
            let radius = settings.strength.round() as i32;
            box_pass(&box_pass(&filtered, radius, true), radius, false)
        }
        Algorithm::Motion => sampled_pass(&source, settings, Sampled::Motion),
        Algorithm::Bilateral => {
            let filtered = bilateral_pass(&source, settings);
            let radius = bilateral_softening_radius(settings) as i32;
            if radius == 0 {
                filtered
            } else {
                box_pass(&box_pass(&filtered, radius, true), radius, false)
            }
        }
        Algorithm::Directional => sampled_pass(&source, settings, Sampled::Directional),
        Algorithm::Defocus => sampled_pass(&source, settings, Sampled::Defocus),
        Algorithm::Kawase => (0..settings.passes).fold(source, |image, pass| {
            kawase_pass(&image, kawase_offset(settings, pass))
        }),
    }
}

fn sample(image: &image::RgbaImage, x: i32, y: i32) -> [f32; 4] {
    let x = x.clamp(0, image.width() as i32 - 1) as u32;
    let y = y.clamp(0, image.height() as i32 - 1) as u32;
    image.get_pixel(x, y).0.map(f32::from)
}

fn rgba(values: [f32; 4]) -> image::Rgba<u8> {
    image::Rgba(values.map(|value| value.round().clamp(0.0, 255.0) as u8))
}

fn box_pass(source: &image::RgbaImage, radius: i32, horizontal: bool) -> image::RgbaImage {
    let mut target = image::RgbaImage::new(source.width(), source.height());
    let lines = if horizontal {
        source.height()
    } else {
        source.width()
    };
    let length = if horizontal {
        source.width()
    } else {
        source.height()
    } as i32;
    let weight = 1.0 / (radius * 2 + 1) as f32;
    for line in 0..lines as i32 {
        let mut sum = [0.0; 4];
        for offset in -radius..=radius {
            let pixel = if horizontal {
                sample(source, offset, line)
            } else {
                sample(source, line, offset)
            };
            for channel in 0..4 {
                sum[channel] += pixel[channel];
            }
        }
        for at in 0..length {
            let pixel = rgba(sum.map(|value| value * weight));
            if horizontal {
                target.put_pixel(at as u32, line as u32, pixel);
            } else {
                target.put_pixel(line as u32, at as u32, pixel);
            }
            let next = if horizontal {
                sample(source, at + radius + 1, line)
            } else {
                sample(source, line, at + radius + 1)
            };
            let previous = if horizontal {
                sample(source, at - radius, line)
            } else {
                sample(source, line, at - radius)
            };
            for channel in 0..4 {
                sum[channel] += next[channel] - previous[channel];
            }
        }
    }
    target
}

fn median_pass(source: &image::RgbaImage) -> image::RgbaImage {
    let mut target = image::RgbaImage::new(source.width(), source.height());
    for y in 0..source.height() as i32 {
        for x in 0..source.width() as i32 {
            let mut values = [[0u8; 9]; 4];
            let mut at = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let pixel = sample(source, x + dx, y + dy);
                    for channel in 0..4 {
                        values[channel][at] = pixel[channel] as u8;
                    }
                    at += 1;
                }
            }
            for channel in &mut values {
                channel.sort_unstable();
            }
            target.put_pixel(
                x as u32,
                y as u32,
                image::Rgba(values.map(|channel| channel[4])),
            );
        }
    }
    target
}

enum Sampled {
    Motion,
    Directional,
    Defocus,
}

fn sampled_pass(source: &image::RgbaImage, settings: Settings, kind: Sampled) -> image::RgbaImage {
    let mut target = image::RgbaImage::new(source.width(), source.height());
    for y in 0..source.height() as i32 {
        for x in 0..source.width() as i32 {
            let pixel = match kind {
                Sampled::Motion => motion_pixel(source, x, y, settings),
                Sampled::Directional => directional_pixel(source, x, y, settings),
                Sampled::Defocus => defocus_pixel(source, x, y, settings),
            };
            target.put_pixel(x as u32, y as u32, rgba(pixel));
        }
    }
    target
}

fn turn(angle: f32, x: f32, y: f32) -> (f32, f32) {
    let radians = angle.to_radians();
    let (sin, cos) = radians.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

fn motion_pixel(source: &image::RgbaImage, x: i32, y: i32, settings: Settings) -> [f32; 4] {
    let mut sum = [0.0; 4];
    for step in 0..17 {
        let along = (step as f32 / 16.0 - 0.5) * settings.strength;
        let (dx, dy) = turn(settings.angle, along, 0.0);
        let pixel = sample(source, x + dx.round() as i32, y + dy.round() as i32);
        for channel in 0..4 {
            sum[channel] += pixel[channel] / 17.0;
        }
    }
    sum
}

fn directional_pixel(source: &image::RgbaImage, x: i32, y: i32, settings: Settings) -> [f32; 4] {
    let major = settings.strength / 2.0;
    let minor = major * (1.0 - settings.detail).max(0.05);
    let mut sum = [0.0; 4];
    let mut total = 0.0;
    for v in -2i32..=2 {
        for u in -2i32..=2 {
            let weight = (-((u * u + v * v) as f32) / 4.0).exp();
            let (dx, dy) = turn(
                settings.angle,
                u as f32 * major / 2.0,
                v as f32 * minor / 2.0,
            );
            let pixel = sample(source, x + dx.round() as i32, y + dy.round() as i32);
            for channel in 0..4 {
                sum[channel] += pixel[channel] * weight;
            }
            total += weight;
        }
    }
    sum.map(|value| value / total)
}

fn defocus_pixel(source: &image::RgbaImage, x: i32, y: i32, settings: Settings) -> [f32; 4] {
    let mut sum = sample(source, x, y);
    let rotation = settings.angle.to_radians();
    let blades = settings.blades as f32;
    for sample_index in 0..24 {
        let ring = if sample_index < 8 { 0.5 } else { 1.0 };
        let around = if sample_index < 8 { 8.0 } else { 16.0 };
        let index = if sample_index < 8 {
            sample_index
        } else {
            sample_index - 8
        };
        let angle = index as f32 * std::f32::consts::TAU / around + rotation;
        let sector = (angle * blades / std::f32::consts::TAU + 0.5).rem_euclid(1.0) - 0.5;
        let aperture =
            (std::f32::consts::PI / blades).cos() / (sector * std::f32::consts::TAU / blades).cos();
        let radius = settings.strength * ring * aperture;
        let pixel = sample(
            source,
            x + (angle.cos() * radius).round() as i32,
            y + (angle.sin() * radius).round() as i32,
        );
        for channel in 0..4 {
            sum[channel] += pixel[channel];
        }
    }
    sum.map(|value| value / 25.0)
}

fn bilateral_pass(source: &image::RgbaImage, settings: Settings) -> image::RgbaImage {
    let mut target = image::RgbaImage::new(source.width(), source.height());
    let spacing = settings.strength / 2.0;
    let looseness = 1.0 - settings.detail;
    let range = 12.0 + looseness * looseness * 500.0;
    for y in 0..source.height() as i32 {
        for x in 0..source.width() as i32 {
            let centre = sample(source, x, y);
            let mut sum = [0.0; 4];
            let mut total = 0.0;
            for v in -2i32..=2 {
                for u in -2i32..=2 {
                    let pixel = sample(
                        source,
                        x + (u as f32 * spacing).round() as i32,
                        y + (v as f32 * spacing).round() as i32,
                    );
                    let spatial = (-((u * u + v * v) as f32) / 4.0).exp();
                    let difference = ((pixel[0] - centre[0]).powi(2)
                        + (pixel[1] - centre[1]).powi(2)
                        + (pixel[2] - centre[2]).powi(2))
                    .sqrt();
                    let weight =
                        spatial * (-(difference * difference) / (2.0 * range * range)).exp();
                    for channel in 0..4 {
                        sum[channel] += pixel[channel] * weight;
                    }
                    total += weight;
                }
            }
            target.put_pixel(x as u32, y as u32, rgba(sum.map(|value| value / total)));
        }
    }
    target
}

fn kawase_pass(source: &image::RgbaImage, offset: f32) -> image::RgbaImage {
    let mut target = image::RgbaImage::new(source.width(), source.height());
    let offset = offset.round() as i32;
    for y in 0..source.height() as i32 {
        for x in 0..source.width() as i32 {
            let points = [
                sample(source, x - offset, y - offset),
                sample(source, x + offset, y - offset),
                sample(source, x - offset, y + offset),
                sample(source, x + offset, y + offset),
            ];
            let mut sum = [0.0; 4];
            for pixel in points {
                for channel in 0..4 {
                    sum[channel] += pixel[channel] * 0.25;
                }
            }
            target.put_pixel(x as u32, y as u32, rgba(sum));
        }
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split() -> Rgba8 {
        let mut pixels = Rgba8::new(41, 21, [0, 0, 0, 255]);
        for y in 0..21 {
            for x in 20..41 {
                let i = (y * 41 + x) as usize * CHANNELS;
                pixels.pixels_mut()[i..i + CHANNELS].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        pixels
    }

    fn texture() -> Rgba8 {
        let mut pixels = Rgba8::new(129, 129, [0, 0, 0, 255]);
        for y in 0..129 {
            for x in 0..129 {
                let value = ((x * 73) ^ (y * 151) ^ (x * y * 13)) as u8;
                let i = (y * 129 + x) as usize * CHANNELS;
                pixels.pixels_mut()[i..i + CHANNELS].copy_from_slice(&[
                    value,
                    value.wrapping_mul(3),
                    value.wrapping_mul(7),
                    255,
                ]);
            }
        }
        pixels
    }

    fn detail(pixels: &Rgba8) -> u64 {
        let mut total = 0;
        for y in 0..pixels.height() {
            for x in 1..pixels.width() {
                let i = (y * pixels.width() + x) as usize * CHANNELS;
                total += pixels.as_bytes()[i].abs_diff(pixels.as_bytes()[i - CHANNELS]) as u64;
            }
        }
        total
    }

    #[test]
    fn every_algorithm_renders_without_touching_its_source() {
        for algorithm in ALGORITHMS {
            let pixels = split();
            let before = pixels.clone();
            let settings = Settings {
                algorithm,
                ..Settings::default()
            };
            let blurred = render(&pixels, Rect::new(5, 2, 36, 19), settings).unwrap();
            assert_eq!(pixels, before, "{algorithm} changed its source");
            assert_eq!(blurred.size(), (31, 17), "{algorithm} changed the box");
        }
    }

    #[test]
    fn gpu_pass_radii_follow_the_fast_blur_kernel() {
        assert_eq!(radii(1.0), [1, 1, 1]);
        assert_eq!(radii(12.0), [11, 11, 13]);
        assert_eq!(radii(50.0), [49, 49, 51]);
    }

    #[test]
    fn defaults_are_ready_for_quick_gaussian_blur() {
        let settings = Settings::default();
        assert_eq!(settings.algorithm, Algorithm::Gaussian);
        assert_eq!(settings.strength, 12.0);
        assert_eq!(settings, settings.normalised());
    }

    #[test]
    fn edge_preserving_blurs_can_still_reach_an_extreme() {
        let pixels = texture();
        let rect = Rect::new(0, 0, pixels.width(), pixels.height());
        for algorithm in [Algorithm::Median, Algorithm::Bilateral] {
            let subtle = Settings::for_algorithm(algorithm);
            let extreme = Settings {
                strength: MAX_STRENGTH,
                detail: MIN_DETAIL,
                passes: MAX_PASSES as u32,
                ..subtle
            };
            let subtle = render(&pixels, rect, subtle).unwrap();
            let extreme = render(&pixels, rect, extreme).unwrap();
            let subtle_detail = detail(&subtle);
            let extreme_detail = detail(&extreme);
            assert!(
                extreme_detail < subtle_detail / 2,
                "{algorithm:?} did not span from subtle ({subtle_detail}) to extreme ({extreme_detail})"
            );
        }
    }
}
