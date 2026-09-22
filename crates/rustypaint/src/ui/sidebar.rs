use crate::app::{CanvasPanel, Drawing, Field, Message, Picking, Tab};
use crate::i18n;
use crate::paint::curve::{self, CurveKind};
use crate::paint::shapes::{self, ShapeKind, ShapeStyle};
use crate::paint::{Brush, Mirror, Tool, blur, brush};
use crate::select::cutout::workflow::Target;
use crate::text::{Align, TextStyle};
use crate::ui::controls;
use crate::ui::icons::{self, icon};
use crate::ui::segmented;
use crate::ui::theme::{self, metrics};

use iced::widget::{
    Space, button, checkbox, column, container, mouse_area, row, scrollable, slider, text,
    text_input,
};
use iced::{Color, Element, Length};

// The label is a catalogue key, resolved where the strip is drawn.
pub const TABS: [(&str, &[u8], Option<Tab>); 6] = [
    ("tab-brushes", icons::BRUSHES, Some(Tab::Brushes)),
    ("tab-symmetry", icons::SYMMETRY, Some(Tab::Symmetry)),
    ("tab-shapes", icons::SHAPES_2D, Some(Tab::Shapes)),
    ("tab-stickers", icons::STICKERS, Some(Tab::Stickers)),
    ("tab-text", icons::TEXT, Some(Tab::Text)),
    ("tab-canvas", icons::CANVAS, Some(Tab::Canvas)),
];

#[allow(
    clippy::too_many_arguments,
    reason = "one panel per tab, each with its own state"
)]
pub fn panel<'a>(
    tab: Tab,
    brush: &Brush,
    mirror: Mirror,
    typed: Option<(Field, &'a str)>,
    canvas: &CanvasPanel,
    size: (u32, u32),
    transparent: bool,
    drawing: Drawing,
    style: ShapeStyle,
    blur_settings: blur::Settings,
    text_style: &'a TextStyle,
    width: f32,
    colour_target: bool,
    live: Option<Live>,
    placement: Option<Placement>,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
    history: &'a [crate::app::Sticker],
) -> Element<'a, Message> {
    let body = match tab {
        Tab::Brushes => brushes(brush, typed, custom, custom_menu),
        Tab::Symmetry => symmetry_panel(mirror),
        Tab::Shapes => shapes_panel(
            drawing,
            style,
            colour_target,
            live,
            typed,
            custom,
            custom_menu,
        ),
        Tab::Stickers => stickers(history, brush.tool == Tool::Blur, blur_settings, typed),
        Tab::Text => text_panel(text_style, custom, custom_menu),
        Tab::Canvas => canvas_panel(canvas, size, transparent),
    };

    let body = match placement {
        Some(placement) => column![body, placement_rows(placement, typed)]
            .spacing(16)
            .into(),
        None => body,
    };

    shell(body, width)
}

// Every side panel wears this, tabbed or not: the gutter, the scroll area and the veil behind them.
pub fn shell<'a>(body: impl Into<Element<'a, Message>>, width: f32) -> Element<'a, Message> {
    let [left, _, right, _] = metrics::SIDE_PANEL_GUTTER_MARGIN;
    // The padding sits inside the scroll area so the bar rides the gutter rather than the cards.
    let body = container(body.into()).padding(iced::Padding {
        top: 16.0,
        right,
        bottom: 16.0,
        left,
    });

    container(scrollable(body).height(Length::Fill).style(scroll_style))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(theme::veiled(theme::colours().side_panel).into()),
            ..Default::default()
        })
        .into()
}

fn scroll_style(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let c = theme::colours();
    let lit = matches!(status, scrollable::Status::Hovered { .. });
    let rail = scrollable::Rail {
        background: None,
        border: iced::Border::default(),
        scroller: scrollable::Scroller {
            background: if lit { c.accent } else { c.border }.into(),
            border: iced::border::rounded(2),
        },
    };
    scrollable::Style {
        vertical_rail: rail,
        horizontal_rail: rail,
        ..scrollable::default(theme, status)
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one panel, each control with its own state"
)]
fn shapes_panel<'a>(
    chosen: Drawing,
    style: ShapeStyle,
    target: bool,
    live: Option<Live>,
    typed: Option<(Field, &'a str)>,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    match live {
        Some(live) => shape_style_panel(style, target, live, typed, custom, custom_menu),
        None => shape_grid(chosen),
    }
}

