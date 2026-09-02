use iced::widget::container;
use iced::{Element, Length};

pub fn centred<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).center(Length::Fill).into()
}

pub mod controls {
    use super::theme;
    use iced::widget::overlay::menu;
    use iced::widget::{checkbox, pick_list, slider, text_input, toggler};
    use iced::{Background, Border};

    pub fn slider_style(_theme: &iced::Theme, status: slider::Status) -> slider::Style {
        let c = theme::colours();
        let lit = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        slider::Style {
            rail: slider::Rail {
                backgrounds: (c.accent.into(), c.border.into()),
                width: 4.0,
                border: Border::default(),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle {
                    radius: if lit { 8.0 } else { 7.0 },
                },
                background: c.accent.into(),
                border_width: 0.0,
                border_color: iced::Color::TRANSPARENT,
            },
        }
    }

    pub fn toggler_style(_theme: &iced::Theme, status: toggler::Status) -> toggler::Style {
        let c = theme::colours();
        let on = matches!(
            status,
            toggler::Status::Active { is_toggled: true }
                | toggler::Status::Hovered { is_toggled: true }
        );
        toggler::Style {
            background: if on {
                c.accent.into()
            } else {
                Background::Color(c.border)
            },
            background_border_width: 0.0,
            background_border_color: iced::Color::TRANSPARENT,
            foreground: if on {
                c.selection_text.into()
            } else {
                c.control.into()
            },
            foreground_border_width: 0.0,
            foreground_border_color: iced::Color::TRANSPARENT,
            text_color: Some(c.text),
            border_radius: None,
            padding_ratio: 0.2,
        }
    }

    pub fn checkbox_style(_theme: &iced::Theme, status: checkbox::Status) -> checkbox::Style {
        let c = theme::colours();
        let ticked = matches!(
            status,
            checkbox::Status::Active { is_checked: true }
                | checkbox::Status::Hovered { is_checked: true }
        );
        checkbox::Style {
            background: if ticked {
                c.accent.into()
            } else {
                c.control.into()
            },
            icon_color: c.selection_text,
            border: Border {
                color: if ticked { c.accent } else { c.border },
                width: 1.0,
                radius: 2.0.into(),
            },
            text_color: Some(c.text),
        }
    }

    pub fn text_input_style(_theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
        let c = theme::colours();
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: c.control.into(),
            border: Border {
                color: if focused { c.accent } else { c.border },
                width: 1.0,
                radius: 2.0.into(),
            },
            icon: c.text_dim,
            placeholder: c.text_dim,
            value: c.text,
            selection: c.accent,
        }
    }

    // pick_list styles only the closed control; the list it drops down is its own catalog.
    pub fn menu_style(_theme: &iced::Theme) -> menu::Style {
        let c = theme::colours();
        menu::Style {
            background: c.control.into(),
            border: Border {
                color: c.border,
                width: 1.0,
                radius: 2.0.into(),
            },
            text_color: c.text,
            selected_text_color: c.selection_text,
            selected_background: c.accent.into(),
            shadow: iced::Shadow::default(),
        }
    }

    pub fn pick_list_style(_theme: &iced::Theme, status: pick_list::Status) -> pick_list::Style {
        let c = theme::colours();
        let lit = matches!(
            status,
            pick_list::Status::Hovered | pick_list::Status::Opened { .. }
        );
        pick_list::Style {
            text_color: c.text,
            placeholder_color: c.text_dim,
            handle_color: c.text,
            background: if lit {
                c.control_hover.into()
            } else {
                c.control.into()
            },
            border: Border {
                color: if lit { c.accent } else { c.border },
                width: 1.0,
                radius: 2.0.into(),
            },
        }
    }
}

pub mod dialog;
#[allow(dead_code, reason = "reference table, filled in ahead of the widgets")]
pub mod icons;
pub mod menu;
pub mod picker;
pub mod sidebar;
pub mod strings;
pub mod theme;
pub mod titlebar;
