use crate::app::{CanvasPanel, Drawing, Message, Tab};
use crate::paint::curve::{self, CurveKind};
use crate::paint::shapes::{self, ShapeKind, ShapeStyle};
use crate::paint::{Brush, Tool, brush};
use crate::text::{Align, TextStyle};
use crate::ui::controls;
use crate::ui::icons::{self, icon};
use crate::ui::theme::{self, metrics};

use iced::widget::{
    Space, button, checkbox, column, container, mouse_area, row, slider, text, text_input,
};
use iced::{Color, Element, Length};

pub const TABS: [(&str, &[u8], Option<Tab>); 5] = [
    ("Brushes", icons::BRUSHES, Some(Tab::Brushes)),
    ("2D shapes", icons::SHAPES_2D, Some(Tab::Shapes)),
    ("Stickers", icons::STICKERS, Some(Tab::Stickers)),
    ("Text", icons::TEXT, Some(Tab::Text)),
    ("Canvas", icons::CANVAS, Some(Tab::Canvas)),
];

#[allow(
    clippy::too_many_arguments,
    reason = "one panel per tab, each with its own state"
)]
pub fn panel<'a>(
    tab: Tab,
    brush: &Brush,
    canvas: &CanvasPanel,
    size: (u32, u32),
    transparent: bool,
    drawing: Drawing,
    style: ShapeStyle,
    text_style: &'a TextStyle,
    width: f32,
    colour_target: bool,
    live: Option<Live>,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
    history: &'a [crate::app::Sticker],
) -> Element<'a, Message> {
    let [gutter_l, _, gutter_r, _] = metrics::SIDE_PANEL_GUTTER_MARGIN;

    let body = match tab {
        Tab::Brushes => brushes(brush, custom, custom_menu),
        Tab::Shapes => shapes_panel(drawing, style, colour_target, live, custom, custom_menu),
        Tab::Stickers => stickers(history),
        Tab::Text => text_panel(text_style, custom, custom_menu),
        Tab::Canvas => canvas_panel(canvas, size, transparent),
    };

    container(body)
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .padding(iced::Padding {
            top: 16.0,
            right: gutter_r,
            bottom: 16.0,
            left: gutter_l,
        })
        .style(|_theme| container::Style {
            background: Some(theme::veiled(theme::colours().side_panel).into()),
            ..Default::default()
        })
        .into()
}