// Where the live object sits and how big it is, in canvas pixels.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Live {
    pub name: &'static str,
    pub points: Option<usize>,
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
        Drawing::Shape(_) => i18n::shapes_hint(),
        Drawing::Curve(_) => i18n::curves_hint(),
    };

    Panel::new(i18n::shapes_heading())
        .card(i18n::shapes_line_and_curve(), curves)
        .plain(grid)
        .hint(hint)
        .into()
}

fn shape_style_panel<'a>(
    style: ShapeStyle,
    target: bool,
    live: Live,
    typed: Option<(Field, &'a str)>,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    let mut panel = Panel::new(live.name);
    if let Some(count) = live.points {
        panel = panel.hint(i18n::point_count(count));
    }

    let mut options: Vec<Element<'a, Message>> = Vec::new();
    if style.outline.is_some() || live.curve {
        options.push(field_row(
            i18n::thickness(),
            Field::ShapeThickness,
            style.thickness,
            typed,
        ));
        options.push(
            slider(
                shapes::MIN_THICKNESS..=shapes::MAX_THICKNESS,
                style
                    .thickness
                    .clamp(shapes::MIN_THICKNESS, shapes::MAX_THICKNESS),
                Message::ShapeThicknessChanged,
            )
            .style(controls::slider_style)
            .into(),
        );
    }
    options.push(field_row(
        i18n::sticker_opacity(),
        Field::FloatOpacity,
        live.opacity,
        typed,
    ));
    options.push(
        slider(0.0..=1.0, live.opacity, Message::FloatOpacityChanged)
            .step(0.01_f32)
            .style(controls::slider_style)
            .into(),
    );

    let mut paint = column![].spacing(8);
    if !live.curve {
        paint = paint.push(section(i18n::fill())).push(paint_row(
            style.fill,
            target,
            true,
            Message::ShapeFillTypePicked,
        ));
    }
    paint = paint
        .push(section(i18n::line_type()))
        .push(paint_row(
            style.outline,
            target,
            false,
            Message::ShapeLineTypePicked,
        ))
        .push(swatches(
            style.outline.or(style.fill).unwrap_or([0, 0, 0, 255]),
            custom,
            custom_menu,
        ));

    let hint = if live.boned || live.curve {
        i18n::bones_hint()
    } else {
        i18n::put_down_hint()
    };

    panel = panel
        .card(i18n::options(), column![].extend(options).spacing(8))
        .card(
            i18n::rotate_and_flip(),
            row![
                tool_button(
                    icons::ROTATE_ANTICLOCKWISE,
                    i18n::rotate_left(),
                    Message::FloatTurned(false)
                ),
                tool_button(
                    icons::ROTATE,
                    i18n::rotate_right(),
                    Message::FloatTurned(true)
                ),
                tool_button(
                    icons::FLIP_HORIZONTAL,
                    i18n::flip_horizontally(),
                    Message::FloatMirrored(true)
                ),
                tool_button(
                    icons::FLIP_VERTICAL,
                    i18n::flip_vertically(),
                    Message::FloatMirrored(false)
                ),
            ]
            .spacing(4),
        );

    if live.bones {
        panel = panel.card(
            i18n::bones(),
            wide_button(i18n::add_bones(), Message::BonesRequested),
        );
    }

    panel.card(i18n::colour(), paint).hint(hint).into()
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
            .menu_style(controls::menu_style)
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
    .menu_style(controls::menu_style)
    .text_size(13)
    .width(Length::Fill);

    let size = iced::widget::pick_list(
        crate::text::SIZES,
        Some(style.size.round() as u32),
        Message::TextSizePicked,
    )
    .style(controls::pick_list_style)
    .menu_style(controls::menu_style)
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
    .on_press(Message::PickerOpened(Picking::Current));

    let weight = row![
        letter(i18n::text_bold(), style.bold, Message::TextBoldToggled),
        letter(
            i18n::text_italic(),
            style.italic,
            Message::TextItalicToggled
        ),
        letter(
            i18n::text_underline(),
            style.underline,
            Message::TextUnderlineToggled
        ),
    ]
    .spacing(4);

    let aligns = row![
        align_tile(Align::Left, style.align),
        align_tile(Align::Centre, style.align),
        align_tile(Align::Right, style.align),
    ]
    .spacing(4);

    Panel::new(i18n::text_heading())
        .card(
            i18n::options(),
            column![
                family,
                row![size, well].spacing(6).align_y(iced::Alignment::Center),
                weight,
                aligns,
                checkbox(style.background)
                    .style(controls::checkbox_style)
                    .label(i18n::text_background_fill())
                    .text_size(13)
                    .on_toggle(Message::TextBackgroundToggled),
            ]
            .spacing(10),
        )
        .card(i18n::colour(), swatches(style.colour, custom, custom_menu))
        .hint(i18n::text_hint())
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

// Every panel is a title with a stack of cards under it, in the same order throughout: what is
// selected, then its options, then colour.
struct Panel<'a> {
    title: &'a str,
    items: Vec<Element<'a, Message>>,
}

