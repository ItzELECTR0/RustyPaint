use crate::canvas::NewCanvas;
use crate::config::Config;
use crate::doc::clipboard::Clip;
use crate::doc::{self, Document, Rect, Rgba8, Version};
use crate::gpu::{self, View};
use crate::paint::{Brush, Stroke, Tool, curve, shapes};
use crate::select::{self, Floating, Lasso};
use crate::text::{Align, TextStyle};
use crate::ui::menu::Page as MenuPage;
use crate::ui::picker::Picker;
use crate::ui::sidebar;
use crate::ui::theme::{self, Choice, Scheme, metrics};
use crate::ui::titlebar;

use iced::time::Instant;
use iced::{Size, Task};
use std::path::PathBuf;

mod document;
mod input;
mod live;
mod update;
mod view;

use document::*;
use input::*;
use live::*;
use view::*;

pub(crate) use live::Sticker;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Brushes,
    Shapes,
    Stickers,
    Text,
    Canvas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawing {
    Shape(shapes::ShapeKind),
    Curve(curve::CurveKind),
}

pub struct CanvasPanel {
    pub width: String,
    pub height: String,
    pub lock_aspect: bool,
    pub resize_image: bool,
    pub percent: bool,
    pub show_canvas: bool,
}

impl CanvasPanel {
    fn new(size: (u32, u32)) -> Self {
        Self {
            width: size.0.to_string(),
            height: size.1.to_string(),
            lock_aspect: true,
            resize_image: false,
            percent: false,
            show_canvas: true,
        }
    }

    fn sync(&mut self, size: (u32, u32)) {
        if self.percent {
            self.width = "100".into();
            self.height = "100".into();
        } else {
            self.width = size.0.to_string();
            self.height = size.1.to_string();
        }
    }

    fn preview(&mut self, size: (u32, u32), current: (u32, u32)) {
        if self.percent {
            let percentage = |value: u32, base: u32| {
                let shown = format!("{:.2}", value as f64 / base.max(1) as f64 * 100.0);
                shown
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            };
            self.width = percentage(size.0, current.0);
            self.height = percentage(size.1, current.1);
        } else {
            self.width = size.0.to_string();
            self.height = size.1.to_string();
        }
    }

    fn target(&self, size: (u32, u32)) -> Option<(u32, u32)> {
        let w: f32 = self.width.trim().parse().ok()?;
        let h: f32 = self.height.trim().parse().ok()?;
        let (w, h) = if self.percent {
            (size.0 as f32 * w / 100.0, size.1 as f32 * h / 100.0)
        } else {
            (w, h)
        };
        let clamp = |v: f32| (v.round().max(1.0) as u32).min(MAX_CANVAS);
        Some((clamp(w), clamp(h)))
    }
}

const STICKER_HISTORY: usize = 12;

pub const UI_SCALE: f32 = 1.15;

use crate::canvas::MAX_CANVAS;

// iced's thread-pool executor carries no interval helper, so the snapshot beat gets its own thread.
fn snapshot_ticks() -> impl iced::futures::Stream<Item = ()> {
    let (mut send, receive) = iced::futures::channel::mpsc::channel(1);
    std::thread::spawn(move || {
        while !send.is_closed() {
            std::thread::sleep(doc::recovery::SNAPSHOT_EVERY);
            let _ = send.try_send(());
        }
    });
    receive
}

// The active document's state lives on App itself, so every tool keeps reaching for it directly.
// Switching tabs swaps that state out with a parked sheet instead of threading an index everywhere.
pub(super) struct Sheet {
    doc: Document,
    view: View,
    panel: CanvasPanel,
    stroke: Option<Stroke>,
    last_point: Option<(f32, f32)>,
    floating: Option<Floating>,
    live_redo: Option<LiveRedo>,
    float_version: u64,
    grab_from: Option<Grabbed>,
    grab: Option<gpu::Grab>,
    selecting: Option<((f32, f32), (f32, f32))>,
    lasso: Option<Lasso>,
    resize_preview: Option<(u32, u32)>,
    dirty: Option<(Version, Rect)>,
    save_format: doc::io::SaveFormat,
    cropping: Option<Cropping>,
    cutting_out: Option<CuttingOut>,
    nudge: Option<Nudge>,
    recovery_id: String,
    recovery_lock: Option<doc::recovery::Guard>,
    snapshotted: Option<(Version, u64)>,
}

