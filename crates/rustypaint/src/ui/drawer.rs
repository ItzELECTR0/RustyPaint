use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget, overlay};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

// A panel pinned inside the right edge of the area it floats over, `shift` further right while it
// slides, and clipped to that area so it slides out from under whatever borders it.
pub fn drawer<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    width: f32,
    margin: f32,
    shift: f32,
) -> Element<'a, Message> {
    Element::new(Drawer {
        content: content.into(),
        width,
        margin,
        shift,
    })
}

struct Drawer<'a, Message> {
    content: Element<'a, Message>,
    width: f32,
    margin: f32,
    shift: f32,
}

// Set while a button that went down outside the panel is still held: a stroke or a pan that
// crosses the panel carries on underneath it instead of stalling at its edge.
#[derive(Default)]
struct State {
    passing: bool,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Drawer<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let area = limits.max();
        let panel = Size::new(self.width, (area.height - 2.0 * self.margin).max(0.0));
        let node = self
            .content
            .as_widget_mut()
            .layout(
                &mut tree.children[0],
                renderer,
                &layout::Limits::new(panel, panel),
            )
            .move_to(Point::new(
                area.width - self.width - self.margin + self.shift,
                self.margin,
            ));
        layout::Node::with_children(area, vec![node])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let cursor = within(cursor, layout.bounds());
        let panel = layout.children().next().unwrap();
        let over = cursor.is_over(panel.bounds());
        let state = tree.state.downcast_mut::<State>();
        let skipped = state.passing
            && matches!(event, Event::Mouse(m) if !matches!(m, mouse::Event::ButtonReleased(_)));
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(_)) if !over => state.passing = true,
            Event::Mouse(mouse::Event::ButtonReleased(_)) => state.passing = false,
            _ => {}
        }
        if skipped {
            return;
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            panel,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let kept = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. })
        );
        if kept && over {
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<State>().passing {
            return mouse::Interaction::None;
        }
        let cursor = within(cursor, layout.bounds());
        let panel = layout.children().next().unwrap();
        let inner = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            panel,
            cursor,
            viewport,
            renderer,
        );
        if inner == mouse::Interaction::None && cursor.is_over(panel.bounds()) {
            mouse::Interaction::Idle
        } else {
            inner
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(area) = layout.bounds().intersection(viewport) else {
            return;
        };
        let cursor = if tree.state.downcast_ref::<State>().passing {
            mouse::Cursor::Unavailable
        } else {
            within(cursor, area)
        };
        renderer.with_layer(area, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout.children().next().unwrap(),
                cursor,
                &area,
            );
        });
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

// Whatever of the panel has slid past the area is clipped away, so it cannot be pointed at either.
fn within(cursor: mouse::Cursor, area: Rectangle) -> mouse::Cursor {
    match cursor.position() {
        Some(at) if !area.contains(at) => mouse::Cursor::Unavailable,
        _ => cursor,
    }
}