impl<'a> Panel<'a> {
    fn new(title: &'a str) -> Self {
        Self {
            title,
            items: Vec::new(),
        }
    }

    fn card(self, label: &'a str, content: impl Into<Element<'a, Message>>) -> Self {
        self.loose(card(label, content))
    }

    fn plain(self, content: impl Into<Element<'a, Message>>) -> Self {
        self.loose(group(content))
    }

    // Cards are for controls. Prose and buttons that close the panel ride outside one, so no card
    // ends up drawn around nothing.
    fn loose(mut self, content: impl Into<Element<'a, Message>>) -> Self {
        self.items.push(content.into());
        self
    }

    fn hint(self, label: impl iced::widget::text::IntoFragment<'a>) -> Self {
        self.loose(
            text(label)
                .size(12)
                .center()
                .color(theme::colours().text_dim)
                .width(Length::Fill),
        )
    }
}

impl<'a> From<Panel<'a>> for Element<'a, Message> {
    fn from(panel: Panel<'a>) -> Self {
        column![heading(panel.title)]
            .extend(panel.items)
            .spacing(12)
            .align_x(iced::Alignment::Center)
            .into()
    }
}

fn card<'a>(title: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    group(column![label(title), content.into()].spacing(8))
}

fn group<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .padding(10)
        .style(|_theme| container::Style {
            background: Some(theme::colours().panel_group.into()),
            border: iced::Border {
                color: theme::colours().border,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

// Left of the panel rather than under it, because the note is wider than the panel is.
fn note<'a>(control: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    iced::widget::tooltip(
        control,
        text(label).size(12).width(Length::Fixed(200.0)),
        iced::widget::tooltip::Position::Left,
    )
    .style(tooltip_style)
    .padding(6)
    .into()
}

fn label<'a>(content: &'a str) -> Element<'a, Message> {
    text(content)
        .size(14)
        .center()
        .color(theme::colours().text)
        .width(Length::Fill)
        .into()
}

fn section<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(13)
        .center()
        .color(theme::colours().text_dim)
        .width(Length::Fill)
        .into()
}

fn curve_thumbnail<'a>(kind: CurveKind, active: bool) -> Element<'a, Message> {
    icons::art(icons::for_curve(kind), 26.0, Some(tile_ink(active)))
}

const THUMBNAIL: u32 = 28;

type Thumbnails = std::sync::Arc<Vec<Option<iced::widget::image::Handle>>>;

type ThumbnailCache =
    std::collections::HashMap<(theme::Mode, theme::Scheme, theme::CustomAccent, bool), Thumbnails>;

static THUMBNAILS: std::sync::LazyLock<std::sync::Mutex<ThumbnailCache>> =
    std::sync::LazyLock::new(Default::default);

fn ink_of(palette: theme::Palette, active: bool) -> iced::Color {
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
    let key = (
        theme::mode(),
        theme::scheme(),
        theme::custom_accent(),
        active,
    );
    let mut cache = THUMBNAILS.lock().expect("thumbnails");
    cache
        .entry(key)
        .or_insert_with(|| thumbnails(key.0, key.1, key.3))
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
        .center()
        .color(theme::colours().accent_text)
        .width(Length::Fill)
        .into()
}

