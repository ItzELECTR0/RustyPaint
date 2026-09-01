use crate::canvas::NewCanvas;
use crate::doc::{Document, Rect};
use crate::gpu::{self};
use crate::paint::Tool;
use crate::select::{Lasso, Xform};
use crate::ui::dialog;
use crate::ui::icons::{self, icon};
use crate::ui::menu::{self};
use crate::ui::picker::{self};
use crate::ui::sidebar;
use crate::ui::strings;
use crate::ui::theme::{self, metrics};
use crate::ui::titlebar;

use iced::widget::{Space, button, column, container, row, shader, text};
use iced::{Element, Length, Point};

use super::*;

pub(super) const CHROME_HEIGHT: f32 = metrics::TOP_PANEL_BUTTON_HEIGHT
    + metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT
    + metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT;

pub(super) fn surface<'a>(
    content: impl Into<Element<'a, Message>>,
    colour: iced::Color,
) -> iced::widget::Container<'a, Message> {
    container(content)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(colour.into()),
            ..Default::default()
        })
}

pub(super) fn tab_button<'a>(
    label: &'a str,
    drawing: &'static [u8],
    wide: bool,
    active: bool,
) -> iced::widget::Button<'a, Message> {
    let ink = colour_on_tab(active);
    let face: Element<'a, Message> = if wide {
        column![
            icon(drawing, 14.0, ink),
            text(label)
                .size(12)
                .center()
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(2)
        .align_x(iced::Alignment::Center)
        .into()
    } else {
        icon(drawing, 16.0, ink)
    };

    button(crate::ui::centred(face))
        .width(Length::Fixed(if wide {
            metrics::TOP_PANEL_BUTTON_WIDTH
        } else {
            metrics::TOP_PANEL_THIN_BUTTON_WIDTH
        }))
        .height(Length::Fixed(metrics::TOP_PANEL_BUTTON_HEIGHT))
        .padding(2)
        .clip(true)
}

pub(super) fn bar_button<'a>(
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Button<'a, Message> {
    button(content)
        .width(Length::Fixed(metrics::TOP_PANEL_THIN_BUTTON_WIDTH))
        .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_BUTTON_HEIGHT))
}

pub(super) fn strip<'a>(
    drawing: &'static [u8],
    message: Message,
) -> iced::widget::Button<'a, Message> {
    button(crate::ui::centred(icon(
        drawing,
        16.0,
        colour_on_strip(false),
    )))
    .width(Length::Fixed(metrics::TOP_PANEL_THIN_BUTTON_WIDTH))
    .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_BUTTON_HEIGHT))
    .style(|_theme, _status| tool_style(false))
    .on_press(message)
}

pub(super) fn hint<'a>(
    control: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        control,
        text(label).size(12),
        iced::widget::tooltip::Position::Bottom,
    )
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

pub(super) const DOCUMENT_TAB_HEIGHT: f32 = 28.0;
const DOCUMENT_STRIP_PAD: f32 = 4.0;

// Tabs share the strip and shrink as more open, but never stretch past this on their own.
pub(super) const DOCUMENT_TAB_WIDTH: f32 = 190.0;

// Chrome bars are dark in both schemes, so the ink is the same and only its weight changes.
fn on_chrome(active: bool) -> iced::Color {
    iced::Color {
        a: if active { 1.0 } else { 0.62 },
        ..theme::colours().text_on_dark
    }
}

