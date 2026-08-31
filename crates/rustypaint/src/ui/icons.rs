use iced::Color;

macro_rules! ui_icon {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../res/icons/ui/",
            $name,
            ".svg"
        ))
    };
}

pub const MENU: &[u8] = ui_icon!("menu");
pub const OPEN: &[u8] = ui_icon!("open");
pub const SAVE: &[u8] = ui_icon!("save");
pub const SAVE_AS: &[u8] = ui_icon!("save-as");
pub const UNDO: &[u8] = ui_icon!("undo");
pub const REDO: &[u8] = ui_icon!("redo");

pub const ZOOM_IN: &[u8] = ui_icon!("zoom-in");
pub const ZOOM_OUT: &[u8] = ui_icon!("zoom-out");
pub const FIT: &[u8] = ui_icon!("fit");

pub const BRUSHES: &[u8] = ui_icon!("brushes");
pub const SHAPES_2D: &[u8] = ui_icon!("shapes");
pub const STICKERS: &[u8] = ui_icon!("stickers");
pub const TEXT: &[u8] = ui_icon!("text");
pub const CANVAS: &[u8] = ui_icon!("canvas");

pub const SELECT: &[u8] = ui_icon!("select");
pub const SMART_CUTOUT: &[u8] = ui_icon!("smart-cutout");
pub const CROP: &[u8] = ui_icon!("crop");

pub const ROTATE: &[u8] = ui_icon!("rotate");
pub const ROTATE_ANTICLOCKWISE: &[u8] = ui_icon!("rotate-anticlockwise");
pub const FLIP_HORIZONTAL: &[u8] = ui_icon!("flip-horizontal");
pub const FLIP_VERTICAL: &[u8] = ui_icon!("flip-vertical");
pub const PIPETTE: &[u8] = ui_icon!("pipette");

pub const BACK: &[u8] = ui_icon!("back");
pub const NEW: &[u8] = ui_icon!("new");
pub const INSERT: &[u8] = ui_icon!("insert");
pub const SETTINGS: &[u8] = ui_icon!("settings");
pub const ABOUT: &[u8] = ui_icon!("about");
pub const LINK: &[u8] = ui_icon!("link");
pub const IMAGE: &[u8] = ui_icon!("image");

pub fn for_tool(tool: crate::paint::Tool) -> &'static [u8] {
    use crate::assets::tool_icons as art;
    use crate::paint::Tool;
    match tool {
        Tool::Marker => art::MARKER,
        Tool::Calligraphy => art::CALLIGRAPHY,
        Tool::OilBrush => art::OIL_BRUSH,
        Tool::Watercolour => art::WATERCOLOUR,
        Tool::PixelPen => art::PIXEL_PEN,
        Tool::Pencil => art::PENCIL,
        Tool::Eraser => art::ERASER,
        Tool::Crayon => art::CRAYON,
        Tool::SprayCan => art::SPRAY_CAN,
        Tool::Fill => art::FILL,
        Tool::Pipette => PIPETTE,
        Tool::Text => TEXT,
        Tool::Select | Tool::Shape => SELECT,
    }
}

pub fn art<'a>(
    bytes: &'static [u8],
    size: f32,
    colour: Option<Color>,
) -> iced::Element<'a, crate::app::Message> {
    iced::widget::svg(iced::widget::svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |_theme, _status| iced::widget::svg::Style { color: colour })
        .into()
}

pub fn for_curve(kind: crate::paint::curve::CurveKind) -> &'static [u8] {
    use crate::assets::curve_icons as art;
    use crate::paint::curve::CurveKind;
    match kind {
        CurveKind::Line => art::LINE,
        CurveKind::Curve3 => art::CURVE_3,
        CurveKind::Curve4 => art::CURVE_4,
        CurveKind::Curve5 => art::CURVE_5,
    }
}

pub fn icon<'a>(
    drawing: &'static [u8],
    size: f32,
    colour: Color,
) -> iced::Element<'a, crate::app::Message> {
    art(drawing, size, Some(colour))
}

#[cfg(test)]
mod tests {
    fn all_tool_icons() -> Vec<(&'static str, &'static [u8])> {
        crate::paint::brush::PANEL_ORDER
            .iter()
            .map(|t| (t.name(), for_tool(*t)))
            .collect()
    }

    #[test]
    fn every_tool_icon_parses_and_draws_something() {
        for (name, bytes) in all_tool_icons() {
            let tree = usvg::Tree::from_data(bytes, &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
            let size = tree.size();
            assert!(
                size.width() > 0.0 && size.height() > 0.0,
                "{name} has no size"
            );
        }
    }

    #[test]
    fn every_drawing_parses_and_covers_some_of_its_box() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../res");
        let mut checked = 0;

        for entry in walk(&root) {
            let name = entry.file_name().unwrap().to_string_lossy().to_string();
            let bytes = std::fs::read(&entry).expect("the file we just listed");
            let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
            let size = tree.size();
            assert!(
                size.width() > 0.0 && size.height() > 0.0,
                "{name} has no size"
            );

            let bounds = tree.root().abs_stroke_bounding_box();
            assert!(
                bounds.width().max(bounds.height()) > size.width() / 2.0,
                "{name} covers almost none of its box: {bounds:?}"
            );
            assert!(
                bounds.left() >= -1.0
                    && bounds.top() >= -1.0
                    && bounds.right() <= size.width() + 1.0,
                "{name} hangs outside its box: {bounds:?}"
            );
            checked += 1;
        }

        assert!(
            checked > 30,
            "only found {checked} drawings, which is not all of them"
        );
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else if path.extension().is_some_and(|e| e == "svg")
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("icon"))
            {
                out.push(path);
            }
        }
        out
    }

    #[test]
    fn every_brush_icon_carries_its_own_colours() {
        for (name, bytes) in all_tool_icons() {
            let svg = str::from_utf8(bytes).expect("icons are text");
            let fills = svg.matches("fill=\"#").count() + svg.matches("stop-color=\"#").count();
            assert!(fills >= 2, "{name} came out with only {fills} fill(s)");
            assert!(
                !svg.contains("currentColor"),
                "{name} would take the theme's ink, and these are meant to keep their own"
            );
        }
    }

    use super::*;
}
