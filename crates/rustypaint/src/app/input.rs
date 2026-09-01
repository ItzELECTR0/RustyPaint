use crate::doc::Rect;
use crate::gpu::{self, Handle};
use crate::paint::{Stroke, Tool};
use crate::select::Lasso;

use super::*;

pub(super) fn dragged_edges(
    from: Rect,
    handle: Handle,
    to: (f32, f32),
    canvas: (u32, u32),
) -> Rect {
    let (cw, ch) = (canvas.0 as i64, canvas.1 as i64);
    let (mut x0, mut y0, mut x1, mut y1) = (
        from.x0 as i64,
        from.y0 as i64,
        from.x1 as i64,
        from.y1 as i64,
    );
    let (px, py) = (to.0.round() as i64, to.1.round() as i64);

    match handle {
        Handle::Left | Handle::TopLeft | Handle::BottomLeft => x0 = px.clamp(0, x1 - 1),
        Handle::Right | Handle::TopRight | Handle::BottomRight => x1 = px.clamp(x0 + 1, cw),
        _ => {}
    }
    match handle {
        Handle::Top | Handle::TopLeft | Handle::TopRight => y0 = py.clamp(0, y1 - 1),
        Handle::Bottom | Handle::BottomLeft | Handle::BottomRight => y1 = py.clamp(y0 + 1, ch),
        _ => {}
    }
    Rect::new(x0 as u32, y0 as u32, x1 as u32, y1 as u32)
}

pub(super) fn drag_rect(a: (f32, f32), b: (f32, f32), canvas: (u32, u32)) -> Option<Rect> {
    let clamp = |v: f32, limit: u32| v.clamp(0.0, limit as f32);
    let x0 = clamp(a.0.min(b.0), canvas.0).floor() as u32;
    let y0 = clamp(a.1.min(b.1), canvas.1).floor() as u32;
    let x1 = clamp(a.0.max(b.0), canvas.0).ceil() as u32;
    let y1 = clamp(a.1.max(b.1), canvas.1).ceil() as u32;
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1, y1))
}

pub(super) const POINT_REACH: f32 = 12.0;

pub(super) const NUDGE_DELAY: f32 = 0.35;
pub(super) const NUDGE_RAMP: f32 = 1.2;
pub(super) const NUDGE_SLOW: f32 = 8.0;
pub(super) const NUDGE_FAST: f32 = 45.0;
pub(super) const NUDGE_STALL: f32 = 0.1;

pub(super) const OVERHANG: f32 = 512.0;

pub(super) fn arrow(key: iced::keyboard::Key<&str>) -> Option<Arrow> {
    use iced::keyboard::{Key, key::Named};

    match key {
        Key::Named(Named::ArrowLeft) => Some(Arrow::Left),
        Key::Named(Named::ArrowRight) => Some(Arrow::Right),
        Key::Named(Named::ArrowUp) => Some(Arrow::Up),
        Key::Named(Named::ArrowDown) => Some(Arrow::Down),
        _ => None,
    }
}

pub(super) fn shortcut(event: iced::keyboard::Event) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};

    if let Event::KeyReleased { key, .. } = &event {
        return arrow(key.as_ref()).map(Message::NudgeEnded);
    }
    let Event::KeyPressed {
        key,
        modifiers,
        repeat,
        ..
    } = event
    else {
        return None;
    };

    if matches!(key.as_ref(), Key::Named(Named::Escape)) {
        return Some(Message::Deselect);
    }
    if !modifiers.command() {
        // The ramp is ours to draw out, so the keyboard's own repeat is not wanted.
        if let Some(arrow) = arrow(key.as_ref()) {
            return (!repeat).then_some(Message::NudgeStarted(arrow));
        }
        return match key.as_ref() {
            Key::Named(Named::Delete) => Some(Message::DeleteFloating),
            Key::Character("[") => Some(Message::ThicknessNudged(-1.0)),
            Key::Character("]") => Some(Message::ThicknessNudged(1.0)),
            _ => None,
        };
    }

    match key.as_ref() {
        Key::Character("n") => Some(Message::NewRequested),
        Key::Character("w" | "q") => Some(Message::TabCloseRequested),
        Key::Character("c") if modifiers.shift() => Some(Message::CopyCanvas),
        Key::Named(Named::Tab) if modifiers.shift() => Some(Message::TabStepped(-1)),
        Key::Named(Named::Tab) => Some(Message::TabStepped(1)),
        Key::Character("o") => Some(Message::OpenRequested),
        Key::Character("s") if modifiers.shift() => Some(Message::SaveAsRequested),
        Key::Character("s") => Some(Message::SaveRequested),
        Key::Character("x") => Some(Message::Cut),
        Key::Character("c") => Some(Message::Copy),
        Key::Character("v") => Some(Message::Paste),
        Key::Character("a") => Some(Message::SelectAll),
        Key::Character("z") if modifiers.shift() => Some(Message::Redo),
        Key::Character("z") => Some(Message::Undo),
        Key::Character("y") => Some(Message::Redo),
        Key::Character("+" | "=") => Some(Message::ZoomIn),
        Key::Character("-" | "_") => Some(Message::ZoomOut),
        Key::Character("0") => Some(Message::ZoomFit),
        Key::Character("1") => Some(Message::ZoomActual),
        Key::Named(Named::ArrowUp) => Some(Message::ZoomIn),
        Key::Named(Named::ArrowDown) => Some(Message::ZoomOut),
        _ => None,
    }
}