fn brushes<'a>(
    brush: &Brush,
    typed: Option<(Field, &'a str)>,
    custom: &[[u8; 4]],
    custom_menu: Option<usize>,
) -> Element<'a, Message> {
    let mut options: Vec<Element<'a, Message>> = Vec::new();

    if brush.tool.profile().is_some() {
        options.push(field_row(
            i18n::thickness(),
            Field::Thickness,
            brush.thickness(),
            typed,
        ));
        options.push(
            slider(
                brush::MIN_THICKNESS..=brush::MAX_THICKNESS,
                brush.thickness().min(brush::MAX_THICKNESS),
                Message::ThicknessChanged,
            )
            .style(controls::slider_style)
            .into(),
        );
    }
    if brush.tool.edge_is_tunable() {
        options.push(
            checkbox(brush.antialiased())
                .style(controls::checkbox_style)
                .label(i18n::antialiasing())
                .text_size(13)
                .on_toggle(Message::AntialiasingToggled)
                .into(),
        );
        if brush.antialiased() {
            options.push(field_row(
                i18n::hardness(),
                Field::Hardness,
                brush.hardness(),
                typed,
            ));
            options.push(
                slider(0.0..=1.0_f32, brush.hardness(), Message::HardnessChanged)
                    .step(0.01_f32)
                    .style(controls::slider_style)
                    .into(),
            );
        }
    }
    if brush.tool.profile().is_some() && !brush.tool.sprays() {
        options.push(field_row(
            i18n::stabilizer(),
            Field::Stabilizer,
            brush.stabilizer(),
            typed,
        ));
        options.push(
            slider(
                0.0..=1.0_f32,
                brush.stabilizer(),
                Message::StabilizerChanged,
            )
            .step(0.01_f32)
            .style(controls::slider_style)
            .into(),
        );
    }
    if brush.tool.snaps_to_pixels() {
        options.push(section(i18n::tip_shape()));
        options.push(segmented::segmented(
            [i18n::tip_round(), i18n::tip_square()],
            brush.square_tip() as usize,
            |index| Message::SquareTipPicked(index == 1),
        ));
        options.push(note(
            checkbox(brush.pixel_perfect())
                .style(controls::checkbox_style)
                .label(i18n::pixel_perfect())
                .text_size(13)
                .on_toggle(Message::PixelPerfectToggled),
            i18n::pixel_perfect_hint(),
        ));
    }
    if brush.tool == Tool::Fill {
        options.push(field_row(
            i18n::tolerance(),
            Field::Tolerance,
            brush.tolerance,
            typed,
        ));
        options.push(
            slider(0.0..=1.0_f32, brush.tolerance, Message::ToleranceChanged)
                .style(controls::slider_style)
                .step(0.01_f32)
                .into(),
        );
    }
    if brush.tool != Tool::Pipette {
        options.push(field_row(
            i18n::opacity(),
            Field::Opacity,
            brush.opacity(),
            typed,
        ));
        options.push(
            slider(0.0..=1.0_f32, brush.opacity(), Message::OpacityChanged)
                .step(0.01_f32)
                .style(controls::slider_style)
                .into(),
        );
    }

    let mut panel = Panel::new(i18n::tab_brushes()).card(brush.tool.name(), brush_grid(brush.tool));

    if !options.is_empty() {
        panel = panel.card(i18n::options(), column![].extend(options).spacing(8));
    }

    panel
        .card(
            i18n::colour(),
            column![
                current_colour(brush),
                swatches(brush.colour, custom, custom_menu),
            ]
            .spacing(8),
        )
        .into()
}

fn symmetry_panel<'a>(mirror: Mirror) -> Element<'a, Message> {
    Panel::new(i18n::symmetry())
        .card(
            i18n::symmetry_axes(),
            column![
                checkbox(mirror.horizontal)
                    .style(controls::checkbox_style)
                    .label(i18n::mirror_horizontal())
                    .text_size(13)
                    .on_toggle(Message::MirrorHorizontalToggled),
                checkbox(mirror.vertical)
                    .style(controls::checkbox_style)
                    .label(i18n::mirror_vertical())
                    .text_size(13)
                    .on_toggle(Message::MirrorVerticalToggled),
            ]
            .spacing(8),
        )
        .hint(i18n::symmetry_hint())
        .into()
}

fn placement_rows<'a>(
    placement: Placement,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    card(
        i18n::position_and_size(),
        column![
            field_row(i18n::position_x(), Field::FloatX, placement.x, typed),
            field_row(i18n::position_y(), Field::FloatY, placement.y, typed),
            field_row(i18n::width(), Field::FloatWidth, placement.width, typed),
            field_row(i18n::height(), Field::FloatHeight, placement.height, typed),
        ]
        .spacing(8),
    )
}

fn field_row<'a>(
    label: &'a str,
    field: Field,
    value: f32,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    let shown = match typed {
        Some((typed, text)) if typed == field => field.editable(text),
        _ => field.editable(&field.format(value)),
    };
    let unit = field.unit();
    let input = text_input("", &shown)
        .style(controls::text_input_style)
        .on_input(move |text| Message::FieldTyped(field, text))
        .on_submit(Message::FieldSubmitted)
        .size(13)
        .width(Length::Fixed(if unit.is_empty() { 72.0 } else { 50.0 }));
    let control: Element<'a, Message> = if unit.is_empty() {
        input.into()
    } else {
        row![input, text(unit).size(13)]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .width(Length::Fixed(72.0))
            .into()
    };

    row![
        text(label).size(13),
        Space::new().width(Length::Fill),
        control,
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn blur_slider<'a>(
    label: &'a str,
    field: Field,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    changed: fn(f32) -> Message,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    column![
        field_row(label, field, value, typed),
        slider(range, value, changed)
            .on_release(Message::FieldSubmitted)
            .step(field.slider_step())
            .style(controls::slider_style),
    ]
    .spacing(8)
    .into()
}

