use crate::app::Message;
use crate::canvas::{self, NewCanvas, Ratio};
use crate::config::Config;
use crate::ui::icons::{self, icon};
use crate::ui::theme::{self, Choice, Mode, Scheme};

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input, toggler};
use iced::{Element, Length, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    About,
    Open,
    SaveAs,
    Settings,
}

const ISSUES_URL: &str = "https://github.com/ItzELECTR0/RustyPaint/issues";
const REPO_URL: &str = "https://github.com/ItzELECTR0/RustyPaint";

const RAIL: f32 = 250.0;
const ITEM: f32 = 38.0;
const MARKER: f32 = 3.0;

pub fn view<'a>(
    page: Page,
    title: &'a str,
    modified: bool,
    config: &Config,
    viewport: Size,
    custom: (&'a str, &'a str),
) -> Element<'a, Message> {
    let name = if modified {
        format!("{title}* - RustyPaint")
    } else {
        format!("{title} - RustyPaint")
    };
    let heading =
        container(text(name).size(13).color(theme::colours().text)).padding(iced::Padding {
            top: 10.0,
            right: 0.0,
            bottom: 14.0,
            left: 46.0,
        });

    let rail = column![
        heading,
        item(icons::BACK, "Back", None, page, Message::MenuClosed),
        rule(),
        item(icons::NEW, "New", None, page, Message::NewRequested),
        item(
            icons::OPEN,
            "Open",
            Some(Page::Open),
            page,
            Message::MenuPagePicked(Page::Open)
        ),
        item(
            icons::INSERT,
            "Insert",
            None,
            page,
            Message::StickerRequested
        ),
        item(icons::SAVE, "Save", None, page, Message::SaveRequested),
        item(
            icons::SAVE_AS,
            "Save as",
            Some(Page::SaveAs),
            page,
            Message::MenuPagePicked(Page::SaveAs),
        ),
        Space::new().height(Length::Fill),
        item(
            icons::SETTINGS,
            "Settings",
            Some(Page::Settings),
            page,
            Message::MenuPagePicked(Page::Settings),
        ),
        item(
            icons::ABOUT,
            "About",
            Some(Page::About),
            page,
            Message::MenuPagePicked(Page::About),
        ),
        Space::new().height(Length::Fixed(12.0)),
    ];

    let pane: Element<'_, Message> = match page {
        Page::About => pane_about(),
        Page::Open => pane_open(),
        Page::SaveAs => pane_save_as(),
        Page::Settings => pane_settings(config, viewport, custom),
    };

    row![
        container(rail)
            .width(Length::Fixed(RAIL))
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(theme::veiled(theme::colours().menu_rail).into()),
                ..Default::default()
            }),
        container(pane)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(iced::Padding {
                top: 30.0,
                right: 40.0,
                bottom: 40.0,
                left: 46.0
            })
            .style(|_theme| container::Style {
                background: Some(theme::veiled(theme::colours().side_panel).into()),
                ..Default::default()
            }),
    ]
    .into()
}

fn item<'a>(
    drawing: &'static [u8],
    label: &'a str,
    page: Option<Page>,
    open: Page,
    press: Message,
) -> Element<'a, Message> {
    let active = page == Some(open);
    let marker = container(
        Space::new()
            .width(Length::Fixed(MARKER))
            .height(Length::Fill),
    )
    .style(move |_theme| container::Style {
        background: active.then(|| theme::colours().accent.into()),
        ..Default::default()
    });

    let face = row![
        marker,
        Space::new().width(Length::Fixed(16.0)),
        icon(
            drawing,
            15.0,
            if active {
                theme::colours().accent_text
            } else {
                theme::colours().text
            },
        ),
        Space::new().width(Length::Fixed(14.0)),
        text(label).size(14).color(if active {
            theme::colours().accent_text
        } else {
            theme::colours().text
        }),
    ]
    .height(Length::Fill)
    .align_y(iced::Alignment::Center);

    button(face)
        .width(Length::Fill)
        .height(Length::Fixed(ITEM))
        .padding(0)
        .style(|_theme, status| button::Style {
            background: matches!(status, button::Status::Hovered)
                .then(|| theme::colours().control_hover.into()),
            text_color: theme::colours().text,
            ..Default::default()
        })
        .on_press(press)
        .into()
}

fn rule<'a>() -> Element<'a, Message> {
    container(
        container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(|_theme| {
            container::Style {
                background: Some(theme::colours().border.into()),
                ..Default::default()
            }
        }),
    )
    .padding(iced::Padding {
        top: 6.0,
        right: 24.0,
        bottom: 6.0,
        left: 30.0,
    })
    .into()
}

fn pane_open<'a>() -> Element<'a, Message> {
    column![title("Open"), flat("Browse files", Message::OpenRequested)]
        .spacing(20)
        .into()
}

fn pane_save_as<'a>() -> Element<'a, Message> {
    column![
        title("Save as copy"),
        text("Choose a file format")
            .size(14)
            .color(theme::colours().text),
        format_tile(),
    ]
    .spacing(12)
    .into()
}

