use crate::canvas::NewCanvas;
use crate::config::Config;
use crate::doc::clipboard::Clip;
use crate::doc::{self, Document, Rect, Rgba8, Version};
use crate::gpu::{self, View};
use crate::i18n::Language;
use crate::paint::{Brush, Mirror, Stroke, Tool, curve, shapes};
use crate::select::{self, Floating, Lasso};
use crate::text::{Align, TextStyle};
use crate::ui::menu::Page as MenuPage;
use crate::ui::picker::Picker;
use crate::ui::sidebar;
use crate::ui::theme::{self, AccentColour, Choice, CustomAccent, Scheme, metrics};
use crate::ui::titlebar;

use iced::time::Instant;
use iced::{Size, Task};
use std::path::PathBuf;

mod cutout;
mod document;
mod input;
mod live;
mod update;
mod view;

use cutout::*;
use document::*;
use input::*;
use live::*;
use view::*;

pub(crate) use cutout::CutoutPreview;
pub(crate) use live::CuttingOut;
pub(crate) use live::Sticker;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Brushes,
    Symmetry,
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
    pub resampling: crate::doc::transform::Resampling,
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
            resampling: crate::doc::transform::Resampling::default(),
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
    slot: String,
    snapshotted: Option<(Version, u64)>,
}

struct Typed {
    field: Field,
    tool: Tool,
    text: String,
}

// Which number of a live object's box a field stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    X,
    Y,
    Width,
    Height,
}

// A number the side panel puts in a box as well as on a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Thickness,
    Hardness,
    Opacity,
    Stabilizer,
    Tolerance,
    ShapeThickness,
    BlurStrength,
    BlurAngle,
    BlurDetail,
    BlurPasses,
    BlurBlades,
    FloatOpacity,
    FloatX,
    FloatY,
    FloatWidth,
    FloatHeight,
    CutoutRadius,
    CutoutFeather,
    CutoutSmooth,
    CutoutShift,
    CutoutBrush,
    CutoutTolerance,
}

impl Field {
    fn bounds(self) -> (f32, f32) {
        match self {
            Field::CutoutRadius => (0.0, 32.0),
            Field::CutoutFeather | Field::CutoutSmooth => (0.0, 20.0),
            Field::CutoutShift => (-20.0, 20.0),
            Field::CutoutBrush => (1.0, 200.0),
            Field::CutoutTolerance => (0.0, 1.0),
            Field::Thickness => (
                crate::paint::brush::MIN_THICKNESS,
                crate::paint::brush::THICKNESS_CEILING,
            ),
            Field::ShapeThickness => (shapes::MIN_THICKNESS, shapes::MAX_THICKNESS),
            Field::BlurStrength => (
                crate::paint::blur::MIN_STRENGTH,
                crate::paint::blur::MAX_STRENGTH,
            ),
            Field::BlurAngle => (crate::paint::blur::MIN_ANGLE, crate::paint::blur::MAX_ANGLE),
            Field::BlurDetail => (
                crate::paint::blur::MIN_DETAIL,
                crate::paint::blur::MAX_DETAIL,
            ),
            Field::BlurPasses => (
                crate::paint::blur::MIN_PASSES,
                crate::paint::blur::MAX_PASSES,
            ),
            Field::BlurBlades => (
                crate::paint::blur::MIN_BLADES,
                crate::paint::blur::MAX_BLADES,
            ),
            Field::FloatX | Field::FloatY => (-(select::MAX_DRAW as f32), select::MAX_DRAW as f32),
            Field::FloatWidth | Field::FloatHeight => {
                (select::xform::MIN_SIDE, select::MAX_DRAW as f32)
            }
            Field::Hardness
            | Field::Opacity
            | Field::Stabilizer
            | Field::Tolerance
            | Field::FloatOpacity => (0.0, 1.0),
        }
    }

    fn in_percent(self) -> bool {
        matches!(
            self,
            Field::Hardness
                | Field::Opacity
                | Field::CutoutTolerance
                | Field::Stabilizer
                | Field::Tolerance
                | Field::BlurDetail
                | Field::FloatOpacity
        )
    }

