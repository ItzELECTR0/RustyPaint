pub mod handles;
mod pipeline;

pub use handles::Handle;
pub use pipeline::Viewport as ViewportPipeline;

use crate::doc::Rect;
use crate::paint::curve;
use crate::select::Xform;
use crate::ui::theme;
use iced::widget::shader;
use iced::{Point, Rectangle, Size, Vector, mouse};
use std::sync::Arc;

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 32.0;

const ANTS_SPEED: f32 = 20.0;

const CARET_BLINK: std::time::Duration = std::time::Duration::from_millis(530);

const FIT_MARGIN: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub pan: Vector,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pan: Vector::ZERO,
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn canvas_rect(&self, viewport: Size, canvas: (u32, u32)) -> Rectangle {
        let width = canvas.0 as f32 * self.zoom;
        let height = canvas.1 as f32 * self.zoom;
        Rectangle {
            x: (viewport.width - width) / 2.0 + self.pan.x,
            y: (viewport.height - height) / 2.0 + self.pan.y,
            width,
            height,
        }
    }

    pub fn to_image(self, point: Point, viewport: Size, canvas: (u32, u32)) -> (f32, f32) {
        let rect = self.canvas_rect(viewport, canvas);
        (
            (point.x - rect.x) / self.zoom,
            (point.y - rect.y) / self.zoom,
        )
    }

    pub fn fit_zoom(viewport: Size, canvas: (u32, u32)) -> f32 {
        if canvas.0 == 0 || canvas.1 == 0 {
            return 1.0;
        }
        let scale = (viewport.width / canvas.0 as f32).min(viewport.height / canvas.1 as f32);
        (scale * FIT_MARGIN).clamp(MIN_ZOOM, 1.0)
    }

    pub fn fitted(viewport: Size, canvas: (u32, u32)) -> Self {
        Self {
            pan: Vector::ZERO,
            zoom: Self::fit_zoom(viewport, canvas),
        }
    }

    pub fn zoomed_at(self, anchor: Point, zoom: f32, viewport: Size, canvas: (u32, u32)) -> Self {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let before = self.canvas_rect(viewport, canvas);
        let image_x = (anchor.x - before.x) / self.zoom;
        let image_y = (anchor.y - before.y) / self.zoom;

        let width = canvas.0 as f32 * zoom;
        let height = canvas.1 as f32 * zoom;
        Self {
            pan: Vector::new(
                anchor.x - image_x * zoom - (viewport.width - width) / 2.0,
                anchor.y - image_y * zoom - (viewport.height - height) / 2.0,
            ),
            zoom,
        }
    }
}

#[derive(Clone)]
pub struct CanvasFrame {
    pub pixels: Arc<Vec<u8>>,
    pub size: (u32, u32),
    pub version: u64,
    pub dirty: Option<(u64, Rect)>,
    pub view: View,
    pub show_canvas: bool,
    pub handles: bool,
    pub preview: Option<(u32, u32)>,
    pub backing: bool,
    pub floating: Option<FloatingFrame>,
    pub ants: f32,
    pub frame: Option<crate::doc::Rect>,
    pub marquee: Option<Rect>,
}

#[derive(Clone)]
pub struct FloatingFrame {
    pub pixels: Arc<Vec<u8>>,
    pub size: (u32, u32),
    pub version: u64,
    pub xform: Xform,
    pub points: Vec<(f32, f32)>,
    pub editing_text: bool,
    pub text_empty: bool,
    pub opacity: f32,
    pub masked: bool,
    pub grips: bool,
}

impl std::fmt::Debug for CanvasFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasFrame")
            .field("size", &self.size)
            .field("version", &self.version)
            .field("view", &self.view)
            .finish()
    }
}

#[derive(Default)]
struct Floated {
    centre: [f32; 2],
    half: [f32; 2],
    rotation: f32,
    present: f32,
    handles: f32,
    hot: f32,
    reach: f32,
    count: f32,
    opacity: f32,
    masked: f32,
    points: [[f32; 4]; 12],
}

#[derive(Debug)]
pub struct Primitive {
    frame: CanvasFrame,
    ants: f32,
    hot: Option<Handle>,
    hot_grab: Option<Grab>,
    brush: Option<(f32, Point)>,
}

impl Primitive {
    fn floating_uniform(&self, canvas: Rectangle, scale: f32) -> Floated {
        let Some(floating) = &self.frame.floating else {
            return Floated::default();
        };
        let zoom = self.frame.view.zoom * scale;
        let origin = (canvas.x * scale, canvas.y * scale);
        let (cx, cy) = floating.xform.centre();
        let to_screen = |(x, y): (f32, f32)| [origin.0 + x * zoom, origin.1 + y * zoom];

        let mut points = [[0.0f32; 4]; 12];
        for (i, point) in floating.points.iter().take(curve::MAX_POINTS).enumerate() {
            let p = to_screen(*point);
            let slot = &mut points[i / 2];
            let at = (i % 2) * 2;
            slot[at] = p[0];
            slot[at + 1] = p[1];
        }

        Floated {
            centre: to_screen((cx, cy)),
            half: [
                floating.xform.width * zoom / 2.0,
                floating.xform.height * zoom / 2.0,
            ],
            rotation: floating.xform.rotation,
            present: 1.0,
            handles: if floating.grips { 1.0 } else { 0.0 },
            hot: self.hot_grab.map_or(-1.0, Grab::hot),
            reach: ROTATION_REACH * scale,
            count: floating.points.len().min(curve::MAX_POINTS) as f32,
            opacity: floating.opacity,
            masked: if floating.masked { 1.0 } else { 0.0 },
            points,
        }
    }

    fn frame_rect(&self, canvas: Rectangle, scale: f32) -> [f32; 4] {
        let Some(crop) = self.frame.frame else {
            return [0.0, 0.0, -1.0, 0.0];
        };
        let zoom = self.frame.view.zoom * scale;
        [
            (canvas.x * scale) + crop.x0 as f32 * zoom,
            (canvas.y * scale) + crop.y0 as f32 * zoom,
            crop.width() as f32 * zoom,
            crop.height() as f32 * zoom,
        ]
    }

    fn brush_ring(&self, scale: f32) -> [f32; 4] {
        let Some((diameter, at)) = self.brush else {
            return [0.0, 0.0, 0.0, 0.0];
        };
        let radius = (diameter * self.frame.view.zoom * scale / 2.0).max(0.5);
        [at.x * scale, at.y * scale, radius, 1.0]
    }