// The open tab is painted the colour of the strip it sits on top of, so it reads as joined to it.
fn document_tab_style(active: bool, status: button::Status) -> button::Style {
    let background = if active {
        Some(theme::colours().tab_bar.into())
    } else if matches!(status, button::Status::Hovered) {
        Some(
            iced::Color {
                a: 0.5,
                ..theme::colours().tab_bar
            }
            .into(),
        )
    } else {
        None
    };
    button::Style {
        background,
        text_color: on_chrome(active),
        border: iced::Border {
            radius: iced::border::radius(10).bottom(0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn document_close_style(status: button::Status) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered).then(|| {
            iced::Color {
                a: 0.22,
                ..theme::colours().text_on_dark
            }
            .into()
        }),
        border: iced::Border {
            radius: iced::border::radius(6),
            ..Default::default()
        },
        ..Default::default()
    }
}

// The two chrome greys are a shade apart, so the open tab is named by a line in the accent colour
// rather than by its background alone.
fn open_marker<'a>(active: bool) -> Element<'a, Message> {
    if !active {
        return Space::new().into();
    }
    column![
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(2.0))
            .style(|_theme| container::Style {
                background: Some(theme::colours().accent.into()),
                border: iced::Border {
                    radius: iced::border::radius(0).top(2),
                    ..Default::default()
                },
                ..Default::default()
            }),
        Space::new().height(Length::Fill),
    ]
    .into()
}

fn tab_menu_item<'a>(label: &'a str, key: &str, action: TabAction) -> Element<'a, Message> {
    button(
        row![
            text(label).size(12).color(theme::colours().text),
            Space::new().width(Length::Fill),
            text(key.to_owned())
                .size(11)
                .color(theme::colours().text_dim),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding([5, 9])
    .style(|_theme, status| button::Style {
        background: matches!(status, button::Status::Hovered)
            .then(|| theme::colours().control_hover.into()),
        text_color: theme::colours().text,
        border: iced::Border {
            radius: iced::border::radius(5),
            ..Default::default()
        },
        ..Default::default()
    })
    .on_press(Message::TabMenuPicked(action))
    .into()
}

fn unsaved_dot<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fixed(6.0))
        .style(|_theme| container::Style {
            background: Some(theme::colours().accent.into()),
            border: iced::Border {
                radius: iced::border::radius(3),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn tab_style(active: bool) -> button::Style {
    button::Style {
        background: Some(if active {
            crate::ui::theme::selection_wash()
        } else {
            iced::Color::TRANSPARENT.into()
        }),
        text_color: if active {
            theme::colours().selection_text
        } else {
            theme::colours().text_on_dark
        },
        border: iced::Border::default(),
        ..Default::default()
    }
}

pub(super) fn colour_on_tab(active: bool) -> iced::Color {
    let c = theme::colours();
    if active {
        c.selection_text
    } else {
        c.text_on_dark
    }
}

pub(super) fn colour_on_strip(active: bool) -> iced::Color {
    let c = theme::colours();
    if active { c.selection_text } else { c.text }
}

pub(super) fn tool_style(active: bool) -> button::Style {
    button::Style {
        background: Some(if active {
            crate::ui::theme::selection_wash()
        } else {
            iced::Color::TRANSPARENT.into()
        }),
        text_color: if active {
            theme::colours().selection_text
        } else {
            theme::colours().text
        },
        border: iced::Border::default(),
        ..Default::default()
    }
}

pub(super) fn custom_fields(preset: NewCanvas) -> (String, String) {
    let (w, h) = match preset {
        NewCanvas::Custom(w, h) | NewCanvas::Fixed(w, h) => (w, h),
        NewCanvas::Fit(_) => Document::DEFAULT_SIZE,
    };
    (w.to_string(), h.to_string())
}

pub(super) struct Outline<'a> {
    drawn: Option<&'a Lasso>,
    readout: Option<(Rect, (f32, f32))>,
    view: gpu::View,
    canvas: (u32, u32),
}

pub(super) const READOUT_TEXT: f32 = 12.0;

pub(super) const READOUT_GLYPH: f32 = 6.6;

pub(super) const READOUT_PAD: f32 = 8.0;
pub(super) const READOUT_LABEL: f32 = 18.0;
pub(super) const READOUT_LINE: f32 = 16.0;
pub(super) const READOUT_OFFSET: (f32, f32) = (16.0, 12.0);

pub(super) fn readout_origin(from: Point, size: iced::Size, bounds: iced::Size) -> Point {
    let mut origin = Point::new(from.x + READOUT_OFFSET.0, from.y + READOUT_OFFSET.1);
    if origin.x + size.width > bounds.width {
        origin.x = from.x - READOUT_OFFSET.0 - size.width;
    }
    if origin.y + size.height > bounds.height {
        origin.y = from.y - READOUT_OFFSET.1 - size.height;
    }
    Point::new(origin.x.max(0.0), origin.y.max(0.0))
}

impl iced::widget::canvas::Program<Message> for Outline<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke};

        if self.drawn.is_none() && self.readout.is_none() {
            return Vec::new();
        }

        let rect = self.view.canvas_rect(bounds.size(), self.canvas);
        let at = |(x, y): (f32, f32)| {
            Point::new(rect.x + x * self.view.zoom, rect.y + y * self.view.zoom)
        };

        let mut frame = Frame::new(renderer, bounds.size());

        if let Some(points) = self.drawn.map(Lasso::points).filter(|p| p.len() >= 2) {
            let path = Path::new(|builder| {
                builder.move_to(at(points[0]));
                for point in &points[1..] {
                    builder.line_to(at(*point));
                }
                builder.close();
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(iced::Color::BLACK)
                    .with_width(3.0),
            );
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(iced::Color::WHITE)
                    .with_width(1.0),
            );
        }

        if let Some((region, from)) = self.readout {
            self.draw_readout(&mut frame, bounds, at(from), region);
        }

        vec![frame.into_geometry()]
    }
}