    pub fn unit(self) -> &'static str {
        if self == Field::BlurAngle {
            "°"
        } else if self.in_percent() {
            "%"
        } else if matches!(self, Field::BlurPasses | Field::BlurBlades) {
            ""
        } else {
            "px"
        }
    }

    pub fn slider_step(self) -> f32 {
        if self.in_percent() { 0.01 } else { 1.0 }
    }

    pub fn editable(self, text: &str) -> String {
        let unit = self.unit();
        if unit.is_empty() {
            text.to_owned()
        } else {
            text.trim_end_matches(unit).to_owned()
        }
    }

    pub fn format(self, value: f32) -> String {
        if self == Field::BlurAngle {
            crate::i18n::degrees_value(value)
        } else if matches!(self, Field::BlurPasses | Field::BlurBlades) {
            format!("{value:.0}")
        } else if self.in_percent() {
            crate::i18n::percent_value(value)
        } else {
            crate::i18n::pixels_value(value)
        }
    }

    pub fn parse(self, text: &str) -> Option<f32> {
        let typed: f32 = text
            .trim()
            .trim_end_matches(self.unit())
            .trim()
            .parse()
            .ok()?;
        if !typed.is_finite() {
            return None;
        }
        let (low, high) = self.bounds();
        let value = if self.in_percent() {
            typed / 100.0
        } else {
            typed
        };
        Some(value.clamp(low, high))
    }

    fn message(self, value: f32) -> Message {
        match self {
            Field::CutoutRadius
            | Field::CutoutFeather
            | Field::CutoutSmooth
            | Field::CutoutShift
            | Field::CutoutBrush
            | Field::CutoutTolerance => Message::CutoutFieldChanged(self, value),
            Field::Thickness => Message::ThicknessChanged(value),
            Field::Hardness => Message::HardnessChanged(value),
            Field::Opacity => Message::OpacityChanged(value),
            Field::Stabilizer => Message::StabilizerChanged(value),
            Field::Tolerance => Message::ToleranceChanged(value),
            Field::ShapeThickness => Message::ShapeThicknessChanged(value),
            Field::BlurStrength => Message::BlurStrengthChanged(value),
            Field::BlurAngle => Message::BlurAngleChanged(value),
            Field::BlurDetail => Message::BlurDetailChanged(value),
            Field::BlurPasses => Message::BlurPassesChanged(value),
            Field::BlurBlades => Message::BlurBladesChanged(value),
            Field::FloatOpacity => Message::FloatOpacityChanged(value),
            Field::FloatX => Message::FloatSideChanged(Side::X, value),
            Field::FloatY => Message::FloatSideChanged(Side::Y, value),
            Field::FloatWidth => Message::FloatSideChanged(Side::Width, value),
            Field::FloatHeight => Message::FloatSideChanged(Side::Height, value),
        }
    }
}

impl Message {
    // A slider drag or a nudge takes the value back, so half-typed text in its box is gone.
    fn moves_a_field(&self) -> bool {
        matches!(
            self,
            Message::ThicknessChanged(_)
                | Message::CutoutFieldChanged(_, _)
                | Message::ThicknessNudged(_)
                | Message::HardnessChanged(_)
                | Message::OpacityChanged(_)
                | Message::StabilizerChanged(_)
                | Message::ToleranceChanged(_)
                | Message::ShapeThicknessChanged(_)
                | Message::BlurStrengthChanged(_)
                | Message::BlurAngleChanged(_)
                | Message::BlurDetailChanged(_)
                | Message::BlurPassesChanged(_)
                | Message::BlurBladesChanged(_)
                | Message::FloatOpacityChanged(_)
                | Message::FloatSideChanged(_, _)
        )
    }
}

// What the open picker does with the colour it gives back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picking {
    Current,
    Adding,
    Editing(usize),
    Accent(AccentColour),
    CutoutGround,
}