fn shapes_panel<'a>(
    chosen: Drawing,
    style: ShapeStyle,
    target: bool,
    live: Option<Live>,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    match live {
        Some(live) => shape_style_panel(style, target, live, custom, custom_menu),
        None => shape_grid(chosen),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Live {
    pub name: &'static str,
    pub opacity: f32,
    pub curve: bool,
    pub bones: bool,
    pub boned: bool,
}

fn shape_grid<'a>(chosen: Drawing) -> Element<'a, Message> {
    let mut curves = row![].spacing(4);
    for kind in curve::ALL {
        let kind = *kind;
        let active = chosen == Drawing::Curve(kind);
        curves = curves.push(tile(
            curve_thumbnail(kind, active),
            kind.name(),
            active,
            Message::CurvePicked(kind),
        ));
    }

    let mut grid = column![].spacing(4);
    for chunk in shapes::ALL.chunks(5) {
        let mut line = row![].spacing(4);
        for kind in chunk {
            let kind = *kind;
            let active = chosen == Drawing::Shape(kind);
            line = line.push(tile(
                shape_thumbnail(kind, active),
                kind.name(),
                active,
                Message::ShapePicked(kind),
            ));
        }
        grid = grid.push(line);
    }

    let hint = match chosen {
        Drawing::Shape(_) => "Drag on the canvas to draw.",
        Drawing::Curve(_) => "Drag to draw, then pull the points to bend it.",
    };

    column![
        heading("2D shapes"),
        section("Line and curve"),
        curves,
        section("2D shapes"),
        grid,
        text(hint).size(12).color(theme::colours().text_dim),
    ]
    .spacing(12)
    .into()
}

fn shape_style_panel<'a>(
    style: ShapeStyle,
    target: bool,
    live: Live,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    let mut panel = column![heading(live.name)].spacing(10);

    if !live.curve {
        panel = panel.push(section("Fill")).push(paint_row(
            style.fill,
            target,
            true,
            Message::ShapeFillTypePicked,
        ));
    }
    panel = panel.push(section("Line type")).push(paint_row(
        style.outline,
        target,
        false,
        Message::ShapeLineTypePicked,
    ));

    if style.outline.is_some() || live.curve {
        panel = panel
            .push(labelled("Thickness", format!("{:.0}px", style.thickness)))
            .push(
                slider(
                    shapes::MIN_THICKNESS..=shapes::MAX_THICKNESS,
                    style.thickness,
                    Message::ShapeThicknessChanged,
                )
                .style(controls::slider_style),
            );
    }

    panel = panel
        .push(labelled(
            "Sticker opacity",
            format!("{:.0}%", live.opacity * 100.0),
        ))
        .push(
            slider(0.0..=1.0, live.opacity, Message::FloatOpacityChanged)
                .step(0.01_f32)
                .style(controls::slider_style),
        )
        .push(section("Rotate and flip"))
        .push(
            row![
                tool_button(
                    icons::ROTATE_ANTICLOCKWISE,
                    "Rotate left",
                    Message::FloatTurned(false)
                ),
                tool_button(icons::ROTATE, "Rotate right", Message::FloatTurned(true)),
                tool_button(
                    icons::FLIP_HORIZONTAL,
                    "Flip horizontally",
                    Message::FloatMirrored(true)
                ),
                tool_button(
                    icons::FLIP_VERTICAL,
                    "Flip vertically",
                    Message::FloatMirrored(false)
                ),
            ]
            .spacing(4),
        )
        .push(swatches(
            style.outline.or(style.fill).unwrap_or([0, 0, 0, 255]),
            custom,
            custom_menu,
        ));

    if live.bones {
        panel = panel
            .push(section("Bones"))
            .push(wide_button("Add bones", Message::BonesRequested));
    }

    let note = if live.boned || live.curve {
        "Double click the line to add a bone, or a bone to take it away."
    } else {
        "Click away from the shape to put it down."
    };
    panel = panel.push(text(note).size(12).color(theme::colours().text_dim));

    panel.into()
}

fn wide_button<'a>(label: &'a str, press: Message) -> Element<'a, Message> {
    button(crate::ui::centred(text(label).size(13).center()))
        .width(Length::Fill)
        .height(Length::Fixed(32.0))
        .style(|_theme, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered) {
                    theme::colours().control_hover
                } else {
                    theme::colours().control
                }
                .into(),
            ),
            text_color: theme::colours().text,
            border: iced::Border {
                color: theme::colours().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .on_press(press)
        .into()
}

