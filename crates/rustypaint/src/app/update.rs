use crate::canvas::NewCanvas;
use crate::doc::clipboard::Clip;
use crate::doc::transform::Anchor;
use crate::doc::{self, Document, Rect};
use crate::gpu::View;
use crate::paint::{Tool, shapes};
use crate::select::{self};
use crate::ui::menu::Page as MenuPage;
use crate::ui::picker::Picker;
use crate::ui::sidebar;
use crate::ui::theme::{self, Choice};

use iced::time::Instant;
use iced::{Point, Task};

use super::*;

impl App {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SnapshotTick => {
                self.touch_parked();
                return self.snapshot();
            }
            Message::Snapshotted(at, Ok(())) => self.snapshotted = Some(at),
            Message::Snapshotted(_, Err(e)) => self.status = e,
            Message::RecoveryAnswered(restore) => {
                self.offering = false;
                if self.recovered.is_empty() {
                    return Task::none();
                }
                if restore {
                    let found: Vec<_> = self.recovered.drain(..).rev().collect();
                    let mut tasks = Vec::new();
                    for one in found {
                        if let Some(dir) = &self.recovery {
                            doc::recovery::clear(dir, &one.id);
                        }
                        tasks.push(self.restore(one));
                    }
                    return Task::batch(tasks);
                } else {
                    for found in self.recovered.drain(..) {
                        if let Some(dir) = &self.recovery {
                            doc::recovery::clear(dir, &found.id);
                        }
                    }
                }
            }
            Message::ParkedSnapshotted(Err(e)) => self.status = e,
            Message::ParkedSnapshotted(Ok(())) => {}
            Message::TabSelected(tab) => return self.switch_to(tab),
            Message::TabCloseRequested => return self.update(Message::TabClosed(self.active)),
            Message::TabStepped(by) => {
                let count = self.sheets() as i32;
                let next = (self.active as i32 + by).rem_euclid(count);
                return self.switch_to(next as usize);
            }
            Message::TabClosed(tab) => {
                if self.sheets() == 1 {
                    return self.discarding(Pending::Close);
                }
                let switch = self.switch_to(tab);
                return Task::batch([switch, self.discarding(Pending::Tab)]);
            }
            Message::OpenInPicked(open_in) => {
                self.config.open_in = open_in;
                self.save_config();
            }
            Message::OpenedElsewhere(Ok(path)) => return self.elsewhere(Some(&path)),
            Message::OpenedElsewhere(Err(e)) => {
                if !e.is_empty() {
                    self.status = e;
                }
            }
            Message::OpenRequested => {
                if self.config.open_in == crate::config::OpenIn::Window && !self.untouched() {
                    return Task::perform(pick_path(), Message::OpenedElsewhere);
                }
                return Task::perform(pick_and_load(), Message::Opened);
            }

            Message::Opened(Ok((path, pixels))) => {
                let format = doc::io::SaveFormat::from_path(&path).unwrap_or_default();
                let doc = Document::from_image(pixels, Some(path));
                return self.open_document(doc, format);
            }
            Message::Opened(Err(e)) => self.status = e,

            Message::SaveRequested => return self.save(),
            Message::SaveAsRequested => self.menu = Some(MenuPage::SaveAs),
            Message::SaveAsConfirmed => return self.save_as(),
            Message::SaveFormatPicked(format) => self.save_format = format,
            Message::Saved(Ok(path)) => {
                if let Some(format) = doc::io::SaveFormat::from_path(&path) {
                    self.save_format = format;
                }
                self.doc.mark_saved();
                self.doc.path = Some(path);
                self.status.clear();
                self.forget_snapshot();
                if let Some(pending) = self.after_save.take() {
                    return self.carry_on(pending);
                }
            }
            Message::Saved(Err(e)) => {
                self.after_save = None;
                self.status = e;
            }

            Message::Canvas(interaction) => self.canvas(interaction),

            Message::WindowResized(window) => {
                let first = self.viewport.width <= 1.0;
                self.window = window;
                self.resync_viewport();
                if first {
                    if matches!(self.config.new_canvas, NewCanvas::Fit(_)) && self.untouched() {
                        let (w, h) = self.new_canvas_size();
                        if (w, h) != self.doc.size() {
                            self.doc = Document::blank_sized(w, h, false);
                            self.panel.sync(self.doc.size());
                            self.dirty = None;
                        }
                    }
                    self.view = View::fitted(self.viewport, self.doc.size());
                }
            }

