pub mod detect;

use iced::Color;
use std::sync::atomic::{AtomicU8, Ordering};

const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

const fn rgba(hex: u32, a: f32) -> Color {
    Color { a, ..rgb(hex) }
}

#[allow(dead_code, reason = "reference table, filled in ahead of the widgets")]
pub mod metrics {
    pub const TOP_PANEL_BUTTON_WIDTH: f32 = 68.0;
    pub const TOP_PANEL_BUTTON_HEIGHT: f32 = 48.0;
    pub const TOP_PANEL_BUTTON_MAX_EXPANDED_HEIGHT: f32 = 74.0;
    pub const TOP_PANEL_THIN_BUTTON_WIDTH: f32 = 48.0;
    pub const TOP_PANEL_ICON_GRID_SIZE: f32 = 48.0;
    pub const TOP_PANEL_HISTORY_COLLAPSED_DROP_DOWN_WIDTH: f32 = 32.0;
    pub const TOP_PANEL_HISTORY_FLYOUT_MARGIN: [f32; 4] = [0.0, -6.0, 0.0, 0.0];

    pub const GLOBAL_TOOLS_TOP_BAR_HEIGHT: f32 = 48.0;
    pub const GLOBAL_TOOLS_TOP_BAR_BUTTON_HEIGHT: f32 = 32.0;
    pub const GLOBAL_TOOLS_TOP_BAR_SINGLE_ICON_BUTTON_WIDTH: f32 = 48.0;

    pub const SIDE_PANEL_WIDTH: f32 = 264.0;
    pub const SIDE_PANEL_VERTICAL_OFFSET: f32 = 100.0;
    pub const SIDE_PANEL_GUTTER_MARGIN: [f32; 4] = [24.0, 0.0, 24.0, 0.0];

    pub const SHAPE_WIDTH: f32 = 40.0;
    pub const SHAPE_HEIGHT: f32 = 40.0;

    pub const SWATCH_SIZE: f32 = 36.0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Choice {
    pub const ALL: [Choice; 3] = [Choice::Auto, Choice::Light, Choice::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Choice::Auto => "Auto",
            Choice::Light => "Light",
            Choice::Dark => "Dark",
        }
    }