fn paint_row<'a>(
    colour: Option<[u8; 4]>,
    target: bool,
    is_fill: bool,
    picked: fn(shapes::Paint) -> Message,
) -> Element<'a, Message> {
    let shown = colour.unwrap_or([255, 255, 255, 0]);
    let swatch = button(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(34.0))
        .style(move |_theme, _status| {
            let c = theme::colours();
            button::Style {
                background: Some(from_bytes(shown).into()),
                border: iced::Border {
                    color: if target == is_fill {
                        c.accent
                    } else {
                        c.border
                    },
                    width: if target == is_fill { 2.0 } else { 1.0 },
                    radius: 0.0.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::ShapeColourTargetPicked(is_fill));

    row![
        swatch,
        iced::widget::pick_list(shapes::Paint::ALL, Some(shapes::Paint::of(colour)), picked)
            .style(controls::pick_list_style)
            .text_size(13)
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

fn text_panel<'a>(
    style: &'a TextStyle,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    let family = iced::widget::pick_list(
        crate::text::FAMILIES.as_slice(),
        Some(&style.family),
        Message::TextFontPicked,
    )
    .style(controls::pick_list_style)
    .text_size(13)
    .width(Length::Fill);

    let size = iced::widget::pick_list(
        crate::text::SIZES,
        Some(style.size.round() as u32),
        Message::TextSizePicked,
    )
    .style(controls::pick_list_style)
    .text_size(13)
    .width(Length::Fixed(90.0));

    let colour = from_bytes(style.colour);
    let well = button(
        Space::new()
            .width(Length::Fixed(36.0))
            .height(Length::Fixed(32.0)),
    )
    .style(move |_theme, _status| button::Style {
        background: Some(colour.into()),
        border: iced::Border {
            color: theme::colours().border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .on_press(Message::PickerOpened);

    let weight = row![
        letter("B", style.bold, Message::TextBoldToggled),
        letter("I", style.italic, Message::TextItalicToggled),
        letter("U", style.underline, Message::TextUnderlineToggled),
    ]
    .spacing(4);

    let aligns = row![
        align_tile(Align::Left, style.align),
        align_tile(Align::Centre, style.align),
        align_tile(Align::Right, style.align),
    ]
    .spacing(4);

    column![
        heading("2D text"),
        family,
        row![size, well].spacing(6).align_y(iced::Alignment::Center),
        weight,
        aligns,
        checkbox(style.background)
            .style(controls::checkbox_style)
            .label("Background fill")
            .text_size(13)
            .on_toggle(Message::TextBackgroundToggled),
        section("Colour"),
        swatches(style.colour, custom, custom_menu),
        text("Drag on the canvas to make a text box.")
            .size(12)
            .color(theme::colours().text_dim),
    ]
    .spacing(12)
    .into()
}

fn letter<'a>(glyph: &'a str, active: bool, press: Message) -> Element<'a, Message> {
    button(crate::ui::centred(text(glyph).size(16).center()))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(36.0))
        .style(move |_theme, _status| tile_style(active))
        .on_press(press)
        .into()
}

fn align_tile<'a>(align: Align, chosen: Align) -> Element<'a, Message> {
    let art = match align {
        Align::Left => crate::assets::ALIGN_LEFT_SVG,
        Align::Centre => crate::assets::ALIGN_CENTRE_SVG,
        Align::Right => crate::assets::ALIGN_RIGHT_SVG,
    };
    let tint = if align == chosen {
        theme::colours().selection_text
    } else {
        theme::colours().text
    };
    button(crate::ui::centred(icons::art(art, 16.0, Some(tint))))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(36.0))
        .style(move |_theme, _status| tile_style(align == chosen))
        .on_press(Message::TextAlignPicked(align))
        .into()
}

fn tile<'a>(
    art: Element<'a, Message>,
    label: &'a str,
    active: bool,
    press: Message,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        button(art)
            .width(Length::Fixed(metrics::SHAPE_WIDTH))
            .height(Length::Fixed(metrics::SHAPE_HEIGHT))
            .padding(6)
            .style(move |_theme, _status| tile_style(active))
            .on_press(press),
        text(label).size(12),
        iced::widget::tooltip::Position::Bottom,
    )
    .style(tooltip_style)
    .padding(6)
    .into()
}

fn section<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(13).color(theme::colours().text_dim).into()
}

fn curve_thumbnail<'a>(kind: CurveKind, active: bool) -> Element<'a, Message> {
    icons::art(icons::for_curve(kind), 26.0, Some(tile_ink(active)))
}

const THUMBNAIL: u32 = 28;

type Thumbnails = std::sync::Arc<Vec<Option<iced::widget::image::Handle>>>;

type ThumbnailCache = std::collections::HashMap<(theme::Mode, theme::Scheme, bool), Thumbnails>;