            Message::ZoomIn => self.zoom_by(1.25),
            Message::ZoomOut => self.zoom_by(1.0 / 1.25),
            Message::ZoomFit => self.view = View::fitted(self.viewport, self.doc.size()),
            Message::ZoomActual => {
                self.view = View {
                    pan: iced::Vector::ZERO,
                    zoom: 1.0,
                }
            }
            Message::ZoomPicked(steps) => {
                let centre = Point::new(self.viewport.width / 2.0, self.viewport.height / 2.0);
                self.view =
                    self.view
                        .zoomed_at(centre, 2.0f32.powf(steps), self.viewport, self.doc.size());
            }

            Message::ModifiersChanged(mods) => self.mods = mods,

            Message::Cut | Message::Copy => {
                if self.typing() {
                    self.copy_selected_text(matches!(message, Message::Cut));
                    return Task::none();
                }
                if let Some(pixels) = self.selected_pixels() {
                    self.last_copy = Some(fingerprint(&pixels));
                    let _ = doc::clipboard::copy(&pixels);
                    if matches!(message, Message::Cut) {
                        self.erase_selection();
                    }
                }
            }
            Message::Paste => {
                return if self.typing() {
                    Task::perform(async { doc::clipboard::paste_into_text() }, Message::Pasted)
                } else {
                    Task::perform(async { doc::clipboard::paste() }, Message::Pasted)
                };
            }
            Message::Pasted(Some(Clip::Image(pixels))) => {
                let ours = self.last_copy == Some(fingerprint(&pixels));
                let at = if ours {
                    self.looking_at()
                } else {
                    self.middle()
                };
                self.float_at(pixels, at);
            }
            Message::Pasted(Some(Clip::Text(text))) => self.paste_text(&text),
            Message::Pasted(None) => self.status = "nothing on the clipboard".into(),

            Message::SelectAll => {
                self.commit_floating();
                let (w, h) = self.doc.size();
                self.begin_float_from(Rect::new(0, 0, w, h), None);
            }
            Message::Deselect => {
                if self.cutting_out.take().is_some() || self.cropping.take().is_some() {
                    self.float_version += 1;
                } else {
                    self.commit_floating();
                }
            }

            Message::CropToSelection => {
                let Some(rect) = self.selection_rect() else {
                    return Task::none();
                };
                self.commit_floating();
                self.doc.crop(rect);
                self.reshaped();
                self.view = View::fitted(self.viewport, self.doc.size());
            }

            Message::StickerRequested => {
                return Task::perform(pick_and_load(), Message::Dropped);
            }
            Message::FileDropped(path) => {
                return Task::perform(load(path), Message::Dropped);
            }
            Message::Dropped(Ok((_, pixels))) => {
                let at = self.middle();
                self.float_at(pixels, at);
            }
            Message::Dropped(Err(e)) => self.status = e,

            Message::SprayTick => {
                if let (Some(stroke), Some((x, y))) = (&mut self.stroke, self.last_point) {
                    stroke.puff(x, y);
                }
                self.flush_stroke();
            }