fn pane_settings<'a>(
    config: &Config,
    viewport: Size,
    custom: (&'a str, &'a str),
) -> Element<'a, Message> {
    let options = column![
        title("Settings"),
        subheading("Appearance"),
        note("Paint 3D only ever had the light one."),
        row(Choice::ALL.map(|c| pill(c.name(), config.theme == c, Message::ThemePicked(c))))
            .spacing(6),
        resolved(config.theme),
        Space::new().height(Length::Fixed(6.0)),
        note("Accent"),
        row(Scheme::ALL.map(|s| accent_tile(s, config.accent))).spacing(8),
        divider(),
        subheading("Acrylic panels"),
        note("Let your compositor blur what is behind RustyPaint."),
        toggler(config.acrylic)
            .style(crate::ui::controls::toggler_style)
            .label(if config.acrylic { "On" } else { "Off" })
            .text_size(13)
            .on_toggle(Message::AcrylicToggled),
        divider(),
        subheading("Unsaved changes"),
        note(
            "Ask before anything that would throw away work you have not saved. Turn it off and \
             New, Open and closing the window take you at your word.",
        ),
        toggler(config.confirm_discard)
            .style(crate::ui::controls::toggler_style)
            .label(if config.confirm_discard { "On" } else { "Off" })
            .text_size(13)
            .on_toggle(Message::ConfirmDiscardToggled),
        divider(),
        subheading("New canvas"),
        note("What size a new picture starts at."),
        row![
            pill(
                "Fit the window",
                matches!(config.new_canvas, NewCanvas::Fit(_)),
                fit_default()
            ),
            pill(
                "Resolution",
                matches!(config.new_canvas, NewCanvas::Fixed(..)),
                Message::NewCanvasPicked(NewCanvas::Fixed(
                    canvas::RESOLUTIONS[1].1,
                    canvas::RESOLUTIONS[1].2
                ))
            ),
            pill(
                "Custom",
                matches!(config.new_canvas, NewCanvas::Custom(..)),
                Message::NewCanvasPicked(NewCanvas::Custom(1152, 648))
            ),
        ]
        .spacing(6),
        new_canvas_choice(config.new_canvas, custom),
        text(canvas::describe(
            config.new_canvas,
            viewport,
            crate::doc::Document::DEFAULT_SIZE
        ))
        .size(12)
        .color(theme::colours().accent_text),
        divider(),
        subheading("Title bar"),
        note(
            "Ours has the window buttons in the tab strip. Native is whatever the rest of your \
             windows have, which on some compositors is nothing at all.",
        ),
        iced::widget::checkbox(config.decorations)
            .style(crate::ui::controls::checkbox_style)
            .label("Use native decorations")
            .text_size(13)
            .on_toggle(Message::DecorationsToggled),
    ]
    .spacing(8)
    .max_width(760.0);

    scrollable(options).height(Length::Fill).into()
}

fn pane_about<'a>() -> Element<'a, Message> {
    let credit = column![
        text("Made by ELECTRO")
            .size(13)
            .center()
            .color(theme::colours().text_dim),
        link("Source on GitHub", REPO_URL),
    ]
    .spacing(6)
    .align_x(iced::Alignment::Center);

    crate::ui::centred(
        column![
            icons::art(crate::assets::APP_ICON_SVG, 96.0, None),
            Space::new().height(Length::Fixed(10.0)),
            text("RustyPaint")
                .size(28)
                .center()
                .color(theme::colours().text),
            text("Paint 3D's 2D editor without the 3D.")
                .size(13)
                .center()
                .color(theme::colours().text_dim),
            text(concat!("Version ", env!("CARGO_PKG_VERSION")))
                .size(12)
                .center()
                .color(theme::colours().text_dim),
            Space::new().height(Length::Fixed(18.0)),
            text("Something broken, or something missing?")
                .size(13)
                .center()
                .color(theme::colours().text),
            link("Report a problem or ask for a feature", ISSUES_URL),
            Space::new().height(Length::Fixed(18.0)),
            credit,
        ]
        .spacing(6)
        .max_width(420.0)
        .align_x(iced::Alignment::Center),
    )
}

fn link<'a>(label: &'a str, url: &'static str) -> Element<'a, Message> {
    button(
        row![
            icon(icons::LINK, 13.0, theme::colours().text),
            text(label).size(13),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .height(Length::Fixed(30.0))
    .padding(iced::Padding {
        top: 0.0,
        right: 14.0,
        bottom: 0.0,
        left: 14.0,
    })
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
            radius: 15.0.into(),
        },
        ..Default::default()
    })
    .on_press(Message::LinkOpened(url))
    .into()
}

fn fit_default() -> Message {
    Message::NewCanvasPicked(NewCanvas::Fit(Ratio::Widescreen))
}

fn resolved<'a>(choice: Choice) -> Element<'a, Message> {
    if choice != Choice::Auto {
        return Space::new().into();
    }
    let line = match theme::detect::system() {
        Some(Mode::Dark) => "Your desktop is set to dark.",
        Some(Mode::Light) => "Your desktop is set to light.",
        None => "No system preference found, using light.",
    };
    text(line).size(12).color(theme::colours().text_dim).into()
}