    fn marquee_rect(&self, canvas: Rectangle, scale: f32) -> [f32; 4] {
        let Some(rect) = self.frame.marquee else {
            return [0.0, 0.0, -1.0, 0.0];
        };
        let zoom = self.frame.view.zoom * scale;
        [
            (canvas.x * scale) + rect.x0 as f32 * zoom,
            (canvas.y * scale) + rect.y0 as f32 * zoom,
            rect.width() as f32 * zoom,
            rect.height() as f32 * zoom,
        ]
    }

    fn preview_rect(&self, canvas: Rectangle, scale: f32) -> [f32; 4] {
        let Some((w, h)) = self.frame.preview else {
            return [0.0, 0.0, -1.0, 0.0];
        };
        let zoom = self.frame.view.zoom;
        let (pw, ph) = (w as f32 * zoom, h as f32 * zoom);

        let (dx, dy) = match self.hot.map(Handle::anchor) {
            Some(a) => a.growth(),
            None => (0.5, 0.5),
        };
        let x = canvas.x - (pw - canvas.width) * dx;
        let y = canvas.y - (ph - canvas.height) * dy;
        [x * scale, y * scale, pw * scale, ph * scale]
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = ViewportPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &shader::Viewport,
    ) {
        pipeline.sync_canvas(
            device,
            queue,
            self.frame.size,
            self.frame.version,
            self.frame.dirty,
            &self.frame.pixels,
        );
        pipeline.sync_floating(
            device,
            queue,
            self.frame
                .floating
                .as_ref()
                .map(|f| (f.size.0, f.size.1, f.version, f.pixels.as_slice())),
        );
        pipeline.rebind(device);

        let scale = viewport.scale_factor();
        let logical = Size::new(bounds.width, bounds.height);
        let canvas = self.frame.view.canvas_rect(logical, self.frame.size);
        let srgb = pipeline.is_srgb_target();
        let float = self.floating_uniform(canvas, scale);

        pipeline.write_uniforms(
            queue,
            &pipeline::Uniforms {
                viewport_size: [bounds.width * scale, bounds.height * scale],
                canvas_pos: [canvas.x * scale, canvas.y * scale],
                canvas_size: [canvas.width * scale, canvas.height * scale],
                texture_size: [self.frame.size.0 as f32, self.frame.size.1 as f32],
                workspace_top: rgba(theme::colours().workspace_top),
                workspace_bottom: rgba(theme::colours().workspace_bottom),
                checker_light: rgba(theme::colours().checker_light),
                checker_dark: rgba(theme::colours().checker_dark),
                zoom: self.frame.view.zoom,
                checker_size: theme::CHECKER_SQUARE * scale,
                srgb_target: if srgb { 1.0 } else { 0.0 },
                show_canvas: if self.frame.show_canvas { 1.0 } else { 0.0 },
                preview: self.preview_rect(canvas, scale),
                handles: if self.frame.handles { 1.0 } else { 0.0 },
                hot_handle: self.hot.map_or(-1.0, |h| h.index() as f32),
                backing: if self.frame.backing { 1.0 } else { 0.0 },
                shadow: theme::colours().shadow,
                float_centre: float.centre,
                float_half: float.half,
                float_rotation: float.rotation,
                float_present: float.present,
                ants: self.ants,
                float_handles: float.handles,
                float_hot: float.hot,
                float_reach: float.reach,
                curve_count: float.count,
                float_opacity: float.opacity,
                curve_points: float.points,
                accent: rgba(theme::colours().accent),
                brush_ring: self.brush_ring(scale),
                crop: self.frame_rect(canvas, scale),
                marquee: self.marquee_rect(canvas, scale),
                float_masked: float.masked,
                _pad3: [0.0; 3],
            },
        );
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.draw(render_pass)
    }
}