pub(super) fn answering(event: iced::keyboard::Event) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};

    let Event::KeyPressed { key, .. } = event else {
        return None;
    };
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::DiscardAnswered(Discard::Keep)),
        Key::Named(Named::Enter) => Some(Message::DiscardAnswered(Discard::Save)),
        _ => None,
    }
}

pub(super) fn typing(event: iced::keyboard::Event) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};

    let Event::KeyPressed {
        key,
        modifiers,
        text,
        ..
    } = event
    else {
        return None;
    };
    let edit = |a: TextAction| Some(Message::TextEdited(a));

    if modifiers.command() {
        return match key.as_ref() {
            Key::Character("a") => edit(TextAction::Motion(Motion::SelectAll)),
            Key::Character("c") => Some(Message::Copy),
            Key::Character("x") => Some(Message::Cut),
            Key::Character("v") => Some(Message::Paste),
            Key::Character("z") if modifiers.shift() => Some(Message::Redo),
            Key::Character("z") => Some(Message::Undo),
            Key::Character("y") => Some(Message::Redo),
            Key::Character("s") if modifiers.shift() => Some(Message::SaveAsRequested),
            Key::Character("s") => Some(Message::SaveRequested),
            _ => None,
        };
    }

    let shift = modifiers.shift();
    match key.as_ref() {
        Key::Named(Named::Escape) => Some(Message::Deselect),
        Key::Named(Named::Enter) => edit(TextAction::Enter),
        Key::Named(Named::Backspace) => edit(TextAction::Backspace),
        Key::Named(Named::Delete) => edit(TextAction::Delete),
        Key::Named(Named::ArrowLeft) => edit(TextAction::Motion(if shift {
            Motion::SelectLeft
        } else {
            Motion::Left
        })),
        Key::Named(Named::ArrowRight) => edit(TextAction::Motion(if shift {
            Motion::SelectRight
        } else {
            Motion::Right
        })),
        Key::Named(Named::ArrowUp) => edit(TextAction::Motion(if shift {
            Motion::SelectUp
        } else {
            Motion::Up
        })),
        Key::Named(Named::ArrowDown) => edit(TextAction::Motion(if shift {
            Motion::SelectDown
        } else {
            Motion::Down
        })),
        Key::Named(Named::Home) => edit(TextAction::Motion(Motion::Home)),
        Key::Named(Named::End) => edit(TextAction::Motion(Motion::End)),
        Key::Named(Named::Space) => edit(TextAction::Insert(' ')),
        _ => {
            let typed = text?;
            let c = typed.chars().next()?;
            (!c.is_control()).then_some(Message::TextEdited(TextAction::Insert(c)))
        }
    }
}

pub(super) fn open_link(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "start", ""]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");

    let child = command
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot open {url}: {e}"))?;

    // The opener hands off to the browser and exits, so reap it off the UI thread.
    std::thread::spawn(move || drop(child.wait_with_output()));
    Ok(())
}

