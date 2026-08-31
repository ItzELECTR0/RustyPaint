pub const UI_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/fonts/Urbanist[wght].ttf"
));

pub const UI_FONT_FAMILY: &str = "Urbanist";

pub fn ui_font() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Medium,
        ..iced::Font::with_name(UI_FONT_FAMILY)
    }
}

pub const APP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/icon-256.png"
));

pub const APP_ICON_SVG: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../res/icon.svg"));

pub const LASSO_SVG: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../res/lasso.svg"));

pub const ALIGN_LEFT_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/align-left.svg"
));
pub const ALIGN_CENTRE_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/align-centre.svg"
));
pub const ALIGN_RIGHT_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/align-right.svg"
));

pub const WINDOW_MINIMISE_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/window-minimise.svg"
));
pub const WINDOW_MAXIMISE_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/window-maximise.svg"
));
pub const WINDOW_CLOSE_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/window-close.svg"
));

pub const STICKER_SLOT_SVG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/sticker-slot.svg"
));

pub mod tool_icons {
    macro_rules! tool_icon {
        ($name:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../res/icons/tools/",
                $name,
                ".svg"
            ))
        };
    }

    pub const MARKER: &[u8] = tool_icon!("marker");
    pub const CALLIGRAPHY: &[u8] = tool_icon!("calligraphy");
    pub const OIL_BRUSH: &[u8] = tool_icon!("oil-brush");
    pub const WATERCOLOUR: &[u8] = tool_icon!("watercolour");
    pub const PIXEL_PEN: &[u8] = tool_icon!("pixel-pen");
    pub const PENCIL: &[u8] = tool_icon!("pencil");
    pub const ERASER: &[u8] = tool_icon!("eraser");
    pub const CRAYON: &[u8] = tool_icon!("crayon");
    pub const SPRAY_CAN: &[u8] = tool_icon!("spray-can");
    pub const FILL: &[u8] = tool_icon!("fill");
}

pub mod curve_icons {
    macro_rules! curve_icon {
        ($name:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../res/icons/curves/",
                $name,
                ".svg"
            ))
        };
    }

    pub const LINE: &[u8] = curve_icon!("line");
    pub const CURVE_3: &[u8] = curve_icon!("curve-3");
    pub const CURVE_4: &[u8] = curve_icon!("curve-4");
    pub const CURVE_5: &[u8] = curve_icon!("curve-5");
}

macro_rules! shape {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../res/shapes/",
            $name,
            ".svg"
        ))
    };
}
pub(crate) use shape;