            Message::TabPicked(tab) => {
                if tab != Tab::Brushes && self.tab == Tab::Brushes {
                    self.stashed_tool = self.brush.tool;
                }
                self.brush.tool = match tab {
                    Tab::Brushes => self.stashed_tool,
                    Tab::Shapes => Tool::Shape,
                    Tab::Text => Tool::Text,
                    Tab::Stickers | Tab::Canvas => Tool::Select,
                };
                self.tab = tab;
            }
            Message::ToolPicked(tool) => self.brush.tool = tool,
            Message::ShapePicked(kind) => {
                self.brush.tool = Tool::Shape;
                self.drawing = Drawing::Shape(kind);
                match self.floating.as_ref().map(|f| &f.source) {
                    Some(select::Source::Shape { .. }) => {
                        if let Some(floating) = &mut self.floating {
                            floating.source = select::Source::Shape {
                                kind,
                                style: self.shape_style,
                            };
                            floating.redraw();
                            self.float_version += 1;
                        }
                    }
                    Some(select::Source::Curve { .. }) => self.commit_floating(),
                    _ => {}
                }
            }
            Message::CurvePicked(kind) => {
                self.brush.tool = Tool::Shape;
                self.drawing = Drawing::Curve(kind);
                let ends = self.floating.as_ref().and_then(|f| {
                    let p = f.points();
                    (p.len() >= 2).then(|| (p[0], p[p.len() - 1]))
                });
                match ends {
                    Some((from, to)) => {
                        if let Some(floating) = &mut self.floating {
                            floating.relay(kind, from, to);
                            self.float_version += 1;
                        }
                    }
                    None => self.commit_floating(),
                }
            }
            Message::ShapeFillTypePicked(paint) => {
                let solid = paint == shapes::Paint::Solid;
                self.shape_style.fill = solid.then_some(self.brush.colour);
                if solid {
                    self.colour_target = true;
                }
                self.restyle_shape();
            }
            Message::ShapeLineTypePicked(paint) => {
                let solid = paint == shapes::Paint::Solid;
                self.shape_style.outline = solid.then_some(self.brush.colour);
                if solid {
                    self.colour_target = false;
                }
                self.restyle_shape();
            }
            Message::ShapeColourTargetPicked(fill) => self.colour_target = fill,
            Message::WindowDragged => {
                return iced::window::latest().and_then(iced::window::drag);
            }
            Message::WindowResizeDragged(direction) => {
                return iced::window::latest()
                    .and_then(move |id| iced::window::drag_resize(id, direction));
            }
            Message::WindowMinimised => {
                return iced::window::latest().and_then(|id| iced::window::minimize(id, true));
            }
            Message::WindowMaximiseToggled => {
                return iced::window::latest().and_then(iced::window::toggle_maximize);
            }
            Message::WindowClosed => return self.discarding(Pending::Close),
            Message::PickerOpened => {
                self.picker = Some(Picker::on(self.brush.colour));
                self.editing_custom_colour = None;
                self.custom_colour_menu = None;
            }
            Message::PickerClosed => {
                self.picker = None;
                self.editing_custom_colour = None;
                self.picking_field = None;
            }
            Message::PickerConfirmed => {
                if let Some(picker) = self.picker.take() {
                    let colour = picker.colour();
                    if let Some(index) = self.editing_custom_colour.take() {
                        if let Some(custom) = self.config.custom_colours.get_mut(index) {
                            *custom = colour;
                            self.save_config();
                        }
                    } else if !self.config.custom_colours.contains(&colour) {
                        self.config.custom_colours.push(colour);
                        while self.config.custom_colours.len() > 6 {
                            self.config.custom_colours.remove(0);
                        }
                        self.save_config();
                    }
                    self.take_colour(colour);
                }
                self.picking_field = None;
            }
            Message::PickerFieldPressed => self.picking_field = Some(true),
            Message::PickerStripPressed => self.picking_field = Some(false),
            Message::PickerReleased => self.picking_field = None,
            Message::PickerFieldPicked(saturation, value) => {
                if self.picking_field == Some(true)
                    && let Some(picker) = &mut self.picker
                {
                    picker.saturation = saturation.clamp(0.0, 1.0);
                    picker.value = value.clamp(0.0, 1.0);
                    picker.clear_typed();
                }
            }
            Message::PickerHuePicked(hue) => {
                if self.picking_field == Some(false)
                    && let Some(picker) = &mut self.picker
                {
                    picker.hue = hue.clamp(0.0, 360.0);
                    picker.clear_typed();
                }
            }
            Message::PickerHexEdited(hex) => {
                if let Some(picker) = &mut self.picker {
                    picker.typed(hex);
                }
            }
            Message::PickerRgbEdited(channel, value) => {
                if let Some(picker) = &mut self.picker {
                    picker.typed_channel(channel, value);
                }
            }
            Message::CustomColourPicked(i) => {
                if let Some(colour) = self.config.custom_colours.get(i).copied() {
                    self.take_colour(colour);
                }
                self.custom_colour_menu = None;
            }
            Message::CustomColourMenuOpened(i) => {
                self.custom_colour_menu = (i < self.config.custom_colours.len()).then_some(i);
            }
            Message::CustomColourEditRequested(i) => {
                if let Some(colour) = self.config.custom_colours.get(i).copied() {
                    self.picker = Some(Picker::on(colour));
                    self.editing_custom_colour = Some(i);
                }
                self.custom_colour_menu = None;
            }
            Message::CustomColourRemoved(i) => {
                if i < self.config.custom_colours.len() {
                    self.config.custom_colours.remove(i);
                    self.save_config();
                }
                self.custom_colour_menu = None;
            }
            Message::TextToolPicked => return self.update(Message::TabPicked(Tab::Text)),
            Message::CropOpened => {
                if self.floating.is_some() {
                    return self.update(Message::CropToSelection);
                }
                self.cropping = Some(Cropping::new(self.doc.size()));
            }
            Message::CropCancelled => self.cropping = None,
            Message::CropApplied => {
                if let Some(cropping) = self.cropping.take()
                    && !cropping.rect.is_empty()
                {
                    self.doc.crop(cropping.rect);
                    self.reshaped();
                    self.view = View::fitted(self.viewport, self.doc.size());
                }
            }
            Message::CropFramingPicked(framing) => {
                let canvas = self.doc.size();
                if let Some(cropping) = &mut self.cropping {
                    cropping.framing = framing;
                    if let Some(framing) = framing {
                        cropping.reframe(framing.ratio(), canvas);
                    }
                }
            }
            Message::CropWidthEdited(typed) => self.crop_field(typed, true),
            Message::CropHeightEdited(typed) => self.crop_field(typed, false),
            Message::CropLockToggled(on) => {
                if let Some(cropping) = &mut self.cropping {
                    cropping.lock = on;
                }
            }
            Message::CutoutOpened => {
                self.commit_floating();
                self.cropping = None;
                self.cutting_out = Some(CuttingOut::new(self.doc.size()));
            }
            Message::CutoutCancelled => {
                self.cutting_out = None;
                self.float_version += 1;
            }
            Message::CutoutNext => self.run_cutout(Some(3)),
            Message::CutoutBack => {
                if let Some(cutting_out) = &mut self.cutting_out {
                    cutting_out.refining = false;
                    cutting_out.cutout = None;
                    cutting_out.mask = None;
                    cutting_out.overlay = None;
                    self.float_version += 1;
                }
            }
            Message::CutoutDone => self.cutout_done(),
            Message::CutoutBrushPicked(adding) => {
                if let Some(cutting_out) = &mut self.cutting_out {
                    cutting_out.adding = adding;
                }
            }
            Message::CutoutAutofillToggled(on) => {
                if let Some(cutting_out) = &mut self.cutting_out {
                    cutting_out.autofill = on;
                }
            }
            Message::BonesRequested => {
                if let Some(floating) = &mut self.floating
                    && floating.add_bones()
                {
                    self.float_version += 1;
                    self.dirty = None;
                }
            }
            Message::StickerRecalled(i) => {
                if let Some(sticker) = self.stickers.get(i) {
                    let pixels = sticker.pixels.clone();
                    let at = self.middle();
                    self.float_at(pixels, at);
                }
            }
            Message::DeleteFloating => {
                if self.floating.take().is_some() {
                    self.float_version += 1;
                    self.dirty = None;
                }
            }
            Message::ThicknessNudged(by) => {
                if self.tab == Tab::Shapes {
                    self.shape_style.thickness = (self.shape_style.thickness + by)
                        .clamp(shapes::MIN_THICKNESS, shapes::MAX_THICKNESS);
                    self.restyle_shape();
                } else {
                    self.brush.thickness = (self.brush.thickness + by).clamp(
                        crate::paint::brush::MIN_THICKNESS,
                        crate::paint::brush::MAX_THICKNESS,
                    );
                }
            }
            Message::FreeformToggled(on) => {
                self.freeform = on;
                self.brush.tool = Tool::Select;
            }
            Message::FloatOpacityChanged(v) => {
                if let Some(floating) = &mut self.floating {
                    floating.set_opacity(v);
                    self.float_version += 1;
                }
            }
            Message::FloatTurned(clockwise) => {
                if let Some(floating) = &mut self.floating {
                    floating.turn(clockwise);
                    self.float_version += 1;
                }
            }
            Message::FloatMirrored(horizontal) => {
                if let Some(floating) = &mut self.floating {
                    floating.mirror(horizontal);
                    self.float_version += 1;
                }
            }
            Message::ShapeThicknessChanged(v) => {
                self.shape_style.thickness = v;
                self.restyle_shape();
            }
            Message::TextFontPicked(name) => {
                self.text_style.family = name;
                self.restyle_text();
            }
            Message::TextSizePicked(size) => {
                self.text_style.size = size as f32;
                self.restyle_text();
            }
            Message::TextBoldToggled => {
                self.text_style.bold = !self.text_style.bold;
                self.restyle_text();
            }
            Message::TextItalicToggled => {
                self.text_style.italic = !self.text_style.italic;
                self.restyle_text();
            }
            Message::TextUnderlineToggled => {
                self.text_style.underline = !self.text_style.underline;
                self.restyle_text();
            }
            Message::TextAlignPicked(align) => {
                self.text_style.align = align;
                self.restyle_text();
            }
            Message::TextBackgroundToggled(on) => {
                self.text_style.background = on;
                self.restyle_text();
            }
            Message::TextEdited(action) => self.edit_text(action),
            Message::ThicknessChanged(v) => self.brush.thickness = v,
            Message::OpacityChanged(v) => self.brush.opacity = v,
            Message::ToleranceChanged(v) => self.brush.tolerance = v,
            Message::ColourPicked(i) => {
                if let Some(c) = theme::SWATCHES.get(i) {
                    self.take_colour(sidebar::to_bytes(*c));
                }
            }