static THUMBNAILS: std::sync::LazyLock<std::sync::Mutex<ThumbnailCache>> =
    std::sync::LazyLock::new(Default::default);

fn ink_of(palette: &theme::Palette, active: bool) -> iced::Color {
    if active {
        palette.selection_text
    } else {
        palette.text
    }
}

fn tile_ink(active: bool) -> iced::Color {
    ink_of(theme::colours(), active)
}

fn thumbnails(mode: theme::Mode, scheme: theme::Scheme, active: bool) -> Thumbnails {
    let colour = to_bytes(ink_of(theme::palette_for(mode, scheme), active));
    let outline = ShapeStyle {
        fill: None,
        outline: Some(colour),
        thickness: 2.0,
    };
    shapes::ALL
        .iter()
        .map(|kind| {
            let pixels = shapes::render(*kind, &outline, THUMBNAIL, THUMBNAIL)?;
            let (w, h) = pixels.size();
            Some(iced::widget::image::Handle::from_rgba(
                w,
                h,
                pixels.as_bytes().to_vec(),
            ))
        })
        .collect::<Vec<_>>()
        .into()
}

fn thumbnail_set(active: bool) -> Thumbnails {
    let key = (theme::mode(), theme::scheme(), active);
    let mut cache = THUMBNAILS.lock().expect("thumbnails");
    cache
        .entry(key)
        .or_insert_with(|| thumbnails(key.0, key.1, key.2))
        .clone()
}

fn shape_thumbnail<'a>(kind: ShapeKind, active: bool) -> Element<'a, Message> {
    match thumbnail_set(active)
        .get(kind.index())
        .and_then(|h| h.clone())
    {
        Some(handle) => iced::widget::image(handle)
            .width(THUMBNAIL as f32)
            .height(THUMBNAIL as f32)
            .into(),
        None => Space::new()
            .width(THUMBNAIL as f32)
            .height(THUMBNAIL as f32)
            .into(),
    }
}

fn heading<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(20)
        .color(theme::colours().accent_text)
        .into()
}

fn brushes<'a>(
    brush: &Brush,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    let mut panel = column![heading(brush.tool.name()), brush_grid(brush.tool)].spacing(12);

    if brush.tool.profile().is_some() {
        panel = panel
            .push(labelled("Thickness", format!("{:.0}px", brush.thickness)))
            .push(
                slider(
                    brush::MIN_THICKNESS..=brush::MAX_THICKNESS,
                    brush.thickness,
                    Message::ThicknessChanged,
                )
                .style(controls::slider_style),
            );
    }
    if brush.tool == Tool::Fill {
        panel = panel
            .push(labelled(
                "Tolerance",
                format!("{:.0}%", brush.tolerance * 100.0),
            ))
            .push(
                slider(0.0..=1.0_f32, brush.tolerance, Message::ToleranceChanged)
                    .style(controls::slider_style)
                    .step(0.01_f32),
            );
    }
    if brush.tool != Tool::Pipette {
        panel = panel
            .push(labelled(
                "Opacity",
                format!("{:.0}%", brush.opacity * 100.0),
            ))
            .push(
                slider(0.0..=1.0_f32, brush.opacity, Message::OpacityChanged)
                    .step(0.01_f32)
                    .style(controls::slider_style),
            );
    }

    panel
        .push(Space::new().height(Length::Fixed(8.0)))
        .push(current_colour(brush))
        .push(swatches(brush.colour, custom, custom_menu))
        .into()
}

fn current_colour<'a>(brush: &Brush) -> Element<'a, Message> {
    let colour = from_bytes(brush.colour);
    let picking = brush.tool == Tool::Pipette;

    row![
        container(Space::new().width(Length::Fill).height(Length::Fixed(40.0)))
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(colour.into()),
                border: iced::Border {
                    color: theme::colours().border,
                    width: 1.0,
                    radius: 0.0.into()
                },
                ..Default::default()
            }),
        button(crate::ui::centred(icon(
            icons::PIPETTE,
            16.0,
            tile_ink(picking)
        )))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(40.0))
        .style(move |_theme, _status| tile_style(picking))
        .on_press(Message::ToolPicked(Tool::Pipette)),
    ]
    .spacing(4)
    .into()
}