    pub fn resolve(self) -> Mode {
        match self {
            Choice::Auto => detect::system().unwrap_or(Mode::Light),
            Choice::Light => Mode::Light,
            Choice::Dark => Mode::Dark,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Classic,
    #[default]
    Rusty,
}

impl Scheme {
    pub const ALL: [Scheme; 2] = [Scheme::Classic, Scheme::Rusty];

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Classic => "Classic",
            Scheme::Rusty => "Rusty",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Surfaces {
    side_panel: Color,
    tool_bar: Color,
    top_bar: Color,
    control: Color,
    control_hover: Color,
    menu_rail: Color,
    workspace_top: Color,
    workspace_bottom: Color,
    checker_light: Color,
    checker_dark: Color,
    shadow: f32,
    text: Color,
    text_dim: Color,
    border: Color,
    text_on_dark: Color,
    title_bar: Color,
    tab_bar: Color,
    overlay: Color,
}

#[derive(Debug, Clone, Copy)]
struct Accent {
    fill: Color,
    label: Color,
    from: Color,
    to: Color,
    on: Color,
}

#[allow(dead_code, reason = "reference table, filled in ahead of the widgets")]
pub struct Palette {
    pub side_panel: Color,
    pub tool_bar: Color,
    pub top_bar: Color,
    pub control: Color,
    pub control_hover: Color,
    pub menu_rail: Color,
    pub workspace_top: Color,
    pub workspace_bottom: Color,
    pub checker_light: Color,
    pub checker_dark: Color,
    pub shadow: f32,
    pub text: Color,
    pub text_dim: Color,
    pub border: Color,
    pub text_on_dark: Color,
    pub title_bar: Color,
    pub tab_bar: Color,
    pub overlay: Color,

    pub accent: Color,
    pub accent_text: Color,
    pub selection_from: Color,
    pub selection_to: Color,
    pub selection_text: Color,

    pub canvas: Color,
}

const LIGHT: Surfaces = Surfaces {
    side_panel: rgb(0xf0f2f3),
    tool_bar: rgb(0xf0f2f3),
    top_bar: rgb(0x363941),
    control: rgb(0xf4f6f7),
    control_hover: rgb(0xdfe3e5),
    menu_rail: rgb(0xe9eced),
    workspace_top: rgb(0xaeb1af),
    workspace_bottom: rgb(0xc0c1c0),
    checker_light: rgb(0xbabcba),
    checker_dark: rgb(0xb4b6b4),
    shadow: 0.22,
    text: rgb(0x1a1a1a),
    text_dim: rgb(0x6b6b6b),
    border: rgb(0xbebebe),
    text_on_dark: rgb(0xffffff),
    title_bar: rgb(0x454242),
    tab_bar: rgb(0x514f4f),
    overlay: rgba(0xffffff, 0.62),
};

const DARK: Surfaces = Surfaces {
    side_panel: rgb(0x2a2d33),
    tool_bar: rgb(0x2a2d33),
    top_bar: rgb(0x1f2126),
    control: rgb(0x34383f),
    control_hover: rgb(0x3f444c),
    menu_rail: rgb(0x24262b),
    workspace_top: rgb(0x17181b),
    workspace_bottom: rgb(0x1e2023),
    checker_light: rgb(0x1d1f22),
    checker_dark: rgb(0x17191c),
    shadow: 0.42,
    text: rgb(0xececee),
    text_dim: rgb(0x9aa0a8),
    border: rgb(0x44484f),
    text_on_dark: rgb(0xffffff),
    title_bar: rgb(0x1a1c20),
    tab_bar: rgb(0x232529),
    overlay: rgba(0x000000, 0.62),
};

const CLASSIC_LIGHT: Accent = Accent {
    fill: rgb(0x0064b6),
    label: rgb(0x0064b6),
    from: rgb(0x0064b6),
    to: rgb(0x6653e6),
    on: rgb(0xffffff),
};

const CLASSIC_DARK: Accent = Accent {
    fill: rgb(0x3f9ae8),
    label: rgb(0x3f9ae8),
    from: rgb(0x0064b6),
    to: rgb(0x6653e6),
    on: rgb(0xffffff),
};

const RUSTY_LIGHT: Accent = Accent {
    fill: rgb(0xfea845),
    label: rgb(0xa85a0a),
    from: rgb(0xfea845),
    to: rgb(0xffc21a),
    on: rgb(0x1a1a1a),
};

const RUSTY_DARK: Accent = Accent {
    fill: rgb(0xfea845),
    label: rgb(0xfea845),
    from: rgb(0xfea845),
    to: rgb(0xffc21a),
    on: rgb(0x1a1a1a),
};

const fn merge(s: Surfaces, a: Accent) -> Palette {
    Palette {
        side_panel: s.side_panel,
        tool_bar: s.tool_bar,
        top_bar: s.top_bar,
        control: s.control,
        control_hover: s.control_hover,
        menu_rail: s.menu_rail,
        workspace_top: s.workspace_top,
        workspace_bottom: s.workspace_bottom,
        checker_light: s.checker_light,
        checker_dark: s.checker_dark,
        shadow: s.shadow,
        text: s.text,
        text_dim: s.text_dim,
        border: s.border,
        text_on_dark: s.text_on_dark,
        title_bar: s.title_bar,
        tab_bar: s.tab_bar,
        overlay: s.overlay,
        accent: a.fill,
        accent_text: a.label,
        selection_from: a.from,
        selection_to: a.to,
        selection_text: a.on,
        canvas: rgb(0xffffff),
    }
}

static PALETTES: [[Palette; 2]; 2] = [
    [merge(LIGHT, CLASSIC_LIGHT), merge(LIGHT, RUSTY_LIGHT)],
    [merge(DARK, CLASSIC_DARK), merge(DARK, RUSTY_DARK)],
];

static MODE: AtomicU8 = AtomicU8::new(Mode::Light as u8);
static SCHEME: AtomicU8 = AtomicU8::new(Scheme::Rusty as u8);
static ACRYLIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const VEIL: f32 = 0.85;

pub fn set_acrylic(on: bool) {
    ACRYLIC.store(on, Ordering::Relaxed);
}

pub fn veiled(colour: Color) -> Color {
    if ACRYLIC.load(Ordering::Relaxed) {
        Color { a: VEIL, ..colour }
    } else {
        colour
    }
}

pub fn colours() -> &'static Palette {
    palette_for(mode(), scheme())
}

pub fn palette_for(mode: Mode, scheme: Scheme) -> &'static Palette {
    &PALETTES[mode as usize][scheme as usize]
}

pub fn mode() -> Mode {
    if MODE.load(Ordering::Relaxed) == Mode::Dark as u8 {
        Mode::Dark
    } else {
        Mode::Light
    }
}

pub fn scheme() -> Scheme {
    if SCHEME.load(Ordering::Relaxed) == Scheme::Classic as u8 {
        Scheme::Classic
    } else {
        Scheme::Rusty
    }
}

pub fn set_theme(mode: Mode, scheme: Scheme) {
    MODE.store(mode as u8, Ordering::Relaxed);
    SCHEME.store(scheme as u8, Ordering::Relaxed);
}

#[rustfmt::skip]
pub const SWATCHES: [Color; 18] = [
    rgb(0xffffff), rgb(0xc3c3c3), rgb(0x585858), rgb(0x000000), rgb(0x88001b), rgb(0xec1c24),
    rgb(0xff7f27), rgb(0xffca18), rgb(0xfdeca6), rgb(0xfff200), rgb(0xc4ff0e), rgb(0x0ed145),
    rgb(0x8cfffb), rgb(0x00a8f3), rgb(0x3f48cc), rgb(0xb83dba), rgb(0xffaec8), rgb(0xb97a56),
];

pub fn selection_wash() -> iced::Background {
    let c = colours();
    iced::Background::Gradient(iced::Gradient::Linear(
        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
            .add_stop(0.0, c.selection_from)
            .add_stop(1.0, c.selection_to),
    ))
}

pub const CHECKER_SQUARE: f32 = 8.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color) -> f32 {
        fn channel(v: f32) -> f32 {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }

    pub(super) fn contrast(a: Color, b: Color) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_label_reads_against_what_it_sits_on() {
        for mode in [Mode::Light, Mode::Dark] {
            for scheme in Scheme::ALL {
                let c = palette_for(mode, scheme);
                for (name, fg, bg) in [
                    ("text on panel", c.text, c.side_panel),
                    ("dim on panel", c.text_dim, c.side_panel),
                    ("accent label on panel", c.accent_text, c.side_panel),
                    ("text on control", c.text, c.control),
                    ("text on rail", c.text, c.menu_rail),
                    ("top bar text", c.text_on_dark, c.top_bar),
                    ("wash text, left", c.selection_text, c.selection_from),
                    ("wash text, right", c.selection_text, c.selection_to),
                ] {
                    let ratio = contrast(fg, bg);
                    assert!(ratio >= 4.5, "{mode:?} {scheme:?}: {name} is {ratio:.2}:1");
                }
            }
        }
    }

    #[test]
    fn light_and_classic_is_what_it_always_was() {
        let c = palette_for(Mode::Light, Scheme::Classic);
        assert_eq!(c.side_panel, rgb(0xf0f2f3));
        assert_eq!(c.tool_bar, rgb(0xf0f2f3));
        assert_eq!(c.top_bar, rgb(0x363941));
        assert_eq!(c.control, rgb(0xf4f6f7));
        assert_eq!(c.control_hover, rgb(0xdfe3e5));
        assert_eq!(c.menu_rail, rgb(0xe9eced));
        assert_eq!(c.workspace_top, rgb(0xaeb1af));
        assert_eq!(c.workspace_bottom, rgb(0xc0c1c0));
        assert_eq!(c.checker_light, rgb(0xbabcba));
        assert_eq!(c.checker_dark, rgb(0xb4b6b4));
        assert_eq!(c.accent, rgb(0x0064b6));
        assert_eq!(c.selection_from, rgb(0x0064b6));
        assert_eq!(c.selection_to, rgb(0x6653e6));
        assert_eq!(c.text, rgb(0x1a1a1a));
        assert_eq!(c.text_dim, rgb(0x6b6b6b));
        assert_eq!(c.border, rgb(0xbebebe));
        assert_eq!(c.shadow, 0.22);
    }

    #[test]
    fn the_overlay_washes_towards_the_mode_it_dims() {
        assert!(palette_for(Mode::Light, Scheme::Rusty).overlay.r > 0.9);
        assert!(palette_for(Mode::Dark, Scheme::Rusty).overlay.r < 0.1);
    }

    #[test]
    fn the_canvas_ignores_the_mode() {
        let light = palette_for(Mode::Light, Scheme::Rusty);
        let dark = palette_for(Mode::Dark, Scheme::Rusty);
        assert_eq!(light.canvas, dark.canvas);
        assert_ne!(light.checker_light, dark.checker_light);
    }

    #[test]
    fn the_accent_is_the_only_thing_the_scheme_moves() {
        let classic = palette_for(Mode::Dark, Scheme::Classic);
        let rusty = palette_for(Mode::Dark, Scheme::Rusty);
        assert_eq!(classic.side_panel, rusty.side_panel);
        assert_eq!(classic.text, rusty.text);
        assert_ne!(classic.accent, rusty.accent);
        assert_ne!(classic.selection_to, rusty.selection_to);
    }

    #[test]
    fn the_dark_shadow_is_the_heavier_one() {
        assert!(
            palette_for(Mode::Dark, Scheme::Rusty).shadow
                > palette_for(Mode::Light, Scheme::Rusty).shadow
        );
    }
}