fn rgba(c: iced::Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

#[derive(Debug, Clone, Copy)]
pub enum Interaction {
    Viewed(View),
    PaintBegan(f32, f32),
    PaintMoved(f32, f32),
    PaintEnded,
    ResizePreview(u32, u32),
    Resized(u32, u32, Handle),
    ResizeCancelled,

    SelectBegan(f32, f32),
    SelectMoved(f32, f32),
    SelectEnded,
    FloatGrabbed(Grab, f32, f32),
    FloatDragged(f32, f32),
    FloatReleased,
    FloatReleasedAt(f32, f32),
    CaretTick,
    FrameGrabbed(Handle),
    FrameDragged(f32, f32),
    FrameReleased,
    PointAdded(f32, f32),
    PointRemoved(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    Move,
    Resize(Handle),
    Rotate,
    Point(usize),
    Caret,
}

impl Grab {
    fn hot(self) -> f32 {
        match self {
            Grab::Resize(handle) => handle.index() as f32,
            Grab::Rotate => 8.0,
            Grab::Point(i) => 9.0 + i as f32,
            Grab::Move | Grab::Caret => -1.0,
        }
    }
}

pub struct Program {
    pub frame: CanvasFrame,
    pub cursor: mouse::Interaction,
    pub brush: Option<f32>,
    pub selecting: bool,
}

impl Program {
    fn frame_on_screen(&self, bounds: Rectangle) -> Option<Rectangle> {
        let crop = self.frame.frame?;
        let canvas = self.canvas_rect(bounds);
        let zoom = self.frame.view.zoom;
        Some(Rectangle {
            x: canvas.x + crop.x0 as f32 * zoom,
            y: canvas.y + crop.y0 as f32 * zoom,
            width: crop.width() as f32 * zoom,
            height: crop.height() as f32 * zoom,
        })
    }

    fn canvas_rect(&self, bounds: Rectangle) -> Rectangle {
        self.frame
            .view
            .canvas_rect(Size::new(bounds.width, bounds.height), self.frame.size)
    }

    fn image_point(&self, point: Point, bounds: Rectangle) -> (f32, f32) {
        self.frame.view.to_image(
            point,
            Size::new(bounds.width, bounds.height),
            self.frame.size,
        )
    }

    fn grab_at(&self, x: f32, y: f32) -> Option<Grab> {
        let floating = self.frame.floating.as_ref()?;
        let xform = floating.xform;
        let zoom = self.frame.view.zoom.max(0.01);
        let reach = (handles::HALF + 3.0) / zoom;

        let near =
            |target: (f32, f32)| (x - target.0).abs() <= reach && (y - target.1).abs() <= reach;

        for (i, point) in floating.points.iter().enumerate() {
            if near(*point) {
                return Some(Grab::Point(i));
            }
        }

        if near(xform.rotation_grip(ROTATION_REACH / zoom)) {
            return Some(Grab::Rotate);
        }
        for handle in handles::ALL {
            if near(xform.handle_at(handle)) {
                return Some(Grab::Resize(handle));
            }
        }
        if !xform.contains(x, y) {
            return None;
        }
        if floating.editing_text && !floating.text_empty {
            let (u, v) = xform.to_local(x, y);
            let band = (
                EDGE_BAND / zoom / xform.width.max(f32::EPSILON),
                EDGE_BAND / zoom / xform.height.max(f32::EPSILON),
            );
            let on_edge = u < band.0 || u > 1.0 - band.0 || v < band.1 || v > 1.0 - band.1;
            return Some(if on_edge { Grab::Move } else { Grab::Caret });
        }
        Some(Grab::Move)
    }
}

const EDGE_BAND: f32 = 12.0;

pub const ROTATION_REACH: f32 = 22.0;

#[derive(Clone, Copy)]
pub struct Drag {
    pub handle: Handle,
    from: (f32, f32),
    size: (u32, u32),
}

#[derive(Default)]
pub struct State {
    panning_from: Option<Point>,
    pan_origin: Vector,
    painting: bool,
    hot: Option<Handle>,
    dragging: Option<Drag>,
    selecting: bool,
    grabbing: Option<Grab>,
    pending_float: Option<(f32, f32)>,
    ants: f32,
    ants_since: Option<iced::time::Instant>,
    caret_version: Option<u64>,
    caret_since: Option<iced::time::Instant>,
    hot_grab: Option<Grab>,
    last_click: Option<iced::advanced::mouse::Click>,
    cropping: Option<Handle>,
}

impl<Message> shader::Program<Message> for Program
where
    Message: From<Interaction>,
{
    type State = State;
    type Primitive = Primitive;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        use iced::Event;
        use mouse::{Button, Event as Mouse, ScrollDelta};

        let local = cursor.position_in(bounds);

        match event {
            Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let floating = self.frame.floating.is_some();
                if floating {
                    if let Some(since) = state.ants_since {
                        let elapsed = now.duration_since(since).as_secs_f32().min(0.25);
                        state.ants += elapsed * ANTS_SPEED;
                    }
                    state.ants_since = Some(*now);
                } else {
                    state.ants_since = None;
                }

                let mut blink = false;
                match self.frame.floating.as_ref().filter(|f| f.editing_text) {
                    Some(text) if state.caret_version != Some(text.version) => {
                        state.caret_version = Some(text.version);
                        state.caret_since = Some(*now);
                    }
                    Some(_) => {
                        if state
                            .caret_since
                            .is_some_and(|since| now.duration_since(since) >= CARET_BLINK)
                        {
                            blink = true;
                        }
                    }
                    None => {
                        state.caret_version = None;
                        state.caret_since = None;
                    }
                }

                if state.grabbing.is_some()
                    && let Some((x, y)) = state.pending_float.take()
                {
                    return Some(shader::Action::publish(
                        Interaction::FloatDragged(x, y).into(),
                    ));
                }

                if blink {
                    state.caret_since = Some(*now);
                    return Some(shader::Action::publish(Interaction::CaretTick.into()));
                }

                floating.then(shader::Action::request_redraw)
            }

            Event::Mouse(Mouse::WheelScrolled { delta }) => {
                let anchor = local?;
                let steps = match delta {
                    ScrollDelta::Lines { y, .. } => *y,
                    ScrollDelta::Pixels { y, .. } => *y / 50.0,
                };
                if steps == 0.0 {
                    return None;
                }
                let zoom = self.frame.view.zoom * 1.2_f32.powf(steps);
                let view = self.frame.view.zoomed_at(
                    anchor,
                    zoom,
                    Size::new(bounds.width, bounds.height),
                    self.frame.size,
                );
                Some(shader::Action::publish(Interaction::Viewed(view).into()).and_capture())
            }

            Event::Mouse(Mouse::ButtonPressed(Button::Middle)) => {
                state.panning_from = local;
                state.pan_origin = self.frame.view.pan;
                local.map(|_| shader::Action::capture())
            }

            Event::Mouse(Mouse::ButtonReleased(Button::Middle)) => {
                state.panning_from = None;
                None
            }

            Event::Mouse(Mouse::ButtonPressed(Button::Left)) => {
                let point = local?;
                let (x, y) = self.image_point(point, bounds);

                if let Some(frame) = self.frame_on_screen(bounds) {
                    let grabbed = handles::hit(frame, point);
                    state.cropping = grabbed;
                    return Some(match grabbed {
                        Some(handle) => {
                            shader::Action::publish(Interaction::FrameGrabbed(handle).into())
                                .and_capture()
                        }
                        None => shader::Action::capture(),
                    });
                }

                let click =
                    iced::advanced::mouse::Click::new(point, Button::Left, state.last_click);
                state.last_click = Some(click);

                let bendy = self
                    .frame
                    .floating
                    .as_ref()
                    .is_some_and(|f| !f.points.is_empty());
                if bendy && click.kind() == iced::advanced::mouse::click::Kind::Double {
                    let message = match self.grab_at(x, y) {
                        Some(Grab::Point(i)) => Interaction::PointRemoved(i),
                        _ => Interaction::PointAdded(x, y),
                    };
                    state.grabbing = None;
                    return Some(shader::Action::publish(message.into()).and_capture());
                }

                if (self.selecting || self.frame.floating.is_some())
                    && let Some(grab) = self.grab_at(x, y)
                {
                    state.grabbing = Some(grab);
                    state.pending_float = None;
                    return Some(
                        shader::Action::publish(Interaction::FloatGrabbed(grab, x, y).into())
                            .and_capture(),
                    );
                }

                if self.frame.handles
                    && let Some(handle) = handles::hit(self.canvas_rect(bounds), point)
                {
                    state.dragging = Some(Drag {
                        handle,
                        from: (x, y),
                        size: self.frame.size,
                    });
                    return Some(shader::Action::capture());
                }

                if self.selecting {
                    state.selecting = true;
                    return Some(
                        shader::Action::publish(Interaction::SelectBegan(x, y).into())
                            .and_capture(),
                    );
                }

                state.painting = true;
                Some(shader::Action::publish(Interaction::PaintBegan(x, y).into()).and_capture())
            }

            Event::Mouse(Mouse::ButtonReleased(Button::Left)) => {
                if state.cropping.take().is_some() {
                    return Some(
                        shader::Action::publish(Interaction::FrameReleased.into()).and_capture(),
                    );
                }
                if state.grabbing.take().is_some() {
                    let released = match state.pending_float.take() {
                        Some((x, y)) => Interaction::FloatReleasedAt(x, y),
                        None => Interaction::FloatReleased,
                    };
                    return Some(shader::Action::publish(released.into()).and_capture());
                }
                if state.selecting {
                    state.selecting = false;
                    return Some(
                        shader::Action::publish(Interaction::SelectEnded.into()).and_capture(),
                    );
                }
                if let Some(drag) = state.dragging.take() {
                    let message = match self.frame.preview {
                        Some((w, h)) => Interaction::Resized(w, h, drag.handle),
                        None => Interaction::ResizeCancelled,
                    };
                    return Some(shader::Action::publish(message.into()).and_capture());
                }
                if !state.painting {
                    return None;
                }
                state.painting = false;
                Some(shader::Action::publish(Interaction::PaintEnded.into()).and_capture())
            }

            Event::Mouse(Mouse::CursorMoved { .. }) => {
                let cursor = cursor.position()?;
                let point = Point::new(cursor.x - bounds.x, cursor.y - bounds.y);

                if state.cropping.is_some() {
                    let (x, y) = self.image_point(point, bounds);
                    return Some(
                        shader::Action::publish(Interaction::FrameDragged(x, y).into())
                            .and_capture(),
                    );
                }
                if let Some(start) = state.panning_from {
                    let view = View {
                        pan: state.pan_origin + (point - start),
                        ..self.frame.view
                    };
                    return Some(
                        shader::Action::publish(Interaction::Viewed(view).into()).and_capture(),
                    );
                }
                if state.grabbing.is_some() {
                    let (x, y) = self.image_point(point, bounds);
                    state.pending_float = Some((x, y));
                    return Some(shader::Action::request_redraw().and_capture());
                }
                if state.selecting {
                    let (x, y) = self.image_point(point, bounds);
                    return Some(
                        shader::Action::publish(Interaction::SelectMoved(x, y).into())
                            .and_capture(),
                    );
                }
                if let Some(drag) = state.dragging {
                    let (x, y) = self.image_point(point, bounds);
                    let delta = (x - drag.from.0, y - drag.from.1);
                    let (w, h) = drag.handle.resize(drag.size, delta);
                    return Some(
                        shader::Action::publish(Interaction::ResizePreview(w, h).into())
                            .and_capture(),
                    );
                }
                if state.painting {
                    let (x, y) = self.image_point(point, bounds);
                    return Some(
                        shader::Action::publish(Interaction::PaintMoved(x, y).into()).and_capture(),
                    );
                }

                let hot = match self.frame_on_screen(bounds) {
                    Some(frame) => handles::hit(frame, point),
                    None => self
                        .frame
                        .handles
                        .then(|| handles::hit(self.canvas_rect(bounds), point))
                        .flatten(),
                };
                let (x, y) = self.image_point(point, bounds);
                let hot_grab = (self.selecting || self.frame.floating.is_some())
                    .then(|| self.grab_at(x, y))
                    .flatten();
                if hot != state.hot || hot_grab != state.hot_grab {
                    state.hot = hot;
                    state.hot_grab = hot_grab;
                    return Some(shader::Action::request_redraw());
                }
                if self.brush.is_some() {
                    return Some(shader::Action::request_redraw());
                }
                None
            }

            _ => None,
        }
    }

    fn draw(&self, state: &Self::State, cursor: mouse::Cursor, bounds: Rectangle) -> Primitive {
        Primitive {
            frame: self.frame.clone(),
            ants: self.frame.ants + state.ants,
            hot: state.dragging.map(|d| d.handle).or(state.hot),
            hot_grab: state.grabbing.or(state.hot_grab),
            brush: self.brush.zip(cursor.position_in(bounds)),
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.panning_from.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(handle) = state.dragging.map(|d| d.handle).or(state.hot) {
            return handle.cursor();
        }
        if (self.selecting || self.frame.floating.is_some())
            && let Some(point) = cursor.position_in(bounds)
        {
            let (x, y) = self.image_point(point, bounds);
            return match self.grab_at(x, y) {
                Some(Grab::Move) => mouse::Interaction::Move,
                Some(Grab::Resize(handle)) => handle.cursor(),
                Some(Grab::Rotate) | Some(Grab::Point(_)) => mouse::Interaction::Grab,
                Some(Grab::Caret) => mouse::Interaction::Text,
                None => mouse::Interaction::Crosshair,
            };
        }
        if cursor.is_over(bounds) {
            return self.cursor;
        }
        mouse::Interaction::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: (u32, u32) = (800, 600);

    fn viewport() -> Size {
        Size::new(1000.0, 700.0)
    }

    fn grey(p: &[u8; 4]) -> bool {
        p[0] == p[1] && p[1] == p[2] && p[3] == 255
    }

    fn red(w: u32, h: u32) -> Vec<u8> {
        [255u8, 0, 0, 255].repeat(w as usize * h as usize)
    }

    fn floating_program(points: Vec<(f32, f32)>) -> Program {
        Program {
            frame: CanvasFrame {
                pixels: Arc::new(vec![0; CANVAS.0 as usize * CANVAS.1 as usize * 4]),
                size: CANVAS,
                version: 0,
                dirty: None,
                view: View::default(),
                show_canvas: true,
                handles: false,
                preview: None,
                backing: true,
                floating: Some(FloatingFrame {
                    pixels: Arc::new(vec![0; 4]),
                    size: (1, 1),
                    version: 0,
                    xform: Xform {
                        x: 100.0,
                        y: 100.0,
                        width: 80.0,
                        height: 60.0,
                        rotation: 0.0,
                    },
                    points,
                    editing_text: false,
                    text_empty: false,
                    opacity: 1.0,
                    grips: true,
                    masked: false,
                }),
                ants: 0.0,
                frame: None,
                marquee: None,
            },
            cursor: mouse::Interaction::Crosshair,
            brush: None,
            selecting: false,
        }
    }

    fn interact(
        program: &Program,
        state: &mut State,
        event: iced::Event,
        cursor: mouse::Cursor,
    ) -> Option<Interaction> {
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 700.0,
        };
        <Program as shader::Program<Interaction>>::update(program, state, &event, bounds, cursor)
            .and_then(|action| action.into_inner().0)
    }

    #[test]
    fn floating_drag_samples_are_coalesced_to_the_redraw() {
        use iced::mouse::Event as Mouse;

        let program = floating_program(Vec::new());
        let mut state = State {
            grabbing: Some(Grab::Rotate),
            ..State::default()
        };

        let mut latest = Point::ORIGIN;
        for i in 0..1000 {
            let point = Point::new(200.0 + i as f32 * 0.3, 200.0 + i as f32 * 0.1);
            latest = point;
            let message = interact(
                &program,
                &mut state,
                iced::Event::Mouse(Mouse::CursorMoved { position: point }),
                mouse::Cursor::Available(point),
            );
            assert!(message.is_none(), "a raw sample reached the application");
        }

        let now = iced::time::Instant::now();
        let message = interact(
            &program,
            &mut state,
            iced::Event::Window(iced::window::Event::RedrawRequested(now)),
            mouse::Cursor::Unavailable,
        );
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 700.0,
        };
        let wanted = program.image_point(latest, bounds);
        assert!(
            matches!(message, Some(Interaction::FloatDragged(x, y))
                if (x - wanted.0).abs() < 0.001 && (y - wanted.1).abs() < 0.001),
            "the redraw did not get the newest sample: {message:?}"
        );

        let repeated = interact(
            &program,
            &mut state,
            iced::Event::Window(iced::window::Event::RedrawRequested(
                now + std::time::Duration::from_millis(5),
            )),
            mouse::Cursor::Unavailable,
        );
        assert!(repeated.is_none(), "the same sample was sent twice");
    }

    #[test]
    fn releasing_a_floating_drag_flushes_the_last_sample() {
        use iced::mouse::{Button, Event as Mouse};

        let program = floating_program(Vec::new());
        let mut state = State {
            grabbing: Some(Grab::Rotate),
            ..State::default()
        };
        let point = Point::new(640.0, 360.0);
        interact(
            &program,
            &mut state,
            iced::Event::Mouse(Mouse::CursorMoved { position: point }),
            mouse::Cursor::Available(point),
        );

        let message = interact(
            &program,
            &mut state,
            iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)),
            mouse::Cursor::Available(point),
        );
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 700.0,
        };
        let wanted = program.image_point(point, bounds);
        assert!(
            matches!(message, Some(Interaction::FloatReleasedAt(x, y))
                if (x - wanted.0).abs() < 0.001 && (y - wanted.1).abs() < 0.001),
            "the release lost its final sample: {message:?}"
        );
        assert!(state.grabbing.is_none());
        assert!(state.pending_float.is_none());
    }

    #[test]
    fn the_ants_clock_does_not_rebuild_the_application() {
        let program = floating_program(Vec::new());
        let mut state = State::default();
        let start = iced::time::Instant::now();

        let first = interact(
            &program,
            &mut state,
            iced::Event::Window(iced::window::Event::RedrawRequested(start)),
            mouse::Cursor::Unavailable,
        );
        assert!(first.is_none());
        assert_eq!(state.ants, 0.0, "the first frame has no elapsed time");

        let second = interact(
            &program,
            &mut state,
            iced::Event::Window(iced::window::Event::RedrawRequested(
                start + std::time::Duration::from_millis(20),
            )),
            mouse::Cursor::Unavailable,
        );
        assert!(second.is_none(), "animation leaked into App::update");
        let walked = state.ants;
        assert!(
            (walked - 0.4).abs() < 0.01,
            "twenty milliseconds walked {walked} pixels"
        );

        let mut bare = floating_program(Vec::new());
        bare.frame.floating = None;
        interact(
            &bare,
            &mut state,
            iced::Event::Window(iced::window::Event::RedrawRequested(
                start + std::time::Duration::from_secs(1),
            )),
            mouse::Cursor::Unavailable,
        );
        assert_eq!(state.ants, walked, "the clock ran with nothing to outline");
        assert!(state.ants_since.is_none());
    }

    #[test]
    fn the_caret_only_reaches_the_application_when_it_changes() {
        let mut program = floating_program(Vec::new());
        program.frame.floating.as_mut().unwrap().editing_text = true;
        let mut state = State::default();
        let start = iced::time::Instant::now();
        let redraw = |after: std::time::Duration| {
            iced::Event::Window(iced::window::Event::RedrawRequested(start + after))
        };

        assert!(
            interact(
                &program,
                &mut state,
                redraw(std::time::Duration::ZERO),
                mouse::Cursor::Unavailable
            )
            .is_none(),
            "the timer should only start on the first frame"
        );
        assert!(
            interact(
                &program,
                &mut state,
                redraw(std::time::Duration::from_millis(529)),
                mouse::Cursor::Unavailable,
            )
            .is_none(),
            "a frame before the deadline reached the application"
        );
        assert!(matches!(
            interact(
                &program,
                &mut state,
                redraw(CARET_BLINK),
                mouse::Cursor::Unavailable,
            ),
            Some(Interaction::CaretTick)
        ));

        program.frame.floating.as_mut().unwrap().version += 1;
        assert!(
            interact(
                &program,
                &mut state,
                redraw(CARET_BLINK + std::time::Duration::from_millis(1)),
                mouse::Cursor::Unavailable,
            )
            .is_none(),
            "a changed text buffer inherited the old deadline"
        );
    }

    #[test]
    fn a_drag_sample_does_not_consume_a_due_caret_beat() {
        let mut program = floating_program(Vec::new());
        let text = program.frame.floating.as_mut().unwrap();
        text.editing_text = true;
        let version = text.version;
        let start = iced::time::Instant::now();
        let mut state = State {
            grabbing: Some(Grab::Rotate),
            pending_float: Some((12.0, 34.0)),
            caret_version: Some(version),
            caret_since: Some(start),
            ..State::default()
        };
        let redraw =
            || iced::Event::Window(iced::window::Event::RedrawRequested(start + CARET_BLINK));

        assert!(matches!(
            interact(&program, &mut state, redraw(), mouse::Cursor::Unavailable),
            Some(Interaction::FloatDragged(12.0, 34.0))
        ));
        assert!(matches!(
            interact(&program, &mut state, redraw(), mouse::Cursor::Unavailable),
            Some(Interaction::CaretTick)
        ));
    }

    #[test]
    fn a_floating_object_is_grabbable_with_no_select_tool() {
        let program = floating_program(Vec::new());
        assert_eq!(program.grab_at(140.0, 130.0), Some(Grab::Move));
        for handle in handles::ALL {
            let (x, y) = program
                .frame
                .floating
                .as_ref()
                .unwrap()
                .xform
                .handle_at(handle);
            assert_eq!(
                program.grab_at(x, y),
                Some(Grab::Resize(handle)),
                "{handle:?}"
            );
        }
        assert_eq!(program.grab_at(500.0, 500.0), None, "and nothing far away");
    }

    #[test]
    fn a_curve_offers_its_points_as_well_as_its_box() {
        let points = vec![(100.0, 100.0), (140.0, 130.0), (180.0, 160.0)];
        let mut program = floating_program(points.clone());
        program.frame.floating.as_mut().unwrap().xform = Xform {
            x: 100.0,
            y: 100.0,
            width: 80.0,
            height: 60.0,
            rotation: 0.0,
        };

        for (i, (x, y)) in points.iter().enumerate() {
            assert_eq!(program.grab_at(*x, *y), Some(Grab::Point(i)), "point {i}");
        }
        assert_eq!(
            program.grab_at(180.0, 100.0),
            Some(Grab::Resize(Handle::TopRight))
        );
    }

    #[test]
    fn the_shader_is_told_which_grip_is_lit() {
        assert_eq!(Grab::Resize(Handle::TopLeft).hot(), 0.0);
        assert_eq!(Grab::Rotate.hot(), 8.0);
        assert_eq!(Grab::Point(0).hot(), 9.0);
        assert_eq!(Grab::Move.hot(), -1.0);
    }

    #[test]
    fn the_grips_are_drawn_round_a_floating_object() {
        const NAME: &str = "shape-manipulator";
        let (w, h) = (600u32, 500u32);
        let mut program = floating_program(Vec::new());
        program.frame.size = (400, 300);
        program.frame.pixels = Arc::new(red(400, 300));
        program.frame.view = View::default();
        program.frame.floating.as_mut().unwrap().xform = Xform {
            x: 100.0,
            y: 80.0,
            width: 160.0,
            height: 120.0,
            rotation: 0.0,
        };

        let Some(pixels) = render_offscreen(&program, (w, h), NAME, mouse::Cursor::Unavailable)
        else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let at = |x: f32, y: f32| -> [u8; 4] {
            let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
            pixels[i..i + 4].try_into().unwrap()
        };

        let canvas = program.canvas_rect(Rectangle {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        });
        let xform = program.frame.floating.as_ref().unwrap().xform;
        let zoom = program.frame.view.zoom;
        let to_screen = |(ix, iy): (f32, f32)| (canvas.x + ix * zoom, canvas.y + iy * zoom);

        for handle in handles::ALL {
            let (x, y) = to_screen(xform.handle_at(handle));
            assert_eq!(at(x, y), [255, 255, 255, 255], "{handle:?} grip is missing");
        }
        let (dx, dy) = to_screen(xform.rotation_grip(ROTATION_REACH / zoom));
        assert_eq!(
            at(dx, dy),
            [255, 255, 255, 255],
            "the rotation dial is missing"
        );

        let (mx, my) = to_screen((xform.x + xform.width * 0.25, xform.y + 8.0));
        assert_eq!(at(mx, my), [255, 0, 0, 255], "a grip leaked along the edge");
    }

    #[test]
    fn the_brush_ring_is_the_size_of_the_brush() {
        const NAME: &str = "brush-ring";
        let (w, h) = (600u32, 500u32);
        let mut program = floating_program(Vec::new());
        program.frame.size = (400, 300);
        program.frame.pixels = Arc::new(red(400, 300));
        program.frame.floating = None;
        program.brush = Some(40.0);

        let pointer = mouse::Cursor::Available(Point::new(300.0, 250.0));
        let Some(pixels) = render_offscreen(&program, (w, h), NAME, pointer) else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let at = |x: f32, y: f32| -> [u8; 4] {
            let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
            pixels[i..i + 4].try_into().unwrap()
        };

        let brightest = (240..=260)
            .flat_map(|y| (310..=330).map(move |x| (x, y)))
            .map(|(x, y)| at(x as f32, y as f32)[1])
            .max()
            .expect("a pixel");
        assert!(
            brightest > 150,
            "no ring at the brush's edge, brightest was {brightest}"
        );
        assert_eq!(
            at(300.0, 250.0),
            [255, 0, 0, 255],
            "the middle should be bare canvas"
        );
        assert_eq!(
            at(345.0, 250.0),
            [255, 0, 0, 255],
            "and so should well outside it"
        );
    }

    #[test]
    fn a_preview_is_drawn_in_canvas_pixels() {
        const NAME: &str = "float-pixels";
        const SIDE: u32 = 8;
        const ZOOM: f32 = 8.0;
        let (w, h) = (600u32, 500u32);

        let mut float = Vec::new();
        for _ in 0..SIDE {
            for x in 0..SIDE {
                float.extend_from_slice(if x % 2 == 0 {
                    &[0u8, 0, 255, 255]
                } else {
                    &[0u8, 255, 0, 255]
                });
            }
        }

        let mut program = floating_program(Vec::new());
        program.frame.size = (24, 20);
        program.frame.pixels = Arc::new(red(24, 20));
        program.frame.view = View {
            pan: Vector::ZERO,
            zoom: ZOOM,
        };
        let floating = program.frame.floating.as_mut().unwrap();
        floating.pixels = Arc::new(float);
        floating.size = (SIDE, SIDE);
        floating.xform = Xform {
            x: 5.5,
            y: 4.5,
            width: SIDE as f32,
            height: SIDE as f32,
            rotation: 0.0,
        };

        let Some(pixels) = render_offscreen(&program, (w, h), NAME, mouse::Cursor::Unavailable)
        else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let at = |x: f32, y: f32| -> [u8; 4] {
            let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
            pixels[i..i + 4].try_into().unwrap()
        };

        let canvas = program.canvas_rect(Rectangle {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        });
        let row = canvas.y + 6.5 * ZOOM;

        for step in 0..((SIDE - 1) * ZOOM as u32) {
            let x = canvas.x + 6.0 * ZOOM + step as f32;
            let pixel = at(x, row);
            assert!(
                pixel == [0, 0, 255, 255] || pixel == [0, 255, 0, 255],
                "blended pixel {pixel:?} at step {step}"
            );
        }

        let edge = canvas.x + 5.0 * ZOOM;
        assert_eq!(
            at(edge - 2.0, row),
            [255, 0, 0, 255],
            "the pixel before it is bare canvas"
        );
        assert_eq!(
            at(edge + 1.0, row),
            [0, 0, 255, 255],
            "and it starts on the canvas pixel"
        );
    }

    #[test]
    fn the_ants_crawl_the_way_round_paint_3d_does() {
        const NAME: &str = "ants-direction";
        const SHIFT: f32 = 4.0;
        let (w, h) = (600u32, 500u32);

        let pattern = |ants: f32, name: &str| -> Option<Vec<bool>> {
            let mut program = floating_program(Vec::new());
            program.frame.size = (400, 300);
            program.frame.pixels = Arc::new(red(400, 300));
            program.frame.view = View::default();
            program.frame.ants = ants;
            let floating = program.frame.floating.as_mut().unwrap();
            floating.xform = Xform {
                x: 100.0,
                y: 80.0,
                width: 200.0,
                height: 120.0,
                rotation: 0.0,
            };

            let pixels = render_offscreen(&program, (w, h), name, mouse::Cursor::Unavailable)?;
            let canvas = program.canvas_rect(Rectangle {
                x: 0.0,
                y: 0.0,
                width: w as f32,
                height: h as f32,
            });
            let at = |x: f32, y: f32| -> [u8; 4] {
                let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
                pixels[i..i + 4].try_into().unwrap()
            };

            let y = canvas.y + 80.0;
            Some(
                (120..=170)
                    .map(|x| at(canvas.x + x as f32, y)[0] > 127)
                    .collect(),
            )
        };

        let Some(still) = pattern(0.0, NAME) else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let moved = pattern(SHIFT, "ants-direction-later").expect("the second frame too");

        let shift = SHIFT as usize;
        let overlap = still.len() - shift;
        let same = (0..overlap)
            .filter(|i| moved[i + shift] == still[*i])
            .count();
        let backwards = (0..overlap)
            .filter(|i| moved[*i] == still[i + shift])
            .count();
        assert!(
            same > overlap - 3,
            "the ants are not crawling right along the top: {same} of {overlap} match going that \
             way, {backwards} going the other"
        );
    }

    #[test]
    fn a_fraction_of_a_pixel_of_crawl_still_moves_the_ants() {
        const NAME: &str = "ants-subpixel";
        const FRAME: f32 = 20.0 / 144.0;
        let (w, h) = (600u32, 500u32);

        let edge = |ants: f32, name: &str| -> Option<Vec<[u8; 4]>> {
            let mut program = floating_program(Vec::new());
            program.frame.size = (400, 300);
            program.frame.pixels = Arc::new(red(400, 300));
            program.frame.view = View::default();
            program.frame.ants = ants;
            let floating = program.frame.floating.as_mut().unwrap();
            floating.xform = Xform {
                x: 100.0,
                y: 80.0,
                width: 200.0,
                height: 120.0,
                rotation: 0.0,
            };

            let pixels = render_offscreen(&program, (w, h), name, mouse::Cursor::Unavailable)?;
            let canvas = program.canvas_rect(Rectangle {
                x: 0.0,
                y: 0.0,
                width: w as f32,
                height: h as f32,
            });
            let at = |x: f32, y: f32| -> [u8; 4] {
                let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
                pixels[i..i + 4].try_into().unwrap()
            };
            let y = canvas.y + 80.0;
            Some((120..=170).map(|x| at(canvas.x + x as f32, y)).collect())
        };

        let Some(still) = edge(0.0, NAME) else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let nudged = edge(FRAME, "ants-subpixel-later").expect("the second frame too");

        assert!(still.iter().all(grey), "the top edge is not the outline");
        assert!(
            still.iter().any(|p| p[0] > 40 && p[0] < 215),
            "the dashes have hard edges, nothing in between them"
        );
        assert_ne!(
            still, nudged,
            "a frame of crawl at 144Hz changed nothing on screen"
        );
    }

    #[test]
    fn the_rectangle_being_dragged_out_is_outlined_and_stands_still() {
        const NAME: &str = "marquee";
        let (w, h) = (600u32, 500u32);
        let dragged = Rect::new(100, 80, 300, 200);

        let pattern = |ants: f32, name: &str| -> Option<Vec<[u8; 4]>> {
            let mut program = floating_program(Vec::new());
            program.frame.size = (400, 300);
            program.frame.pixels = Arc::new(red(400, 300));
            program.frame.view = View::default();
            program.frame.ants = ants;
            program.frame.floating = None;
            program.frame.marquee = Some(dragged);

            let pixels = render_offscreen(&program, (w, h), name, mouse::Cursor::Unavailable)?;
            let canvas = program.canvas_rect(Rectangle {
                x: 0.0,
                y: 0.0,
                width: w as f32,
                height: h as f32,
            });
            let at = |x: f32, y: f32| -> [u8; 4] {
                let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
                pixels[i..i + 4].try_into().unwrap()
            };
            let y = canvas.y + dragged.y0 as f32;
            Some((120..=260).map(|x| at(canvas.x + x as f32, y)).collect())
        };

        let Some(still) = pattern(0.0, NAME) else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let later = pattern(40.0, "marquee-later").expect("the second frame too");

        assert!(
            still.iter().all(grey),
            "the top edge is not the selection outline"
        );
        assert!(still.iter().any(|p| p[0] < 40), "no dark dashes");
        assert!(still.iter().any(|p| p[0] > 215), "no light dashes");
        assert_eq!(still, later, "the ants moved while nothing was selected");
    }

    #[test]
    fn a_masked_selection_is_outlined_along_its_own_edge() {
        const NAME: &str = "lasso-ants";
        const SIDE: u32 = 64;
        let (w, h) = (600u32, 500u32);

        let mut float = vec![0u8; (SIDE * SIDE * 4) as usize];
        for y in 0..SIDE {
            for x in 0..SIDE {
                if x + y < SIDE {
                    let i = ((y * SIDE + x) * 4) as usize;
                    float[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }

        let mut program = floating_program(Vec::new());
        program.frame.size = (400, 300);
        program.frame.pixels = Arc::new(red(400, 300));
        program.frame.view = View::default();
        let floating = program.frame.floating.as_mut().unwrap();
        floating.pixels = Arc::new(float);
        floating.size = (SIDE, SIDE);
        floating.masked = true;
        floating.xform = Xform {
            x: 100.0,
            y: 80.0,
            width: SIDE as f32,
            height: SIDE as f32,
            rotation: 0.0,
        };

        let Some(pixels) = render_offscreen(&program, (w, h), NAME, mouse::Cursor::Unavailable)
        else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let at = |x: f32, y: f32| -> [u8; 4] {
            let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
            pixels[i..i + 4].try_into().unwrap()
        };

        let canvas = program.canvas_rect(Rectangle {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        });
        let zoom = program.frame.view.zoom;
        let to_screen = |(ix, iy): (f32, f32)| (canvas.x + ix * zoom, canvas.y + iy * zoom);
        let dashed = |p: [u8; 4]| grey(&p);

        let (cx, cy) = to_screen((100.0 + SIDE as f32 * 0.78, 80.0 + SIDE as f32 - 1.0));
        assert_eq!(
            at(cx, cy),
            [255, 0, 0, 255],
            "the box was outlined, not the loop"
        );

        let outlined = (8..SIDE - 8).any(|i| {
            let (x, y) = to_screen((100.0 + i as f32, 80.0 + (SIDE - 1 - i) as f32));
            dashed(at(x, y))
        });
        assert!(outlined, "the loop has no outline at all");
    }

    #[test]
    fn a_curve_is_drawn_with_its_points_and_its_box() {
        const NAME: &str = "curve-grips";
        let (w, h) = (600u32, 500u32);
        let points = vec![(120.0, 100.0), (200.0, 60.0), (260.0, 180.0)];
        let mut program = floating_program(points.clone());
        program.frame.size = (400, 300);
        program.frame.pixels = Arc::new(red(400, 300));
        program.frame.floating.as_mut().unwrap().xform = Xform {
            x: 100.0,
            y: 40.0,
            width: 180.0,
            height: 160.0,
            rotation: 0.0,
        };

        let Some(pixels) = render_offscreen(&program, (w, h), NAME, mouse::Cursor::Unavailable)
        else {
            eprintln!("no GPU available, skipping");
            return;
        };
        let at = |x: f32, y: f32| -> [u8; 4] {
            let i = ((y.round() as usize) * w as usize + x.round() as usize) * 4;
            pixels[i..i + 4].try_into().unwrap()
        };

        let canvas = program.canvas_rect(Rectangle {
            x: 0.0,
            y: 0.0,
            width: w as f32,
            height: h as f32,
        });
        let zoom = program.frame.view.zoom;
        let to_screen = |(ix, iy): (f32, f32)| (canvas.x + ix * zoom, canvas.y + iy * zoom);

        for point in &points {
            let (x, y) = to_screen(*point);
            assert_eq!(at(x, y), [255, 255, 255, 255], "no grip at {point:?}");
        }
        let xform = program.frame.floating.as_ref().unwrap().xform;
        let (cx, cy) = to_screen((xform.x, xform.y));
        assert_eq!(
            at(cx, cy),
            [255, 255, 255, 255],
            "no grip on the box's corner"
        );
    }

    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn render_offscreen(
        program: &Program,
        size: (u32, u32),
        name: &str,
        cursor: mouse::Cursor,
    ) -> Option<Vec<u8>> {
        use iced::widget::shader::{Pipeline, Primitive as _};

        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());

        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let mut pipeline = ViewportPipeline::new(&device, &queue, format);
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: size.0 as f32,
            height: size.1 as f32,
        };
        let viewport = iced::widget::shader::Viewport::with_physical_size(
            iced::Size::new(size.0, size.1),
            1.0,
        );
        let primitive = <Program as shader::Program<Interaction>>::draw(
            program,
            &State::default(),
            cursor,
            bounds,
        );
        primitive.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);

        let unpadded = size.0 as usize * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * size.1 as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            assert!(
                primitive.draw(&pipeline, &mut pass.forget_lifetime()),
                "nothing was drawn"
            );
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(size.1),
                },
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let mapped = slice.get_mapped_range();

        let mut out = Vec::with_capacity(unpadded * size.1 as usize);
        for row in 0..size.1 as usize {
            out.extend_from_slice(&mapped[row * padded..row * padded + unpadded]);
        }

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/viewport");
        if std::fs::create_dir_all(&dir).is_ok()
            && let Some(image) = image::RgbaImage::from_raw(size.0, size.1, out.clone())
        {
            let _ = image.save(dir.join(format!("{name}.png")));
        }
        Some(out)
    }

    fn project(view: View, image: Point) -> Point {
        let rect = view.canvas_rect(viewport(), CANVAS);
        Point::new(rect.x + image.x * view.zoom, rect.y + image.y * view.zoom)
    }

    #[test]
    fn a_fitted_canvas_is_centred_and_fully_visible() {
        let view = View::fitted(viewport(), CANVAS);
        let rect = view.canvas_rect(viewport(), CANVAS);

        assert!(rect.width <= viewport().width && rect.height <= viewport().height);
        assert!((rect.x + rect.width / 2.0 - viewport().width / 2.0).abs() < 0.01);
        assert!((rect.y + rect.height / 2.0 - viewport().height / 2.0).abs() < 0.01);
    }

    #[test]
    fn fitting_never_enlarges_past_actual_size() {
        assert_eq!(View::fit_zoom(viewport(), (10, 10)), 1.0);
    }

    #[test]
    fn zooming_keeps_the_anchor_under_the_cursor() {
        let view = View {
            pan: Vector::new(37.0, -12.0),
            zoom: 1.0,
        };
        let anchor = Point::new(620.0, 410.0);

        let rect = view.canvas_rect(viewport(), CANVAS);
        let image = Point::new(
            (anchor.x - rect.x) / view.zoom,
            (anchor.y - rect.y) / view.zoom,
        );

        for zoom in [0.25, 0.5, 2.0, 8.0, 31.0] {
            let after = view.zoomed_at(anchor, zoom, viewport(), CANVAS);
            let landed = project(after, image);
            assert!(
                (landed.x - anchor.x).abs() < 0.01 && (landed.y - anchor.y).abs() < 0.01,
                "at {zoom}x the anchor moved to {landed:?}"
            );
        }
    }

    #[test]
    fn zoom_stays_within_limits() {
        let view = View::default();
        let anchor = Point::new(0.0, 0.0);
        assert_eq!(
            view.zoomed_at(anchor, 1e6, viewport(), CANVAS).zoom,
            MAX_ZOOM
        );
        assert_eq!(
            view.zoomed_at(anchor, 1e-6, viewport(), CANVAS).zoom,
            MIN_ZOOM
        );
    }
}
