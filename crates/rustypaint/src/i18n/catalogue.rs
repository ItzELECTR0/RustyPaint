#![allow(dead_code, reason = "reference table, filled in ahead of the widgets")]

use super::lookup;
use crate::tr;

// One accessor per message the interface can show, so a key that is not in the
// catalogue is a compile error rather than a blank label.
macro_rules! catalogue {
    (
        plain { $($name:ident => $key:literal),* $(,)? }
        formatted { $($formatted:literal),* $(,)? }
    ) => {
        $(
            pub fn $name() -> &'static str {
                lookup($key)
            }
        )*

        pub const KEYS: &[&str] = &[$($key,)* $($formatted,)*];
    };
}

catalogue! {
    plain {
        // Window and tabs
        untitled => "untitled",
        new_picture => "new-picture",
        close => "close",
        copy_to_clipboard => "copy-to-clipboard",

        // Global tools
        menu => "menu",
        undo => "undo",
        redo => "redo",
        history => "history",
        paste => "paste",
        cut => "cut",
        copy => "copy",
        select => "select",
        select_all => "select-all",
        select_box => "select-box",
        select_freeform => "select-freeform",
        crop => "crop",
        smart_cutout => "smart-cutout",
        zoom => "zoom",
        zoom_in => "zoom-in",
        zoom_out => "zoom-out",
        actual_size => "actual-size",
        fit_to_window => "fit-to-window",

        // Menu rail
        menu_back => "menu-back",
        menu_new => "menu-new",
        menu_open => "menu-open",
        menu_insert => "menu-insert",
        menu_save => "menu-save",
        menu_save_as => "menu-save-as",
        menu_settings => "menu-settings",
        menu_about => "menu-about",

        // Open and save pages
        open_title => "open-title",
        open_browse => "open-browse",
        save_as_title => "save-as-title",
        save_as_choose => "save-as-choose",
        dialog_images => "dialog-images",

        // Settings
        settings_title => "settings-title",
        settings_appearance => "settings-appearance",
        settings_accent => "settings-accent",
        settings_acrylic => "settings-acrylic",
        settings_acrylic_note => "settings-acrylic-note",
        settings_unsaved => "settings-unsaved",
        settings_unsaved_note => "settings-unsaved-note",
        settings_second_picture => "settings-second-picture",
        settings_second_picture_note => "settings-second-picture-note",
        settings_new_canvas => "settings-new-canvas",
        settings_new_canvas_note => "settings-new-canvas-note",
        settings_title_bar => "settings-title-bar",
        settings_title_bar_note => "settings-title-bar-note",
        settings_native_decorations => "settings-native-decorations",
        settings_language => "settings-language",
        settings_language_note => "settings-language-note",
        on => "on",
        off => "off",
        theme_auto => "theme-auto",
        theme_light => "theme-light",
        theme_dark => "theme-dark",
        theme_system_dark => "theme-system-dark",
        theme_system_light => "theme-system-light",
        theme_system_unknown => "theme-system-unknown",
        accent_classic => "accent-classic",
        accent_rusty => "accent-rusty",
        language_auto => "language-auto",
        open_in_tab => "open-in-tab",
        open_in_window => "open-in-window",
        new_canvas_fit => "new-canvas-fit",
        new_canvas_resolution => "new-canvas-resolution",
        new_canvas_custom => "new-canvas-custom",
        resolution_720p => "resolution-720p",
        resolution_1080p => "resolution-1080p",
        resolution_2160p => "resolution-2160p",
        resolution_square => "resolution-square",
        resolution_a4 => "resolution-a4",
        resolution_letter => "resolution-letter",

        // About
        about_tagline => "about-tagline",
        about_broken => "about-broken",
        about_report => "about-report",
        about_credit => "about-credit",
        about_source => "about-source",

        // Side panel tabs
        tab_brushes => "tab-brushes",
        tab_shapes => "tab-shapes",
        tab_stickers => "tab-stickers",
        tab_text => "tab-text",
        tab_canvas => "tab-canvas",

        // Brushes
        tool_marker => "tool-marker",
        tool_calligraphy => "tool-calligraphy",
        tool_oil_brush => "tool-oil-brush",
        tool_watercolour => "tool-watercolour",
        tool_pixel_pen => "tool-pixel-pen",
        tool_pencil => "tool-pencil",
        tool_eraser => "tool-eraser",
        tool_crayon => "tool-crayon",
        tool_spray_can => "tool-spray-can",
        tool_fill => "tool-fill",
        tool_pipette => "tool-pipette",
        tool_select => "tool-select",
        tool_text => "tool-text",
        tool_shape => "tool-shape",
        thickness => "thickness",
        tolerance => "tolerance",
        opacity => "opacity",
        sticker_opacity => "sticker-opacity",

        // Colours
        colour => "colour",
        add_colour => "add-colour",
        edit => "edit",
        remove => "remove",
        picker_title => "picker-title",
        picker_red => "picker-red",
        picker_green => "picker-green",
        picker_blue => "picker-blue",
        picker_hex => "picker-hex",
        ok => "ok",
        cancel => "cancel",

        // Shapes and curves
        shapes_heading => "shapes-heading",
        shapes_line_and_curve => "shapes-line-and-curve",
        shapes_hint => "shapes-hint",
        curves_hint => "curves-hint",
        fill => "fill",
        fill_type => "fill-type",
        line_type => "line-type",
        paint_none => "paint-none",
        paint_solid => "paint-solid",
        rotate_and_flip => "rotate-and-flip",
        rotate_left => "rotate-left",
        rotate_right => "rotate-right",
        flip_horizontal => "flip-horizontal",
        flip_vertical => "flip-vertical",
        flip_horizontally => "flip-horizontally",
        flip_vertically => "flip-vertically",
        bones => "bones",
        add_bones => "add-bones",
        bones_hint => "bones-hint",
        put_down_hint => "put-down-hint",
        curve_line => "curve-line",
        curve_3 => "curve-3",
        curve_4 => "curve-4",
        curve_5 => "curve-5",
        shape_circle => "shape-circle",
        shape_capsule => "shape-capsule",
        shape_rectangle => "shape-rectangle",
        shape_rounded_rectangle => "shape-rounded-rectangle",
        shape_triangle => "shape-triangle",
        shape_pentagon => "shape-pentagon",
        shape_hexagon => "shape-hexagon",
        shape_diamond => "shape-diamond",
        shape_right_triangle => "shape-right-triangle",
        shape_arrow => "shape-arrow",
        shape_pointed_arrow => "shape-pointed-arrow",
        shape_half_arc => "shape-half-arc",
        shape_five_point_star => "shape-five-point-star",
        shape_six_point_star => "shape-six-point-star",
        shape_four_point_star => "shape-four-point-star",
        shape_multipoint_star => "shape-multipoint-star",
        shape_speech_bubble => "shape-speech-bubble",
        shape_thought_bubble => "shape-thought-bubble",
        shape_cross => "shape-cross",
        shape_check_mark => "shape-check-mark",
        shape_moon => "shape-moon",
        shape_banner => "shape-banner",
        shape_lightning => "shape-lightning",
        shape_heart => "shape-heart",

        // Text
        text_heading => "text-heading",
        text => "text",
        text_bold => "text-bold",
        text_italic => "text-italic",
        text_underline => "text-underline",
        text_background_fill => "text-background-fill",
        text_hint => "text-hint",

        // Stickers
        stickers_heading => "stickers-heading",
        stickers_hint => "stickers-hint",
        add_sticker => "add-sticker",
        stickers_added => "stickers-added",

        // Canvas panel
        canvas_heading => "canvas-heading",
        transparent_canvas => "transparent-canvas",
        show_canvas => "show-canvas",
        resize_canvas => "resize-canvas",
        lock_aspect_ratio => "lock-aspect-ratio",
        width => "width",
        height => "height",
        resize_image_with_canvas => "resize-image-with-canvas",
        unit_pixels => "unit-pixels",
        unit_percent => "unit-percent",
        unit_px => "unit-px",
        unit_percent_sign => "unit-percent-sign",
        apply => "apply",
        selection_width => "selection-width",
        selection_height => "selection-height",

        // Crop
        crop_framing => "crop-framing",
        crop_custom => "crop-custom",
        done => "done",

        // Smart cutout
        cutout_choose => "cutout-choose",
        cutout_choose_hint => "cutout-choose-hint",
        cutout_next => "cutout-next",
        cutout_refine => "cutout-refine",
        cutout_add => "cutout-add",
        cutout_remove => "cutout-remove",
        cutout_add_hint => "cutout-add-hint",
        cutout_remove_hint => "cutout-remove-hint",
        cutout_autofill => "cutout-autofill",
        cutout_back => "cutout-back",

        // Dialogs
        save_work_title => "save-work-title",
        save_work_keep_session => "save-work-keep-session",
        save_work_dont_save => "save-work-dont-save",
        dont_ask_again => "dont-ask-again",
        recovery_title => "recovery-title",
        recovery_recover => "recovery-recover",
        recovery_discard => "recovery-discard",

        // File formats
        format_png => "format-png",
        format_jpeg => "format-jpeg",
        format_webp => "format-webp",
        format_bmp => "format-bmp",
        format_gif => "format-gif",
        format_tiff => "format-tiff",
        format_tga => "format-tga",
        format_ico => "format-ico",
        format_icns => "format-icns",
        format_qoi => "format-qoi",
        format_pnm => "format-pnm",
        format_farbfeld => "format-farbfeld",

        // Errors
        error_impossible_size => "error-impossible-size",
    }

    formatted {
        // Window and tabs
        "window-title",
        "window-title-modified",

        // Settings
        "new-canvas-fitted",
        "new-canvas-fixed",

        // About
        "about-version",

        // Brushes
        "pixels-value",
        "percent-value",

        // Canvas panel
        "canvas-size",
        "size-in-pixels",
        "document-size",

        // Dialogs
        "save-work-body",
        "save-work-body-closing",
        "recovery-body",

        // File formats
        "format-with-extension",

        // Errors
        "error-cannot-open",
        "error-cannot-read",
        "error-cannot-save",
        "error-unsupported-format",
        "error-ico-too-big",
        "error-no-images-in-icon",
        "error-no-clipboard",
        "error-cannot-copy",
        "error-cannot-open-window",
        "error-settings-unreadable",
        "error-settings-malformed",
    }
}

