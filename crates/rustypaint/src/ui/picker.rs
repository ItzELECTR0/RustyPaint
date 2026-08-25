use crate::app::Message;
use crate::ui::theme;

use iced::widget::{Space, button, column, container, image, mouse_area, row, text, text_input};
use iced::{Element, Length};

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub const FIELD: u16 = 260;
pub const STRIP: u16 = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
    pub typed: Option<String>,
    channels: [String; 3],
}

impl Picker {
    pub fn on(colour: [u8; 4]) -> Self {
        let (hue, saturation, value) = to_hsv(colour);
        Self {
            hue,
            saturation,
            value,
            typed: None,
            channels: [
                colour[0].to_string(),
                colour[1].to_string(),
                colour[2].to_string(),
            ],
        }
    }

    pub fn colour(&self) -> [u8; 4] {
        from_hsv(self.hue, self.saturation, self.value)
    }

    pub fn typed(&mut self, hex: String) {
        if let Some(colour) = parse_hex(&hex) {
            let (h, s, v) = to_hsv(colour);
            self.hue = if s > 0.0 { h } else { self.hue };
            self.saturation = s;
            self.value = v;
            self.channels = [
                colour[0].to_string(),
                colour[1].to_string(),
                colour[2].to_string(),
            ];
        }
        self.typed = Some(hex);
    }

    pub fn typed_channel(&mut self, channel: usize, value: String) {
        let Some(typed) = self.channels.get_mut(channel) else {
            return;
        };
        *typed = value;
        self.typed = None;

        let (Ok(red), Ok(green), Ok(blue)) = (
            self.channels[0].parse::<u8>(),
            self.channels[1].parse::<u8>(),
            self.channels[2].parse::<u8>(),
        ) else {
            return;
        };
        let (hue, saturation, value) = to_hsv([red, green, blue, 255]);
        self.hue = if saturation > 0.0 { hue } else { self.hue };
        self.saturation = saturation;
        self.value = value;
    }

    pub fn channel(&self, channel: usize) -> &str {
        &self.channels[channel]
    }

    pub fn clear_typed(&mut self) {
        self.typed = None;
        let [r, g, b, _] = self.colour();
        self.channels = [r.to_string(), g.to_string(), b.to_string()];
    }

    pub fn hex(&self) -> String {
        match &self.typed {
            Some(typed) => typed.clone(),
            None => {
                let [r, g, b, _] = self.colour();
                format!("#{r:02X}{g:02X}{b:02X}")
            }
        }
    }
}

pub fn view(picker: &Picker) -> Element<'_, Message> {
    let colour = picker.colour();

    let field = mouse_area(iced::widget::stack![
        image(square(picker.hue))
            .width(FIELD as f32)
            .height(FIELD as f32),
        iced::widget::canvas(Marker::Spot(picker.saturation, 1.0 - picker.value))
            .width(FIELD as f32)
            .height(FIELD as f32),
    ])
    .on_press(Message::PickerFieldPressed)
    .on_move(|at| Message::PickerFieldPicked(at.x / FIELD as f32, 1.0 - at.y / FIELD as f32))
    .on_release(Message::PickerReleased)
    .on_exit(Message::PickerReleased);

    let strip = mouse_area(iced::widget::stack![
        image(hues()).width(FIELD as f32).height(STRIP as f32),
        iced::widget::canvas(Marker::Hue(picker.hue / 360.0))
            .width(FIELD as f32)
            .height(STRIP as f32),
    ])
    .on_press(Message::PickerStripPressed)
    .on_move(|at| Message::PickerHuePicked(at.x / FIELD as f32 * 360.0))
    .on_release(Message::PickerReleased)
    .on_exit(Message::PickerReleased);

    let preview = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(90.0))
        .height(Length::Fixed(90.0))
        .style(move |_theme| container::Style {
            background: Some(super::sidebar::from_bytes(colour).into()),
            border: iced::Border {
                color: theme::colours().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });

    let values = column![
        preview,
        channel_field(picker, "Red", 0),
        channel_field(picker, "Green", 1),
        channel_field(picker, "Blue", 2),
        column![
            text("Hex").size(12).color(theme::colours().text),
            text_input("#000000", &picker.hex())
                .style(crate::ui::controls::text_input_style)
                .on_input(Message::PickerHexEdited)
                .on_submit(Message::PickerConfirmed)
                .size(13)
                .width(Length::Fill),
        ]
        .spacing(3),
    ]
    .spacing(8)
    .width(Length::Fixed(90.0));

    let card = column![
        text("Choose a new colour")
            .size(20)
            .color(theme::colours().accent_text),
        row![column![field, strip].spacing(12), values]
            .spacing(24)
            .align_y(iced::Alignment::Start),
        row![
            flat("OK", Message::PickerConfirmed),
            flat("Cancel", Message::PickerClosed),
        ]
        .spacing(12),
    ]
    .spacing(18)
    .width(Length::Fixed(FIELD as f32 + 114.0));

    let backdrop = mouse_area(
        container(
            container(card)
                .padding(24)
                .style(|_theme| container::Style {
                    background: Some(theme::colours().side_panel.into()),
                    border: iced::Border {
                        color: theme::colours().border,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                }),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(
                iced::Color {
                    a: 0.45,
                    ..iced::Color::BLACK
                }
                .into(),
            ),
            ..Default::default()
        }),
    )
    .on_press(Message::PickerClosed);

    backdrop.into()
}