            Message::Undo => self.step_history(true),
            Message::Redo => self.step_history(false),

            Message::TransparencyToggled(on) => {
                self.doc.set_transparent(on);
                self.reshaped();
            }
            Message::ShowCanvasToggled(on) => self.panel.show_canvas = on,
            Message::LockAspectToggled(on) => self.panel.lock_aspect = on,
            Message::ResizeImageToggled(on) => self.panel.resize_image = on,
            Message::CanvasUnitPicked(percent) => {
                self.panel.percent = percent;
                self.panel.sync(self.doc.size());
            }

            Message::CanvasWidthEdited(value) => {
                self.panel.width = value;
                self.match_aspect(true);
            }
            Message::CanvasHeightEdited(value) => {
                self.panel.height = value;
                self.match_aspect(false);
            }
            Message::MenuOpened => {
                self.commit_floating();
                self.menu = Some(MenuPage::About);
            }
            Message::MenuClosed => self.menu = None,
            Message::MenuPagePicked(page) => self.menu = Some(page),
            Message::LinkOpened(url) => {
                if let Err(e) = open_link(url) {
                    self.status = e;
                }
            }
            Message::NewRequested => {
                self.menu = None;
                if self.untouched() {
                    return Task::none();
                }
                if self.config.open_in == crate::config::OpenIn::Window {
                    return self.elsewhere(None);
                }
                let (w, h) = self.new_canvas_size();
                let blank = Document::blank_sized(w, h, false);
                let sheet = self.new_sheet(blank, doc::io::SaveFormat::default());
                return self.add_sheet(sheet);
            }
            Message::DiscardAnswered(answer) => {
                let Some(pending) = self.asking.take() else {
                    return Task::none();
                };
                match answer {
                    Discard::Throw => return self.carry_on(pending),
                    Discard::Save => {
                        self.after_save = Some(pending);
                        return self.save();
                    }
                    Discard::Keep => {}
                }
            }
            Message::WindowUnfocused => self.nudge = None,
            Message::NudgeStarted(arrow) => {
                if self.floating.is_none() {
                    return Task::none();
                }
                self.nudge_float(arrow);
                let now = Instant::now();
                self.nudge = Some(Nudge {
                    arrow,
                    since: now,
                    last: now,
                    carry: 0.0,
                });
            }
            Message::NudgeEnded(arrow) => {
                if self.nudge.as_ref().is_some_and(|held| held.arrow == arrow) {
                    self.nudge = None;
                }
            }
            Message::NudgeTick(now) => {
                let Some(held) = &mut self.nudge else {
                    return Task::none();
                };
                let waited = now.duration_since(held.since).as_secs_f32();
                let elapsed = now.duration_since(held.last).as_secs_f32().min(NUDGE_STALL);
                held.last = now;
                if waited < NUDGE_DELAY {
                    return Task::none();
                }
                let ramp = ((waited - NUDGE_DELAY) / NUDGE_RAMP).clamp(0.0, 1.0);
                held.carry += elapsed * (NUDGE_SLOW + (NUDGE_FAST - NUDGE_SLOW) * ramp);
                let steps = held.carry.floor();
                held.carry -= steps;
                let arrow = held.arrow;
                for _ in 0..steps as u32 {
                    self.nudge_float(arrow);
                }
            }
            Message::ConfirmDiscardToggled(on) => {
                self.config.confirm_discard = on;
                self.save_config();
            }
            Message::AcrylicToggled(on) => {
                self.config.acrylic = on;
                theme::set_acrylic(on);
                self.save_config();
            }
            Message::DecorationsToggled(on) => {
                if self.config.decorations == on {
                    return Task::none();
                }
                self.config.decorations = on;
                self.resync_viewport();
                self.save_config();
                return iced::window::latest().and_then(iced::window::toggle_decorations);
            }
            Message::ThemePicked(choice) => {
                self.config.theme = choice;
                self.apply_theme();
                self.save_config();
            }
            Message::AccentPicked(scheme) => {
                self.config.accent = scheme;
                self.apply_theme();
                self.save_config();
            }
            Message::NewCanvasPicked(preset) => {
                self.config.new_canvas = preset;
                self.custom_canvas = custom_fields(preset);
                self.save_config();
            }
            Message::NewCanvasWidthEdited(value) => {
                self.custom_canvas.0 = value;
                self.sync_custom_canvas();
            }
            Message::NewCanvasHeightEdited(value) => {
                self.custom_canvas.1 = value;
                self.sync_custom_canvas();
            }
            Message::WindowFocused => {
                if self.config.theme == Choice::Auto {
                    self.apply_theme();
                }
            }
            Message::CanvasResizeSubmitted => {
                if let Some((w, h)) = self.panel.target(self.doc.size()) {
                    if self.panel.resize_image {
                        self.doc.resize_image(w, h);
                    } else {
                        self.doc.resize_canvas(w, h, Anchor::Centre);
                    }
                    self.reshaped();
                }
            }