impl Outline<'_> {
    pub(super) fn draw_readout(
        &self,
        frame: &mut iced::widget::canvas::Frame,
        bounds: iced::Rectangle,
        from: Point,
        region: Rect,
    ) {
        use iced::widget::canvas::{Path, Stroke, Text};

        let lines = [
            ("W:", format!("{} px", region.width())),
            ("H:", format!("{} px", region.height())),
        ];
        let widest = lines.iter().map(|(_, v)| v.len()).max().unwrap_or(0) as f32;
        let size = iced::Size::new(
            READOUT_PAD * 2.0 + READOUT_LABEL + widest * READOUT_GLYPH,
            READOUT_PAD * 2.0 + READOUT_LINE * lines.len() as f32,
        );

        let origin = readout_origin(from, size, bounds.size());

        let c = theme::colours();
        let panel = Path::rectangle(origin, size);
        frame.fill(&panel, c.control);
        frame.stroke(
            &panel,
            Stroke::default().with_color(c.border).with_width(1.0),
        );

        let label = |content: String, x: f32, y: f32, align| Text {
            content,
            position: Point::new(x, y),
            color: c.text,
            size: READOUT_TEXT.into(),
            font: crate::assets::ui_font(),
            align_x: align,
            align_y: iced::alignment::Vertical::Center,
            ..Text::default()
        };
        for (i, (name, value)) in lines.into_iter().enumerate() {
            let y = origin.y + READOUT_PAD + READOUT_LINE * (i as f32 + 0.5);
            frame.fill_text(label(
                name.to_string(),
                origin.x + READOUT_PAD,
                y,
                iced::alignment::Horizontal::Left.into(),
            ));
            frame.fill_text(label(
                value,
                origin.x + size.width - READOUT_PAD,
                y,
                iced::alignment::Horizontal::Right.into(),
            ));
        }
    }
}

impl App {
    pub(super) fn zoom_controls(&self) -> Element<'_, Message> {
        let steps = self.view.zoom.log2();
        let range = gpu::MIN_ZOOM.log2()..=gpu::MAX_ZOOM.log2();

        row![
            hint(
                strip(icons::FIT, Message::ZoomFit),
                strings::with_key(strings::FIT_TO_WINDOW, &strings::command_key("0")),
            ),
            hint(
                strip(icons::ZOOM_OUT, Message::ZoomOut),
                strings::with_key(strings::ZOOM_OUT, &strings::command_key("-")),
            ),
            iced::widget::slider(range, steps, Message::ZoomPicked)
                .step(0.01_f32)
                .style(crate::ui::controls::slider_style)
                .width(Length::Fixed(140.0)),
            hint(
                strip(icons::ZOOM_IN, Message::ZoomIn),
                strings::with_key(strings::ZOOM_IN, &strings::command_key("+")),
            ),
            hint(
                button(text(format!("{:.0}%", self.view.zoom * 100.0)).size(12))
                    .style(|_t, _s| tool_style(false))
                    .on_press(Message::ZoomActual),
                strings::with_key(strings::ACTUAL_SIZE, &strings::command_key("1")),
            ),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .into()
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.config.decorations {
            return self.workspace();
        }
        iced::widget::stack![
            column![titlebar::view(self.title()), self.workspace()],
            titlebar::edges(),
        ]
        .into()
    }