fn channel_field<'a>(
    picker: &'a Picker,
    label: &'static str,
    channel: usize,
) -> Element<'a, Message> {
    column![
        text(label).size(12).color(theme::colours().text),
        text_input("0", picker.channel(channel))
            .style(crate::ui::controls::text_input_style)
            .on_input(move |value| Message::PickerRgbEdited(channel, value))
            .on_submit(Message::PickerConfirmed)
            .size(13)
            .width(Length::Fill),
    ]
    .spacing(3)
    .into()
}

enum Marker {
    Spot(f32, f32),
    Hue(f32),
}

impl iced::widget::canvas::Program<Message> for Marker {
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

        let mut frame = Frame::new(renderer, bounds.size());
        let pair = |frame: &mut Frame, path: &Path| {
            frame.stroke(
                path,
                Stroke::default()
                    .with_color(iced::Color::BLACK)
                    .with_width(3.0),
            );
            frame.stroke(
                path,
                Stroke::default()
                    .with_color(iced::Color::WHITE)
                    .with_width(1.5),
            );
        };

        match *self {
            Marker::Spot(x, y) => {
                let at = iced::Point::new(
                    x.clamp(0.0, 1.0) * bounds.width,
                    y.clamp(0.0, 1.0) * bounds.height,
                );
                pair(
                    &mut frame,
                    &Path::rectangle(
                        iced::Point::new(at.x - 9.0, at.y - 9.0),
                        iced::Size::new(18.0, 18.0),
                    ),
                );
            }
            Marker::Hue(x) => {
                let x = x.clamp(0.0, 1.0) * bounds.width;
                let path = Path::rectangle(
                    iced::Point::new(x - 7.0, 1.0),
                    iced::Size::new(14.0, bounds.height - 2.0),
                );
                pair(&mut frame, &path);
            }
        }
        vec![frame.into_geometry()]
    }
}

fn flat<'a>(label: &'a str, press: Message) -> Element<'a, Message> {
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

fn square(hue: f32) -> image::Handle {
    static SQUARES: LazyLock<Mutex<HashMap<u16, image::Handle>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    let key = (hue.rem_euclid(360.0)).round() as u16 % 360;
    let mut cache = SQUARES.lock().expect("the square cache");
    cache
        .entry(key)
        .or_insert_with(|| {
            let side = FIELD as u32;
            let mut pixels = Vec::with_capacity((side * side * 4) as usize);
            for y in 0..side {
                for x in 0..side {
                    let saturation = x as f32 / (side - 1) as f32;
                    let value = 1.0 - y as f32 / (side - 1) as f32;
                    pixels.extend_from_slice(&from_hsv(key as f32, saturation, value));
                }
            }
            image::Handle::from_rgba(side, side, pixels)
        })
        .clone()
}

fn hues() -> image::Handle {
    static HUES: LazyLock<image::Handle> = LazyLock::new(|| {
        let (w, h) = (FIELD as u32, STRIP as u32);
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                let hue = x as f32 / (w - 1) as f32 * 360.0;
                let colour = from_hsv(hue, 1.0, 1.0);
                pixels.extend_from_slice(&colour);
            }
        }
        image::Handle::from_rgba(w, h, pixels)
    });
    HUES.clone()
}