            Message::Rotate(clockwise) => {
                self.doc.rotate(clockwise);
                self.reshaped();
            }
            Message::Flip(horizontal) => {
                self.doc.flip(horizontal);
                self.reshaped();
            }
        }
        Task::none()
    }

    pub(super) fn reshaped(&mut self) {
        self.panel.sync(self.doc.size());
        self.dirty = None;
    }

    pub(super) fn match_aspect(&mut self, width_led: bool) {
        if !self.panel.lock_aspect {
            return;
        }
        let (w, h) = self.doc.size();
        if w == 0 || h == 0 {
            return;
        }
        if self.panel.percent {
            let source = if width_led {
                &self.panel.width
            } else {
                &self.panel.height
            };
            let mirrored = source.clone();
            if width_led {
                self.panel.height = mirrored
            } else {
                self.panel.width = mirrored
            }
            return;
        }
        let aspect = w as f32 / h as f32;
        if width_led {
            if let Ok(v) = self.panel.width.trim().parse::<f32>() {
                self.panel.height = ((v / aspect).round().max(1.0) as u32).to_string();
            }
        } else if let Ok(v) = self.panel.height.trim().parse::<f32>() {
            self.panel.width = ((v * aspect).round().max(1.0) as u32).to_string();
        }
    }

    pub(super) fn crop_field(&mut self, typed: String, width: bool) {
        let canvas = self.doc.size();
        let Some(cropping) = &mut self.cropping else {
            return;
        };
        if width {
            cropping.fields.0 = typed;
        } else {
            cropping.fields.1 = typed;
        }

        let Ok(value) = if width {
            &cropping.fields.0
        } else {
            &cropping.fields.1
        }
        .trim()
        .parse::<u32>() else {
            return;
        };
        let value = value.max(1);
        let rect = cropping.rect;
        cropping.rect = if width {
            Rect::new(rect.x0, rect.y0, (rect.x0 + value).min(canvas.0), rect.y1)
        } else {
            Rect::new(rect.x0, rect.y0, rect.x1, (rect.y0 + value).min(canvas.1))
        };
    }

    pub(super) fn zoom_by(&mut self, factor: f32) {
        let centre = Point::new(self.viewport.width / 2.0, self.viewport.height / 2.0);
        self.view = self.view.zoomed_at(
            centre,
            self.view.zoom * factor,
            self.viewport,
            self.doc.size(),
        );
    }
}
