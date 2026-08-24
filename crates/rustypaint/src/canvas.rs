use iced::Size;

pub const MAX_CANVAS: u32 = 20_000;

pub const MIN_CANVAS: u32 = 16;

const MARGIN: f32 = 24.0;

const USABLE: f32 = 96.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewCanvas {
    Fit(Ratio),
    Fixed(u32, u32),
    Custom(u32, u32),
}

impl Default for NewCanvas {
    fn default() -> Self {
        NewCanvas::Fit(Ratio::Widescreen)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ratio {
    Widescreen,
    Standard,
    Photo,
    Square,
    Portrait,
}

impl Ratio {
    pub const ALL: [Ratio; 5] = [
        Ratio::Widescreen,
        Ratio::Standard,
        Ratio::Photo,
        Ratio::Square,
        Ratio::Portrait,
    ];

    pub fn shape(self) -> (f32, f32) {
        match self {
            Ratio::Widescreen => (16.0, 9.0),
            Ratio::Standard => (4.0, 3.0),
            Ratio::Photo => (3.0, 2.0),
            Ratio::Square => (1.0, 1.0),
            Ratio::Portrait => (9.0, 16.0),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ratio::Widescreen => "16:9",
            Ratio::Standard => "4:3",
            Ratio::Photo => "3:2",
            Ratio::Square => "1:1",
            Ratio::Portrait => "9:16",
        }
    }
}

pub const RESOLUTIONS: &[(&str, u32, u32)] = &[
    ("1280 x 720", 1280, 720),
    ("1920 x 1080", 1920, 1080),
    ("3840 x 2160", 3840, 2160),
    ("1080 x 1080", 1080, 1080),
    ("A4 at 300 dpi", 2480, 3508),
    ("Letter at 300 dpi", 2550, 3300),
];

pub fn size_for(preset: NewCanvas, viewport: Size, fallback: (u32, u32)) -> (u32, u32) {
    match preset {
        NewCanvas::Fixed(w, h) | NewCanvas::Custom(w, h) => (clamp(w as f32), clamp(h as f32)),
        NewCanvas::Fit(ratio) => fit(ratio, viewport).unwrap_or(fallback),
    }
}

fn fit(ratio: Ratio, viewport: Size) -> Option<(u32, u32)> {
    if !viewport.width.is_finite() || !viewport.height.is_finite() {
        return None;
    }
    let room = Size::new(
        viewport.width - MARGIN * 2.0,
        viewport.height - MARGIN * 2.0,
    );
    if room.width < USABLE || room.height < USABLE {
        return None;
    }

    let (rw, rh) = ratio.shape();
    let scale = (room.width / rw).min(room.height / rh);
    Some((even(rw * scale), even(rh * scale)))
}

fn even(v: f32) -> u32 {
    let clamped = clamp(v);
    if clamped.is_multiple_of(2) {
        clamped
    } else {
        clamped.saturating_sub(1).max(MIN_CANVAS)
    }
}

fn clamp(v: f32) -> u32 {
    if !v.is_finite() {
        return MIN_CANVAS;
    }
    (v.round().max(0.0) as u32).clamp(MIN_CANVAS, MAX_CANVAS)
}

pub fn describe(preset: NewCanvas, viewport: Size, fallback: (u32, u32)) -> String {
    let (w, h) = size_for(preset, viewport, fallback);
    match preset {
        NewCanvas::Fit(ratio) => {
            format!("{} px, {} fitted to the window", size(w, h), ratio.name())
        }
        _ => format!("{} px", size(w, h)),
    }
}

fn size(w: u32, h: u32) -> String {
    format!("{w} x {h}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FALLBACK: (u32, u32) = (1152, 648);

    fn viewport(w: f32, h: f32) -> Size {
        Size::new(w, h)
    }

    #[test]
    fn a_fixed_size_is_itself() {
        assert_eq!(
            size_for(
                NewCanvas::Fixed(1920, 1080),
                viewport(400.0, 300.0),
                FALLBACK
            ),
            (1920, 1080)
        );
        assert_eq!(
            size_for(
                NewCanvas::Custom(37, 4001),
                viewport(400.0, 300.0),
                FALLBACK
            ),
            (37, 4001)
        );
    }

    #[test]
    fn a_fit_touches_the_axis_that_runs_out_first() {
        let (w, h) = size_for(
            NewCanvas::Fit(Ratio::Widescreen),
            viewport(4000.0, 648.0),
            FALLBACK,
        );
        assert_eq!(h, 600, "the height should fill the room less both margins");
        assert!(
            (w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.01,
            "got {w} x {h}"
        );

        let (w, h) = size_for(
            NewCanvas::Fit(Ratio::Widescreen),
            viewport(848.0, 4000.0),
            FALLBACK,
        );
        assert_eq!(w, 800);
        assert!(
            (w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.01,
            "got {w} x {h}"
        );
    }

    #[test]
    fn every_ratio_comes_out_the_shape_it_says() {
        for ratio in Ratio::ALL {
            let (w, h) = size_for(NewCanvas::Fit(ratio), viewport(1400.0, 1000.0), FALLBACK);
            let (rw, rh) = ratio.shape();
            let got = w as f32 / h as f32;
            assert!((got - rw / rh).abs() < 0.02, "{ratio:?} gave {w} x {h}");
            assert!(
                w <= 1400 - 48 && h <= 1000 - 48,
                "{ratio:?} overflowed at {w} x {h}"
            );
        }
    }

    #[test]
    fn a_fit_is_always_even() {
        for width in [500.0, 501.0, 733.0, 1279.0] {
            let (w, h) = size_for(
                NewCanvas::Fit(Ratio::Photo),
                viewport(width, 900.0),
                FALLBACK,
            );
            assert_eq!(w % 2, 0, "{w} is odd");
            assert_eq!(h % 2, 0, "{h} is odd");
        }
    }

    #[test]
    fn a_window_too_small_to_fit_into_falls_back() {
        assert_eq!(
            size_for(NewCanvas::Fit(Ratio::Square), viewport(0.0, 0.0), FALLBACK),
            FALLBACK
        );
        assert_eq!(
            size_for(
                NewCanvas::Fit(Ratio::Square),
                viewport(60.0, 60.0),
                FALLBACK
            ),
            FALLBACK
        );
        assert_eq!(
            size_for(
                NewCanvas::Fit(Ratio::Square),
                Size::new(f32::NAN, 800.0),
                FALLBACK
            ),
            FALLBACK
        );
    }

    #[test]
    fn nothing_gets_out_of_bounds() {
        assert_eq!(
            size_for(NewCanvas::Custom(0, 0), viewport(800.0, 600.0), FALLBACK),
            (16, 16)
        );
        assert_eq!(
            size_for(
                NewCanvas::Custom(999_999, 999_999),
                viewport(800.0, 600.0),
                FALLBACK
            ),
            (MAX_CANVAS, MAX_CANVAS)
        );
        let (w, h) = size_for(
            NewCanvas::Fit(Ratio::Square),
            viewport(90_000.0, 90_000.0),
            FALLBACK,
        );
        assert!(w <= MAX_CANVAS && h <= MAX_CANVAS, "{w} x {h}");
    }

    #[test]
    fn the_default_is_what_the_document_already_was() {
        assert_eq!(NewCanvas::default(), NewCanvas::Fit(Ratio::Widescreen));
    }

    #[test]
    fn the_readout_says_which_way_the_size_was_arrived_at() {
        let fitted = describe(
            NewCanvas::Fit(Ratio::Widescreen),
            viewport(1400.0, 1000.0),
            FALLBACK,
        );
        assert!(
            fitted.contains("16:9") && fitted.contains("fitted"),
            "{fitted}"
        );
        let fixed = describe(
            NewCanvas::Fixed(1920, 1080),
            viewport(1400.0, 1000.0),
            FALLBACK,
        );
        assert_eq!(fixed, "1920 x 1080 px");
    }

    #[test]
    fn a_preset_round_trips_through_the_settings_file() {
        for preset in [
            NewCanvas::Fit(Ratio::Portrait),
            NewCanvas::Fixed(3840, 2160),
            NewCanvas::Custom(800, 600),
        ] {
            let text = toml::to_string(&Wrapper { new_canvas: preset }).unwrap();
            let back: Wrapper = toml::from_str(&text).unwrap();
            assert_eq!(back.new_canvas, preset, "{text}");
        }
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Wrapper {
        new_canvas: NewCanvas,
    }
}