fn current_colour<'a>(brush: &Brush) -> Element<'a, Message> {
    let colour = from_bytes(brush.colour);
    let picking = brush.tool == Tool::Pipette;

    row![
        button(Space::new().width(Length::Fill).height(Length::Fixed(40.0)))
            .width(Length::Fill)
            .style(move |_theme, status| button::Style {
                background: Some(colour.into()),
                border: iced::Border {
                    color: if matches!(status, button::Status::Hovered) {
                        theme::colours().accent
                    } else {
                        theme::colours().border
                    },
                    width: 1.0,
                    radius: 0.0.into()
                },
                ..Default::default()
            })
            .on_press(Message::PickerOpened(Picking::Current)),
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

    column![
        grid,
        wide_button(i18n::add_colour(), Message::PickerOpened(Picking::Adding))
    ]
    .spacing(6)
    .into()
}

fn custom_menu_view<'a>(index: usize) -> Element<'a, Message> {
    container(
        column![
            context_button(i18n::edit(), Message::CustomColourEditRequested(index)),
            context_button(i18n::remove(), Message::CustomColourRemoved(index)),
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

fn stickers<'a>(
    history: &'a [crate::app::Sticker],
    blur_active: bool,
    settings: blur::Settings,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    let mut panel = Panel::new(i18n::stickers_heading())
        .hint(i18n::stickers_hint())
        .card(
            i18n::insert(),
            row![
                tile(
                    icons::art(icons::IMAGE, THUMBNAIL as f32, Some(tile_ink(false))),
                    i18n::add_image(),
                    false,
                    Message::StickerRequested,
                ),
                tile(
                    icons::art(icons::BLUR, THUMBNAIL as f32, Some(tile_ink(blur_active)),),
                    i18n::blur_box(),
                    blur_active,
                    Message::BlurPicked,
                ),
            ]
            .spacing(4),
        );

    if blur_active {
        let algorithm = iced::widget::pick_list(
            blur::ALGORITHMS,
            Some(settings.algorithm),
            Message::BlurAlgorithmPicked,
        )
        .style(controls::pick_list_style)
        .menu_style(controls::menu_style)
        .text_size(13)
        .width(Length::Fill);
        let mut options = column![text(i18n::blur_algorithm()).size(13), algorithm,].spacing(8);

        match settings.algorithm {
            blur::Algorithm::Box => {
                options = options.push(blur_slider(
                    i18n::blur_radius(),
                    Field::BlurStrength,
                    settings.strength,
                    blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                    Message::BlurStrengthChanged,
                    typed,
                ));
            }
            blur::Algorithm::Gaussian => {
                options = options.push(blur_slider(
                    i18n::blur_strength(),
                    Field::BlurStrength,
                    settings.strength,
                    blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                    Message::BlurStrengthChanged,
                    typed,
                ));
            }
            blur::Algorithm::Median => {
                options = options
                    .push(blur_slider(
                        i18n::blur_radius(),
                        Field::BlurStrength,
                        settings.strength,
                        blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                        Message::BlurStrengthChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_passes(),
                        Field::BlurPasses,
                        settings.passes as f32,
                        blur::MIN_PASSES..=blur::MAX_PASSES,
                        Message::BlurPassesChanged,
                        typed,
                    ));
            }
            blur::Algorithm::Motion => {
                options = options
                    .push(blur_slider(
                        i18n::blur_distance(),
                        Field::BlurStrength,
                        settings.strength,
                        blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                        Message::BlurStrengthChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_angle(),
                        Field::BlurAngle,
                        settings.angle,
                        blur::MIN_ANGLE..=blur::MAX_ANGLE,
                        Message::BlurAngleChanged,
                        typed,
                    ));
            }
            blur::Algorithm::Bilateral => {
                options = options
                    .push(blur_slider(
                        i18n::blur_radius(),
                        Field::BlurStrength,
                        settings.strength,
                        blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                        Message::BlurStrengthChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_edge_preservation(),
                        Field::BlurDetail,
                        settings.detail,
                        blur::MIN_DETAIL..=blur::MAX_DETAIL,
                        Message::BlurDetailChanged,
                        typed,
                    ));
            }
            blur::Algorithm::Directional => {
                options = options
                    .push(blur_slider(
                        i18n::blur_strength(),
                        Field::BlurStrength,
                        settings.strength,
                        blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                        Message::BlurStrengthChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_angle(),
                        Field::BlurAngle,
                        settings.angle,
                        blur::MIN_ANGLE..=blur::MAX_ANGLE,
                        Message::BlurAngleChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_directionality(),
                        Field::BlurDetail,
                        settings.detail,
                        blur::MIN_DETAIL..=blur::MAX_DETAIL,
                        Message::BlurDetailChanged,
                        typed,
                    ));
            }
            blur::Algorithm::Defocus => {
                options = options
                    .push(blur_slider(
                        i18n::blur_radius(),
                        Field::BlurStrength,
                        settings.strength,
                        blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                        Message::BlurStrengthChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_blades(),
                        Field::BlurBlades,
                        settings.blades as f32,
                        blur::MIN_BLADES..=blur::MAX_BLADES,
                        Message::BlurBladesChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_rotation(),
                        Field::BlurAngle,
                        settings.angle,
                        blur::MIN_ANGLE..=blur::MAX_ANGLE,
                        Message::BlurAngleChanged,
                        typed,
                    ));
            }
            blur::Algorithm::Kawase => {
                options = options
                    .push(blur_slider(
                        i18n::blur_radius(),
                        Field::BlurStrength,
                        settings.strength,
                        blur::MIN_STRENGTH..=blur::MAX_STRENGTH,
                        Message::BlurStrengthChanged,
                        typed,
                    ))
                    .push(blur_slider(
                        i18n::blur_passes(),
                        Field::BlurPasses,
                        settings.passes as f32,
                        blur::MIN_PASSES..=blur::MAX_PASSES,
                        Message::BlurPassesChanged,
                        typed,
                    ));
            }
        }
        panel = panel.card(i18n::options(), options).hint(i18n::blur_hint());
    }

    if !history.is_empty() {
        let mut grid = column![].spacing(6);
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
        panel = panel.card(i18n::stickers_added(), grid);
    }

    panel.into()
}

fn canvas_panel<'a>(
    state: &CanvasPanel,
    size: (u32, u32),
    transparent: bool,
) -> Element<'a, Message> {
    let unit = if state.percent {
        i18n::unit_percent_sign()
    } else {
        i18n::unit_px()
    };

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

    let mut resize = column![
        checkbox(state.lock_aspect)
            .style(controls::checkbox_style)
            .label(i18n::lock_aspect_ratio())
            .text_size(13)
            .on_toggle(Message::LockAspectToggled),
        checkbox(state.resize_image)
            .style(controls::checkbox_style)
            .label(i18n::resize_image_with_canvas())
            .text_size(13)
            .on_toggle(Message::ResizeImageToggled),
        field(i18n::width(), &state.width, Message::CanvasWidthEdited),
        field(i18n::height(), &state.height, Message::CanvasHeightEdited),
    ]
    .spacing(10);

    if state.resize_image {
        resize = resize.push(section(i18n::resampling())).push(
            iced::widget::pick_list(
                [
                    crate::doc::transform::Resampling::Smooth,
                    crate::doc::transform::Resampling::Nearest,
                ],
                Some(state.resampling),
                Message::ResamplingPicked,
            )
            .style(controls::pick_list_style)
            .menu_style(controls::menu_style)
            .text_size(13)
            .width(Length::Fill),
        );
    }

    resize = resize
        .push(
            row![
                button(text(i18n::unit_pixels()).size(12))
                    .style(|_theme, status| action_style(status))
                    .on_press(Message::CanvasUnitPicked(false)),
                button(text(i18n::unit_percent()).size(12))
                    .style(|_theme, status| action_style(status))
                    .on_press(Message::CanvasUnitPicked(true)),
                Space::new().width(Length::Fill),
                button(text(i18n::apply()).size(12))
                    .style(|_theme, status| action_style(status))
                    .on_press(Message::CanvasResizeSubmitted),
            ]
            .spacing(4),
        )
        .push(
            text(i18n::canvas_size(size.0, size.1))
                .size(12)
                .color(theme::colours().text_dim),
        );

    Panel::new(i18n::canvas_heading())
        .card(
            i18n::options(),
            column![
                checkbox(transparent)
                    .style(controls::checkbox_style)
                    .label(i18n::transparent_canvas())
                    .text_size(13)
                    .on_toggle(Message::TransparencyToggled),
                checkbox(state.show_canvas)
                    .style(controls::checkbox_style)
                    .label(i18n::show_canvas())
                    .text_size(13)
                    .on_toggle(Message::ShowCanvasToggled),
            ]
            .spacing(10),
        )
        .card(i18n::resize_canvas(), resize)
        .card(
            i18n::rotate_and_flip(),
            row![
                tool_button(
                    icons::ROTATE_ANTICLOCKWISE,
                    i18n::rotate_left(),
                    Message::Rotate(false)
                ),
                tool_button(icons::ROTATE, i18n::rotate_right(), Message::Rotate(true)),
                tool_button(
                    icons::FLIP_HORIZONTAL,
                    i18n::flip_horizontally(),
                    Message::Flip(true)
                ),
                tool_button(
                    icons::FLIP_VERTICAL,
                    i18n::flip_vertically(),
                    Message::Flip(false)
                ),
            ]
            .spacing(4),
        )
        .into()
}

// The only accent-filled buttons in the panel, so they carry the wash the tabs use when lit.
fn action_style(status: button::Status) -> button::Style {
    let c = theme::colours();
    button::Style {
        background: Some(if matches!(status, button::Status::Hovered) {
            theme::selection_wash()
        } else {
            c.accent.into()
        }),
        text_color: c.selection_text,
        border: iced::Border {
            radius: 2.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
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

    Panel::new(i18n::crop())
        .card(
            i18n::crop_framing(),
            column![
                grid,
                framing_tile(None, framing),
                row![
                    size_field(i18n::width(), fields.0, Message::CropWidthEdited),
                    size_field(i18n::height(), fields.1, Message::CropHeightEdited),
                ]
                .spacing(8),
                checkbox(lock)
                    .style(controls::checkbox_style)
                    .label(i18n::lock_aspect_ratio())
                    .text_size(13)
                    .on_toggle(Message::CropLockToggled),
            ]
            .spacing(10),
        )
        .loose(
            row![
                wide_button(i18n::cancel(), Message::CropCancelled),
                wide_button(i18n::done(), Message::CropApplied),
            ]
            .spacing(8),
        )
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

    let label = kind.map_or_else(i18n::crop_custom, Framing::name);
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

pub fn cutout_panel<'a>(
    cutout: &'a crate::app::CuttingOut,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    let (refining, adding, autofill) = (cutout.refining, cutout.adding, cutout.autofill);
    if !refining {
        return Panel::new(i18n::smart_cutout())
            .loose(label(i18n::cutout_choose()))
            .hint(i18n::cutout_choose_hint())
            .card(i18n::cutout_target(), cutout_target(cutout, typed))
            .loose(
                row![
                    wide_button(i18n::cancel(), Message::CutoutCancelled),
                    wide_button(i18n::cutout_next(), Message::CutoutNext),
                ]
                .spacing(8),
            )
            .into();
    }

    let hint = if adding {
        i18n::cutout_add_hint()
    } else {
        i18n::cutout_remove_hint()
    };

    let panel = Panel::new(i18n::smart_cutout());
    let panel = match cutout.aim.target {
        Target::Colour => panel.card(i18n::cutout_colour(), cutout_tone(cutout, typed)),
        _ => panel,
    };
    panel
        .card(
            i18n::cutout_refine(),
            column![
                row![
                    brush_tile(
                        i18n::cutout_add(),
                        crate::assets::tool_icons::MARKER,
                        adding,
                        Message::CutoutBrushPicked(true)
                    ),
                    brush_tile(
                        i18n::cutout_remove(),
                        crate::assets::tool_icons::ERASER,
                        !adding,
                        Message::CutoutBrushPicked(false)
                    ),
                ]
                .spacing(8),
                text(hint).size(12).color(theme::colours().text_dim),
                blur_slider(
                    i18n::cutout_brush_radius(),
                    Field::CutoutBrush,
                    cutout.brush,
                    1.0..=200.0,
                    |v| Message::CutoutFieldChanged(Field::CutoutBrush, v),
                    typed
                ),
            ]
            .spacing(10),
        )
        .card(
            i18n::cutout_edges(),
            column![
                blur_slider(
                    i18n::cutout_radius(),
                    Field::CutoutRadius,
                    cutout.settings.radius,
                    0.0..=32.0,
                    |v| Message::CutoutFieldChanged(Field::CutoutRadius, v),
                    typed
                ),
                blur_slider(
                    i18n::cutout_feather(),
                    Field::CutoutFeather,
                    cutout.settings.feather,
                    0.0..=20.0,
                    |v| Message::CutoutFieldChanged(Field::CutoutFeather, v),
                    typed
                ),
                blur_slider(
                    i18n::cutout_smooth(),
                    Field::CutoutSmooth,
                    cutout.settings.smooth,
                    0.0..=20.0,
                    |v| Message::CutoutFieldChanged(Field::CutoutSmooth, v),
                    typed
                ),
                blur_slider(
                    i18n::cutout_shift(),
                    Field::CutoutShift,
                    cutout.settings.shift,
                    -20.0..=20.0,
                    |v| Message::CutoutFieldChanged(Field::CutoutShift, v),
                    typed
                ),
            ]
            .spacing(10),
        )
        .card(i18n::cutout_options(), cutout_options(cutout, autofill))
        .hint(if cutout.busy {
            i18n::cutout_working()
        } else if cutout.failed {
            i18n::cutout_failed()
        } else if cutout.result.as_ref().is_some_and(|r| r.colour_fallback) {
            i18n::cutout_fallback()
        } else {
            i18n::cutout_undo_hint()
        })
        .loose(
            row![
                wide_button(i18n::cutout_back(), Message::CutoutBack),
                button(text(i18n::done()).center())
                    .width(Length::Fill)
                    .on_press_maybe(
                        (!cutout.busy && !cutout.failed).then_some(Message::CutoutDone)
                    ),
            ]
            .spacing(8),
        )
        .into()
}

fn cutout_target<'a>(
    cutout: &'a crate::app::CuttingOut,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    let target = cutout.aim.target;
    let chosen = Target::ALL.iter().position(|t| *t == target).unwrap_or(0);
    let mut card = column![
        segmented::segmented(Target::ALL.map(Target::name).to_vec(), chosen, |index| {
            Message::CutoutTargetPicked(Target::ALL[index])
        },),
        text(if target == Target::Colour {
            i18n::cutout_colour_hint()
        } else {
            i18n::cutout_target_hint()
        })
        .size(12)
        .color(theme::colours().text_dim),
    ]
    .spacing(8);
    if target == Target::Colour {
        card = card.push(cutout_tone(cutout, typed));
    }
    card.into()
}

fn cutout_options<'a>(cutout: &'a crate::app::CuttingOut, autofill: bool) -> Element<'a, Message> {
    let [r, g, b, a] = cutout.ground;
    let ground = Color::from_rgba8(r, g, b, a as f32 / 255.0);
    let mut options = column![
        checkbox(autofill)
            .style(controls::checkbox_style)
            .label(i18n::cutout_autofill())
            .text_size(13)
            .on_toggle(Message::CutoutAutofillToggled),
        checkbox(cutout.settings.decontaminate)
            .style(controls::checkbox_style)
            .label(i18n::cutout_decontaminate())
            .text_size(13)
            .on_toggle(Message::CutoutDecontaminateToggled),
        text(i18n::cutout_preview())
            .size(12)
            .color(theme::colours().text_dim),
        iced::widget::pick_list(
            crate::app::CutoutPreview::ALL,
            Some(cutout.preview),
            Message::CutoutPreviewPicked,
        )
        .text_size(13),
    ]
    .spacing(10);
    if cutout.preview == crate::app::CutoutPreview::Colour {
        options = options.push(
            button(Space::new().width(Length::Fill).height(Length::Fixed(26.0)))
                .width(Length::Fill)
                .style(move |_theme, status| button::Style {
                    background: Some(ground.into()),
                    border: iced::Border {
                        color: if matches!(status, button::Status::Hovered) {
                            theme::colours().accent
                        } else {
                            theme::colours().border
                        },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                })
                .on_press(Message::PickerOpened(Picking::CutoutGround)),
        );
    }
    options.into()
}

fn cutout_tone<'a>(
    cutout: &'a crate::app::CuttingOut,
    typed: Option<(Field, &'a str)>,
) -> Element<'a, Message> {
    let tone = cutout.aim.tone;
    column![
        container(Space::new().height(Length::Fixed(26.0)))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(
                    Color::from_rgba8(tone[0], tone[1], tone[2], tone[3] as f32 / 255.0).into(),
                ),
                border: iced::Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: theme::colours().border,
                },
                ..container::Style::default()
            }),
        blur_slider(
            i18n::tolerance(),
            Field::CutoutTolerance,
            cutout.aim.tolerance,
            0.0..=1.0,
            |v| Message::CutoutFieldChanged(Field::CutoutTolerance, v),
            typed,
        ),
    ]
    .spacing(8)
    .into()
}

fn brush_tile<'a>(
    label: &'a str,
    art: &'static [u8],
    active: bool,
    press: Message,
) -> Element<'a, Message> {
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
    .on_press(press)
    .into()
}
