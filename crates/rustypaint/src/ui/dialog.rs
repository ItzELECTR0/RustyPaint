use crate::app::{Discard, Message};
use crate::ui::theme;

use iced::widget::{Space, button, checkbox, column, container, mouse_area, row, text};
use iced::{Element, Length};

const CARD: f32 = 396.0;
const BUTTON: f32 = 116.0;

pub fn ask_to_save<'a>(name: &'a str, confirm: bool) -> Element<'a, Message> {
    let card = column![
        text("Do you want to save your work?")
            .size(18)
            .color(theme::colours().text),
        text(format!("There are unsaved changes to {name}."))
            .size(13)
            .color(theme::colours().text),
        checkbox(!confirm)
            .style(crate::ui::controls::checkbox_style)
            .label("Don't ask me again")
            .text_size(12)
            .on_toggle(|quiet| Message::ConfirmDiscardToggled(!quiet)),
        row![
            Space::new().width(Length::Fill),
            answer("Save", Discard::Save),
            answer("Don't save", Discard::Throw),
            answer("Cancel", Discard::Keep),
        ]
        .spacing(6),
    ]
    .spacing(14)
    .width(Length::Fixed(CARD));

    mouse_area(
        container(
            container(card)
                .padding(22)
                .style(|_theme| container::Style {
                    background: Some(theme::colours().side_panel.into()),
                    border: iced::Border {
                        color: theme::colours().border,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
        )
        .center(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(theme::colours().overlay.into()),
            ..Default::default()
        }),
    )
    .on_press(Message::DiscardAnswered(Discard::Keep))
    .into()
}

fn answer<'a>(label: &'a str, discard: Discard) -> Element<'a, Message> {
    button(crate::ui::centred(text(label).size(13).center()))
        .width(Length::Fixed(BUTTON))
        .height(Length::Fixed(28.0))
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
        .on_press(Message::DiscardAnswered(discard))
        .into()
}