fn new_canvas_choice<'a>(preset: NewCanvas, custom: (&'a str, &'a str)) -> Element<'a, Message> {
    match preset {
        NewCanvas::Fit(chosen) => row(Ratio::ALL.map(|r| {
            pill(
                r.name(),
                r == chosen,
                Message::NewCanvasPicked(NewCanvas::Fit(r)),
            )
        }))
        .spacing(6)
        .into(),
        NewCanvas::Fixed(w, h) => column(canvas::RESOLUTIONS.chunks(3).map(|chunk| {
            row(chunk.iter().map(|(name, rw, rh)| {
                pill(
                    name,
                    (*rw, *rh) == (w, h),
                    Message::NewCanvasPicked(NewCanvas::Fixed(*rw, *rh)),
                )
            }))
            .spacing(6)
            .into()
        }))
        .spacing(6)
        .into(),
        NewCanvas::Custom(..) => row![
            field("Width", custom.0, Message::NewCanvasWidthEdited),
            field("Height", custom.1, Message::NewCanvasHeightEdited),
        ]
        .spacing(10)
        .into(),
    }
}

fn field<'a>(
    label: &'a str,
    value: &'a str,
    on_change: fn(String) -> Message,
) -> Element<'a, Message> {
    row![
        text(label).size(13).color(theme::colours().text),
        text_input("", value)
            .style(crate::ui::controls::text_input_style)
            .on_input(on_change)
            .size(13)
            .width(Length::Fixed(78.0)),
        text("px").size(13).color(theme::colours().text_dim),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn pill<'a>(label: &'a str, active: bool, press: Message) -> Element<'a, Message> {
    button(crate::ui::centred(text(label).size(12).center()))
        .height(Length::Fixed(26.0))
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 12.0,
        })
        .style(move |_theme, status| {
            let c = theme::colours();
            button::Style {
                background: Some(if active {
                    c.accent.into()
                } else if matches!(status, button::Status::Hovered) {
                    c.control_hover.into()
                } else {
                    c.control.into()
                }),
                text_color: if active { c.selection_text } else { c.text },
                border: iced::Border {
                    color: if active { c.accent } else { c.border },
                    width: 1.0,
                    radius: 13.0.into(),
                },
                ..Default::default()
            }
        })
        .on_press(press)
        .into()
}

fn accent_tile<'a>(scheme: Scheme, chosen: Scheme) -> Element<'a, Message> {
    let colours = theme::palette_for(theme::mode(), scheme);
    let wash = iced::Background::Gradient(iced::Gradient::Linear(
        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
            .add_stop(0.0, colours.selection_from)
            .add_stop(1.0, colours.selection_to),
    ));
    let active = scheme == chosen;

    button(
        column![
            container(Space::new().width(Length::Fill).height(Length::Fixed(26.0))).style(
                move |_theme| container::Style {
                    background: Some(wash),
                    ..Default::default()
                }
            ),
            text(scheme.name()).size(12).center().width(Length::Fill),
        ]
        .spacing(6),
    )
    .width(Length::Fixed(96.0))
    .padding(6)
    .style(move |_theme, status| {
        let c = theme::colours();
        button::Style {
            background: Some(
                if active || matches!(status, button::Status::Hovered) {
                    c.control_hover
                } else {
                    c.control
                }
                .into(),
            ),
            text_color: c.text,
            border: iced::Border {
                color: if active { c.accent } else { c.border },
                width: if active { 2.0 } else { 1.0 },
                radius: 2.0.into(),
            },
            ..Default::default()
        }
    })
    .on_press(Message::AccentPicked(scheme))
    .into()
}

fn subheading<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(18).color(theme::colours().text).into()
}

fn note<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(13).color(theme::colours().text_dim).into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(
        container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(|_theme| {
            container::Style {
                background: Some(theme::colours().border.into()),
                ..Default::default()
            }
        }),
    )
    .padding(iced::Padding {
        top: 8.0,
        right: 40.0,
        bottom: 8.0,
        left: 0.0,
    })
    .into()
}

fn format_tile<'a>() -> Element<'a, Message> {
    button(crate::ui::centred(
        column![
            icon(icons::IMAGE, 26.0, theme::colours().text),
            text("Image").size(12)
        ]
        .spacing(10)
        .align_x(iced::Alignment::Center),
    ))
    .width(Length::Fixed(82.0))
    .height(Length::Fixed(82.0))
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
        ..Default::default()
    })
    .on_press(Message::SaveAsRequested)
    .into()
}

fn title<'a>(label: &'a str) -> Element<'a, Message> {
    text(label).size(28).color(theme::colours().text).into()
}

fn flat<'a>(label: &'a str, press: Message) -> Element<'a, Message> {
    button(crate::ui::centred(text(label).size(13).center()))
        .width(Length::Fixed(132.0))
        .height(Length::Fixed(30.0))
        .style(|_theme, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered) {
                    theme::colours().control_hover
                } else {
                    theme::colours().border
                }
                .into(),
            ),
            text_color: theme::colours().text,
            ..Default::default()
        })
        .on_press(press)
        .into()
}