pub fn from_bytes(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3] as f32 / 255.0)
}

fn brush_grid<'a>(selected: Tool) -> Element<'a, Message> {
    let mut grid = column![].spacing(4);
    for chunk in brush::PANEL_ORDER.chunks(5) {
        let mut line = row![].spacing(4);
        for tool in chunk {
            let tool = *tool;
            let active = tool == selected;
            line = line.push(
                iced::widget::tooltip(
                    button(crate::ui::centred(icons::art(
                        icons::for_tool(tool),
                        34.0,
                        None,
                    )))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0))
                    .style(move |_theme, _status| tile_style(active))
                    .on_press(Message::ToolPicked(tool)),
                    text(tool.name()).size(12),
                    iced::widget::tooltip::Position::Bottom,
                )
                .style(tooltip_style)
                .padding(6),
            );
        }
        grid = grid.push(line);
    }
    grid.into()
}

fn swatches<'a>(
    current: [u8; 4],
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(2);
    for (r, chunk) in theme::SWATCHES.chunks(6).enumerate() {
        let mut line = row![].spacing(2);
        for (c, colour) in chunk.iter().enumerate() {
            line = line.push(swatch(*colour, current, Message::ColourPicked(r * 6 + c)));
        }
        grid = grid.push(line);
    }

    if !custom.is_empty() {
        let mut line = row![].spacing(2);
        for (i, colour) in custom.iter().enumerate() {
            line = line.push(
                mouse_area(swatch(
                    from_bytes(*colour),
                    current,
                    Message::CustomColourPicked(i),
                ))
                .on_right_press(Message::CustomColourMenuOpened(i)),
            );
        }
        grid = grid.push(line);
    }

    if let Some(i) = custom_menu.filter(|i| *i < custom.len()) {
        grid = grid.push(
            row![
                Space::new().width(Length::Fixed(i.min(3) as f32 * 34.0)),
                custom_menu_view(i),
            ]
            .width(Length::Fill),
        );
    }

    column![grid, wide_button("+  Add colour", Message::PickerOpened)]
        .spacing(6)
        .into()
}

fn custom_menu_view<'a>(index: usize) -> Element<'a, Message> {
    container(
        column![
            context_button("Edit", Message::CustomColourEditRequested(index)),
            context_button("Remove", Message::CustomColourRemoved(index)),
        ]
        .spacing(2),
    )
    .width(Length::Fixed(112.0))
    .padding(4)
    .style(|_theme| container::Style {
        background: Some(theme::colours().side_panel.into()),
        border: iced::Border {
            color: theme::colours().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn context_button<'a>(label: &'a str, press: Message) -> Element<'a, Message> {
    button(text(label).size(13))
        .width(Length::Fill)
        .height(Length::Fixed(28.0))
        .style(|_theme, status| button::Style {
            background: matches!(status, button::Status::Hovered)
                .then(|| theme::colours().control_hover.into()),
            text_color: theme::colours().text,
            ..Default::default()
        })
        .on_press(press)
        .into()
}

fn swatch<'a>(colour: Color, current: [u8; 4], press: Message) -> Element<'a, Message> {
    let active = to_bytes(colour) == current;
    button(Space::new())
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .style(move |_theme, _status| swatch_style(colour, active))
        .on_press(press)
        .into()
}

fn stickers<'a>(history: &'a [crate::app::Sticker]) -> Element<'a, Message> {
    let recent: Element<'a, Message> = if history.is_empty() {
        Space::new().into()
    } else {
        let mut grid = column![section("Added")].spacing(6);
        for chunk in history
            .iter()
            .enumerate()
            .rev()
            .collect::<Vec<_>>()
            .chunks(4)
        {
            let mut line = row![].spacing(6);
            for (i, sticker) in chunk {
                line = line.push(
                    button(iced::widget::image(sticker.thumb().clone()))
                        .width(Length::Fixed(48.0))
                        .height(Length::Fixed(48.0))
                        .padding(4)
                        .style(|_theme, _status| tile_style(false))
                        .on_press(Message::StickerRecalled(*i)),
                );
            }
            grid = grid.push(line);
        }
        grid.into()
    };

    column![
        heading("Stickers"),
        text(
            "Drop an image on the window, paste one, or pick one here. It floats above the \
              canvas until you click away."
        )
        .size(12)
        .color(theme::colours().text_dim),
        button(
            column![
                icons::art(
                    crate::assets::STICKER_SLOT_SVG,
                    48.0,
                    Some(theme::colours().text)
                ),
                text("Add sticker").size(12),
            ]
            .spacing(6)
            .align_x(iced::Alignment::Center)
        )
        .width(Length::Fill)
        .padding(12)
        .style(|_theme, _status| tile_style(false))
        .on_press(Message::StickerRequested),
        recent,
    ]
    .spacing(12)
    .into()
}