    // One document needs no strip, and Paint 3D never had one, so it only shows when it earns it.
    pub(super) fn document_tabs(&self) -> Option<Element<'_, Message>> {
        if self.sheets() < 2 {
            return None;
        }

        let tabs = (0..self.sheets()).map(|tab| {
            let active = self.parked_at(tab).is_none();
            let mut face = row![]
                .spacing(6)
                .height(Length::Fill)
                .align_y(iced::Alignment::Center);
            if self.tab_unsaved(tab) {
                face = face.push(unsaved_dot());
            }
            face = face.push(
                text(self.tab_name(tab))
                    .size(12)
                    .color(on_chrome(active))
                    .wrapping(iced::widget::text::Wrapping::None),
            );

            // The close button sits over the tab rather than beside it, so the whole tab is one
            // hover target and the label keeps the full width until it actually runs out.
            let tab_face = iced::widget::stack![
                button(face)
                    .width(Length::Fill)
                    .height(Length::Fixed(DOCUMENT_TAB_HEIGHT))
                    .padding(iced::Padding::default().left(10).right(DOCUMENT_TAB_HEIGHT))
                    .clip(true)
                    .style(move |_theme, status| document_tab_style(active, status))
                    .on_press(Message::TabSelected(tab)),
                open_marker(active),
                row![
                    Space::new().width(Length::Fill),
                    button(crate::ui::centred(icons::art(
                        crate::assets::WINDOW_CLOSE_SVG,
                        8.0,
                        Some(on_chrome(active)),
                    )))
                    .width(Length::Fixed(DOCUMENT_TAB_HEIGHT - 8.0))
                    .height(Length::Fixed(DOCUMENT_TAB_HEIGHT - 8.0))
                    .padding(0)
                    .style(|_theme, status| document_close_style(status))
                    .on_press(Message::TabClosed(tab)),
                    Space::new().width(Length::Fixed(4.0)),
                ]
                .height(Length::Fixed(DOCUMENT_TAB_HEIGHT))
                .align_y(iced::Alignment::Center),
            ]
            .width(Length::Fill);

            iced::widget::mouse_area(tab_face)
                .on_right_press(Message::TabMenuOpened(tab))
                .into()
        });

        let strip = row![
            container(row(tabs).spacing(2))
                .width(Length::Fill)
                .max_width(self.sheets() as f32 * DOCUMENT_TAB_WIDTH),
            hint(
                button(crate::ui::centred(
                    text("+").size(17).color(on_chrome(false))
                ))
                .width(Length::Fixed(DOCUMENT_TAB_HEIGHT))
                .height(Length::Fixed(DOCUMENT_TAB_HEIGHT))
                .padding(0)
                .style(|_theme, status| document_close_style(status))
                .on_press(Message::NewRequested),
                strings::with_key("New picture", &strings::command_key("N")),
            ),
            Space::new().width(Length::Fill),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center);