pub struct App {
    doc: Document,
    view: View,
    window: Size,
    viewport: Size,
    status: String,
    tab: Tab,
    brush: Brush,
    mirror: Mirror,
    pipette_return: Option<Tool>,
    typed: Option<Typed>,
    panel: CanvasPanel,
    stroke: Option<Stroke>,
    last_point: Option<(f32, f32)>,
    floating: Option<Floating>,
    live_redo: Option<LiveRedo>,
    drawing: Drawing,
    shape_style: shapes::ShapeStyle,
    blur_settings: crate::paint::blur::Settings,
    insert_tool: Tool,
    colour_target: bool,
    picker: Option<Picker>,
    picking_colour: Option<Picking>,
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
    accent: Scheme,
    custom_accent: CustomAccent,
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
    snapshotted: Option<(Version, u64)>,
    slot: String,
    next_slot: u64,
    session: String,
    #[allow(
        dead_code,
        reason = "holding this is what tells the next launch this session is alive"
    )]
    session_lock: Option<doc::recovery::Guard>,
    snapshotting: bool,
    recorded_unsaved: bool,
    last_snapshot: Instant,
    recovered: Vec<doc::recovery::Session>,
    offering: bool,
    parked: Vec<Sheet>,
    active: usize,
    tab_menu: Option<usize>,
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
    TabMenuOpened(usize),
    TabMenuClosed,
    TabMenuPicked(TabAction),
    CopyCanvas,
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
    BlurPicked,
    BlurAlgorithmPicked(crate::paint::blur::Algorithm),
    Dropped(Result<(PathBuf, Rgba8), String>),
    TabPicked(Tab),
    ToolPicked(Tool),
    ShapePicked(shapes::ShapeKind),
    CurvePicked(curve::CurveKind),
    ShapeFillTypePicked(shapes::Paint),
    ShapeLineTypePicked(shapes::Paint),
    ShapeColourTargetPicked(bool),
    FloatOpacityChanged(f32),
    FloatSideChanged(Side, f32),
    FloatTurned(bool),
    FloatMirrored(bool),
    FreeformToggled(bool),
    DeleteFloating,
    ThicknessNudged(f32),
    PickerOpened(Picking),
    PickerClosed,
    PickerConfirmed,
    PickerFieldPressed,
    PickerStripPressed,
    PickerFieldPicked(f32, f32),
    PickerHuePicked(f32),
    PickerReleased,
    PickerHexEdited(String),
    PickerChannelEdited(usize, String),
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
    CutoutObjectToggled(bool),
    CutoutTargetPicked(crate::select::cutout::workflow::Target),
    CutoutToneSampled(f32, f32),
    CutoutModelRequested,
    CutoutModelPicked(Option<PathBuf>),
    CutoutModelReset,
    CutoutFieldChanged(Field, f32),
    CutoutDecontaminateToggled(bool),
    CutoutPreviewPicked(CutoutPreview),
    CutoutFinished(
        u64,
        u64,
        Version,
        Result<std::sync::Arc<crate::select::cutout::workflow::ResultMask>, String>,
    ),
    WindowDragged,
    WindowResizeDragged(iced::window::Direction),
    WindowMinimised,
    WindowMaximiseToggled,
    WindowClosed,
    ShapeThicknessChanged(f32),
    BlurStrengthChanged(f32),
    BlurAngleChanged(f32),
    BlurDetailChanged(f32),
    BlurPassesChanged(f32),
    BlurBladesChanged(f32),
    TextFontPicked(String),
    TextSizePicked(u32),
    TextBoldToggled,
    TextItalicToggled,
    TextUnderlineToggled,
    TextAlignPicked(Align),
    TextBackgroundToggled(bool),
    TextEdited(TextAction),
    ThicknessChanged(f32),
    HardnessChanged(f32),
    AntialiasingToggled(bool),
    SquareTipPicked(bool),
    PixelPerfectToggled(bool),
    StabilizerChanged(f32),
    MirrorHorizontalToggled(bool),
    MirrorVerticalToggled(bool),
    OpacityChanged(f32),
    ToleranceChanged(f32),
    FieldTyped(Field, String),
    FieldSubmitted,
    ColourPicked(usize),
    Undo,
    Redo,
    TransparencyToggled(bool),
    ShowCanvasToggled(bool),
    LockAspectToggled(bool),
    ResizeImageToggled(bool),
    ResamplingPicked(crate::doc::transform::Resampling),
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
    LanguagePicked(Language),
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
    RotationDialToggled(bool),
    PixelGridToggled,
    AutoPixelGridToggled(bool),
    DecorationsToggled(bool),
    ModifiersChanged(iced::keyboard::Modifiers),
    Rotate(bool),
    Flip(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Save,
    Copy,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discard {
    Session,
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
        let mode = config.theme.resolve();
        let accent = config.accent;
        let seed_scheme = if accent == Scheme::Custom {
            Scheme::Rusty
        } else {
            accent
        };
        theme::set_theme(mode, accent);
        let custom_accent = match config.custom_accent {
            Some(custom) => {
                theme::set_custom_accent(custom);
                custom
            }
            None => {
                let palette = theme::palette_for(mode, seed_scheme);
                theme::seed_custom_accent(palette);
                CustomAccent::from_palette(palette)
            }
        };
        let recovered = recovery
            .as_deref()
            .map(doc::recovery::abandoned)
            .unwrap_or_default();
        let session = doc::recovery::id();
        let session_lock = recovery
            .as_deref()
            .and_then(|root| doc::recovery::hold(root, &session));
        theme::set_acrylic(config.acrylic);
        let (start_w, start_h) = crate::canvas::size_for(
            config.new_canvas,
            Size::new(1.0, 1.0),
            Document::DEFAULT_SIZE,
        );
        let mut config = config;
        if config.auto_pixel_grid {
            config.pixel_grid = View::default().zoom >= 8.0;
        }
        let app = Self {
            doc: Document::blank_sized(start_w, start_h, false),
            view: View::default(),
            window: Size::new(1.0, 1.0),
            viewport: Size::new(1.0, 1.0),
            status: complaint.unwrap_or_default(),
            tab: Tab::Brushes,
            brush: Brush::default(),
            mirror: Mirror::default(),
            pipette_return: None,
            typed: None,
            panel: CanvasPanel::new((start_w, start_h)),
            stroke: None,
            floating: None,
            live_redo: None,
            drawing: Drawing::Shape(shapes::ShapeKind::Rectangle),
            text_style: TextStyle::default(),
            caret_on: true,
            shape_style: shapes::ShapeStyle::default(),
            blur_settings: crate::paint::blur::Settings::default(),
            insert_tool: Tool::Select,
            colour_target: false,
            picker: None,
            picking_colour: None,
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
            session,
            session_lock,
            slot: "0".into(),
            next_slot: 1,
            snapshotting: false,
            recorded_unsaved: false,
            // Behind the gap already, so the first edit of a session is written straight away.
            last_snapshot: Instant::now()
                .checked_sub(doc::recovery::SNAPSHOT_GAP)
                .unwrap_or_else(Instant::now),
            offering: !recovered.is_empty(),
            recovered,
            snapshotted: None,
            parked: Vec::new(),
            active: 0,
            tab_menu: None,
            config,
            accent,
            custom_accent,
            config_path,
            dirty: None,
        };

        let measure = iced::window::latest()
            .and_then(iced::window::size)
            .map(Message::WindowResized);
        let watch_drops = iced::window::latest()
            .and_then(|id| iced::window::run(id, crate::dnd::watch))
            .discard();
        let watch_handovers = iced::window::latest()
            .and_then(|id| iced::window::run(id, crate::open_with::watch))
            .discard();
        let start = Task::batch([measure, watch_drops, watch_handovers]);

        let task = match crate::open_with::first() {
            Some(path) => Task::batch([start, Task::perform(load(path), Message::Opened)]),
            None => start,
        };
        (app, task)
    }

    pub(super) fn new_sheet(&mut self, doc: Document, save_format: doc::io::SaveFormat) -> Sheet {
        let size = doc.size();
        let slot = self.next_slot.to_string();
        self.next_slot += 1;
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
            slot,
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
            slot: std::mem::take(&mut self.slot),
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
        self.slot = sheet.slot;
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
            crate::open_with::later().map(Message::FileDropped),
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

    // Half-typed text belongs to the box and the tool it was typed in, so a stale one ages out on
    // its own rather than needing every place that changes tools to clear it.
    pub(super) fn typed_field(&self) -> Option<(Field, &str)> {
        self.typed
            .as_ref()
            .filter(|typed| typed.tool == self.brush.tool)
            .map(|typed| (typed.field, typed.text.as_str()))
    }

    fn lassoing(&self) -> bool {
        self.freeform && self.brush.tool == Tool::Select
    }

    fn live_placement(&self) -> Option<sidebar::Placement> {
        let xform = self.floating.as_ref()?.xform;
        Some(sidebar::Placement {
            x: xform.x,
            y: xform.y,
            width: xform.width,
            height: xform.height,
        })
    }

    fn live_drawing(&self) -> Option<sidebar::Live> {
        let floating = self.floating.as_ref()?;
        let (name, curve, points) = match &floating.source {
            select::Source::Shape { kind, .. } => (kind.name(), false, None),
            select::Source::Curve { points, closed, .. } => {
                let name = if *closed {
                    crate::i18n::live_shape()
                } else if points.len() == 2 {
                    crate::i18n::curve_line()
                } else {
                    crate::i18n::live_curve()
                };
                (name, !closed, Some(points.len()))
            }
            _ => return None,
        };
        Some(sidebar::Live {
            name,
            points,
            opacity: floating.opacity(),
            curve,
            bones: !floating.is_curve(),
            boned: floating.is_closed(),
        })
    }

    fn apply_theme(&mut self) {
        theme::set_theme(self.config.theme.resolve(), self.accent);
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
