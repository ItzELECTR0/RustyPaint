use crate::app::{Discard, Message};
use crate::ui::theme;

use iced::widget::{Space, button, checkbox, column, container, mouse_area, row, text};
use iced::{Element, Length};

const CARD: f32 = 396.0;
const WIDE_CARD: f32 = 520.0;
const BUTTON: f32 = 116.0;

pub fn ask_to_save<'a>(name: &'a str, confirm: bool, closing: bool) -> Element<'a, Message> {
    let mut answers = row![Space::new().width(Length::Fill)].spacing(6);
    if closing {
        answers = answers.push(answer("Keep session", Discard::Session));
    }
    answers = answers
        .push(answer("Save", Discard::Save))
        .push(answer("Don't save", Discard::Throw))
        .push(answer("Cancel", Discard::Keep));

    let card = column![
        text("Do you want to save your work?")
            .size(18)
            .color(theme::colours().text),
        text(if closing {
            format!(
                "There are unsaved changes to {name}. Keeping the session brings everything \
                     back the next time RustyPaint starts."
            )
        } else {
            format!("There are unsaved changes to {name}.")
        })
        .size(13)
        .color(theme::colours().text),
        checkbox(!confirm)
            .style(crate::ui::controls::checkbox_style)
            .label("Don't ask me again")
            .text_size(12)
            .on_toggle(|quiet| Message::ConfirmDiscardToggled(!quiet)),
        answers,
    ]
    .spacing(14)
    .width(Length::Fixed(if closing { WIDE_CARD } else { CARD }));

    over(card, Message::DiscardAnswered(Discard::Keep))
}

pub fn offer_recovery<'a>(waiting: usize) -> Element<'a, Message> {
    let left = if waiting > 1 {
        format!(
            "{waiting} documents were left unsaved. The most recent one can come back now, and the rest are offered next time."
        )
    } else {
        "A document was left unsaved. It can come back the way it was.".to_owned()
    };
    let card = column![
        text("Recover unsaved work?")
            .size(18)
            .color(theme::colours().text),
        text(left).size(13).color(theme::colours().text),
        row![
            Space::new().width(Length::Fill),
            action("Recover", Message::RecoveryAnswered(true)),
            action("Discard", Message::RecoveryAnswered(false)),
        ]
        .spacing(6),
    ]
    .spacing(14)
    .width(Length::Fixed(CARD));

    over(card, Message::RecoveryAnswered(true))
}

fn over<'a>(card: iced::widget::Column<'a, Message>, dismiss: Message) -> Element<'a, Message> {
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
    .on_press(dismiss)
    .into()
}

fn answer<'a>(label: &'a str, discard: Discard) -> Element<'a, Message> {
    action(label, Message::DiscardAnswered(discard))
}

fn action<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
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
        .on_press(message)
        .into()
}