pub fn parse_hex(hex: &str) -> Option<[u8; 4]> {
    let digits = hex.trim().trim_start_matches('#');
    let byte = |at: usize, len: usize| -> Option<u8> {
        let part = digits.get(at..at + len)?;
        let value = u8::from_str_radix(part, 16).ok()?;
        Some(if len == 1 { value * 17 } else { value })
    };
    match digits.len() {
        3 => Some([byte(0, 1)?, byte(1, 1)?, byte(2, 1)?, 255]),
        6 => Some([byte(0, 2)?, byte(2, 2)?, byte(4, 2)?, 255]),
        _ => None,
    }
}

pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> [u8; 4] {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);

    let chroma = value * saturation;
    let second = chroma * (1.0 - ((hue % 2.0) - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = value - chroma;
    let byte = |c: f32| ((c + base) * 255.0).round().clamp(0.0, 255.0) as u8;
    [byte(r), byte(g), byte(b), 255]
}

pub fn to_hsv([r, g, b, _]: [u8; 4]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;

    let hue = if chroma == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / chroma) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    let saturation = if max == 0.0 { 0.0 } else { chroma / max };
    (hue.rem_euclid(360.0), saturation, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corners_of_the_wheel_survive_a_round_trip() {
        for colour in [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
            [0, 255, 255, 255],
            [255, 0, 255, 255],
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [128, 64, 32, 255],
        ] {
            let (h, s, v) = to_hsv(colour);
            assert_eq!(from_hsv(h, s, v), colour, "{colour:?} came back wrong");
        }
    }

    #[test]
    fn a_picker_opens_on_the_colour_it_was_given() {
        let picker = Picker::on([128, 64, 32, 255]);
        assert_eq!(picker.colour(), [128, 64, 32, 255]);
        assert_eq!(picker.hex(), "#804020");
    }

    #[test]
    fn hex_is_read_in_both_lengths_and_with_or_without_the_hash() {
        assert_eq!(parse_hex("#ff0000"), Some([255, 0, 0, 255]));
        assert_eq!(parse_hex("ff0000"), Some([255, 0, 0, 255]));
        assert_eq!(parse_hex("#f00"), Some([255, 0, 0, 255]));
        assert_eq!(parse_hex("  #0064B6 "), Some([0, 100, 182, 255]));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("nonsense"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn typing_a_colour_moves_the_square_to_it() {
        let mut picker = Picker::on([0, 0, 0, 255]);
        picker.typed("#00ff00".into());
        assert_eq!(picker.colour(), [0, 255, 0, 255]);
        assert!((picker.hue - 120.0).abs() < 0.5);
    }

    #[test]
    fn typing_rgb_channels_moves_the_square_and_preserves_half_typed_input() {
        let mut picker = Picker::on([0, 0, 0, 255]);
        picker.typed_channel(0, "128".into());
        picker.typed_channel(1, "64".into());
        picker.typed_channel(2, "32".into());
        assert_eq!(picker.colour(), [128, 64, 32, 255]);

        picker.typed_channel(0, "256".into());
        assert_eq!(picker.colour(), [128, 64, 32, 255]);
        assert_eq!(picker.channel(0), "256");
    }

    #[test]
    fn a_half_typed_hex_leaves_the_colour_where_it_was() {
        let mut picker = Picker::on([255, 0, 0, 255]);
        picker.typed("#00f".into());
        let blue = picker.colour();
        picker.typed("#00".into());
        assert_eq!(picker.colour(), blue, "still blue while the rest is typed");
        assert_eq!(picker.hex(), "#00", "and the field shows what was typed");
    }

    #[test]
    fn a_grey_keeps_the_hue_that_was_chosen() {
        let mut picker = Picker::on([0, 255, 0, 255]);
        let hue = picker.hue;
        picker.typed("#000000".into());
        assert_eq!(picker.hue, hue);
    }

    #[test]
    fn the_gradients_are_built_once_each() {
        let first = square(200.0);
        let again = square(200.0);
        assert_eq!(format!("{:?}", first.id()), format!("{:?}", again.id()));
        assert_eq!(format!("{:?}", hues().id()), format!("{:?}", hues().id()));
    }
}