pub fn window_title(name: &str, modified: bool) -> String {
    if modified {
        tr!("window-title-modified", name = name)
    } else {
        tr!("window-title", name = name)
    }
}

pub fn new_canvas_fitted(size: &str, ratio: &str) -> String {
    tr!("new-canvas-fitted", size = size, ratio = ratio)
}

pub fn new_canvas_fixed(size: &str) -> String {
    tr!("new-canvas-fixed", size = size)
}

pub fn about_version(version: &str) -> String {
    tr!("about-version", version = version)
}

pub fn pixels_value(value: f32) -> String {
    tr!("pixels-value", value = format!("{value:.0}"))
}

pub fn percent_value(value: f32) -> String {
    tr!("percent-value", value = format!("{:.0}", value * 100.0))
}

pub fn canvas_size(width: u32, height: u32) -> String {
    tr!(
        "canvas-size",
        width = width.to_string(),
        height = height.to_string()
    )
}

pub fn size_in_pixels(value: u32) -> String {
    tr!("size-in-pixels", value = value.to_string())
}

pub fn document_size(width: u32, height: u32) -> String {
    tr!(
        "document-size",
        width = width.to_string(),
        height = height.to_string()
    )
}

pub fn save_work_body(name: &str, closing: bool) -> String {
    if closing {
        tr!("save-work-body-closing", name = name)
    } else {
        tr!("save-work-body", name = name)
    }
}

