use crate::app::{Field, LayerAction, Message};
use crate::i18n;
use crate::ui::controls;
use crate::ui::icons::{self, icon};
use crate::ui::strings;
use crate::ui::theme;

use iced::widget::{
    Space, button, column, container, hover, mouse_area, row, scrollable, slider, stack, text,
    text_input, tooltip,
};
use iced::{Element, Length};

pub const WIDTH: f32 = 120.0;
pub const MARGIN: f32 = 8.0;

// Windows 11 rounds its windows by this much: enough to read as soft, not as a pill.
const RADIUS: f32 = 8.0;
pub const TILE: (f32, f32) = (96.0, 72.0);
const CHIP: f32 = 22.0;
const BUTTON: f32 = 28.0;

pub struct Tile {
    pub visible: bool,
    pub mergeable: bool,
    pub thumbnail: Option<iced::widget::image::Handle>,
}

pub struct Bar<'a> {
    // Bottom layer first, as the stack holds them.
    pub tiles: Vec<Tile>,
    pub active: usize,
    pub opacity: u8,
    pub typed: Option<&'a str>,
    pub layered: bool,
    // Crop and Smart cutout hold the picture still until they finish.
    pub held: bool,
}

pub fn bar<'a>(bar: Bar<'a>) -> Element<'a, Message> {
    let open = !bar.held;
    let count = bar.tiles.len();
    let active = bar.active;

    let mut header = row![wide_button(
        icons::LAYER_NEW,
        strings::with_key(i18n::layer_new(), &strings::shift_key("N")),
        open.then_some(Message::Layer(LayerAction::Add)),
    )]
    .spacing(4);
    if bar.layered {
        header = header.push(hint(
            square_button(
                icons::LAYER_FLATTEN,
                open.then_some(Message::Layer(LayerAction::Flatten)),
            ),
            i18n::layer_flatten(),
        ));
    }

    let tiles = column(
        bar.tiles
            .into_iter()
            .enumerate()
            .rev()
            .map(|(index, tile)| layer_tile(index, tile, index == active, count, open)),
    )
    .spacing(6);
    let list = scrollable(
        container(tiles)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(iced::Padding::default().bottom(4)),
    )
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(4)
            .scroller_width(4)
            .margin(3),
    ))
    .height(Length::Fill)
    .style(crate::ui::sidebar::scroll_style);

    let opacity = bar.opacity as f32 / 255.0;
    let footer = column![
        text(i18n::opacity())
            .size(11)
            .color(theme::colours().text_dim)
            .center()
            .width(Length::Fill),
        opacity_field(opacity, bar.typed),
        slider(0.0..=1.0_f32, opacity, |v| Message::Layer(
            LayerAction::Opacity(v)
        ))
        .step(1.0_f32 / 255.0)
        .style(controls::slider_style)
        .on_release(Message::Layer(LayerAction::OpacitySettled)),
    ]
    .spacing(6)
    .align_x(iced::Alignment::Center);

    let inset = (WIDTH - TILE.0) / 2.0;
    container(
        column![
            container(header).padding([0.0, inset]),
            list,
            container(footer).padding([0.0, inset]),
        ]
        .spacing(10),
    )
    .width(Length::Fixed(WIDTH))
    .height(Length::Fill)
    .padding(iced::Padding::default().top(inset).bottom(inset))
    .style(|_theme| container::Style {
        background: Some(theme::colours().side_panel.into()),
        border: iced::Border {
            color: theme::colours().border,
            width: 1.0,
            radius: RADIUS.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: theme::colours().shadow,
                ..iced::Color::BLACK
            },
            offset: iced::Vector::new(0.0, 3.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    })
    .into()
}