impl App {
    pub(super) fn canvas(&mut self, interaction: gpu::Interaction) {
        match interaction {
            gpu::Interaction::Viewed(view) => self.view = view,

            gpu::Interaction::PaintBegan(x, y) => {
                if self.refining() {
                    self.cutout_dab(x, y, true);
                    return;
                }
                self.last_point = Some((x, y));
                self.begin(x, y)
            }
            gpu::Interaction::PaintMoved(x, y) => {
                if self.refining() {
                    self.cutout_dab(x, y, false);
                    return;
                }
                self.last_point = Some((x, y));
                if let Some(stroke) = &mut self.stroke
                    && !self.brush.tool.sprays()
                {
                    stroke.extend(x, y);
                }
                self.flush_stroke();
            }
            gpu::Interaction::PaintEnded => {
                if self.refining() {
                    if let Some(cutting_out) = &mut self.cutting_out {
                        cutting_out.painting = false;
                    }
                    self.run_cutout(None);
                    return;
                }
                self.flush_stroke();
                if let Some(stroke) = self.stroke.take()
                    && let Some(touched) = stroke.touched()
                {
                    self.doc.commit(stroke.label(), touched, stroke.backup());
                }
                self.last_point = None;
                self.dirty = None;
            }

            gpu::Interaction::SelectBegan(x, y) => {
                let typing = self.typing();
                self.commit_floating();
                if typing {
                    return;
                }
                self.selecting = Some(((x, y), (x, y)));
                self.lasso = self.lassoing().then(|| Lasso::started_at(x, y));
            }
            gpu::Interaction::SelectMoved(x, y) => {
                if let Some((_, end)) = &mut self.selecting {
                    *end = (x, y);
                }
                if let Some(lasso) = &mut self.lasso {
                    lasso.push(x, y);
                }
                if let Some((a, b)) = self.selecting
                    && self.brush.tool == Tool::Shape
                {
                    self.draw_drawing(a, b);
                }
            }
            gpu::Interaction::SelectEnded => {
                let Some((a, b)) = self.selecting.take() else {
                    return;
                };
                if let Some(lasso) = self.lasso.take() {
                    let canvas = self.doc.size();
                    if let Some(rect) = lasso.bounds(canvas) {
                        let mask = lasso.mask(rect);
                        self.begin_float_from(rect, mask.as_deref());
                    }
                    return;
                }
                let tiny = (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0;
                if self.brush.tool == Tool::Text {
                    return self.end_text(a, b, tiny);
                }
                if tiny {
                    if self.brush.tool == Tool::Shape {
                        self.floating = None;
                    }
                    return;
                }
                match self.brush.tool {
                    Tool::Shape => self.draw_drawing(a, b),
                    _ => {
                        if let Some(rect) = drag_rect(a, b, self.doc.size()) {
                            self.begin_float_from(rect, None);
                        }
                    }
                }
            }

            gpu::Interaction::FloatGrabbed(grab, x, y) => {
                if let Some(floating) = &mut self.floating {
                    match grab {
                        gpu::Grab::Resize(handle) => floating.stretched = Some(handle),
                        gpu::Grab::Move | gpu::Grab::Rotate => floating.stretched = None,
                        gpu::Grab::Point(_) | gpu::Grab::Caret => {}
                    }
                    self.grab_from = Some(Grabbed {
                        at: (x, y),
                        xform: floating.xform,
                        points: floating.points().to_vec(),
                    });
                    self.grab = Some(grab);
                }
                if grab == gpu::Grab::Caret {
                    self.edit_text(TextAction::Click(x, y));
                }
            }
            gpu::Interaction::FloatDragged(x, y) => self.drag_float(x, y),
            gpu::Interaction::PointAdded(x, y) => {
                let (x, y) = self.within_reach((x, y));
                let reach = POINT_REACH / self.view.zoom.max(0.01);
                if let Some(floating) = &mut self.floating
                    && floating.add_point(x, y, reach)
                {
                    self.float_version += 1;
                    self.dirty = None;
                }
            }
            gpu::Interaction::PointRemoved(index) => {
                if let Some(floating) = &mut self.floating
                    && floating.remove_point(index)
                {
                    self.float_version += 1;
                    self.dirty = None;
                }
            }
            gpu::Interaction::FrameGrabbed(handle) => {
                if let Some(cropping) = &mut self.cropping {
                    cropping.grabbed = Some((cropping.rect, handle));
                }
                if let Some(cutting_out) = &mut self.cutting_out {
                    cutting_out.grabbed = Some((cutting_out.rect, handle));
                }
            }
            gpu::Interaction::FrameDragged(x, y) => {
                let canvas = self.doc.size();
                if let Some(cropping) = &mut self.cropping
                    && let Some((_, handle)) = cropping.grabbed
                {
                    cropping.dragged(handle, (x, y), canvas);
                }
                if let Some(cutting_out) = &mut self.cutting_out
                    && let Some((from, handle)) = cutting_out.grabbed
                {
                    cutting_out.rect = dragged_edges(from, handle, (x, y), canvas);
                }
            }
            gpu::Interaction::FrameReleased => {
                if let Some(cropping) = &mut self.cropping {
                    cropping.grabbed = None;
                }
                if let Some(cutting_out) = &mut self.cutting_out {
                    cutting_out.grabbed = None;
                }
            }
            gpu::Interaction::FloatReleased => {
                self.grab = None;
                self.grab_from = None;
            }
            gpu::Interaction::FloatReleasedAt(x, y) => {
                self.drag_float(x, y);
                self.grab = None;
                self.grab_from = None;
            }
            gpu::Interaction::CaretTick => {
                self.caret_on = !self.caret_on;
                let on = self.caret_on;
                if let Some(floating) = &mut self.floating
                    && floating.blink(on)
                {
                    self.float_version += 1;
                }
            }
            gpu::Interaction::ResizePreview(w, h) => {
                self.resize_preview = Some((w, h));
                self.panel.preview((w, h), self.doc.size());
            }
            gpu::Interaction::ResizeCancelled => {
                self.resize_preview = None;
                self.panel.sync(self.doc.size());
            }
            gpu::Interaction::Resized(w, h, handle) => {
                self.resize_preview = None;
                self.doc.resize_canvas(w, h, handle.anchor());
                self.reshaped();
            }
        }
    }

    pub(super) fn begin(&mut self, x: f32, y: f32) {
        self.commit_floating();
        match self.brush.tool {
            Tool::Fill => self.bucket(x, y),
            Tool::Pipette => self.eyedropper(x, y),
            _ => {
                self.stroke = Some(Stroke::begin(self.brush, &self.doc, x, y));
                self.flush_stroke();
            }
        }
    }

    pub(super) fn within_reach(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (w, h) = self.doc.size();
        (
            x.clamp(-OVERHANG, w as f32 + OVERHANG),
            y.clamp(-OVERHANG, h as f32 + OVERHANG),
        )
    }

    pub(super) fn drag_float(&mut self, x: f32, y: f32) {
        let (x, y) = self.within_reach((x, y));
        let shift = self.mods.shift();
        let (Some(grab), Some(floating)) = (self.grab, &mut self.floating) else {
            return;
        };
        let Some(grabbed) = &self.grab_from else {
            return;
        };
        let original = grabbed.xform;

        match grab {
            gpu::Grab::Move => {
                let moved = original.moved_by(x - grabbed.at.0, y - grabbed.at.1);
                floating.shift_to(moved.x, moved.y);
            }
            gpu::Grab::Resize(handle) => {
                let keep = floating.keeps_ratio() != shift;
                let target = original.resized(handle, x, y, keep);
                floating.refit(original, target, &grabbed.points);
                self.float_version += 1;
            }
            gpu::Grab::Rotate => {
                let target = original.rotated_towards(x, y);
                if floating.is_curve() {
                    floating.refit(original, target, &grabbed.points);
                    self.float_version += 1;
                } else {
                    floating.xform = target;
                }
            }
            gpu::Grab::Point(i) => {
                floating.bend(i, x, y);
                self.float_version += 1;
            }
            gpu::Grab::Caret => self.edit_text(TextAction::Drag(x, y)),
        }
    }

    pub(super) fn nudge_float(&mut self, arrow: Arrow) {
        let (dx, dy) = arrow.step();
        let shift = self.mods.shift();
        let Some(floating) = &self.floating else {
            self.nudge = None;
            return;
        };
        let was = floating.xform;
        let grip = match floating.stretched {
            Some(handle) => was.handle_at(handle),
            None => was.centre(),
        };
        let to = self.within_reach((grip.0 + dx, grip.1 + dy));
        let (dx, dy) = (to.0 - grip.0, to.1 - grip.1);

        let Some(floating) = &mut self.floating else {
            return;
        };
        match floating.stretched {
            None => {
                let moved = was.moved_by(dx, dy);
                floating.shift_to(moved.x, moved.y);
            }
            Some(handle) => {
                let keep = floating.keeps_ratio() != shift;
                let target = was.resized(handle, to.0, to.1, keep);
                let points = floating.points().to_vec();
                floating.refit(was, target, &points);
                self.float_version += 1;
            }
        }
    }
}