pub fn recovery_body(count: usize) -> String {
    tr!("recovery-body", count = count)
}

pub fn format_with_extension(label: &str, extension: &str) -> String {
    tr!(
        "format-with-extension",
        label = label,
        extension = extension
    )
}

pub fn error_cannot_open(name: &str, reason: &str) -> String {
    tr!("error-cannot-open", name = name, reason = reason)
}

pub fn error_cannot_read(name: &str, reason: &str) -> String {
    tr!("error-cannot-read", name = name, reason = reason)
}

pub fn error_cannot_save(name: &str, reason: &str) -> String {
    tr!("error-cannot-save", name = name, reason = reason)
}

pub fn error_unsupported_format(name: &str) -> String {
    tr!("error-unsupported-format", name = name)
}

pub fn error_ico_too_big(name: &str) -> String {
    tr!("error-ico-too-big", name = name)
}

pub fn error_no_images_in_icon(name: &str) -> String {
    tr!("error-no-images-in-icon", name = name)
}

pub fn error_no_clipboard(reason: &str) -> String {
    tr!("error-no-clipboard", reason = reason)
}

pub fn error_cannot_copy(reason: &str) -> String {
    tr!("error-cannot-copy", reason = reason)
}

pub fn error_cannot_open_window(reason: &str) -> String {
    tr!("error-cannot-open-window", reason = reason)
}

pub fn error_settings_unreadable(reason: &str) -> String {
    tr!("error-settings-unreadable", reason = reason)
}

pub fn error_settings_malformed(reason: &str) -> String {
    tr!("error-settings-malformed", reason = reason)
}
