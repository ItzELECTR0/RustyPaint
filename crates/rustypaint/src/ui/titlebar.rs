use crate::app::Message;
use crate::assets;
use crate::ui::icons;
use crate::ui::theme;

use iced::widget::{Space, button, container, mouse_area, row, text};
use iced::window::Direction;
use iced::{Element, Length};

pub const HEIGHT: f32 = 32.0;

pub const EDGE: f32 = 8.0;

const BUTTON: f32 = 46.0;

pub fn view<'a>(title: String) -> Element<'a, Message> {
    let name = row![
        Space::new().width(Length::Fixed(10.0)),
        text(title).size(12).color(theme::colours().text_on_dark),
    ]
    .align_y(iced::Alignment::Center);

    let grip = mouse_area(
        container(name)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    .on_press(Message::WindowDragged)
    .on_double_click(Message::WindowMaximiseToggled);

    let bar = row![
        grip,
        control(assets::WINDOW_MINIMISE_SVG, Message::WindowMinimised, false),
        control(
            assets::WINDOW_MAXIMISE_SVG,
            Message::WindowMaximiseToggled,
            false
        ),
        control(assets::WINDOW_CLOSE_SVG, Message::WindowClosed, true),
    ]
    .align_y(iced::Alignment::Center);

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(HEIGHT))
        .style(|_theme| container::Style {
            background: Some(theme::veiled(theme::colours().top_bar).into()),
            ..Default::default()
        })
        .into()
}

fn control<'a>(art: &'static [u8], press: Message, danger: bool) -> Element<'a, Message> {
    const DANGER: iced::Color = iced::Color {
        r: 0.898,
        g: 0.114,
        b: 0.208,
        a: 1.0,
    };

    button(crate::ui::centred(icons::art(
        art,
        10.0,
        Some(theme::colours().text_on_dark),
    )))
    .width(Length::Fixed(BUTTON))
    .height(Length::Fixed(HEIGHT))
    .padding(0)
    .style(move |_theme, status| button::Style {
        background: matches!(status, button::Status::Hovered).then(|| {
            if danger {
                DANGER.into()
            } else {
                iced::Color {
                    a: 0.16,
                    ..theme::colours().text_on_dark
                }
                .into()
            }
        }),
        text_color: theme::colours().text_on_dark,
        ..Default::default()
    })
    .on_press(press)
    .into()
}

pub fn edges<'a>() -> Element<'a, Message> {
    let band = |width: Length, height: Length, direction: Direction| -> Element<'a, Message> {
        mouse_area(Space::new().width(width).height(height))
            .on_press(Message::WindowResizeDragged(direction))
            .into()
    };
    let corner = |direction| band(Length::Fixed(EDGE), Length::Fixed(EDGE), direction);
    let side = |direction| band(Length::Fixed(EDGE), Length::Fill, direction);
    let cap = |direction| band(Length::Fill, Length::Fixed(EDGE), direction);

    let top: Element<'_, Message> = row![
        corner(Direction::NorthWest),
        cap(Direction::North),
        corner(Direction::NorthEast),
    ]
    .height(Length::Fixed(EDGE))
    .into();
    let middle: Element<'_, Message> = row![
        side(Direction::West),
        Space::new().width(Length::Fill).height(Length::Fill),
        side(Direction::East),
    ]
    .height(Length::Fill)
    .into();
    let bottom: Element<'_, Message> = row![
        corner(Direction::SouthWest),
        cap(Direction::South),
        corner(Direction::SouthEast),
    ]
    .height(Length::Fixed(EDGE))
    .into();

    iced::widget::column![top, middle, bottom].into()
}