fn canvas_panel<'a>(
    state: &CanvasPanel,
    size: (u32, u32),
    transparent: bool,
) -> Element<'a, Message> {
    let unit = if state.percent { "%" } else { "px" };

    let field = |label: &'a str, value: &str, on_change: fn(String) -> Message| {
        row![
            text(label).size(13).width(Length::Fixed(56.0)),
            text_input("", value)
                .style(controls::text_input_style)
                .on_input(on_change)
                .on_submit(Message::CanvasResizeSubmitted)
                .size(13)
                .width(Length::Fill),
            text(unit).size(13),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
    };

    column![
        heading("Canvas"),
        checkbox(transparent)
            .style(controls::checkbox_style)
            .label("Transparent canvas")
            .text_size(13)
            .on_toggle(Message::TransparencyToggled),
        checkbox(state.show_canvas)
            .style(controls::checkbox_style)
            .label("Show canvas")
            .text_size(13)
            .on_toggle(Message::ShowCanvasToggled),
        divider(),
        text("Resize canvas").size(13),
        checkbox(state.lock_aspect)
            .style(controls::checkbox_style)
            .label("Lock aspect ratio")
            .text_size(13)
            .on_toggle(Message::LockAspectToggled),
        field("Width", &state.width, Message::CanvasWidthEdited),
        field("Height", &state.height, Message::CanvasHeightEdited),
        checkbox(state.resize_image)
            .style(controls::checkbox_style)
            .label("Resize image with canvas")
            .text_size(13)
            .on_toggle(Message::ResizeImageToggled),
        row![
            button(text("Pixels").size(12)).on_press(Message::CanvasUnitPicked(false)),
            button(text("Percent").size(12)).on_press(Message::CanvasUnitPicked(true)),
            Space::new().width(Length::Fill),
            button(text("Apply").size(12)).on_press(Message::CanvasResizeSubmitted),
        ]
        .spacing(4),
        divider(),
        text(format!("{} x {} px", size.0, size.1))
            .size(12)
            .color(theme::colours().text_dim),
        row![
            tool_button(
                icons::ROTATE_ANTICLOCKWISE,
                "Rotate left",
                Message::Rotate(false)
            ),
            tool_button(icons::ROTATE, "Rotate right", Message::Rotate(true)),
            tool_button(
                icons::FLIP_HORIZONTAL,
                "Flip horizontally",
                Message::Flip(true)
            ),
            tool_button(
                icons::FLIP_VERTICAL,
                "Flip vertically",
                Message::Flip(false)
            ),
        ]
        .spacing(4),
    ]
    .spacing(10)
    .into()
}