fn layer_tile<'a>(
    index: usize,
    tile: Tile,
    active: bool,
    count: usize,
    open: bool,
) -> Element<'a, Message> {
    let picture: Element<'a, Message> = match tile.thumbnail {
        Some(handle) => iced::widget::image(handle)
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(iced::ContentFit::Contain)
            .opacity(if tile.visible { 1.0_f32 } else { 0.35 })
            .into(),
        None => Space::new().into(),
    };
    let face = container(picture)
        .width(Length::Fixed(TILE.0))
        .height(Length::Fixed(TILE.1))
        .padding(3)
        .style(move |_theme| frame(active, false));

    // A hidden layer says so without being hovered, in the spot its eye button takes.
    let mut base = stack![face];
    if !tile.visible {
        base = base.push(
            container(chip_face(icons::EYE_CLOSED))
                .padding(4)
                .width(Length::Fill)
                .height(Length::Fill),
        );
    }
    if !open {
        return base.into();
    }
    let base = mouse_area(base)
        .on_press(Message::Layer(LayerAction::Select(index)))
        .interaction(iced::mouse::Interaction::Pointer);

    let at = |action: LayerAction| Message::Layer(LayerAction::At(index, Box::new(action)));
    let (eye, eye_hint) = if tile.visible {
        (icons::EYE, i18n::layer_hide())
    } else {
        (icons::EYE_CLOSED, i18n::layer_show())
    };
    let mut corner = row![].spacing(2);
    corner = corner.push(chip(
        icons::LAYER_DUPLICATE,
        strings::with_key(i18n::layer_duplicate(), &strings::command_key("J")),
        at(LayerAction::Duplicate),
    ));
    if count > 1 {
        corner = corner.push(chip(
            icons::LAYER_DELETE,
            i18n::layer_delete().to_owned(),
            at(LayerAction::Delete),
        ));
    }
    let mut moves = row![].spacing(2);
    if index + 1 < count {
        moves = moves.push(chip(
            icons::LAYER_UP,
            strings::with_key(i18n::layer_move_up(), &strings::command_key("]")),
            at(LayerAction::Move(true)),
        ));
    }
    if index > 0 {
        moves = moves.push(chip(
            icons::LAYER_DOWN,
            strings::with_key(i18n::layer_move_down(), &strings::command_key("[")),
            at(LayerAction::Move(false)),
        ));
    }
    if tile.mergeable {
        moves = moves.push(chip(
            icons::LAYER_MERGE,
            strings::with_key(i18n::layer_merge_down(), &strings::command_key("E")),
            at(LayerAction::Merge),
        ));
    }
    let actions = container(
        column![
            row![
                chip(
                    eye,
                    eye_hint.to_owned(),
                    Message::Layer(LayerAction::ToggleVisible(index)),
                ),
                Space::new().width(Length::Fill),
                corner,
            ],
            Space::new().height(Length::Fill),
            container(moves).center_x(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .padding(4)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme| frame(active, true));

    hover(base, actions)
}

// The active layer wears the accent; the one under the pointer a plain outline.
fn frame(active: bool, hovered: bool) -> container::Style {
    let c = theme::colours();
    let (color, width) = match (active, hovered) {
        (true, _) => (c.accent, 2.0),
        (false, true) => (c.text_dim, 1.0),
        (false, false) => (c.border, 1.0),
    };
    container::Style {
        background: (!hovered).then(|| c.panel_group.into()),
        border: iced::Border {
            color,
            width,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn chip_face<'a>(drawing: &'static [u8]) -> Element<'a, Message> {
    container(crate::ui::centred(icon(
        drawing,
        13.0,
        theme::colours().text,
    )))
    .width(Length::Fixed(CHIP))
    .height(Length::Fixed(CHIP))
    .style(|_theme| container::Style {
        background: Some(chip_fill(false).into()),
        border: chip_border(),
        ..Default::default()
    })
    .into()
}

fn chip<'a>(drawing: &'static [u8], label: String, press: Message) -> Element<'a, Message> {
    hint(
        button(crate::ui::centred(icon(
            drawing,
            13.0,
            theme::colours().text,
        )))
        .width(Length::Fixed(CHIP))
        .height(Length::Fixed(CHIP))
        .padding(0)
        .style(|_theme, status| button::Style {
            background: Some(
                chip_fill(matches!(
                    status,
                    button::Status::Hovered | button::Status::Pressed
                ))
                .into(),
            ),
            text_color: theme::colours().text,
            border: chip_border(),
            ..Default::default()
        })
        .on_press(press),
        label,
    )
}

// Opaque enough to read over any picture, so the buttons never depend on what they sit on.
fn chip_fill(lit: bool) -> iced::Color {
    let c = theme::colours();
    if lit {
        c.control_hover
    } else {
        iced::Color {
            a: 0.92,
            ..c.side_panel
        }
    }
}

fn chip_border() -> iced::Border {
    iced::Border {
        color: theme::colours().border,
        width: 1.0,
        radius: (CHIP / 2.0).into(),
    }
}

fn wide_button<'a>(
    drawing: &'static [u8],
    label: String,
    press: Option<Message>,
) -> Element<'a, Message> {
    hint(
        button(crate::ui::centred(icon(
            drawing,
            16.0,
            theme::colours().text,
        )))
        .width(Length::Fill)
        .height(Length::Fixed(BUTTON))
        .padding(0)
        .style(|_theme, status| header_style(status))
        .on_press_maybe(press),
        label,
    )
}

fn square_button<'a>(
    drawing: &'static [u8],
    press: Option<Message>,
) -> button::Button<'a, Message> {
    button(crate::ui::centred(icon(
        drawing,
        16.0,
        theme::colours().text,
    )))
    .width(Length::Fixed(BUTTON))
    .height(Length::Fixed(BUTTON))
    .padding(0)
    .style(|_theme, status| header_style(status))
    .on_press_maybe(press)
}

fn header_style(status: button::Status) -> button::Style {
    let c = theme::colours();
    button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                c.control_hover
            } else {
                c.control
            }
            .into(),
        ),
        text_color: c.text,
        border: iced::Border {
            color: c.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn opacity_field<'a>(opacity: f32, typed: Option<&'a str>) -> Element<'a, Message> {
    let field = Field::LayerOpacity;
    let shown = match typed {
        Some(text) => field.editable(text),
        None => field.editable(&field.format(opacity)),
    };
    row![
        text_input("", &shown)
            .style(controls::text_input_style)
            .on_input(move |text| Message::FieldTyped(field, text))
            .on_submit(Message::FieldSubmitted)
            .size(13)
            .width(Length::Fixed(48.0)),
        text(field.unit()).size(13),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .into()
}

// Toward the canvas, because past the bar's other side is the tools panel.
fn hint<'a>(
    control: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    tooltip(control, text(label).size(12), tooltip::Position::Left)
        .style(|_theme| container::Style {
            background: Some(theme::colours().control.into()),
            text_color: Some(theme::colours().text),
            border: iced::Border {
                color: theme::colours().border,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .padding(6)
        .into()
}
