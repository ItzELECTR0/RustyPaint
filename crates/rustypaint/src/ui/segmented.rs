use crate::ui::theme;

use iced::animation::{Animation, Easing};
use iced::widget::canvas as draw;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{Element, Length, Point, Rectangle, Size};
use std::time::Instant;

pub const HEIGHT: f32 = 30.0;
const INSET: f32 = 3.0;

pub fn segmented<'a, Message: 'a>(
    options: impl Into<Vec<&'a str>>,
    selected: usize,
    pick: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    draw(Segmented {
        options: options.into(),
        selected,
        pick: Box::new(pick),
    })
    .width(Length::Fill)
    .height(Length::Fixed(HEIGHT))
    .into()
}

struct Segmented<'a, Message> {
    options: Vec<&'a str>,
    selected: usize,
    pick: Box<dyn Fn(usize) -> Message + 'a>,
}

pub struct State {
    slide: Animation<f32>,
    now: Instant,
    placed: bool,
    hovered: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            slide: slide(0.0),
            now: Instant::now(),
            placed: false,
            hovered: None,
        }
    }
}

fn slide(at: f32) -> Animation<f32> {
    Animation::new(at).quick().easing(Easing::EaseOut)
}

impl<Message> Segmented<'_, Message> {
    fn segment_at(&self, bounds: Rectangle, cursor: iced::mouse::Cursor) -> Option<usize> {
        let point = cursor.position_in(bounds)?;
        let width = bounds.width / self.options.len() as f32;
        let index = (point.x / width.max(1.0)) as usize;
        (index < self.options.len()).then_some(index)
    }
}

impl<Message> canvas::Program<Message> for Segmented<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.now = *now;
                let target = self.selected as f32;
                // The first frame places the pill where the choice already is, so opening a panel
                // never plays a slide nobody asked for.
                if !state.placed {
                    state.slide = slide(target);
                    state.placed = true;
                } else if state.slide.value() != target {
                    state.slide.go_mut(target, *now);
                }
                state
                    .slide
                    .is_animating(*now)
                    .then(canvas::Action::request_redraw)
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                let hovered = self.segment_at(bounds, cursor);
                (std::mem::replace(&mut state.hovered, hovered) != hovered)
                    .then(canvas::Action::request_redraw)
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                let index = self.segment_at(bounds, cursor)?;
                (index != self.selected)
                    .then(|| canvas::Action::publish((self.pick)(index)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let colours = theme::colours();
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);

        let track = Path::rounded_rectangle(Point::ORIGIN, size, (size.height / 2.0).into());
        frame.fill(&track, colours.control);
        frame.stroke(
            &track,
            Stroke::default().with_color(colours.border).with_width(1.0),
        );

        let width = size.width / self.options.len() as f32;
        let at = state.slide.interpolate_with(|value| value, state.now);
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(at * width + INSET, INSET),
                Size::new(width - INSET * 2.0, size.height - INSET * 2.0),
                ((size.height - INSET * 2.0) / 2.0).into(),
            ),
            colours.accent,
        );

        for (index, label) in self.options.iter().enumerate() {
            frame.fill_text(canvas::Text {
                content: (*label).to_owned(),
                position: Point::new((index as f32 + 0.5) * width, size.height / 2.0),
                color: match (index == self.selected, state.hovered == Some(index)) {
                    (true, _) => colours.selection_text,
                    (false, true) => colours.accent_text,
                    (false, false) => colours.text,
                },
                size: 13.0.into(),
                font: crate::assets::ui_font(),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        match cursor.is_over(bounds) {
            true => iced::mouse::Interaction::Pointer,
            false => iced::mouse::Interaction::default(),
        }
    }
}