        Some(
            container(strip)
                .width(Length::Fill)
                .padding(
                    iced::Padding::default()
                        .left(6)
                        .right(6)
                        .top(DOCUMENT_STRIP_PAD),
                )
                .style(|_theme| container::Style {
                    background: Some(theme::colours().title_bar.into()),
                    ..Default::default()
                })
                .into(),
        )
    }

    // Anchored by rebuilding the strip's own layout with the card in one slot, because a right
    // press reports no position to place it at.
    pub(super) fn tab_menu(&self) -> Option<Element<'_, Message>> {
        let open = self.tab_menu.filter(|tab| *tab < self.sheets())?;

        let slots = (0..self.sheets()).map(|tab| {
            if tab != open {
                return Element::from(Space::new().width(Length::Fill));
            }
            row![
                container(
                    column![
                        tab_menu_item(strings::SAVE, &strings::command_key("S"), TabAction::Save),
                        tab_menu_item(
                            "Copy to clipboard",
                            &strings::shift_key("C"),
                            TabAction::Copy
                        ),
                        tab_menu_item("Close", &strings::command_key("Q"), TabAction::Close),
                    ]
                    .spacing(1)
                )
                .width(Length::Fixed(DOCUMENT_TAB_WIDTH))
                .padding(4)
                .style(|_theme| container::Style {
                    background: Some(theme::colours().side_panel.into()),
                    border: iced::Border {
                        color: theme::colours().border,
                        width: 1.0,
                        radius: iced::border::radius(8),
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
                }),
                Space::new().width(Length::Fill),
            ]
            .into()
        });

        Some(
            iced::widget::stack![
                iced::widget::mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .on_press(Message::TabMenuClosed)
                    .on_right_press(Message::TabMenuClosed),
                column![
                    Space::new().height(Length::Fixed(DOCUMENT_TAB_HEIGHT + DOCUMENT_STRIP_PAD)),
                    row![
                        container(row(slots).spacing(2))
                            .width(Length::Fill)
                            .max_width(self.sheets() as f32 * DOCUMENT_TAB_WIDTH),
                        Space::new().width(Length::Fill),
                    ]
                    .padding(iced::Padding::default().left(6).right(6)),
                    Space::new().height(Length::Fill),
                ],
            ]
            .into(),
        )
    }

    pub(super) fn workspace(&self) -> Element<'_, Message> {
        let under = match self.document_tabs() {
            Some(strip) => column![strip, self.pages()].into(),
            None => self.pages(),
        };
        let under = match &self.picker {
            Some(picker) => iced::widget::stack![under, picker::view(picker)].into(),
            None => under,
        };
        let under = match self.tab_menu() {
            Some(menu) => iced::widget::stack![under, menu].into(),
            None => under,
        };
        if self.offering {
            return iced::widget::stack![under, dialog::offer_recovery(self.recovered.len())]
                .into();
        }
        if self.asking.is_none() {
            return under;
        }
        iced::widget::stack![
            under,
            dialog::ask_to_save(
                self.document_name(),
                self.config.confirm_discard,
                self.asking == Some(Pending::Close),
            ),
        ]
        .into()
    }

    pub(super) fn pages(&self) -> Element<'_, Message> {
        if let Some(page) = self.menu {
            return menu::view(
                page,
                self.document_name(),
                self.doc.modified(),
                &self.config,
                self.viewport,
                (&self.custom_canvas.0, &self.custom_canvas.1),
                self.save_format,
            );
        }
        column![
            self.tab_strip(),
            self.tool_strip(),
            row![
                self.canvas_view(),
                match (&self.cropping, &self.cutting_out) {
                    (_, Some(cutting_out)) => container(sidebar::cutout_panel(
                        cutting_out.refining,
                        cutting_out.adding,
                        cutting_out.autofill,
                    ))
                    .width(Length::Fixed(metrics::SIDE_PANEL_WIDTH))
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        top: 16.0,
                        right: 24.0,
                        bottom: 16.0,
                        left: 24.0
                    })
                    .style(|_theme| container::Style {
                        background: Some(theme::veiled(theme::colours().side_panel).into()),
                        ..Default::default()
                    })
                    .into(),
                    (Some(cropping), None) => container(sidebar::crop_panel(
                        cropping.framing,
                        cropping.lock,
                        (&cropping.fields.0, &cropping.fields.1),
                    ))
                    .width(Length::Fixed(metrics::SIDE_PANEL_WIDTH))
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        top: 16.0,
                        right: 24.0,
                        bottom: 16.0,
                        left: 24.0,
                    })
                    .style(|_theme| container::Style {
                        background: Some(theme::veiled(theme::colours().side_panel).into()),
                        ..Default::default()
                    })
                    .into(),
                    (None, None) => sidebar::panel(
                        self.tab,
                        &self.brush,
                        &self.panel,
                        self.resize_preview.unwrap_or(self.doc.size()),
                        self.doc.transparent,
                        self.drawing,
                        self.shape_style,
                        &self.text_style,
                        metrics::SIDE_PANEL_WIDTH,
                        self.colour_target,
                        self.live_drawing(),
                        &self.config.custom_colours,
                        self.custom_colour_menu,
                        &self.stickers,
                    ),
                },
            ]
            .height(Length::Fill),
            self.bottom_bar(),
        ]
        .into()
    }

    pub(super) fn tab_strip(&self) -> Element<'_, Message> {
        let wide = self.tabs_fit();
        let mut tabs = row![];
        for (label, glyph, tab) in sidebar::TABS {
            let active = tab == Some(self.tab);
            let button = tab_button(label, glyph, wide, active)
                .style(move |_theme, _status| tab_style(active));
            tabs = tabs.push(hint(
                sidebar::pressable(button, tab.map(Message::TabPicked)),
                label,
            ));
        }

        let menu_button = tab_button(strings::MENU, icons::MENU, wide, false)
            .style(|_theme, _status| tab_style(false))
            .on_press(Message::MenuOpened);

        let bar = row![
            menu_button,
            Space::new().width(Length::Fixed(8.0)),
            tabs,
            Space::new().width(Length::Fill),
            hint(
                sidebar::pressable(
                    bar_button(crate::ui::centred(icon(
                        icons::UNDO,
                        16.0,
                        colour_on_tab(false)
                    )))
                    .style(|_t, _s| tab_style(false)),
                    self.can_undo().then_some(Message::Undo),
                ),
                strings::with_key(strings::UNDO, &strings::command_key("Z")),
            ),
            hint(
                sidebar::pressable(
                    bar_button(crate::ui::centred(icon(
                        icons::REDO,
                        16.0,
                        colour_on_tab(false)
                    )))
                    .style(|_t, _s| tab_style(false)),
                    self.can_redo().then_some(Message::Redo),
                ),
                strings::with_key(strings::REDO, &strings::command_key("Y")),
            ),
        ]
        .padding(iced::Padding {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 6.0,
        })
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

        surface(bar, theme::veiled(theme::colours().top_bar))
            .height(Length::Fixed(metrics::TOP_PANEL_BUTTON_HEIGHT))
            .into()
    }

    pub(super) fn tool_strip(&self) -> Element<'_, Message> {
        let selecting = self.brush.tool == Tool::Select;
        let (boxed, looped) = (selecting && !self.freeform, selecting && self.freeform);
        let writing = self.brush.tool == Tool::Text;
        let cropping = self.cropping.is_some();
        let cutting_out = self.cutting_out.is_some();
        let bar = row![
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::SELECT,
                    16.0,
                    colour_on_strip(boxed)
                )))
                .style(move |_t, _s| tool_style(boxed))
                .on_press(Message::FreeformToggled(false)),
                strings::SELECT_BOX,
            ),
            hint(
                bar_button(crate::ui::centred(icons::art(
                    crate::assets::LASSO_SVG,
                    16.0,
                    Some(colour_on_strip(looped)),
                )))
                .style(move |_t, _s| tool_style(looped))
                .on_press(Message::FreeformToggled(true)),
                strings::SELECT_FREEFORM,
            ),
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::TEXT,
                    16.0,
                    colour_on_strip(writing)
                )))
                .style(move |_t, _s| tool_style(writing))
                .on_press(Message::TextToolPicked),
                strings::TEXT,
            ),
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::CROP,
                    16.0,
                    colour_on_strip(cropping),
                )))
                .style(move |_t, _s| tool_style(cropping))
                .on_press(Message::CropOpened),
                strings::CROP,
            ),
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::SMART_CUTOUT,
                    16.0,
                    colour_on_strip(cutting_out),
                )))
                .style(move |_t, _s| tool_style(cutting_out))
                .on_press(Message::CutoutOpened),
                strings::SMART_CUTOUT,
            ),
            Space::new().width(Length::Fill),
            self.zoom_controls(),
        ]
        .spacing(4)
        .padding(iced::Padding {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 6.0,
        })
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

        surface(bar, theme::veiled(theme::colours().tool_bar))
            .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT))
            .into()
    }

    pub(super) fn frame(&self) -> gpu::CanvasFrame {
        gpu::CanvasFrame {
            pixels: self.doc.pixels().bytes_arc(),
            size: self.doc.size(),
            version: self.doc.version(),
            dirty: self.dirty,
            view: self.view,
            show_canvas: self.panel.show_canvas,
            handles: self.tab == Tab::Canvas && self.panel.show_canvas,
            preview: self.resize_preview,
            backing: self.doc.has_backing(),
            floating: self.cutout_overlay().or_else(|| {
                self.floating.as_ref().map(|f| gpu::FloatingFrame {
                    pixels: f.pixels.bytes_arc(),
                    size: f.pixels.size(),
                    version: self.float_version,
                    xform: f.xform,
                    points: f.points().to_vec(),
                    editing_text: f.editing,
                    text_empty: f.text_is_empty(),
                    opacity: f.opacity(),
                    masked: f.masked(),
                    grips: true,
                })
            }),
            ants: 0.0,
            frame: self.cropping.as_ref().map(|c| c.rect).or_else(|| {
                self.cutting_out
                    .as_ref()
                    .filter(|m| !m.refining)
                    .map(|m| m.rect)
            }),
            marquee: self.marquee(),
        }
    }

    pub(super) fn marquee(&self) -> Option<Rect> {
        if self.lasso.is_some() || !matches!(self.brush.tool, Tool::Select | Tool::Text) {
            return None;
        }
        let (a, b) = self.selecting?;
        drag_rect(a, b, self.doc.size())
    }

    pub(super) fn cutout_overlay(&self) -> Option<gpu::FloatingFrame> {
        let cutting_out = self.cutting_out.as_ref()?;
        let overlay = cutting_out.overlay.clone()?;
        Some(gpu::FloatingFrame {
            pixels: overlay,
            size: self.doc.size(),
            version: self.float_version,
            xform: Xform {
                x: 0.0,
                y: 0.0,
                width: self.doc.size().0 as f32,
                height: self.doc.size().1 as f32,
                rotation: 0.0,
            },
            points: Vec::new(),
            editing_text: false,
            text_empty: false,
            opacity: 1.0,
            masked: true,
            grips: false,
        })
    }

    pub(super) fn canvas_view(&self) -> Element<'_, Message> {
        let viewport = shader::Shader::new(gpu::Program {
            frame: self.frame(),
            cursor: iced::mouse::Interaction::Crosshair,
            selecting: !self.refining()
                && matches!(self.brush.tool, Tool::Select | Tool::Text | Tool::Shape),
            brush: if self.refining() {
                Some(CuttingOut::BRUSH * 2.0 / self.view.zoom.max(0.01))
            } else {
                (self.tab == Tab::Brushes && self.brush.tool.profile().is_some())
                    .then_some(self.brush.thickness)
            },
        })
        .width(Length::Fill)
        .height(Length::Fill);

        iced::widget::stack![
            viewport,
            iced::widget::canvas(Outline {
                drawn: self.being_drawn(),
                readout: self.readout(),
                view: self.view,
                canvas: self.doc.size(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .into()
    }

    pub(super) fn readout(&self) -> Option<(Rect, (f32, f32))> {
        let rect = self.marquee()?;
        let (_, b) = self.selecting?;
        Some((rect, b))
    }

    pub(super) fn being_drawn(&self) -> Option<&Lasso> {
        self.lasso.as_ref()
    }

    pub(super) fn bottom_bar(&self) -> Element<'_, Message> {
        let zoom = format!("{:.0}%", self.view.zoom * 100.0);
        let message = if self.status.is_empty() {
            format!("{} x {}", self.doc.size().0, self.doc.size().1)
        } else {
            self.status.clone()
        };

        let bar = row![
            text(message),
            Space::new().width(Length::Fill),
            text(zoom).size(12)
        ]
        .spacing(8)
        .padding(8)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

        surface(bar, theme::veiled(theme::colours().tool_bar))
            .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT))
            .into()
    }
}
