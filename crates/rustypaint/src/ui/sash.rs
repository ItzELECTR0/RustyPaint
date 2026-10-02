use crate::ui::theme;

use iced::widget::canvas as draw;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{Element, Length, Point, Rectangle, Size};

pub const HEIGHT: f32 = 10.0;
const GRIP: Size = Size::new(32.0, 4.0);

// The bar between two stacked sections. It follows the pointer anywhere once grabbed, which a
// mouse area cannot, so a drag that strays off the bar does not stop.
pub fn sash<'a, Message: Clone + 'a>(
    above: f32,
    room: f32,
    resize: impl Fn(f32) -> Message + 'a,
    settle: Message,
) -> Element<'a, Message> {
    draw(Sash {
        above,
        room,
        resize: Box::new(resize),
        settle,
    })
    .width(Length::Fill)
    .height(Length::Fixed(HEIGHT))
    .into()
}

struct Sash<'a, Message> {
    above: f32,
    room: f32,
    resize: Box<dyn Fn(f32) -> Message + 'a>,
    settle: Message,
}

#[derive(Default)]
pub struct State {
    // Where the pointer and the section above stood when the drag began.
    grabbed: Option<(f32, f32)>,
    hovered: bool,
}

impl<Message: Clone> canvas::Program<Message> for Sash<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::mouse::{Button, Event};
        match event {
            canvas::Event::Mouse(Event::ButtonPressed(Button::Left)) => {
                cursor.position_over(bounds)?;
                let at = cursor.position()?.y;
                state.grabbed = Some((at, self.above));
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(Event::CursorMoved { position }) => {
                let Some((from, above)) = state.grabbed else {
                    let hovered = cursor.is_over(bounds);
                    return (std::mem::replace(&mut state.hovered, hovered) != hovered)
                        .then(canvas::Action::request_redraw);
                };
                let share = (above + position.y - from) / self.room.max(1.0);
                Some(canvas::Action::publish((self.resize)(share.clamp(0.0, 1.0))).and_capture())
            }
            canvas::Event::Mouse(Event::ButtonReleased(Button::Left)) => {
                state.grabbed.take()?;
                state.hovered = cursor.is_over(bounds);
                Some(canvas::Action::publish(self.settle.clone()).and_capture())
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
        let middle = size.height / 2.0;
        frame.stroke(
            &Path::line(Point::new(0.0, middle), Point::new(size.width, middle)),
            Stroke::default().with_color(colours.border).with_width(1.0),
        );
        let lit = state.hovered || state.grabbed.is_some();
        let grip = Path::rounded_rectangle(
            Point::new((size.width - GRIP.width) / 2.0, middle - GRIP.height / 2.0),
            GRIP,
            (GRIP.height / 2.0).into(),
        );
        frame.fill(
            &grip,
            if lit {
                colours.accent
            } else {
                colours.text_dim
            },
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if state.grabbed.is_some() || cursor.is_over(bounds) {
            iced::mouse::Interaction::ResizingVertically
        } else {
            iced::mouse::Interaction::default()
        }
    }
}