pub struct App {
    doc: Document,
    view: View,
    window: Size,
    viewport: Size,
    status: String,
    tab: Tab,
    brush: Brush,
    panel: CanvasPanel,
    stroke: Option<Stroke>,
    last_point: Option<(f32, f32)>,
    floating: Option<Floating>,
    live_redo: Option<LiveRedo>,
    drawing: Drawing,
    shape_style: shapes::ShapeStyle,
    colour_target: bool,
    picker: Option<Picker>,
    editing_custom_colour: Option<usize>,
    custom_colour_menu: Option<usize>,
    picking_field: Option<bool>,
    text_style: TextStyle,
    caret_on: bool,
    float_version: u64,
    grab_from: Option<Grabbed>,
    grab: Option<gpu::Grab>,
    selecting: Option<((f32, f32), (f32, f32))>,
    lasso: Option<Lasso>,
    freeform: bool,
    stashed_tool: Tool,
    resize_preview: Option<(u32, u32)>,
    dirty: Option<(Version, Rect)>,
    menu: Option<MenuPage>,
    save_format: doc::io::SaveFormat,
    config: Config,
    config_path: Option<PathBuf>,
    custom_canvas: (String, String),
    mods: iced::keyboard::Modifiers,
    cropping: Option<Cropping>,
    cutting_out: Option<CuttingOut>,
    stickers: Vec<Sticker>,
    last_copy: Option<u64>,
    after_save: Option<Pending>,
    asking: Option<Pending>,
    nudge: Option<Nudge>,
    recovery: Option<PathBuf>,
    recovery_id: String,
    snapshotted: Option<(Version, u64)>,
    recovery_lock: Option<doc::recovery::Guard>,
    recovered: Vec<doc::recovery::Recovered>,
    offering: bool,
    parked: Vec<Sheet>,
    active: usize,
}

// A held arrow key walking a selection along, slowly at first and then at a rate you can still read.
struct Nudge {
    arrow: Arrow,
    since: Instant,
    last: Instant,
    carry: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Close,
    Tab,
}