fn tool_button<'a>(
    drawing: &'static [u8],
    hint: &'a str,
    message: Message,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        button(crate::ui::centred(icon(
            drawing,
            16.0,
            theme::colours().text,
        )))
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(36.0))
        .style(|_theme, _status| tile_style(false))
        .on_press(message),
        text(hint).size(12),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(Length::Fixed(1.0)).width(Length::Fill))
        .style(|_theme| container::Style {
            background: Some(theme::colours().border.into()),
            ..Default::default()
        })
        .into()
}

fn labelled<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    row![
        text(label).size(13),
        Space::new().width(Length::Fill),
        text(value).size(13)
    ]
    .into()
}

pub fn pressable<'a>(
    b: button::Button<'a, Message>,
    message: Option<Message>,
) -> button::Button<'a, Message> {
    match message {
        Some(m) => b.on_press(m),
        None => b,
    }
}

fn tile_style(active: bool) -> button::Style {
    button::Style {
        background: Some(if active {
            theme::selection_wash()
        } else {
            theme::colours().control.into()
        }),
        text_color: if active {
            theme::colours().selection_text
        } else {
            theme::colours().text
        },
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

fn tooltip_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(theme::colours().control.into()),
        text_color: Some(theme::colours().text),
        border: iced::Border {
            color: theme::colours().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    }
}

fn swatch_style(colour: Color, active: bool) -> button::Style {
    button::Style {
        background: Some(colour.into()),
        border: iced::Border {
            color: if active {
                theme::colours().accent
            } else {
                theme::colours().side_panel
            },
            width: 2.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn to_bytes(c: Color) -> [u8; 4] {
    let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [f(c.r), f(c.g), f(c.b), f(c.a)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{Mode, Scheme, palette_for};

    #[test]
    fn a_chosen_tile_draws_in_the_ink_its_own_wash_wants() {
        let light = |c: Color| c.r + c.g + c.b > 1.5;

        for mode in [Mode::Light, Mode::Dark] {
            let classic = palette_for(mode, Scheme::Classic);
            assert!(
                light(ink_of(classic, true)),
                "{mode:?} classic wants a light drawing"
            );

            let rusty = palette_for(mode, Scheme::Rusty);
            assert!(
                !light(ink_of(rusty, true)),
                "{mode:?} rusty wants a dark one"
            );

            assert_eq!(ink_of(rusty, false), rusty.text);
            assert_eq!(ink_of(classic, false), classic.text);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Widescreen,
    FiveThree,
    ThreeTwo,
    FourThree,
    Square,
    Portrait,
}

impl Framing {
    pub const ALL: [Framing; 6] = [
        Framing::Widescreen,
        Framing::FiveThree,
        Framing::ThreeTwo,
        Framing::FourThree,
        Framing::Square,
        Framing::Portrait,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Framing::Widescreen => "16:9",
            Framing::FiveThree => "5:3",
            Framing::ThreeTwo => "3:2",
            Framing::FourThree => "4:3",
            Framing::Square => "1:1",
            Framing::Portrait => "9:16",
        }
    }

    pub fn ratio(self) -> f32 {
        match self {
            Framing::Widescreen => 16.0 / 9.0,
            Framing::FiveThree => 5.0 / 3.0,
            Framing::ThreeTwo => 3.0 / 2.0,
            Framing::FourThree => 4.0 / 3.0,
            Framing::Square => 1.0,
            Framing::Portrait => 9.0 / 16.0,
        }
    }
}

pub fn crop_panel<'a>(
    framing: Option<Framing>,
    lock: bool,
    fields: (&'a str, &'a str),
) -> Element<'a, Message> {
    let mut grid = column![].spacing(4);
    for row_of in Framing::ALL.chunks(3) {
        let mut line = row![].spacing(4);
        for kind in row_of {
            line = line.push(framing_tile(Some(*kind), framing));
        }
        grid = grid.push(line);
    }

    column![
        heading(crate::ui::strings::CROP),
        section("Choose your framing"),
        grid,
        framing_tile(None, framing),
        row![
            size_field("Width", fields.0, Message::CropWidthEdited),
            size_field("Height", fields.1, Message::CropHeightEdited),
        ]
        .spacing(8),
        checkbox(lock)
            .style(controls::checkbox_style)
            .label("Lock aspect ratio")
            .text_size(13)
            .on_toggle(Message::CropLockToggled),
        row![
            wide_button("Cancel", Message::CropCancelled),
            wide_button("Done", Message::CropApplied),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .into()
}

fn framing_tile<'a>(kind: Option<Framing>, chosen: Option<Framing>) -> Element<'a, Message> {
    const BOX: f32 = 26.0;
    let active = kind == chosen;
    let ink = tile_ink(active);

    let (w, h) = match kind {
        Some(kind) if kind.ratio() >= 1.0 => (BOX, BOX / kind.ratio()),
        Some(kind) => (BOX * kind.ratio(), BOX),
        None => (BOX * 0.8, BOX * 0.8),
    };
    let glyph = container(
        Space::new()
            .width(Length::Fixed(w))
            .height(Length::Fixed(h)),
    )
    .style(move |_theme| container::Style {
        border: iced::Border {
            color: ink,
            width: 1.5,
            radius: 1.0.into(),
        },
        ..Default::default()
    });

    let label = kind.map_or("Custom", Framing::name);
    button(
        column![
            crate::ui::centred(glyph),
            text(label).size(11).center().color(ink)
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fixed(metrics::SHAPE_WIDTH + 24.0))
    .height(Length::Fixed(58.0))
    .style(move |_theme, _status| tile_style(active))
    .on_press(Message::CropFramingPicked(kind))
    .into()
}

fn size_field<'a>(
    label: &'a str,
    value: &'a str,
    on_change: fn(String) -> Message,
) -> Element<'a, Message> {
    column![
        text(label).size(12).color(theme::colours().text_dim),
        text_input("", value)
            .on_input(on_change)
            .style(controls::text_input_style)
            .size(13)
            .padding(6),
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

pub fn cutout_panel<'a>(refining: bool, adding: bool, autofill: bool) -> Element<'a, Message> {
    let title = heading(crate::ui::strings::SMART_CUTOUT);
    if !refining {
        return column![
            title,
            section("Choose an area to cut out"),
            text("Drag the corners or sides of the box to say what to focus on.")
                .size(12)
                .color(theme::colours().text_dim),
            row![
                wide_button("Cancel", Message::CutoutCancelled),
                wide_button("Next", Message::CutoutNext),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .into();
    }

    let hint = if adding {
        "Missing something? Paint over what it left out to add it."
    } else {
        "Too much? Paint over what it took to leave it behind."
    };

    column![
        title,
        section("Refine your cutout"),
        row![
            brush_tile("Add", true, adding),
            brush_tile("Remove", false, adding),
        ]
        .spacing(8),
        text(hint).size(12).color(theme::colours().text_dim),
        checkbox(autofill)
            .style(controls::checkbox_style)
            .label("Autofill background")
            .text_size(13)
            .on_toggle(Message::CutoutAutofillToggled),
        row![
            wide_button("Go back", Message::CutoutBack),
            wide_button("Done", Message::CutoutDone),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .into()
}

fn brush_tile<'a>(label: &'a str, adds: bool, adding: bool) -> Element<'a, Message> {
    let active = adds == adding;
    let art = if adds {
        crate::assets::tool_icons::MARKER
    } else {
        crate::assets::tool_icons::ERASER
    };
    button(
        column![
            crate::ui::centred(icons::art(art, 30.0, None)),
            text(label).size(12).center()
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(66.0))
    .style(move |_theme, _status| tile_style(active))
    .on_press(Message::CutoutBrushPicked(adds))
    .into()
}