#[derive(Debug, Clone)]
pub enum Message {
    SnapshotTick,
    Snapshotted((Version, u64), Result<(), String>),
    ParkedSnapshotted(Result<(), String>),
    TabSelected(usize),
    TabClosed(usize),
    TabCloseRequested,
    TabStepped(i32),
    OpenInPicked(crate::config::OpenIn),
    OpenedElsewhere(Result<PathBuf, String>),
    RecoveryAnswered(bool),
    OpenRequested,
    Opened(Result<(PathBuf, Rgba8), String>),
    SaveRequested,
    SaveAsRequested,
    SaveAsConfirmed,
    SaveFormatPicked(doc::io::SaveFormat),
    Saved(Result<PathBuf, String>),
    Canvas(gpu::Interaction),
    WindowResized(Size),
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomActual,
    ZoomPicked(f32),
    SprayTick,
    Cut,
    Copy,
    Paste,
    Pasted(Option<Clip>),
    CropToSelection,
    SelectAll,
    Deselect,
    FileDropped(PathBuf),
    StickerRequested,
    Dropped(Result<(PathBuf, Rgba8), String>),
    TabPicked(Tab),
    ToolPicked(Tool),
    ShapePicked(shapes::ShapeKind),
    CurvePicked(curve::CurveKind),
    ShapeFillTypePicked(shapes::Paint),
    ShapeLineTypePicked(shapes::Paint),
    ShapeColourTargetPicked(bool),
    FloatOpacityChanged(f32),
    FloatTurned(bool),
    FloatMirrored(bool),
    FreeformToggled(bool),
    DeleteFloating,
    ThicknessNudged(f32),
    PickerOpened,
    PickerClosed,
    PickerConfirmed,
    PickerFieldPressed,
    PickerStripPressed,
    PickerFieldPicked(f32, f32),
    PickerHuePicked(f32),
    PickerReleased,
    PickerHexEdited(String),
    PickerRgbEdited(usize, String),
    CustomColourPicked(usize),
    CustomColourMenuOpened(usize),
    CustomColourEditRequested(usize),
    CustomColourRemoved(usize),
    StickerRecalled(usize),
    TextToolPicked,
    BonesRequested,
    CropOpened,
    CropCancelled,
    CropApplied,
    CropFramingPicked(Option<sidebar::Framing>),
    CropWidthEdited(String),
    CropHeightEdited(String),
    CropLockToggled(bool),
    CutoutOpened,
    CutoutCancelled,
    CutoutNext,
    CutoutBack,
    CutoutDone,
    CutoutBrushPicked(bool),
    CutoutAutofillToggled(bool),
    WindowDragged,
    WindowResizeDragged(iced::window::Direction),
    WindowMinimised,
    WindowMaximiseToggled,
    WindowClosed,
    ShapeThicknessChanged(f32),
    TextFontPicked(String),
    TextSizePicked(u32),
    TextBoldToggled,
    TextItalicToggled,
    TextUnderlineToggled,
    TextAlignPicked(Align),
    TextBackgroundToggled(bool),
    TextEdited(TextAction),
    ThicknessChanged(f32),
    OpacityChanged(f32),
    ToleranceChanged(f32),
    ColourPicked(usize),
    Undo,
    Redo,
    TransparencyToggled(bool),
    ShowCanvasToggled(bool),
    LockAspectToggled(bool),
    ResizeImageToggled(bool),
    CanvasWidthEdited(String),
    CanvasHeightEdited(String),
    CanvasUnitPicked(bool),
    CanvasResizeSubmitted,
    MenuOpened,
    MenuClosed,
    MenuPagePicked(MenuPage),
    LinkOpened(&'static str),
    NewRequested,
    ThemePicked(Choice),
    AccentPicked(Scheme),
    NewCanvasPicked(NewCanvas),
    NewCanvasWidthEdited(String),
    NewCanvasHeightEdited(String),
    WindowFocused,
    WindowUnfocused,
    NudgeStarted(Arrow),
    NudgeEnded(Arrow),
    NudgeTick(Instant),
    DiscardAnswered(Discard),
    ConfirmDiscardToggled(bool),
    AcrylicToggled(bool),
    DecorationsToggled(bool),
    ModifiersChanged(iced::keyboard::Modifiers),
    Rotate(bool),
    Flip(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discard {
    Save,
    Throw,
    Keep,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextAction {
    Insert(char),
    Enter,
    Backspace,
    Delete,
    Motion(Motion),
    Click(f32, f32),
    Drag(f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

impl Arrow {
    fn step(self) -> (f32, f32) {
        match self {
            Arrow::Left => (-1.0, 0.0),
            Arrow::Right => (1.0, 0.0),
            Arrow::Up => (0.0, -1.0),
            Arrow::Down => (0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectAll,
}

impl From<gpu::Interaction> for Message {
    fn from(i: gpu::Interaction) -> Self {
        Message::Canvas(i)
    }
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let (config, complaint) = crate::config::boot();
        Self::boot(
            config.clone(),
            crate::config::path(),
            crate::config::recovery_dir(),
            complaint.clone(),
        )
    }

    fn boot(
        config: Config,
        config_path: Option<PathBuf>,
        recovery: Option<PathBuf>,
        complaint: Option<String>,
    ) -> (Self, Task<Message>) {
        theme::set_theme(config.theme.resolve(), config.accent);
        let recovered = recovery
            .as_deref()
            .map(doc::recovery::abandoned)
            .unwrap_or_default();
        let recovery_id = doc::recovery::id();
        let recovery_lock = recovery
            .as_deref()
            .and_then(|dir| doc::recovery::hold(dir, &recovery_id));
        theme::set_acrylic(config.acrylic);
        let (start_w, start_h) = crate::canvas::size_for(
            config.new_canvas,
            Size::new(1.0, 1.0),
            Document::DEFAULT_SIZE,
        );
        let app = Self {
            doc: Document::blank_sized(start_w, start_h, false),
            view: View::default(),
            window: Size::new(1.0, 1.0),
            viewport: Size::new(1.0, 1.0),
            status: complaint.unwrap_or_default(),
            tab: Tab::Brushes,
            brush: Brush::default(),
            panel: CanvasPanel::new((start_w, start_h)),
            stroke: None,
            floating: None,
            live_redo: None,
            drawing: Drawing::Shape(shapes::ShapeKind::Rectangle),
            text_style: TextStyle::default(),
            caret_on: true,
            shape_style: shapes::ShapeStyle::default(),
            colour_target: false,
            picker: None,
            editing_custom_colour: None,
            custom_colour_menu: None,
            picking_field: None,
            float_version: 0,
            grab_from: None,
            grab: None,
            selecting: None,
            lasso: None,
            freeform: false,
            stashed_tool: Brush::default().tool,
            last_point: None,
            resize_preview: None,
            menu: None,
            save_format: doc::io::SaveFormat::default(),
            custom_canvas: custom_fields(config.new_canvas),
            mods: iced::keyboard::Modifiers::default(),
            cropping: None,
            cutting_out: None,
            stickers: Vec::new(),
            last_copy: None,
            after_save: None,
            asking: None,
            nudge: None,
            recovery: recovery.clone(),
            recovery_id,
            recovery_lock,
            offering: !recovered.is_empty(),
            recovered,
            snapshotted: None,
            parked: Vec::new(),
            active: 0,
            config,
            config_path,
            dirty: None,
        };

        let measure = iced::window::latest()
            .and_then(iced::window::size)
            .map(Message::WindowResized);
        let watch_drops = iced::window::latest()
            .and_then(|id| iced::window::run(id, crate::dnd::watch))
            .discard();
        let start = Task::batch([measure, watch_drops]);

        let task = match std::env::args_os().nth(1) {
            Some(arg) => {
                let path = PathBuf::from(arg);
                Task::batch([start, Task::perform(load(path), Message::Opened)])
            }
            None => start,
        };
        (app, task)
    }

    pub(super) fn new_sheet(&self, doc: Document, save_format: doc::io::SaveFormat) -> Sheet {
        let size = doc.size();
        let recovery_id = doc::recovery::id();
        let recovery_lock = self
            .recovery
            .as_deref()
            .and_then(|dir| doc::recovery::hold(dir, &recovery_id));
        Sheet {
            doc,
            view: View::fitted(self.viewport, size),
            panel: CanvasPanel::new(size),
            stroke: None,
            last_point: None,
            floating: None,
            live_redo: None,
            float_version: 0,
            grab_from: None,
            grab: None,
            selecting: None,
            lasso: None,
            resize_preview: None,
            dirty: None,
            save_format,
            cropping: None,
            cutting_out: None,
            nudge: None,
            recovery_id,
            recovery_lock,
            snapshotted: None,
        }
    }

    // Which parked sheet a tab points at, or None when the tab is the open one.
    pub(super) fn parked_at(&self, tab: usize) -> Option<usize> {
        match tab.cmp(&self.active) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Less => Some(tab),
            std::cmp::Ordering::Greater => Some(tab - 1),
        }
    }

    pub(super) fn any_unsaved(&self) -> bool {
        self.unsaved() || self.parked.iter().any(Sheet::unsaved)
    }

    pub(super) fn tab_name(&self, tab: usize) -> String {
        let named = |path: &Option<PathBuf>| {
            path.as_deref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("Untitled")
                .to_owned()
        };
        match self.parked_at(tab) {
            None => named(&self.doc.path),
            Some(i) => self
                .parked
                .get(i)
                .map(|sheet| named(&sheet.doc.path))
                .unwrap_or_default(),
        }
    }

    pub(super) fn tab_unsaved(&self, tab: usize) -> bool {
        match self.parked_at(tab) {
            None => self.unsaved(),
            Some(i) => self.parked.get(i).is_some_and(Sheet::unsaved),
        }
    }

    pub(super) fn sheet(&mut self) -> Sheet {
        Sheet {
            doc: std::mem::replace(&mut self.doc, Document::blank_sized(1, 1, false)),
            view: std::mem::take(&mut self.view),
            panel: std::mem::replace(&mut self.panel, CanvasPanel::new((1, 1))),
            stroke: self.stroke.take(),
            last_point: self.last_point.take(),
            floating: self.floating.take(),
            live_redo: self.live_redo.take(),
            float_version: self.float_version,
            grab_from: self.grab_from.take(),
            grab: self.grab.take(),
            selecting: self.selecting.take(),
            lasso: self.lasso.take(),
            resize_preview: self.resize_preview.take(),
            dirty: self.dirty.take(),
            save_format: self.save_format,
            cropping: self.cropping.take(),
            cutting_out: self.cutting_out.take(),
            nudge: self.nudge.take(),
            recovery_id: std::mem::take(&mut self.recovery_id),
            recovery_lock: self.recovery_lock.take(),
            snapshotted: self.snapshotted.take(),
        }
    }

    fn adopt(&mut self, sheet: Sheet) {
        self.doc = sheet.doc;
        self.view = sheet.view;
        self.panel = sheet.panel;
        self.stroke = sheet.stroke;
        self.last_point = sheet.last_point;
        self.floating = sheet.floating;
        self.live_redo = sheet.live_redo;
        self.float_version = sheet.float_version;
        self.grab_from = sheet.grab_from;
        self.grab = sheet.grab;
        self.selecting = sheet.selecting;
        self.lasso = sheet.lasso;
        self.resize_preview = sheet.resize_preview;
        self.dirty = sheet.dirty;
        self.save_format = sheet.save_format;
        self.cropping = sheet.cropping;
        self.cutting_out = sheet.cutting_out;
        self.nudge = sheet.nudge;
        self.recovery_id = sheet.recovery_id;
        self.recovery_lock = sheet.recovery_lock;
        self.snapshotted = sheet.snapshotted;
    }

    // Parks the open document back into the tab order and hands the whole list over.
    pub(super) fn collapse(&mut self) -> Vec<Sheet> {
        let mut sheets = std::mem::take(&mut self.parked);
        let active = self.active.min(sheets.len());
        let open = self.sheet();
        sheets.insert(active, open);
        sheets
    }

    pub(super) fn expand(&mut self, mut sheets: Vec<Sheet>, active: usize) {
        let active = active.min(sheets.len().saturating_sub(1));
        let open = sheets.remove(active);
        self.adopt(open);
        self.parked = sheets;
        self.active = active;
        self.menu = None;
        self.picker = None;
    }

    pub(super) fn sheets(&self) -> usize {
        self.parked.len() + 1
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            iced::window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            if self.asking.is_some() {
                iced::keyboard::listen().filter_map(answering)
            } else if self.typing() {
                iced::keyboard::listen().filter_map(typing)
            } else {
                iced::keyboard::listen().filter_map(shortcut)
            },
            if self.spraying() {
                iced::window::frames().map(|_| Message::SprayTick)
            } else if self.nudge.is_some() {
                iced::window::frames().map(Message::NudgeTick)
            } else {
                iced::Subscription::none()
            },
            if self.recovery.is_some() {
                iced::Subscription::run(snapshot_ticks).map(|()| Message::SnapshotTick)
            } else {
                iced::Subscription::none()
            },
            crate::dnd::drops().map(Message::FileDropped),
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                    Some(Message::FileDropped(path))
                }
                iced::Event::Window(iced::window::Event::Focused) => Some(Message::WindowFocused),
                iced::Event::Window(iced::window::Event::Unfocused) => {
                    Some(Message::WindowUnfocused)
                }
                iced::Event::Window(iced::window::Event::CloseRequested) => {
                    Some(Message::WindowClosed)
                }
                iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(mods)) => {
                    Some(Message::ModifiersChanged(mods))
                }
                _ => None,
            }),
        ])
    }

    pub fn theme(&self) -> iced::Theme {
        match theme::mode() {
            theme::Mode::Light => iced::Theme::Light,
            theme::Mode::Dark => iced::Theme::Dark,
        }
    }

    fn tabs_fit(&self) -> bool {
        let wanted = (sidebar::TABS.len() + 1) as f32 * metrics::TOP_PANEL_BUTTON_WIDTH
            + 3.0 * metrics::TOP_PANEL_THIN_BUTTON_WIDTH;
        self.window.width >= wanted
    }

    fn take_colour(&mut self, colour: [u8; 4]) {
        self.brush.colour = colour;
        if self.colour_target {
            self.shape_style.fill = Some(colour);
        } else {
            self.shape_style.outline = Some(colour);
        }
        self.text_style.colour = colour;
        self.restyle_shape();
        self.restyle_text();
    }

    fn lassoing(&self) -> bool {
        self.freeform && self.brush.tool == Tool::Select
    }

    fn live_drawing(&self) -> Option<sidebar::Live> {
        let floating = self.floating.as_ref()?;
        let (name, curve) = match self.drawing {
            Drawing::Shape(kind) => (kind.name(), false),
            Drawing::Curve(kind) => (kind.name(), true),
        };
        floating.is_drawing().then_some(sidebar::Live {
            name,
            opacity: floating.opacity(),
            curve: curve && !floating.is_closed(),
            bones: !floating.is_curve(),
            boned: floating.is_closed(),
        })
    }

    fn apply_theme(&mut self) {
        theme::set_theme(self.config.theme.resolve(), self.config.accent);
    }

    fn save_config(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        if let Err(e) = self.config.save(&path) {
            self.status = format!("Cannot save settings: {e}");
        }
    }

    fn sync_custom_canvas(&mut self) {
        let (Ok(w), Ok(h)) = (
            self.custom_canvas.0.trim().parse::<u32>(),
            self.custom_canvas.1.trim().parse::<u32>(),
        ) else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        self.config.new_canvas = NewCanvas::Custom(w.min(MAX_CANVAS), h.min(MAX_CANVAS));
        self.save_config();
    }

    fn new_canvas_size(&self) -> (u32, u32) {
        crate::canvas::size_for(
            self.config.new_canvas,
            self.viewport,
            Document::DEFAULT_SIZE,
        )
    }

    fn resync_viewport(&mut self) {
        let bar = if self.config.decorations {
            0.0
        } else {
            titlebar::HEIGHT
        };
        self.viewport = Size::new(
            (self.window.width - metrics::SIDE_PANEL_WIDTH).max(1.0),
            (self.window.height - CHROME_HEIGHT - bar).max(1.0),
        );
    }

    fn typing(&self) -> bool {
        self.floating
            .as_ref()
            .is_some_and(|f| f.editing && matches!(f.source, select::Source::Text(_)))
    }

    fn spraying(&self) -> bool {
        self.stroke.is_some() && self.brush.tool.sprays()
    }

    pub fn title(&self) -> String {
        format!("{} - RustyPaint", self.doc.title())
    }
}
