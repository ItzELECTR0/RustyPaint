use crate::canvas::NewCanvas;
use crate::config::Config;
use crate::doc::clipboard::Clip;
use crate::doc::image::CHANNELS;
use crate::doc::transform::Anchor;
use crate::doc::{self, Document, Rect, Rgba8, Version};
use crate::gpu::{self, Handle, View};
use crate::paint::{Brush, Stroke, Tool, curve, fill, shapes};
use crate::select::cutout::Cutout;
use crate::select::{self, Floating, Lasso, Xform};
use crate::text::{Align, TextStyle};
use crate::ui::icons::{self, icon};
use crate::ui::menu::{self, Page as MenuPage};
use crate::ui::picker::{self, Picker};
use crate::ui::sidebar;
use crate::ui::strings;
use crate::ui::theme::{self, Choice, Scheme, metrics};
use crate::ui::titlebar;

use iced::widget::{Space, button, column, container, row, shader, text};
use iced::{Element, Length, Point, Size, Task};
use std::path::PathBuf;

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

struct Grabbed {
    at: (f32, f32),
    xform: Xform,
    points: Vec<(f32, f32)>,
}

struct LiveRedo {
    floating: Floating,
    canvas: Option<(Rgba8, bool)>,
    version: Version,
}

pub struct Cropping {
    pub rect: Rect,
    pub framing: Option<sidebar::Framing>,
    pub lock: bool,
    pub fields: (String, String),
    grabbed: Option<(Rect, Handle)>,
}

impl Cropping {
    fn new(canvas: (u32, u32)) -> Self {
        let rect = Rect::new(0, 0, canvas.0, canvas.1);
        Self {
            rect,
            framing: None,
            lock: false,
            fields: (rect.width().to_string(), rect.height().to_string()),
            grabbed: None,
        }
    }

    fn sync(&mut self) {
        self.fields = (
            self.rect.width().to_string(),
            self.rect.height().to_string(),
        );
    }

    fn reframe(&mut self, ratio: f32, canvas: (u32, u32)) {
        let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
        let (mut w, mut h) = (cw, cw / ratio);
        if h > ch {
            h = ch;
            w = ch * ratio;
        }
        let centre = (
            (self.rect.x0 + self.rect.x1) as f32 / 2.0,
            (self.rect.y0 + self.rect.y1) as f32 / 2.0,
        );
        let x0 = (centre.0 - w / 2.0).round().clamp(0.0, cw - w);
        let y0 = (centre.1 - h / 2.0).round().clamp(0.0, ch - h);
        self.rect = Rect::new(
            x0 as u32,
            y0 as u32,
            (x0 + w).round() as u32,
            (y0 + h).round() as u32,
        );
        self.sync();
    }

    fn dragged(&mut self, handle: Handle, to: (f32, f32), canvas: (u32, u32)) {
        let Some((from, _)) = self.grabbed else {
            return;
        };
        self.rect = dragged_edges(from, handle, to, canvas);

        let ratio = self.framing.map(|f| f.ratio()).or_else(|| {
            self.lock
                .then(|| from.width() as f32 / from.height().max(1) as f32)
        });
        if let Some(ratio) = ratio {
            self.keep_ratio(handle, ratio, canvas);
        }
        self.sync();
    }

    fn keep_ratio(&mut self, handle: Handle, ratio: f32, canvas: (u32, u32)) {
        let (w, h) = (self.rect.width() as f32, self.rect.height() as f32);
        let follow_height = match handle {
            Handle::Left | Handle::Right => true,
            Handle::Top | Handle::Bottom => false,
            _ => w / ratio > h,
        };
        let (mut w, mut h) = if follow_height {
            (w, w / ratio)
        } else {
            (h * ratio, h)
        };

        let fit = (canvas.0 as f32 / w).min(canvas.1 as f32 / h).min(1.0);
        w *= fit;
        h *= fit;

        let west = matches!(handle, Handle::Left | Handle::TopLeft | Handle::BottomLeft);
        let north = matches!(handle, Handle::Top | Handle::TopLeft | Handle::TopRight);
        let x1 = if west {
            self.rect.x1 as f32
        } else {
            self.rect.x0 as f32 + w
        };
        let x0 = if west {
            self.rect.x1 as f32 - w
        } else {
            self.rect.x0 as f32
        };
        let y1 = if north {
            self.rect.y1 as f32
        } else {
            self.rect.y0 as f32 + h
        };
        let y0 = if north {
            self.rect.y1 as f32 - h
        } else {
            self.rect.y0 as f32
        };

        let slide = |low: f32, high: f32, limit: f32| {
            let shift = (-low).max(0.0) - (high - limit).max(0.0);
            (low + shift, high + shift)
        };
        let (x0, x1) = slide(x0, x1, canvas.0 as f32);
        let (y0, y1) = slide(y0, y1, canvas.1 as f32);
        self.rect = Rect::new(
            x0.round().max(0.0) as u32,
            y0.round().max(0.0) as u32,
            x1.round().min(canvas.0 as f32) as u32,
            y1.round().min(canvas.1 as f32) as u32,
        );
    }
}

fn dragged_edges(from: Rect, handle: Handle, to: (f32, f32), canvas: (u32, u32)) -> Rect {
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

pub struct CuttingOut {
    pub refining: bool,
    pub rect: Rect,
    cutout: Option<Cutout>,
    mask: Option<Vec<u8>>,
    overlay: Option<std::sync::Arc<Vec<u8>>>,
    pub adding: bool,
    pub autofill: bool,
    grabbed: Option<(Rect, Handle)>,
    painting: bool,
}

impl CuttingOut {
    const BRUSH: f32 = 16.0;

    fn new(canvas: (u32, u32)) -> Self {
        Self {
            refining: false,
            rect: Rect::new(0, 0, canvas.0, canvas.1),
            cutout: None,
            mask: None,
            overlay: None,
            adding: true,
            autofill: true,
            grabbed: None,
            painting: false,
        }
    }

    fn build_overlay(&mut self, canvas: (u32, u32)) {
        let Some(mask) = &self.mask else { return };
        let mut out = vec![0u8; canvas.0 as usize * canvas.1 as usize * CHANNELS];
        for (i, pixel) in out.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
            if mask.get(i).copied().unwrap_or(0) <= 128 {
                *pixel = [0, 0, 0, 150];
            }
        }
        self.overlay = Some(std::sync::Arc::new(out));
    }

    fn dab(&mut self, at: (f32, f32), radius: f32, canvas: (u32, u32)) {
        let adding = self.adding;
        if let Some(mask) = &mut self.mask {
            let value = if adding { 255 } else { 0 };
            let x0 = (at.0 - radius).floor().max(0.0) as u32;
            let y0 = (at.1 - radius).floor().max(0.0) as u32;
            let x1 = ((at.0 + radius).ceil().max(0.0) as u32).min(canvas.0);
            let y1 = ((at.1 + radius).ceil().max(0.0) as u32).min(canvas.1);
            for y in y0..y1 {
                for x in x0..x1 {
                    let d = (x as f32 + 0.5 - at.0).powi(2) + (y as f32 + 0.5 - at.1).powi(2);
                    if d <= radius * radius {
                        mask[y as usize * canvas.0 as usize + x as usize] = value;
                    }
                }
            }
        }
        if let Some(cutout) = &mut self.cutout {
            cutout.paint(at, radius, adding);
        }
    }

    fn bounds(&self, canvas: (u32, u32)) -> Option<Rect> {
        let mask = self.mask.as_ref()?;
        let (mut x0, mut y0) = (canvas.0, canvas.1);
        let (mut x1, mut y1) = (0u32, 0u32);
        for y in 0..canvas.1 {
            for x in 0..canvas.0 {
                if mask[y as usize * canvas.0 as usize + x as usize] > 128 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1, y1))
    }

    fn mask_in(&self, rect: Rect, canvas: (u32, u32)) -> Option<Vec<u8>> {
        let mask = self.mask.as_ref()?;
        let mut out = Vec::with_capacity(rect.area());
        for y in rect.rows() {
            for x in rect.cols() {
                out.push(mask[y as usize * canvas.0 as usize + x as usize]);
            }
        }
        Some(out)
    }
}

pub struct Sticker {
    pixels: Rgba8,
    thumb: iced::widget::image::Handle,
}

impl Sticker {
    const THUMB: f32 = 36.0;

    fn new(pixels: Rgba8) -> Self {
        let (w, h) = pixels.size();
        let fit = (Self::THUMB / w.max(1) as f32)
            .min(Self::THUMB / h.max(1) as f32)
            .min(1.0);
        let small = doc::transform::scale(
            &pixels,
            ((w as f32 * fit).round() as u32).max(1),
            ((h as f32 * fit).round() as u32).max(1),
        );
        let (tw, th) = small.size();
        let thumb = iced::widget::image::Handle::from_rgba(tw, th, small.as_bytes().to_vec());
        Self { pixels, thumb }
    }

    pub fn thumb(&self) -> &iced::widget::image::Handle {
        &self.thumb
    }
}

const STICKER_HISTORY: usize = 12;

pub const UI_SCALE: f32 = 1.15;

use crate::canvas::MAX_CANVAS;

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
    menu: Option<Option<MenuPage>>,
    config: Config,
    config_path: Option<PathBuf>,
    custom_canvas: (String, String),
    mods: iced::keyboard::Modifiers,
    cropping: Option<Cropping>,
    cutting_out: Option<CuttingOut>,
    stickers: Vec<Sticker>,
    last_copy: Option<u64>,
    after_save: Option<Pending>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Blank,
    Open,
    Close,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenRequested,
    Opened(Result<(PathBuf, Rgba8), String>),
    SaveRequested,
    SaveAsRequested,
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
    CustomColourPicked(usize),
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
    CloseConfirmed(Discard),
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
    NewRequested,
    ThemePicked(Choice),
    AccentPicked(Scheme),
    NewCanvasPicked(NewCanvas),
    NewCanvasWidthEdited(String),
    NewCanvasHeightEdited(String),
    WindowFocused,
    NewConfirmed(Discard),
    OpenConfirmed(Discard),
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

const CHROME_HEIGHT: f32 = metrics::TOP_PANEL_BUTTON_HEIGHT
    + metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT
    + metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT;

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let (config, complaint) = crate::config::boot();
        Self::boot(config.clone(), crate::config::path(), complaint.clone())
    }

    fn boot(
        config: Config,
        config_path: Option<PathBuf>,
        complaint: Option<String>,
    ) -> (Self, Task<Message>) {
        theme::set_theme(config.theme.resolve(), config.accent);
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
            custom_canvas: custom_fields(config.new_canvas),
            mods: iced::keyboard::Modifiers::default(),
            cropping: None,
            cutting_out: None,
            stickers: Vec::new(),
            last_copy: None,
            after_save: None,
            config,
            config_path,
            dirty: None,
        };

        let measure = iced::window::latest()
            .and_then(iced::window::size)
            .map(Message::WindowResized);

        let task = match std::env::args_os().nth(1) {
            Some(arg) => {
                let path = PathBuf::from(arg);
                Task::batch([measure, Task::perform(load(path), Message::Opened)])
            }
            None => measure,
        };
        (app, task)
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            iced::window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            if self.typing() {
                iced::keyboard::listen().filter_map(typing)
            } else {
                iced::keyboard::listen().filter_map(shortcut)
            },
            if self.spraying() {
                iced::window::frames().map(|_| Message::SprayTick)
            } else {
                iced::Subscription::none()
            },
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                    Some(Message::FileDropped(path))
                }
                iced::Event::Window(iced::window::Event::Focused) => Some(Message::WindowFocused),
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

    fn zoom_controls(&self) -> Element<'_, Message> {
        let steps = self.view.zoom.log2();
        let range = gpu::MIN_ZOOM.log2()..=gpu::MAX_ZOOM.log2();

        row![
            hint(
                strip(icons::FIT, Message::ZoomFit),
                strings::with_key(strings::FIT_TO_WINDOW, "Ctrl+0"),
            ),
            hint(
                strip(icons::ZOOM_OUT, Message::ZoomOut),
                strings::with_key(strings::ZOOM_OUT, "Ctrl+-"),
            ),
            iced::widget::slider(range, steps, Message::ZoomPicked)
                .step(0.01_f32)
                .style(crate::ui::controls::slider_style)
                .width(Length::Fixed(140.0)),
            hint(
                strip(icons::ZOOM_IN, Message::ZoomIn),
                strings::with_key(strings::ZOOM_IN, "Ctrl++"),
            ),
            hint(
                button(text(format!("{:.0}%", self.view.zoom * 100.0)).size(12))
                    .style(|_t, _s| tool_style(false))
                    .on_press(Message::ZoomActual),
                strings::with_key(strings::ACTUAL_SIZE, "Ctrl+1"),
            ),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center)
        .into()
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

    fn document_name(&self) -> &str {
        self.doc
            .path
            .as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
    }

    fn save(&self) -> Task<Message> {
        let pixels = self.for_saving();
        match self.doc.path.clone() {
            Some(path) => Task::perform(save_to(pixels, path), Message::Saved),
            None => Task::perform(pick_and_save(pixels), Message::Saved),
        }
    }

    fn for_saving(&self) -> Rgba8 {
        let Some(floating) = &self.floating else {
            return self.doc.flattened();
        };
        let mut scratch = Document::from_image(self.doc.pixels().clone(), None);
        scratch.transparent = self.doc.transparent;
        floating.commit(&mut scratch);
        scratch.flattened()
    }

    fn unsaved(&self) -> bool {
        self.doc.modified || self.floating.is_some()
    }

    fn untouched(&self) -> bool {
        !self.doc.modified && self.doc.path.is_none() && !self.doc.can_undo()
    }

    fn blank(&mut self) {
        let (w, h) = self.new_canvas_size();
        self.doc = Document::blank_sized(w, h, false);
        self.floating = None;
        self.live_redo = None;
        self.view = View::fitted(self.viewport, self.doc.size());
        self.dirty = None;
        self.panel.sync(self.doc.size());
        self.status.clear();
        self.menu = None;
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenRequested => {
                if !self.unsaved() {
                    return Task::perform(pick_and_load(), Message::Opened);
                }
                let name = self.document_name().to_string();
                return Task::perform(ask_to_save(name), Message::OpenConfirmed);
            }
            Message::OpenConfirmed(answer) => match answer {
                Discard::Throw => return Task::perform(pick_and_load(), Message::Opened),
                Discard::Save => {
                    self.after_save = Some(Pending::Open);
                    return self.save();
                }
                Discard::Keep => {}
            },

            Message::Opened(Ok((path, pixels))) => {
                self.doc = Document::from_image(pixels, Some(path));
                self.floating = None;
                self.live_redo = None;
                self.grab = None;
                self.grab_from = None;
                self.float_version += 1;
                self.view = View::fitted(self.viewport, self.doc.size());
                self.dirty = None;
                self.panel.sync(self.doc.size());
                self.status.clear();
            }
            Message::Opened(Err(e)) => self.status = e,

            Message::SaveRequested => return self.save(),
            Message::SaveAsRequested => {
                return Task::perform(pick_and_save(self.for_saving()), Message::Saved);
            }
            Message::Saved(Ok(path)) => {
                self.doc.modified = false;
                self.doc.path = Some(path);
                self.status.clear();
                match self.after_save.take() {
                    Some(Pending::Blank) => self.blank(),
                    Some(Pending::Open) => {
                        return Task::perform(pick_and_load(), Message::Opened);
                    }
                    Some(Pending::Close) => {
                        return iced::window::latest().and_then(iced::window::close);
                    }
                    None => {}
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
            Message::WindowClosed => {
                if !self.unsaved() {
                    return iced::window::latest().and_then(iced::window::close);
                }
                let name = self.document_name().to_string();
                return Task::perform(ask_to_save(name), Message::CloseConfirmed);
            }
            Message::CloseConfirmed(answer) => match answer {
                Discard::Throw => {
                    return iced::window::latest().and_then(iced::window::close);
                }
                Discard::Save => {
                    self.after_save = Some(Pending::Close);
                    return self.save();
                }
                Discard::Keep => {}
            },
            Message::PickerOpened => self.picker = Some(Picker::on(self.brush.colour)),
            Message::PickerClosed => {
                self.picker = None;
                self.picking_field = None;
            }
            Message::PickerConfirmed => {
                if let Some(picker) = self.picker.take() {
                    let colour = picker.colour();
                    if !self.config.custom_colours.contains(&colour) {
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
                    picker.typed = None;
                }
            }
            Message::PickerHuePicked(hue) => {
                if self.picking_field == Some(false)
                    && let Some(picker) = &mut self.picker
                {
                    picker.hue = hue.clamp(0.0, 360.0);
                    picker.typed = None;
                }
            }
            Message::PickerHexEdited(hex) => {
                if let Some(picker) = &mut self.picker {
                    picker.typed(hex);
                }
            }
            Message::CustomColourPicked(i) => {
                if let Some(colour) = self.config.custom_colours.get(i).copied() {
                    self.take_colour(colour);
                }
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
                self.menu = Some(None);
            }
            Message::MenuClosed => self.menu = None,
            Message::MenuPagePicked(page) => self.menu = Some(Some(page)),
            Message::NewRequested => {
                if !self.unsaved() {
                    self.blank();
                    return Task::none();
                }
                let name = self.document_name().to_string();
                return Task::perform(ask_to_save(name), Message::NewConfirmed);
            }
            Message::NewConfirmed(answer) => match answer {
                Discard::Throw => self.blank(),
                Discard::Save => {
                    self.after_save = Some(Pending::Blank);
                    return self.save();
                }
                Discard::Keep => {}
            },
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
                        self.doc.resize_canvas(w, h, Anchor::TopLeft);
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

    fn reshaped(&mut self) {
        self.panel.sync(self.doc.size());
        self.dirty = None;
    }

    fn match_aspect(&mut self, width_led: bool) {
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

    fn canvas(&mut self, interaction: gpu::Interaction) {
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
                if let Some(floating) = &self.floating {
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
            gpu::Interaction::ResizePreview(w, h) => self.resize_preview = Some((w, h)),
            gpu::Interaction::ResizeCancelled => self.resize_preview = None,
            gpu::Interaction::Resized(w, h, handle) => {
                self.resize_preview = None;
                self.doc.resize_canvas(w, h, handle.anchor());
                self.reshaped();
            }
        }
    }

    fn begin(&mut self, x: f32, y: f32) {
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

    fn draw_drawing(&mut self, from: (f32, f32), to: (f32, f32)) {
        match self.drawing {
            Drawing::Shape(kind) => {
                let Some(rect) = drag_rect(from, to, self.doc.size()) else {
                    return;
                };
                self.draw_shape(kind, rect);
            }
            Drawing::Curve(kind) => {
                let (from, to) = (self.within_reach(from), self.within_reach(to));
                self.draw_curve(kind, from, to)
            }
        }
    }

    fn draw_shape(&mut self, kind: shapes::ShapeKind, rect: Rect) {
        match &mut self.floating {
            Some(floating) if matches!(floating.source, select::Source::Shape { .. }) => {
                floating.xform = Xform::from_rect(rect);
                floating.redraw();
            }
            _ => {
                self.floating = Some(Floating::shape(&self.doc, kind, self.shape_style, rect));
            }
        }
        self.float_version += 1;
        self.dirty = None;
    }

    fn draw_curve(&mut self, kind: curve::CurveKind, from: (f32, f32), to: (f32, f32)) {
        match &mut self.floating {
            Some(floating) if matches!(floating.source, select::Source::Curve { .. }) => {
                floating.stretch(from, to);
            }
            _ => {
                let style = self.curve_style();
                self.floating = Some(Floating::curve(&self.doc, kind, style, from, to));
            }
        }
        self.float_version += 1;
        self.dirty = None;
    }

    fn restyle_text(&mut self) {
        let style = self.text_style.clone();
        if let Some(floating) = &mut self.floating
            && floating.text_box().is_some()
        {
            floating.restyle_text(style);
            self.float_version += 1;
        }
    }

    fn edit_text(&mut self, action: TextAction) {
        let style = self.text_style.clone();
        let Some(floating) = &mut self.floating else {
            return;
        };
        if !floating.editing {
            return;
        }
        let local = |x: f32, y: f32, xform: Xform| {
            let (u, v) = xform.to_local(x, y);
            (
                (u * xform.width).round() as i32,
                (v * xform.height).round() as i32,
            )
        };
        let xform = floating.xform;
        let Some(boxed) = floating.text_box() else {
            return;
        };

        use crate::text::Action as A;
        use cosmic_text::Motion as M;

        if let TextAction::Insert(c) = action {
            boxed.insert(c, &style);
            floating.redraw();
            self.float_version += 1;
            self.caret_on = true;
            return;
        }

        let action = match action {
            TextAction::Insert(c) => A::Insert(c),
            TextAction::Enter => A::Enter,
            TextAction::Backspace => A::Backspace,
            TextAction::Delete => A::Delete,
            TextAction::Click(x, y) => {
                let (x, y) = local(x, y, xform);
                A::Click { x, y }
            }
            TextAction::Drag(x, y) => {
                let (x, y) = local(x, y, xform);
                A::Drag { x, y }
            }
            TextAction::Motion(motion) => {
                let (select, motion) = match motion {
                    Motion::Left => (false, M::Left),
                    Motion::Right => (false, M::Right),
                    Motion::Up => (false, M::Up),
                    Motion::Down => (false, M::Down),
                    Motion::Home => (false, M::Home),
                    Motion::End => (false, M::End),
                    Motion::SelectLeft => (true, M::Left),
                    Motion::SelectRight => (true, M::Right),
                    Motion::SelectUp => (true, M::Up),
                    Motion::SelectDown => (true, M::Down),
                    Motion::SelectAll => {
                        boxed.select_all();
                        self.float_version += 1;
                        return;
                    }
                };
                boxed.set_selecting(select);
                A::Motion(motion)
            }
        };
        boxed.act(action);
        floating.redraw();
        self.float_version += 1;
        self.caret_on = true;
    }

    fn paste_text(&mut self, text: &str) {
        let style = self.text_style.clone();
        if self.typing() {
            if let Some(floating) = &mut self.floating
                && let Some(boxed) = floating.text_box()
            {
                boxed.insert_str(text, &style);
                floating.redraw();
                self.float_version += 1;
                self.caret_on = true;
            }
            return;
        }

        self.commit_floating();
        if self.tab == Tab::Brushes {
            self.stashed_tool = self.brush.tool;
        }
        self.tab = Tab::Text;
        self.brush.tool = Tool::Text;
        let at = self.looking_at();
        self.floating = Some(Floating::pasted_text(&self.doc, style, text, at));
        self.float_version += 1;
        self.dirty = None;
        self.status.clear();
    }

    fn copy_selected_text(&mut self, cut: bool) {
        let Some(floating) = &mut self.floating else {
            return;
        };
        let Some(boxed) = floating.text_box() else {
            return;
        };
        let Some(text) = boxed.selected_text() else {
            return;
        };
        let _ = doc::clipboard::copy_text(&text);
        if !cut {
            return;
        }
        boxed.delete_selection();
        floating.redraw();
        self.float_version += 1;
    }

    fn end_text(&mut self, a: (f32, f32), b: (f32, f32), tiny: bool) {
        let canvas = self.doc.size();
        let rect = if tiny {
            let (x, y) = self.within_reach(a);
            let (w, h) = (self.text_style.size * 6.0, self.text_style.line_height());
            drag_rect((x, y), (x + w, y + h), canvas)
        } else {
            drag_rect(a, b, canvas)
        };
        if let Some(rect) = rect {
            self.draw_text(rect);
        }
    }

    fn within_reach(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (w, h) = self.doc.size();
        (
            x.clamp(-OVERHANG, w as f32 + OVERHANG),
            y.clamp(-OVERHANG, h as f32 + OVERHANG),
        )
    }

    fn draw_text(&mut self, rect: Rect) {
        let busy = self
            .floating
            .as_ref()
            .is_some_and(|f| f.editing && matches!(f.source, select::Source::Text(_)));
        if busy {
            return;
        }
        self.commit_floating();
        self.floating = Some(Floating::text(&self.doc, self.text_style.clone(), rect));
        self.float_version += 1;
        self.dirty = None;
    }

    fn curve_style(&self) -> shapes::ShapeStyle {
        shapes::ShapeStyle {
            fill: None,
            outline: Some(self.brush.colour),
            thickness: self.shape_style.thickness,
        }
    }

    fn begin_float_from(&mut self, rect: Rect, mask: Option<&[u8]>) {
        self.commit_floating();
        if let Some(floating) = Floating::lift_masked(&mut self.doc, rect, mask) {
            self.float_version += 1;
            self.floating = Some(floating);
            self.dirty = None;
        }
    }

    fn middle(&self) -> (f32, f32) {
        let (w, h) = self.doc.size();
        (w as f32 / 2.0, h as f32 / 2.0)
    }

    fn looking_at(&self) -> (f32, f32) {
        let centre = Point::new(self.viewport.width / 2.0, self.viewport.height / 2.0);
        let (x, y) = self.view.to_image(centre, self.viewport, self.doc.size());
        let (w, h) = self.doc.size();
        (x.clamp(0.0, w as f32), y.clamp(0.0, h as f32))
    }

    fn remember_sticker(&mut self, pixels: &Rgba8) {
        let print = fingerprint(pixels);
        self.stickers.retain(|s| fingerprint(&s.pixels) != print);
        self.stickers.push(Sticker::new(pixels.clone()));
        while self.stickers.len() > STICKER_HISTORY {
            self.stickers.remove(0);
        }
    }

    fn float_at(&mut self, pixels: Rgba8, at: (f32, f32)) {
        self.commit_floating();
        self.remember_sticker(&pixels);
        self.floating = Some(Floating::place(&self.doc, pixels, at));
        self.float_version += 1;
        self.status.clear();
    }

    fn commit_floating(&mut self) {
        let Some(mut floating) = self.floating.take() else {
            return;
        };
        self.grab = None;
        self.grab_from = None;
        if floating.text_box().is_some_and(|boxed| boxed.is_empty()) {
            self.dirty = None;
            return;
        }
        floating.editing = false;
        if let Some(touched) = floating.commit(&mut self.doc) {
            self.doc
                .commit(floating.label(), touched, floating.backup());
        }
        self.dirty = None;
    }

    fn restyle_shape(&mut self) {
        let curve = self.curve_style();
        let Some(floating) = &mut self.floating else {
            return;
        };
        let style = match floating.source {
            select::Source::Curve { closed: false, .. } => curve,
            _ => self.shape_style,
        };
        floating.restyle(style);
        self.float_version += 1;
    }

    fn drag_float(&mut self, x: f32, y: f32) {
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

    fn crop_field(&mut self, typed: String, width: bool) {
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

    fn refining(&self) -> bool {
        self.cutting_out.as_ref().is_some_and(|m| m.refining)
    }

    fn cutout_dab(&mut self, x: f32, y: f32, first: bool) {
        let canvas = self.doc.size();
        let radius = CuttingOut::BRUSH / self.view.zoom.max(0.01);
        let Some(cutting_out) = &mut self.cutting_out else {
            return;
        };
        if first {
            cutting_out.painting = true;
        }
        cutting_out.dab((x, y), radius, canvas);
        cutting_out.build_overlay(canvas);
        self.float_version += 1;
    }

    fn run_cutout(&mut self, passes: Option<usize>) {
        let pixels = self.doc.pixels().clone();
        let canvas = self.doc.size();
        let Some(cutting_out) = &mut self.cutting_out else {
            return;
        };

        let cutout = cutting_out
            .cutout
            .get_or_insert_with(|| Cutout::new(&pixels, cutting_out.rect));
        match passes {
            Some(passes) => cutout.run(passes),
            None => cutout.recut(),
        }
        cutting_out.mask = Some(cutout.refined_mask(&pixels));
        cutting_out.refining = true;
        cutting_out.build_overlay(canvas);
        self.float_version += 1;
        self.dirty = None;
    }

    fn cutout_done(&mut self) {
        let canvas = self.doc.size();
        let Some(cutting_out) = self.cutting_out.take() else {
            return;
        };
        let Some(rect) = cutting_out.bounds(canvas) else {
            return;
        };
        let Some(mask) = cutting_out.mask_in(rect, canvas) else {
            return;
        };

        self.begin_float_from(rect, Some(&mask));
        if cutting_out.autofill {
            let filled = crate::select::cutout::fill_behind(self.doc.pixels(), &mask, rect);
            *self.doc.edit() = filled;
        }

        self.brush.tool = Tool::Select;
        self.float_version += 1;
        self.dirty = None;
    }

    fn selection_rect(&self) -> Option<Rect> {
        self.floating.as_ref()?.xform.bounds(self.doc.size())
    }

    fn selected_pixels(&self) -> Option<Rgba8> {
        Some(self.floating.as_ref()?.pixels.clone())
    }

    fn erase_selection(&mut self) {
        let Some(floating) = self.floating.take() else {
            return;
        };
        self.grab = None;
        self.grab_from = None;
        if let Some(hole) = floating.lifted_from {
            self.doc.commit("Cut", hole, floating.backup());
        }
        self.dirty = None;
    }

    fn bucket(&mut self, x: f32, y: f32) {
        let before = self.doc.pixels().clone();
        let version = self.doc.version();
        let (colour, tolerance) = (self.brush.colour, self.brush.tolerance);

        let Some(touched) = fill::flood(
            self.doc.edit(),
            x.floor() as i64,
            y.floor() as i64,
            colour,
            tolerance,
        ) else {
            return;
        };
        self.doc.commit("Fill", touched, &before);
        self.dirty = Some((version, touched));
    }

    fn eyedropper(&mut self, x: f32, y: f32) {
        if let Some(colour) = fill::pick(self.doc.pixels(), x.floor() as i64, y.floor() as i64)
            && colour[3] > 0
        {
            self.brush.colour = colour;
        }
    }

    fn flush_stroke(&mut self) {
        let before = self.doc.version();
        let Some(stroke) = &mut self.stroke else {
            return;
        };
        let Some(rect) = stroke.flush(&mut self.doc) else {
            return;
        };

        self.dirty = Some(match self.dirty.take() {
            Some((from, existing)) => (from, existing.union(rect)),
            None => (before, rect),
        });
    }

    fn can_undo(&self) -> bool {
        self.floating.is_some() || self.doc.can_undo()
    }

    fn can_redo(&self) -> bool {
        match &self.floating {
            Some(floating) => floating.can_redo_text(),
            None => {
                self.live_redo
                    .as_ref()
                    .is_some_and(|redo| redo.version == self.doc.version())
                    || self.doc.can_redo()
            }
        }
    }

    fn cancel_floating(&mut self) {
        let Some(floating) = self.floating.take() else {
            return;
        };
        let active = (self.doc.pixels().clone(), self.doc.modified);
        let canvas = floating.cancel(&mut self.doc).then_some(active);
        self.live_redo = Some(LiveRedo {
            floating,
            canvas,
            version: self.doc.version(),
        });
        self.grab = None;
        self.grab_from = None;
        self.float_version += 1;
        self.dirty = None;
    }

    fn redo_floating(&mut self) -> bool {
        let Some(redo) = self.live_redo.take() else {
            return false;
        };
        if redo.version != self.doc.version() {
            return false;
        }
        if let Some((pixels, modified)) = redo.canvas {
            self.doc.restore_live(pixels, modified);
        }
        self.floating = Some(redo.floating);
        self.float_version += 1;
        self.dirty = None;
        true
    }

    fn step_history(&mut self, undo: bool) {
        if let Some(floating) = &mut self.floating {
            let style = if undo {
                floating.undo_text()
            } else {
                floating.redo_text()
            };
            if let Some(style) = style {
                self.text_style = style;
                self.caret_on = true;
                self.float_version += 1;
                return;
            }
            if undo {
                self.cancel_floating();
            }
            return;
        }
        if !undo && self.redo_floating() {
            return;
        }
        let before = self.doc.version();
        let changed = if undo {
            self.doc.undo()
        } else {
            self.doc.redo()
        };
        self.dirty = match changed {
            Some(Some(rect)) => Some((before, rect)),
            Some(None) => None,
            None => return,
        };
        self.panel.sync(self.doc.size());
    }

    fn zoom_by(&mut self, factor: f32) {
        let centre = Point::new(self.viewport.width / 2.0, self.viewport.height / 2.0);
        self.view = self.view.zoomed_at(
            centre,
            self.view.zoom * factor,
            self.viewport,
            self.doc.size(),
        );
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.config.decorations {
            return self.workspace();
        }
        iced::widget::stack![
            column![titlebar::view(self.title()), self.workspace()],
            titlebar::edges(),
        ]
        .into()
    }

    fn workspace(&self) -> Element<'_, Message> {
        let under = self.pages();
        match &self.picker {
            Some(picker) => iced::widget::stack![under, picker::view(picker)].into(),
            None => under,
        }
    }

    fn pages(&self) -> Element<'_, Message> {
        if let Some(page) = self.menu {
            return menu::view(
                page,
                self.document_name(),
                self.doc.modified,
                &self.config,
                self.viewport,
                (&self.custom_canvas.0, &self.custom_canvas.1),
            );
        }
        column![
            self.tab_strip(),
            self.tool_strip(),
            row![
                self.canvas_view(),
                match (&self.cropping, &self.cutting_out) {
                    (_, Some(cutting_out)) => container(sidebar::cutout_panel(
                        cutting_out.refining,
                        cutting_out.adding,
                        cutting_out.autofill,
                    ))
                    .width(Length::Fixed(metrics::SIDE_PANEL_WIDTH))
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        top: 16.0,
                        right: 24.0,
                        bottom: 16.0,
                        left: 24.0
                    })
                    .style(|_theme| container::Style {
                        background: Some(theme::veiled(theme::colours().side_panel).into()),
                        ..Default::default()
                    })
                    .into(),
                    (Some(cropping), None) => container(sidebar::crop_panel(
                        cropping.framing,
                        cropping.lock,
                        (&cropping.fields.0, &cropping.fields.1),
                    ))
                    .width(Length::Fixed(metrics::SIDE_PANEL_WIDTH))
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        top: 16.0,
                        right: 24.0,
                        bottom: 16.0,
                        left: 24.0,
                    })
                    .style(|_theme| container::Style {
                        background: Some(theme::veiled(theme::colours().side_panel).into()),
                        ..Default::default()
                    })
                    .into(),
                    (None, None) => sidebar::panel(
                        self.tab,
                        &self.brush,
                        &self.panel,
                        self.doc.size(),
                        self.doc.transparent,
                        self.drawing,
                        self.shape_style,
                        &self.text_style,
                        metrics::SIDE_PANEL_WIDTH,
                        self.colour_target,
                        self.live_drawing(),
                        &self.config.custom_colours,
                        &self.stickers,
                    ),
                },
            ]
            .height(Length::Fill),
            self.bottom_bar(),
        ]
        .into()
    }

    fn tab_strip(&self) -> Element<'_, Message> {
        let wide = self.tabs_fit();
        let mut tabs = row![];
        for (label, glyph, tab) in sidebar::TABS {
            let active = tab == Some(self.tab);
            let button = tab_button(label, glyph, wide, active)
                .style(move |_theme, _status| tab_style(active));
            tabs = tabs.push(hint(
                sidebar::pressable(button, tab.map(Message::TabPicked)),
                label,
            ));
        }

        let menu_button = tab_button(strings::MENU, icons::MENU, wide, false)
            .style(|_theme, _status| tab_style(false))
            .on_press(Message::MenuOpened);

        let bar = row![
            menu_button,
            Space::new().width(Length::Fixed(8.0)),
            tabs,
            Space::new().width(Length::Fill),
            hint(
                sidebar::pressable(
                    bar_button(crate::ui::centred(icon(
                        icons::UNDO,
                        16.0,
                        colour_on_tab(false)
                    )))
                    .style(|_t, _s| tab_style(false)),
                    self.can_undo().then_some(Message::Undo),
                ),
                strings::with_key(strings::UNDO, "Ctrl+Z"),
            ),
            hint(
                sidebar::pressable(
                    bar_button(crate::ui::centred(icon(
                        icons::REDO,
                        16.0,
                        colour_on_tab(false)
                    )))
                    .style(|_t, _s| tab_style(false)),
                    self.can_redo().then_some(Message::Redo),
                ),
                strings::with_key(strings::REDO, "Ctrl+Y"),
            ),
        ]
        .padding(iced::Padding {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 6.0,
        })
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

        surface(bar, theme::veiled(theme::colours().top_bar))
            .height(Length::Fixed(metrics::TOP_PANEL_BUTTON_HEIGHT))
            .into()
    }

    fn tool_strip(&self) -> Element<'_, Message> {
        let selecting = self.brush.tool == Tool::Select;
        let (boxed, looped) = (selecting && !self.freeform, selecting && self.freeform);
        let writing = self.brush.tool == Tool::Text;
        let cropping = self.cropping.is_some();
        let cutting_out = self.cutting_out.is_some();
        let bar = row![
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::SELECT,
                    16.0,
                    colour_on_strip(boxed)
                )))
                .style(move |_t, _s| tool_style(boxed))
                .on_press(Message::FreeformToggled(false)),
                strings::SELECT_BOX,
            ),
            hint(
                bar_button(crate::ui::centred(icons::art(
                    crate::assets::LASSO_SVG,
                    16.0,
                    Some(colour_on_strip(looped)),
                )))
                .style(move |_t, _s| tool_style(looped))
                .on_press(Message::FreeformToggled(true)),
                strings::SELECT_FREEFORM,
            ),
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::TEXT,
                    16.0,
                    colour_on_strip(writing)
                )))
                .style(move |_t, _s| tool_style(writing))
                .on_press(Message::TextToolPicked),
                strings::TEXT,
            ),
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::CROP,
                    16.0,
                    colour_on_strip(cropping),
                )))
                .style(move |_t, _s| tool_style(cropping))
                .on_press(Message::CropOpened),
                strings::CROP,
            ),
            hint(
                bar_button(crate::ui::centred(icon(
                    icons::SMART_CUTOUT,
                    16.0,
                    colour_on_strip(cutting_out),
                )))
                .style(move |_t, _s| tool_style(cutting_out))
                .on_press(Message::CutoutOpened),
                strings::SMART_CUTOUT,
            ),
            Space::new().width(Length::Fill),
            self.zoom_controls(),
        ]
        .spacing(4)
        .padding(iced::Padding {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 6.0,
        })
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

        surface(bar, theme::veiled(theme::colours().tool_bar))
            .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT))
            .into()
    }

    fn frame(&self) -> gpu::CanvasFrame {
        gpu::CanvasFrame {
            pixels: self.doc.pixels().bytes_arc(),
            size: self.doc.size(),
            version: self.doc.version(),
            dirty: self.dirty,
            view: self.view,
            show_canvas: self.panel.show_canvas,
            handles: self.tab == Tab::Canvas && self.panel.show_canvas,
            preview: self.resize_preview,
            backing: self.doc.has_backing(),
            floating: self.cutout_overlay().or_else(|| {
                self.floating.as_ref().map(|f| gpu::FloatingFrame {
                    pixels: f.pixels.bytes_arc(),
                    size: f.pixels.size(),
                    version: self.float_version,
                    xform: f.xform,
                    points: f.points().to_vec(),
                    editing_text: f.editing,
                    text_empty: f.text_is_empty(),
                    opacity: f.opacity(),
                    masked: f.masked(),
                    grips: true,
                })
            }),
            ants: 0.0,
            frame: self.cropping.as_ref().map(|c| c.rect).or_else(|| {
                self.cutting_out
                    .as_ref()
                    .filter(|m| !m.refining)
                    .map(|m| m.rect)
            }),
            marquee: self.marquee(),
        }
    }

    fn marquee(&self) -> Option<Rect> {
        if self.lasso.is_some() || !matches!(self.brush.tool, Tool::Select | Tool::Text) {
            return None;
        }
        let (a, b) = self.selecting?;
        drag_rect(a, b, self.doc.size())
    }

    fn cutout_overlay(&self) -> Option<gpu::FloatingFrame> {
        let cutting_out = self.cutting_out.as_ref()?;
        let overlay = cutting_out.overlay.clone()?;
        Some(gpu::FloatingFrame {
            pixels: overlay,
            size: self.doc.size(),
            version: self.float_version,
            xform: Xform {
                x: 0.0,
                y: 0.0,
                width: self.doc.size().0 as f32,
                height: self.doc.size().1 as f32,
                rotation: 0.0,
            },
            points: Vec::new(),
            editing_text: false,
            text_empty: false,
            opacity: 1.0,
            masked: true,
            grips: false,
        })
    }

    fn canvas_view(&self) -> Element<'_, Message> {
        let viewport = shader::Shader::new(gpu::Program {
            frame: self.frame(),
            cursor: iced::mouse::Interaction::Crosshair,
            selecting: !self.refining()
                && matches!(self.brush.tool, Tool::Select | Tool::Text | Tool::Shape),
            brush: if self.refining() {
                Some(CuttingOut::BRUSH * 2.0 / self.view.zoom.max(0.01))
            } else {
                (self.tab == Tab::Brushes && self.brush.tool.profile().is_some())
                    .then_some(self.brush.thickness)
            },
        })
        .width(Length::Fill)
        .height(Length::Fill);

        iced::widget::stack![
            viewport,
            iced::widget::canvas(Outline {
                drawn: self.being_drawn(),
                readout: self.readout(),
                view: self.view,
                canvas: self.doc.size(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .into()
    }

    fn readout(&self) -> Option<(Rect, (f32, f32))> {
        let rect = self.marquee()?;
        let (_, b) = self.selecting?;
        Some((rect, b))
    }

    fn being_drawn(&self) -> Option<&Lasso> {
        self.lasso.as_ref()
    }

    fn bottom_bar(&self) -> Element<'_, Message> {
        let zoom = format!("{:.0}%", self.view.zoom * 100.0);
        let message = if self.status.is_empty() {
            format!("{} x {}", self.doc.size().0, self.doc.size().1)
        } else {
            self.status.clone()
        };

        let bar = row![
            text(message),
            Space::new().width(Length::Fill),
            text(zoom).size(12)
        ]
        .spacing(8)
        .padding(8)
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);

        surface(bar, theme::veiled(theme::colours().tool_bar))
            .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_HEIGHT))
            .into()
    }
}

fn surface<'a>(
    content: impl Into<Element<'a, Message>>,
    colour: iced::Color,
) -> iced::widget::Container<'a, Message> {
    container(content)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(colour.into()),
            ..Default::default()
        })
}

fn tab_button<'a>(
    label: &'a str,
    drawing: &'static [u8],
    wide: bool,
    active: bool,
) -> iced::widget::Button<'a, Message> {
    let ink = colour_on_tab(active);
    let face: Element<'a, Message> = if wide {
        column![
            icon(drawing, 14.0, ink),
            text(label)
                .size(12)
                .center()
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(2)
        .align_x(iced::Alignment::Center)
        .into()
    } else {
        icon(drawing, 16.0, ink)
    };

    button(crate::ui::centred(face))
        .width(Length::Fixed(if wide {
            metrics::TOP_PANEL_BUTTON_WIDTH
        } else {
            metrics::TOP_PANEL_THIN_BUTTON_WIDTH
        }))
        .height(Length::Fixed(metrics::TOP_PANEL_BUTTON_HEIGHT))
        .padding(2)
        .clip(true)
}

fn bar_button<'a>(content: impl Into<Element<'a, Message>>) -> iced::widget::Button<'a, Message> {
    button(content)
        .width(Length::Fixed(metrics::TOP_PANEL_THIN_BUTTON_WIDTH))
        .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_BUTTON_HEIGHT))
}

fn strip<'a>(drawing: &'static [u8], message: Message) -> iced::widget::Button<'a, Message> {
    button(crate::ui::centred(icon(
        drawing,
        16.0,
        colour_on_strip(false),
    )))
    .width(Length::Fixed(metrics::TOP_PANEL_THIN_BUTTON_WIDTH))
    .height(Length::Fixed(metrics::GLOBAL_TOOLS_TOP_BAR_BUTTON_HEIGHT))
    .style(|_theme, _status| tool_style(false))
    .on_press(message)
}

fn hint<'a>(
    control: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        control,
        text(label).size(12),
        iced::widget::tooltip::Position::Bottom,
    )
    .style(|_theme| container::Style {
        background: Some(theme::colours().control.into()),
        text_color: Some(theme::colours().text),
        border: iced::Border {
            color: theme::colours().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..Default::default()
    })
    .padding(6)
    .into()
}

fn tab_style(active: bool) -> button::Style {
    button::Style {
        background: Some(if active {
            crate::ui::theme::selection_wash()
        } else {
            iced::Color::TRANSPARENT.into()
        }),
        text_color: if active {
            theme::colours().selection_text
        } else {
            theme::colours().text_on_dark
        },
        border: iced::Border::default(),
        ..Default::default()
    }
}

fn colour_on_tab(active: bool) -> iced::Color {
    let c = theme::colours();
    if active {
        c.selection_text
    } else {
        c.text_on_dark
    }
}

fn colour_on_strip(active: bool) -> iced::Color {
    let c = theme::colours();
    if active { c.selection_text } else { c.text }
}

fn tool_style(active: bool) -> button::Style {
    button::Style {
        background: Some(if active {
            crate::ui::theme::selection_wash()
        } else {
            iced::Color::TRANSPARENT.into()
        }),
        text_color: if active {
            theme::colours().selection_text
        } else {
            theme::colours().text
        },
        border: iced::Border::default(),
        ..Default::default()
    }
}

fn custom_fields(preset: NewCanvas) -> (String, String) {
    let (w, h) = match preset {
        NewCanvas::Custom(w, h) | NewCanvas::Fixed(w, h) => (w, h),
        NewCanvas::Fit(_) => Document::DEFAULT_SIZE,
    };
    (w.to_string(), h.to_string())
}

fn drag_rect(a: (f32, f32), b: (f32, f32), canvas: (u32, u32)) -> Option<Rect> {
    let clamp = |v: f32, limit: u32| v.clamp(0.0, limit as f32);
    let x0 = clamp(a.0.min(b.0), canvas.0).floor() as u32;
    let y0 = clamp(a.1.min(b.1), canvas.1).floor() as u32;
    let x1 = clamp(a.0.max(b.0), canvas.0).ceil() as u32;
    let y1 = clamp(a.1.max(b.1), canvas.1).ceil() as u32;
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1, y1))
}

fn fingerprint(pixels: &Rgba8) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pixels.size().hash(&mut hasher);
    pixels.as_bytes().hash(&mut hasher);
    hasher.finish()
}

const POINT_REACH: f32 = 12.0;

const OVERHANG: f32 = 512.0;

fn shortcut(event: iced::keyboard::Event) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};

    let Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };

    if matches!(key.as_ref(), Key::Named(Named::Escape)) {
        return Some(Message::Deselect);
    }
    if !modifiers.command() {
        return match key.as_ref() {
            Key::Named(Named::Delete) => Some(Message::DeleteFloating),
            Key::Character("[") => Some(Message::ThicknessNudged(-1.0)),
            Key::Character("]") => Some(Message::ThicknessNudged(1.0)),
            _ => None,
        };
    }

    match key.as_ref() {
        Key::Character("n") => Some(Message::NewRequested),
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

fn typing(event: iced::keyboard::Event) -> Option<Message> {
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

async fn ask_to_save(name: String) -> Discard {
    let answer = rfd::AsyncMessageDialog::new()
        .set_title("Do you want to save your work?")
        .set_description(format!("There are unsaved changes to {name}."))
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            "Save".into(),
            "Don't save".into(),
            "Cancel".into(),
        ))
        .show()
        .await;
    match answer {
        rfd::MessageDialogResult::Custom(label) if label == "Save" => Discard::Save,
        rfd::MessageDialogResult::Custom(label) if label == "Don't save" => Discard::Throw,
        rfd::MessageDialogResult::Yes => Discard::Save,
        rfd::MessageDialogResult::No => Discard::Throw,
        _ => Discard::Keep,
    }
}

async fn load(path: PathBuf) -> Result<(PathBuf, Rgba8), String> {
    doc::io::load(&path).map(|pixels| (path, pixels))
}

async fn pick_and_load() -> Result<(PathBuf, Rgba8), String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Images", doc::io::READABLE)
        .set_title("Open")
        .pick_file()
        .await
        .ok_or_else(String::new)?;

    load(handle.path().to_path_buf()).await
}

async fn pick_and_save(pixels: Rgba8) -> Result<PathBuf, String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter("Images", doc::io::WRITABLE)
        .set_title("Save as")
        .set_file_name("Untitled.png")
        .save_file()
        .await
        .ok_or_else(String::new)?;

    let path = doc::io::with_default_extension(handle.path().to_path_buf());
    save_to(pixels, path).await
}

async fn save_to(pixels: Rgba8, path: PathBuf) -> Result<PathBuf, String> {
    doc::io::save(&pixels, &path).map(|()| path)
}

struct Outline<'a> {
    drawn: Option<&'a Lasso>,
    readout: Option<(Rect, (f32, f32))>,
    view: gpu::View,
    canvas: (u32, u32),
}

const READOUT_TEXT: f32 = 12.0;

const READOUT_GLYPH: f32 = 6.6;

const READOUT_PAD: f32 = 8.0;
const READOUT_LABEL: f32 = 18.0;
const READOUT_LINE: f32 = 16.0;
const READOUT_OFFSET: (f32, f32) = (16.0, 12.0);

fn readout_origin(from: Point, size: iced::Size, bounds: iced::Size) -> Point {
    let mut origin = Point::new(from.x + READOUT_OFFSET.0, from.y + READOUT_OFFSET.1);
    if origin.x + size.width > bounds.width {
        origin.x = from.x - READOUT_OFFSET.0 - size.width;
    }
    if origin.y + size.height > bounds.height {
        origin.y = from.y - READOUT_OFFSET.1 - size.height;
    }
    Point::new(origin.x.max(0.0), origin.y.max(0.0))
}

impl iced::widget::canvas::Program<Message> for Outline<'_> {
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

        if self.drawn.is_none() && self.readout.is_none() {
            return Vec::new();
        }

        let rect = self.view.canvas_rect(bounds.size(), self.canvas);
        let at = |(x, y): (f32, f32)| {
            Point::new(rect.x + x * self.view.zoom, rect.y + y * self.view.zoom)
        };

        let mut frame = Frame::new(renderer, bounds.size());

        if let Some(points) = self.drawn.map(Lasso::points).filter(|p| p.len() >= 2) {
            let path = Path::new(|builder| {
                builder.move_to(at(points[0]));
                for point in &points[1..] {
                    builder.line_to(at(*point));
                }
                builder.close();
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(iced::Color::BLACK)
                    .with_width(3.0),
            );
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(iced::Color::WHITE)
                    .with_width(1.0),
            );
        }

        if let Some((region, from)) = self.readout {
            self.draw_readout(&mut frame, bounds, at(from), region);
        }

        vec![frame.into_geometry()]
    }
}

impl Outline<'_> {
    fn draw_readout(
        &self,
        frame: &mut iced::widget::canvas::Frame,
        bounds: iced::Rectangle,
        from: Point,
        region: Rect,
    ) {
        use iced::widget::canvas::{Path, Stroke, Text};

        let lines = [
            ("W:", format!("{} px", region.width())),
            ("H:", format!("{} px", region.height())),
        ];
        let widest = lines.iter().map(|(_, v)| v.len()).max().unwrap_or(0) as f32;
        let size = iced::Size::new(
            READOUT_PAD * 2.0 + READOUT_LABEL + widest * READOUT_GLYPH,
            READOUT_PAD * 2.0 + READOUT_LINE * lines.len() as f32,
        );

        let origin = readout_origin(from, size, bounds.size());

        let c = theme::colours();
        let panel = Path::rectangle(origin, size);
        frame.fill(&panel, c.control);
        frame.stroke(
            &panel,
            Stroke::default().with_color(c.border).with_width(1.0),
        );

        let label = |content: String, x: f32, y: f32, align| Text {
            content,
            position: Point::new(x, y),
            color: c.text,
            size: READOUT_TEXT.into(),
            font: crate::assets::ui_font(),
            align_x: align,
            align_y: iced::alignment::Vertical::Center,
            ..Text::default()
        };
        for (i, (name, value)) in lines.into_iter().enumerate() {
            let y = origin.y + READOUT_PAD + READOUT_LINE * (i as f32 + 0.5);
            frame.fill_text(label(
                name.to_string(),
                origin.x + READOUT_PAD,
                y,
                iced::alignment::Horizontal::Left.into(),
            ));
            frame.fill_text(label(
                value,
                origin.x + size.width - READOUT_PAD,
                y,
                iced::alignment::Horizontal::Right.into(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::Handle;

    #[test]
    fn a_tile_that_is_not_selected_lets_its_bar_through() {
        let nothing: Option<iced::Background> = Some(iced::Color::TRANSPARENT.into());
        assert_eq!(tab_style(false).background, nothing, "the tabs");
        assert_eq!(
            tool_style(false).background,
            nothing,
            "the strip under them"
        );
        assert_ne!(
            tab_style(true).background,
            nothing,
            "but a selected tab has its wash"
        );
        assert_ne!(
            tool_style(true).background,
            nothing,
            "and so does a selected tool"
        );
    }

    fn app(width: u32, height: u32) -> App {
        let config = Config {
            theme: Choice::Light,
            ..Config::default()
        };
        let (mut app, _boot) = App::boot(config, None, None);
        app.doc = Document::blank_sized(width, height, false);
        app.panel = CanvasPanel::new(app.doc.size());
        app.viewport = Size::new(800.0, 600.0);
        app
    }

    fn send(app: &mut App, message: Message) {
        let _ = app.update(message);
    }

    fn resize_to(app: &mut App, w: &str, h: &str) {
        send(app, Message::CanvasWidthEdited(w.into()));
        send(app, Message::CanvasHeightEdited(h.into()));
        send(app, Message::CanvasResizeSubmitted);
    }

    fn pixel(app: &App, x: u32, y: u32) -> [u8; 4] {
        crate::paint::fill::pick(app.doc.pixels(), x as i64, y as i64).unwrap()
    }

    fn click(app: &mut App, x: f32, y: f32) {
        send(app, Message::Canvas(gpu::Interaction::PaintBegan(x, y)));
        send(app, Message::Canvas(gpu::Interaction::PaintEnded));
    }

    #[test]
    fn the_bucket_fills_and_is_one_undo_step() {
        let mut app = app(8, 8);
        app.brush.tool = Tool::Fill;
        app.brush.colour = [255, 0, 0, 255];

        click(&mut app, 4.0, 4.0);
        assert_eq!(pixel(&app, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&app, 7, 7), [255, 0, 0, 255]);

        send(&mut app, Message::Undo);
        assert_eq!(pixel(&app, 4, 4), [0, 0, 0, 0], "back to an empty canvas");
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn the_bucket_ignores_a_click_off_the_canvas() {
        let mut app = app(8, 8);
        app.brush.tool = Tool::Fill;
        click(&mut app, -5.0, 4.0);
        assert!(!app.doc.can_undo(), "nothing should have been recorded");
    }

    #[test]
    fn the_pipette_takes_the_colour_under_it_without_editing() {
        let mut app = app(8, 8);
        app.brush = Brush {
            tool: Tool::PixelPen,
            thickness: 1.0,
            colour: [12, 34, 56, 255],
            ..Default::default()
        };
        click(&mut app, 3.5, 3.5);

        app.brush.colour = [0, 0, 0, 255];
        app.brush.tool = Tool::Pipette;
        click(&mut app, 3.5, 3.5);

        assert_eq!(app.brush.colour, [12, 34, 56, 255]);
        assert!(
            !app.doc.modified || app.doc.can_undo(),
            "picking must not add an edit"
        );
        let edits_before = app.doc.can_redo();
        click(&mut app, 3.5, 3.5);
        assert_eq!(app.doc.can_redo(), edits_before);
    }

    #[test]
    fn the_pipette_leaves_the_colour_alone_over_nothing() {
        let mut app = app(8, 8);
        app.brush.tool = Tool::Pipette;
        app.brush.colour = [9, 9, 9, 255];
        click(&mut app, 4.0, 4.0);
        assert_eq!(app.brush.colour, [9, 9, 9, 255]);
    }

    #[test]
    fn the_spray_paints_while_the_pointer_stands_still() {
        let mut app = app(64, 64);
        app.brush = Brush {
            tool: Tool::SprayCan,
            thickness: 20.0,
            ..Default::default()
        };

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PaintBegan(32.0, 32.0)),
        );
        assert!(app.spraying(), "the frame clock should be running");

        let before = app.doc.pixels().clone();
        for _ in 0..10 {
            send(&mut app, Message::SprayTick);
        }
        assert_ne!(
            app.doc.pixels().as_bytes(),
            before.as_bytes(),
            "ticks should lay paint"
        );

        send(&mut app, Message::Canvas(gpu::Interaction::PaintEnded));
        assert!(!app.spraying(), "and stop when the button comes up");
    }

    #[test]
    fn only_the_spray_runs_off_the_clock() {
        let mut app = app(16, 16);
        for tool in [Tool::Marker, Tool::Crayon, Tool::PixelPen] {
            app.brush.tool = tool;
            send(
                &mut app,
                Message::Canvas(gpu::Interaction::PaintBegan(8.0, 8.0)),
            );
            assert!(!app.spraying(), "{tool:?} should not be on the clock");
            send(&mut app, Message::Canvas(gpu::Interaction::PaintEnded));
        }
    }

    #[test]
    fn a_drag_off_into_the_distance_stays_the_size_of_the_canvas() {
        let far = (40_000.0, 30_000.0);
        let limit = 100 + 2 * OVERHANG as u32;

        for drawing in [
            Drawing::Shape(shapes::ShapeKind::Rectangle),
            Drawing::Curve(crate::paint::curve::CurveKind::Curve5),
        ] {
            let mut app = app(100, 100);
            send(&mut app, Message::TabPicked(Tab::Shapes));
            app.drawing = drawing;
            drag_shape(&mut app, (10.0, 10.0), far);

            let (w, h) = app
                .floating
                .as_ref()
                .expect("something is being drawn")
                .pixels
                .size();
            assert!(
                w <= limit && h <= limit,
                "{drawing:?} drew a {w} by {h} buffer"
            );
        }

        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::CurvePicked(crate::paint::curve::CurveKind::Curve5),
        );
        drag_shape(&mut app, (10.0, 10.0), (90.0, 90.0));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(
                gpu::Grab::Point(2),
                50.0,
                50.0,
            )),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(far.0, far.1)),
        );

        let (w, h) = app.floating.as_ref().unwrap().pixels.size();
        assert!(w <= limit && h <= limit, "a bend drew a {w} by {h} buffer");
    }

    fn fill_canvas(app: &mut App, colour: [u8; 4]) {
        app.doc
            .edit()
            .pixels_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|p| *p = colour);
    }

    fn drag_selection(app: &mut App, a: (f32, f32), b: (f32, f32)) {
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectBegan(a.0, a.1)),
        );
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectMoved(b.0, b.1)),
        );
        send(app, Message::Canvas(gpu::Interaction::SelectEnded));
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    #[test]
    fn dragging_a_selection_lifts_it_off_the_canvas() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (2.0, 2.0), (8.0, 8.0));

        assert!(app.floating.is_some(), "there should be something floating");
        assert_eq!(pixel(&app, 4, 4), [0, 0, 0, 0], "and a hole where it was");
        assert_eq!(pixel(&app, 12, 12), RED, "elsewhere untouched");
    }

    #[test]
    fn undo_cancels_a_live_selection_before_touching_document_history() {
        let mut app = app(16, 16);
        let blank = app.doc.pixels().clone();
        fill_canvas(&mut app, RED);
        app.doc.commit("Paint", Rect::new(0, 0, 16, 16), &blank);
        let before = app.doc.pixels().clone();

        drag_selection(&mut app, (2.0, 2.0), (8.0, 8.0));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 4.0, 4.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(12.0, 12.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FloatReleased));

        send(&mut app, Message::Undo);
        assert!(app.floating.is_none(), "the selection was cancelled");
        assert_eq!(
            app.doc.pixels().as_bytes(),
            before.as_bytes(),
            "the lifted piece went home"
        );
        assert!(app.doc.can_undo(), "the older canvas edit was not touched");

        send(&mut app, Message::Redo);
        assert!(app.floating.is_some(), "redo picks the selection back up");
        assert_eq!(
            pixel(&app, 4, 4),
            [0, 0, 0, 0],
            "with its hole back under it"
        );
    }

    #[test]
    fn cancelling_a_selection_restores_the_documents_modified_state() {
        let mut app = app(16, 16);
        app.doc = Document::from_image(Rgba8::new(16, 16, RED), None);
        assert!(!app.doc.modified);

        drag_selection(&mut app, (2.0, 2.0), (8.0, 8.0));
        assert!(app.doc.modified, "lifting touched the canvas");
        send(&mut app, Message::Undo);
        assert!(
            !app.doc.modified,
            "cancelling returned to the clean document"
        );
        assert_eq!(pixel(&app, 4, 4), RED);
    }

    #[test]
    fn a_selection_dragged_backwards_still_works() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (10.0, 10.0), (4.0, 4.0));
        assert!(app.floating.is_some());
    }

    #[test]
    fn a_stray_click_does_not_select_anything() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (5.0, 5.0), (5.0, 5.0));
        assert!(
            app.floating.is_none(),
            "a zero-sized drag is not a selection"
        );
        assert_eq!(pixel(&app, 5, 5), RED, "and nothing was lifted");
    }

    #[test]
    fn moving_a_selection_and_putting_it_down_is_one_undo_step() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        let before = app.doc.pixels().clone();

        drag_selection(&mut app, (0.0, 0.0), (4.0, 4.0));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 0.0, 0.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(2.0, 2.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(10.0, 10.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FloatReleased));
        send(&mut app, Message::Deselect);

        assert!(app.floating.is_none());
        assert_eq!(pixel(&app, 1, 1), [0, 0, 0, 0], "the hole stays behind");
        assert_eq!(pixel(&app, 9, 9), RED, "and it landed where it was dragged");

        send(&mut app, Message::Undo);
        assert_eq!(
            app.doc.pixels().as_bytes(),
            before.as_bytes(),
            "one step back to the start"
        );
    }

    #[test]
    fn a_sticker_keeps_its_aspect_and_a_selection_stretches() {
        let square = crate::doc::Rgba8::new(40, 20, [0, 0, 255, 255]);

        let mut sticker = app(200, 200);
        send(
            &mut sticker,
            Message::Pasted(Some(Clip::Image(square.clone()))),
        );
        let stretch_to = |app: &mut App, x: f32, y: f32| {
            let corner = app.floating.as_ref().unwrap().xform;
            let (gx, gy) = corner.handle_at(Handle::BottomRight);
            send(
                app,
                Message::Canvas(gpu::Interaction::FloatGrabbed(
                    gpu::Grab::Resize(Handle::BottomRight),
                    gx,
                    gy,
                )),
            );
            send(app, Message::Canvas(gpu::Interaction::FloatDragged(x, y)));
            send(app, Message::Canvas(gpu::Interaction::FloatReleased));
            let out = app.floating.as_ref().unwrap().xform;
            (out.width, out.height)
        };

        let xform = sticker.floating.as_ref().unwrap().xform;
        let (w, h) = stretch_to(&mut sticker, xform.x + 80.0, xform.y + 80.0);
        assert!((w / h - 2.0).abs() < 0.01, "a sticker stayed {w} by {h}");

        send(
            &mut sticker,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        let xform = sticker.floating.as_ref().unwrap().xform;
        let (w, h) = stretch_to(&mut sticker, xform.x + 60.0, xform.y + 60.0);
        assert!(
            (w - 60.0).abs() < 0.01 && (h - 60.0).abs() < 0.01,
            "shift stretched to {w}x{h}"
        );

        let mut lifted = app(200, 200);
        drag_selection(&mut lifted, (10.0, 10.0), (50.0, 30.0));
        let (w, h) = stretch_to(&mut lifted, 90.0, 90.0);
        assert!(
            (w - 80.0).abs() < 0.01 && (h - 80.0).abs() < 0.01,
            "a selection stretched"
        );

        send(
            &mut lifted,
            Message::ModifiersChanged(iced::keyboard::Modifiers::SHIFT),
        );
        let (w, h) = stretch_to(&mut lifted, 60.0, 120.0);
        assert!(
            (w / h - 1.0).abs() < 0.01,
            "shift kept it square, got {w} by {h}"
        );
    }

    #[test]
    fn a_paste_of_our_own_lands_where_you_are_looking() {
        let mut app = app(400, 400);
        fill_canvas(&mut app, RED);
        app.view = gpu::View {
            pan: iced::Vector::new(-600.0, -600.0),
            zoom: 4.0,
        };
        let looking = app.looking_at();
        assert!(
            looking.0 > 250.0 && looking.1 > 250.0,
            "looking at {looking:?}"
        );

        drag_selection(&mut app, (0.0, 0.0), (20.0, 20.0));
        let pixels = app.selected_pixels().unwrap();
        send(&mut app, Message::Copy);
        send(&mut app, Message::Deselect);

        send(&mut app, Message::Pasted(Some(Clip::Image(pixels.clone()))));
        let xform = app.floating.as_ref().expect("a paste").xform;
        let (cx, cy) = xform.centre();
        assert!(
            (cx - looking.0).abs() < 1.0 && (cy - looking.1).abs() < 1.0,
            "our own selection landed at {cx}, {cy} rather than where we were looking"
        );

        let theirs = crate::doc::Rgba8::new(30, 30, [0, 255, 0, 255]);
        send(&mut app, Message::Pasted(Some(Clip::Image(theirs))));
        let (cx, cy) = app.floating.as_ref().unwrap().xform.centre();
        assert!(
            (cx - 200.0).abs() < 1.0 && (cy - 200.0).abs() < 1.0,
            "landed at {cx}, {cy}"
        );
    }

    #[test]
    fn the_stickers_tab_remembers_what_has_been_put_on() {
        let mut app = app(200, 200);
        let one = crate::doc::Rgba8::new(10, 10, [0, 0, 255, 255]);
        let two = crate::doc::Rgba8::new(20, 20, [0, 255, 0, 255]);

        send(
            &mut app,
            Message::Dropped(Ok((PathBuf::from("one.png"), one.clone()))),
        );
        send(
            &mut app,
            Message::Dropped(Ok((PathBuf::from("two.png"), two))),
        );
        assert_eq!(app.stickers.len(), 2);

        send(
            &mut app,
            Message::Dropped(Ok((PathBuf::from("one.png"), one.clone()))),
        );
        assert_eq!(app.stickers.len(), 2);
        assert_eq!(app.stickers.last().unwrap().pixels.size(), (10, 10));

        send(&mut app, Message::Deselect);
        send(&mut app, Message::Undo);
        assert!(app.floating.is_none());
        send(&mut app, Message::StickerRecalled(1));
        assert_eq!(
            app.floating.as_ref().expect("back on top").pixels.size(),
            (10, 10),
            "the one that was put back"
        );
    }

    #[test]
    fn each_tab_puts_its_own_tool_in_your_hand() {
        let mut app = app(32, 32);
        app.brush.tool = Tool::Marker;

        send(&mut app, Message::TabPicked(Tab::Stickers));
        assert_eq!(
            app.brush.tool,
            Tool::Select,
            "the select tool on the stickers tab"
        );

        send(&mut app, Message::TabPicked(Tab::Canvas));
        assert_eq!(app.brush.tool, Tool::Select, "and on the canvas tab");

        send(&mut app, Message::TabPicked(Tab::Text));
        assert_eq!(app.brush.tool, Tool::Text, "the text tool on the text tab");

        send(&mut app, Message::TabPicked(Tab::Shapes));
        assert_eq!(
            app.brush.tool,
            Tool::Shape,
            "and the shape tool on the shapes tab"
        );

        send(&mut app, Message::TabPicked(Tab::Brushes));
        assert_eq!(
            app.brush.tool,
            Tool::Marker,
            "and the brush back on the way home"
        );
    }

    #[test]
    fn select_can_be_had_without_leaving_the_text_panel() {
        let mut app = app(200, 100);
        fill_canvas(&mut app, RED);
        send(&mut app, Message::TabPicked(Tab::Text));

        send(&mut app, Message::FreeformToggled(false));
        assert_eq!(app.brush.tool, Tool::Select);
        assert_eq!(app.tab, Tab::Text, "and the panel stays open");

        drag_selection(&mut app, (10.0, 10.0), (60.0, 60.0));
        let floating = app.floating.as_ref().expect("something was lifted");
        assert!(
            matches!(floating.source, select::Source::Bitmap),
            "the drag selected rather than making a text box"
        );

        send(&mut app, Message::Deselect);
        send(&mut app, Message::TextToolPicked);
        assert_eq!(app.brush.tool, Tool::Text);
        drag_shape(&mut app, (10.0, 10.0), (120.0, 60.0));
        assert!(
            matches!(
                app.floating.as_ref().unwrap().source,
                select::Source::Text(_)
            ),
            "and now the drag makes a text box again"
        );
    }

    #[test]
    fn the_selection_rectangle_is_drawn_while_it_is_being_dragged() {
        let mut app = app(32, 32);
        send(&mut app, Message::FreeformToggled(false));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectBegan(4.0, 4.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(20.0, 12.0)),
        );

        assert_eq!(app.marquee(), Some(Rect::new(4, 4, 20, 12)));
        assert!(
            app.being_drawn().is_none(),
            "and not by the loop overlay as well"
        );

        send(&mut app, Message::Canvas(gpu::Interaction::SelectEnded));
        assert_eq!(app.marquee(), None, "and gone once it is a selection");

        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectBegan(4.0, 4.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(20.0, 12.0)),
        );
        assert_eq!(app.marquee(), None, "no box round a shape being drawn");

        send(&mut app, Message::Canvas(gpu::Interaction::SelectEnded));
        send(&mut app, Message::TabPicked(Tab::Brushes));
        send(&mut app, Message::ToolPicked(Tool::Select));
        send(&mut app, Message::FreeformToggled(true));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectBegan(4.0, 4.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(20.0, 12.0)),
        );
        assert_eq!(app.marquee(), None, "a loop is not a box");
        assert!(
            app.being_drawn().is_some(),
            "the loop is drawn over the top"
        );
    }

    #[test]
    fn clicking_away_from_a_text_box_dismisses_it_rather_than_making_another() {
        let mut app = app(400, 200);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (20.0, 20.0), (200.0, 80.0));
        type_into(&mut app, "Hi");

        drag_shape(&mut app, (300.0, 150.0), (380.0, 190.0));
        assert!(app.floating.is_none(), "the press only put the box down");

        drag_shape(&mut app, (300.0, 150.0), (380.0, 190.0));
        assert!(
            app.floating.is_some(),
            "and the one after it makes the next box"
        );
    }

    #[test]
    fn putting_something_down_does_not_cost_a_click_of_its_own() {
        let mut app = app(32, 32);
        fill_canvas(&mut app, RED);

        drag_selection(&mut app, (0.0, 0.0), (8.0, 8.0));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 4.0, 4.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(20.0, 20.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FloatReleased));

        drag_selection(&mut app, (2.0, 2.0), (6.0, 6.0));
        assert_eq!(
            pixel(&app, 20, 20),
            RED,
            "the first one landed where it was dragged"
        );
        assert_eq!(
            app.floating.as_ref().and_then(|f| f.lifted_from),
            Some(Rect::new(2, 2, 6, 6)),
            "and the press started the next selection"
        );

        send(&mut app, Message::TabPicked(Tab::Brushes));
        app.brush.tool = Tool::PixelPen;
        app.brush.colour = [0, 0, 255, 255];
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PaintBegan(28.0, 28.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::PaintEnded));
        assert!(app.floating.is_none(), "the selection went down");
        assert_eq!(
            pixel(&app, 28, 28),
            [0, 0, 255, 255],
            "and the stroke was drawn"
        );
    }

    #[test]
    fn a_drag_is_relative_to_where_it_was_taken_hold_of() {
        let mut app = app(32, 32);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (0.0, 0.0), (8.0, 8.0));
        let start = app.floating.as_ref().unwrap().xform;

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 6.0, 6.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(10.0, 10.0)),
        );

        let now = app.floating.as_ref().unwrap().xform;
        assert!(
            (now.x - start.x - 4.0).abs() < 0.01,
            "moved to {} from {}",
            now.x,
            start.x
        );
    }

    #[test]
    fn starting_a_new_selection_puts_down_the_old_one() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (0.0, 0.0), (4.0, 4.0));
        drag_selection(&mut app, (8.0, 8.0), (12.0, 12.0));

        assert!(app.floating.is_some());
        assert_eq!(pixel(&app, 1, 1), RED);
    }

    #[test]
    fn a_pasted_image_floats_rather_than_landing_immediately() {
        let mut app = app(16, 16);
        let stamp = Rgba8::new(4, 4, [0, 0, 255, 255]);
        send(&mut app, Message::Pasted(Some(Clip::Image(stamp))));

        assert!(app.floating.is_some());
        assert!(app.floating.as_ref().unwrap().lifted_from.is_none());
        assert_eq!(pixel(&app, 8, 8), [0, 0, 0, 0], "nothing on the canvas yet");

        send(&mut app, Message::Deselect);
        assert_eq!(pixel(&app, 8, 8), [0, 0, 255, 255], "now it is down");
    }

    #[test]
    fn cut_takes_the_pixels_and_leaves_the_hole() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (0.0, 0.0), (4.0, 4.0));
        send(&mut app, Message::Cut);
        assert!(app.floating.is_none() || app.doc.can_undo());
    }

    #[test]
    fn select_all_takes_the_whole_canvas() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        send(&mut app, Message::SelectAll);

        let xform = app.floating.as_ref().unwrap().xform;
        assert_eq!((xform.width, xform.height), (16.0, 16.0));
    }

    #[test]
    fn cropping_to_the_selection_resizes_the_canvas() {
        let mut app = app(16, 16);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (4.0, 4.0), (12.0, 10.0));
        send(&mut app, Message::CropToSelection);

        assert_eq!(app.doc.size(), (8, 6));
        assert!(app.floating.is_none());

        let kept = app
            .doc
            .pixels()
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| **p == RED);
        assert_eq!(kept.count(), 8 * 6, "the cropped image came out empty");
    }

    #[test]
    fn cropping_with_nothing_selected_does_nothing() {
        let mut app = app(16, 16);
        send(&mut app, Message::CropToSelection);
        assert_eq!(app.doc.size(), (16, 16));
    }

    #[test]
    fn switching_tabs_swaps_the_panel_and_the_grips() {
        let mut app = app(100, 100);
        assert_eq!(app.tab, Tab::Brushes);
        assert!(!app.frame().handles, "no grips on the Brushes tab");

        send(&mut app, Message::TabPicked(Tab::Canvas));
        assert_eq!(app.tab, Tab::Canvas);
        assert!(app.frame().handles);

        send(&mut app, Message::ShowCanvasToggled(false));
        assert!(
            !app.frame().handles,
            "hiding the canvas hides its grips too"
        );
    }

    fn drag_shape(app: &mut App, from: (f32, f32), to: (f32, f32)) {
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectBegan(from.0, from.1)),
        );
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectMoved(to.0, to.1)),
        );
        send(app, Message::Canvas(gpu::Interaction::SelectEnded));
    }

    #[test]
    fn dragging_on_the_shapes_tab_leaves_a_shape_floating() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapePicked(shapes::ShapeKind::Circle));
        drag_shape(&mut app, (10.0, 10.0), (60.0, 50.0));

        let floating = app.floating.as_ref().expect("a shape is floating");
        assert!(matches!(
            floating.source,
            select::Source::Shape {
                kind: shapes::ShapeKind::Circle,
                ..
            }
        ));
        assert_eq!(floating.xform.width, 50.0);
        assert_eq!(floating.xform.height, 40.0);
        assert_eq!(pixel(&app, 35, 30), [0, 0, 0, 0]);
    }

    #[test]
    fn committing_a_shape_draws_it() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapePicked(shapes::ShapeKind::Rectangle));
        app.brush.colour = [255, 0, 0, 255];
        send(&mut app, Message::ShapeFillTypePicked(shapes::Paint::Solid));
        send(&mut app, Message::ShapeLineTypePicked(shapes::Paint::None));
        drag_shape(&mut app, (20.0, 20.0), (80.0, 80.0));
        send(&mut app, Message::Deselect);

        assert!(app.floating.is_none(), "committing lets go of it");
        assert_eq!(
            pixel(&app, 50, 50),
            [255, 0, 0, 255],
            "the middle is filled"
        );
        assert_eq!(pixel(&app, 5, 5), [0, 0, 0, 0], "outside it is untouched");
    }

    #[test]
    fn a_shape_is_redrawn_rather_than_stretched_when_resized() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        drag_shape(&mut app, (10.0, 10.0), (50.0, 50.0));
        assert_eq!(app.floating.as_ref().unwrap().pixels.size(), (40, 40));

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(
                gpu::Grab::Resize(gpu::Handle::BottomRight),
                0.0,
                0.0,
            )),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(50.0, 50.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(150.0, 130.0)),
        );
        assert_eq!(app.floating.as_ref().unwrap().pixels.size(), (140, 120));
    }

    #[test]
    fn restyling_redraws_the_shape_that_is_already_out() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        drag_shape(&mut app, (10.0, 10.0), (90.0, 90.0));
        let thin = ink(app.floating.as_ref().unwrap());

        send(&mut app, Message::ShapeThicknessChanged(20.0));
        let thick = ink(app.floating.as_ref().unwrap());
        assert!(
            thick > thin * 2,
            "a thicker outline should cover much more: {thin} -> {thick}"
        );
    }

    #[test]
    fn a_drag_too_small_to_be_meant_leaves_nothing_behind() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        drag_shape(&mut app, (10.0, 10.0), (10.5, 10.5));
        assert!(app.floating.is_none());
    }

    #[test]
    fn a_line_runs_from_where_the_drag_started_to_where_it_ended() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Line));
        drag_shape(&mut app, (80.0, 20.0), (20.0, 70.0));

        let floating = app.floating.as_ref().expect("a line is floating");
        assert_eq!(floating.points(), &[(80.0, 20.0), (20.0, 70.0)]);
    }

    #[test]
    fn a_curve_lays_its_points_along_the_drag_and_can_be_bent() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Curve3));
        drag_shape(&mut app, (20.0, 100.0), (120.0, 100.0));
        assert_eq!(app.floating.as_ref().unwrap().points().len(), 3);

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(
                gpu::Grab::Point(1),
                70.0,
                100.0,
            )),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(70.0, 20.0)),
        );

        let floating = app.floating.as_ref().unwrap();
        assert_eq!(floating.points()[1], (70.0, 20.0));
        assert!(
            floating.xform.y < 20.0,
            "the box grew to hold the bend: {:?}",
            floating.xform
        );
    }

    #[test]
    fn moving_a_curve_carries_its_points_along() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Line));
        drag_shape(&mut app, (20.0, 20.0), (60.0, 60.0));

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 40.0, 40.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(50.0, 70.0)),
        );

        assert_eq!(
            app.floating.as_ref().unwrap().points(),
            &[(30.0, 50.0), (70.0, 90.0)]
        );
    }

    #[test]
    fn switching_curve_tool_keeps_the_two_ends_it_already_has() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Line));
        drag_shape(&mut app, (20.0, 20.0), (120.0, 20.0));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Curve5));

        let points = app.floating.as_ref().unwrap().points();
        assert_eq!(points.len(), 5);
        assert_eq!((points[0], points[4]), ((20.0, 20.0), (120.0, 20.0)));
    }

    #[test]
    fn committing_a_curve_draws_it_where_its_points_are() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Line));
        app.brush.colour = [255, 0, 0, 255];
        send(&mut app, Message::ShapeLineTypePicked(shapes::Paint::Solid));
        drag_shape(&mut app, (10.0, 50.0), (90.0, 50.0));
        send(&mut app, Message::Deselect);

        assert!(app.floating.is_none());
        assert_eq!(pixel(&app, 50, 50), [255, 0, 0, 255], "the line landed");
        assert_eq!(pixel(&app, 50, 10), [0, 0, 0, 0], "and nowhere else");
    }

    #[test]
    fn a_sticker_can_be_moved_without_a_select_tool() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Stickers));
        let at = app.middle();
        app.float_at(Rgba8::new(20, 20, [0, 0, 255, 255]), at);

        let before = app.floating.as_ref().unwrap().xform;
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 50.0, 50.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(70.0, 60.0)),
        );

        let after = app.floating.as_ref().unwrap().xform;
        assert_eq!((after.x - before.x, after.y - before.y), (20.0, 10.0));
    }

    #[test]
    fn a_floating_object_wears_its_grips_whatever_tab_is_open() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Stickers));
        let at = app.middle();
        app.float_at(Rgba8::new(20, 20, [0, 0, 255, 255]), at);
        let frame = app.frame();
        let floating = frame.floating.expect("something is floating");
        assert!(
            floating.points.is_empty(),
            "a bitmap is stretched, not bent"
        );
    }

    #[test]
    fn a_curve_draws_in_the_current_colour_whatever_the_boxes_say() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapeLineTypePicked(shapes::Paint::None));
        send(&mut app, Message::CurvePicked(curve::CurveKind::Line));
        app.brush.colour = [0, 200, 0, 255];
        drag_shape(&mut app, (10.0, 50.0), (90.0, 50.0));
        send(&mut app, Message::Deselect);

        assert_eq!(
            pixel(&app, 50, 50),
            [0, 200, 0, 255],
            "a curve is never invisible"
        );
    }

    fn lasso(app: &mut App, points: &[(f32, f32)]) {
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectBegan(points[0].0, points[0].1)),
        );
        for (x, y) in &points[1..] {
            send(app, Message::Canvas(gpu::Interaction::SelectMoved(*x, *y)));
        }
        send(app, Message::Canvas(gpu::Interaction::SelectEnded));
    }

    #[test]
    fn the_lasso_only_draws_a_loop_when_it_is_the_tool() {
        let mut app = app(100, 100);
        send(&mut app, Message::ToolPicked(Tool::Select));
        assert!(!app.lassoing(), "the box is the default");

        send(&mut app, Message::FreeformToggled(true));
        assert!(app.lassoing());

        send(&mut app, Message::TabPicked(Tab::Shapes));
        assert!(!app.lassoing());
        send(&mut app, Message::TabPicked(Tab::Brushes));
        assert!(app.lassoing());

        send(&mut app, Message::ToolPicked(Tool::Marker));
        assert!(!app.lassoing());
    }

    #[test]
    fn a_lasso_lifts_the_loop_and_not_its_box() {
        let mut app = app(60, 60);
        send(&mut app, Message::ToolPicked(Tool::Fill));
        click(&mut app, 30.0, 30.0);
        assert_eq!(pixel(&app, 5, 5)[3], 255, "the canvas is filled");

        send(&mut app, Message::ToolPicked(Tool::Select));
        send(&mut app, Message::FreeformToggled(true));
        lasso(
            &mut app,
            &[(4.0, 4.0), (50.0, 4.0), (4.0, 50.0), (4.0, 4.0)],
        );

        let floating = app.floating.as_ref().expect("something came up");
        assert!(floating.masked(), "and it knows it is not a rectangle");

        assert_eq!(pixel(&app, 10, 10)[3], 0, "inside the loop is gone");
        assert_eq!(
            pixel(&app, 45, 45)[3],
            255,
            "the corner it cut off is untouched"
        );

        let (w, _) = floating.pixels.size();
        let alpha = |x: u32, y: u32| floating.pixels.as_bytes()[((y * w + x) * 4 + 3) as usize];
        assert!(alpha(4, 4) > 200, "inside");
        assert_eq!(alpha(w - 2, floating.pixels.size().1 - 2), 0, "outside");
    }

    #[test]
    fn a_loop_too_small_to_be_meant_selects_nothing() {
        let mut app = app(60, 60);
        send(&mut app, Message::ToolPicked(Tool::Select));
        send(&mut app, Message::FreeformToggled(true));
        lasso(&mut app, &[(10.0, 10.0)]);
        assert!(app.floating.is_none());
    }

    #[test]
    fn the_loop_is_dropped_when_the_drag_ends() {
        let mut app = app(60, 60);
        send(&mut app, Message::ToolPicked(Tool::Select));
        send(&mut app, Message::FreeformToggled(true));
        lasso(&mut app, &[(4.0, 4.0), (40.0, 4.0), (4.0, 40.0)]);
        assert!(
            app.lasso.is_none(),
            "nothing left to draw over the viewport"
        );
    }

    #[test]
    fn a_lassoed_selection_moves_and_goes_down_through_its_own_shape() {
        let mut app = app(60, 60);
        send(&mut app, Message::ToolPicked(Tool::Fill));
        click(&mut app, 30.0, 30.0);
        send(&mut app, Message::ToolPicked(Tool::Select));
        send(&mut app, Message::FreeformToggled(true));
        lasso(&mut app, &[(4.0, 4.0), (40.0, 4.0), (4.0, 40.0)]);

        let xform = app.floating.as_ref().unwrap().xform;
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Move, 10.0, 10.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(25.0, 25.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FloatReleased));
        assert!(app.floating.as_ref().unwrap().xform.x > xform.x);

        send(&mut app, Message::Deselect);
        assert_eq!(pixel(&app, 5, 5)[3], 0, "the hole is still a hole");
    }

    #[test]
    fn the_shape_panel_swaps_the_grid_for_the_controls_while_one_is_live() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapePicked(shapes::ShapeKind::Heart));
        assert!(app.live_drawing().is_none(), "nothing in hand, so the grid");

        drag_shape(&mut app, (10.0, 10.0), (60.0, 50.0));
        let live = app.live_drawing().expect("a shape in hand");
        assert_eq!(live.name, "Heart", "the panel is headed by what it is");
        assert!(!live.curve);

        send(&mut app, Message::Deselect);
        assert!(
            app.live_drawing().is_none(),
            "put down, so the grid is back"
        );
    }

    #[test]
    fn a_curve_is_told_apart_from_a_shape() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::CurvePicked(crate::paint::curve::CurveKind::Curve3),
        );
        drag_shape(&mut app, (10.0, 10.0), (60.0, 50.0));
        assert!(app.live_drawing().expect("a curve in hand").curve);
    }

    #[test]
    fn the_palette_writes_to_whichever_swatch_is_chosen() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapeFillTypePicked(shapes::Paint::Solid));
        send(&mut app, Message::ShapeLineTypePicked(shapes::Paint::Solid));
        let last = crate::ui::theme::SWATCHES.len() - 1;
        let want = sidebar::to_bytes(crate::ui::theme::SWATCHES[last]);

        send(&mut app, Message::ColourPicked(last));
        assert_eq!(app.shape_style.outline, Some(want));
        assert_ne!(app.shape_style.fill, Some(want), "the fill was left alone");

        send(&mut app, Message::ShapeColourTargetPicked(true));
        send(&mut app, Message::ColourPicked(0));
        let first = sidebar::to_bytes(crate::ui::theme::SWATCHES[0]);
        assert_eq!(app.shape_style.fill, Some(first));
        assert_eq!(app.shape_style.outline, Some(want), "and now the line is");
    }

    #[test]
    fn opacity_reaches_the_canvas_and_not_only_the_preview() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapePicked(shapes::ShapeKind::Rectangle));
        send(&mut app, Message::ShapeFillTypePicked(shapes::Paint::Solid));
        send(&mut app, Message::ShapeLineTypePicked(shapes::Paint::None));
        drag_shape(&mut app, (20.0, 20.0), (80.0, 80.0));

        send(&mut app, Message::FloatOpacityChanged(0.5));
        assert_eq!(app.floating.as_ref().unwrap().opacity(), 0.5);
        send(&mut app, Message::Deselect);

        let put_down = pixel(&app, 50, 50);
        assert!(
            (100..=160).contains(&put_down[3]),
            "alpha came out {}",
            put_down[3]
        );
    }

    #[test]
    fn a_shape_can_be_turned_and_mirrored() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::ShapePicked(shapes::ShapeKind::RightTriangle),
        );
        drag_shape(&mut app, (20.0, 40.0), (140.0, 100.0));
        let before = app.floating.as_ref().unwrap().xform;

        send(&mut app, Message::FloatTurned(true));
        let after = app.floating.as_ref().unwrap().xform;
        assert_eq!(
            (after.width, after.height),
            (before.height, before.width),
            "the box turns too"
        );
        assert!(
            (after.centre().0 - before.centre().0).abs() < 0.01
                && (after.centre().1 - before.centre().1).abs() < 0.01,
            "and it turns about its own centre"
        );

        for _ in 0..3 {
            send(&mut app, Message::FloatTurned(true));
        }
        assert_eq!(app.floating.as_ref().unwrap().xform, before);

        let pixels = app.floating.as_ref().unwrap().pixels.as_bytes().to_vec();
        send(&mut app, Message::FloatMirrored(true));
        assert_ne!(
            app.floating.as_ref().unwrap().pixels.as_bytes(),
            &pixels[..]
        );
        send(&mut app, Message::FloatMirrored(true));
        assert_eq!(
            app.floating.as_ref().unwrap().pixels.as_bytes(),
            &pixels[..]
        );
    }

    #[test]
    fn a_turned_shape_stays_turned_when_it_is_resized() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::ShapePicked(shapes::ShapeKind::RightTriangle),
        );
        drag_shape(&mut app, (20.0, 20.0), (120.0, 80.0));
        send(&mut app, Message::FloatTurned(true));
        let turned = app.floating.as_ref().unwrap().pixels.as_bytes().to_vec();

        let xform = app.floating.as_ref().unwrap().xform;
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(
                gpu::Grab::Resize(crate::gpu::handles::Handle::BottomRight),
                xform.x + xform.width,
                xform.y + xform.height,
            )),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(
                xform.x + xform.width,
                xform.y + xform.height,
            )),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FloatReleased));
        assert_eq!(
            app.floating.as_ref().unwrap().pixels.as_bytes(),
            &turned[..]
        );
    }

    #[test]
    fn a_curve_turns_its_points_rather_than_its_pixels() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::CurvePicked(crate::paint::curve::CurveKind::Line),
        );
        drag_shape(&mut app, (40.0, 100.0), (160.0, 100.0));
        let before = app.floating.as_ref().unwrap().points().to_vec();
        assert!((before[0].1 - before[1].1).abs() < 0.01, "drawn flat");

        send(&mut app, Message::FloatTurned(true));
        let after = app.floating.as_ref().unwrap().points().to_vec();
        assert!(
            (after[0].0 - after[1].0).abs() < 0.01,
            "a quarter turn stands it up"
        );
        let middle = |p: &[(f32, f32)]| ((p[0].0 + p[1].0) / 2.0, (p[0].1 + p[1].1) / 2.0);
        let (bx, by) = middle(&before);
        let (ax, ay) = middle(&after);
        assert!((ax - bx).abs() < 0.01 && (ay - by).abs() < 0.01);
    }

    #[test]
    fn the_cutout_takes_the_thing_out_and_lifts_it() {
        let (w, h) = (120u32, 90u32);
        let mut app = app(w, h);
        fill_canvas(&mut app, [40, 60, 200, 255]);
        {
            let pixels = app.doc.edit().pixels_mut();
            for y in 25..65 {
                for x in 35..85 {
                    let i = (y * w as usize + x) * 4;
                    pixels[i..i + 4].copy_from_slice(&[200, 40, 40, 255]);
                }
            }
        }

        send(&mut app, Message::CutoutOpened);
        let cutting_out = app.cutting_out.as_ref().expect("it opened");
        assert!(!cutting_out.refining, "it starts on the box");
        assert_eq!(
            cutting_out.rect,
            Rect::new(0, 0, w, h),
            "which is the whole picture"
        );

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FrameGrabbed(Handle::TopLeft)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FrameDragged(28.0, 18.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FrameReleased));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FrameGrabbed(Handle::BottomRight)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FrameDragged(92.0, 72.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FrameReleased));

        send(&mut app, Message::CutoutNext);
        let cutting_out = app.cutting_out.as_ref().expect("still open");
        assert!(cutting_out.refining, "and on to the refining");
        let mask = cutting_out.mask.as_ref().expect("a cut");
        let at = |x: usize, y: usize| mask[y * w as usize + x] > 128;
        assert!(at(60, 45), "the middle of the block is in the cut");
        assert!(!at(5, 5), "and the corner of the picture is not");

        send(&mut app, Message::CutoutDone);
        assert!(app.cutting_out.is_none(), "it closed");
        let floating = app.floating.as_ref().expect("the cut is floating");
        assert!(floating.masked(), "and it is a shaped selection, not a box");
        assert_eq!(app.brush.tool, Tool::Select, "with the select tool in hand");

        let behind = pixel(&app, 60, 45);
        assert_eq!(behind[3], 255, "the hole was filled in");
        assert!(
            behind[2] > behind[0],
            "and filled with the blue that was round it"
        );
    }

    #[test]
    fn the_refining_brush_takes_pieces_out_of_the_cut() {
        let (w, h) = (80u32, 60u32);
        let mut app = app(w, h);
        fill_canvas(&mut app, [40, 60, 200, 255]);
        {
            let pixels = app.doc.edit().pixels_mut();
            for y in 15..45 {
                for x in 20..60 {
                    let i = (y * w as usize + x) * 4;
                    pixels[i..i + 4].copy_from_slice(&[200, 40, 40, 255]);
                }
            }
        }
        let before = app.doc.pixels().clone();

        send(&mut app, Message::CutoutOpened);
        send(&mut app, Message::CutoutNext);
        assert!(
            app.cutting_out.as_ref().unwrap().mask.as_ref().unwrap()[30 * w as usize + 40] > 128
        );

        send(&mut app, Message::CutoutBrushPicked(false));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PaintBegan(40.0, 30.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PaintMoved(42.0, 30.0)),
        );
        assert_eq!(
            app.cutting_out.as_ref().unwrap().mask.as_ref().unwrap()[30 * w as usize + 40],
            0,
            "the stroke showed while it was being drawn"
        );

        send(&mut app, Message::Canvas(gpu::Interaction::PaintEnded));
        assert_eq!(
            app.doc.pixels().as_bytes(),
            before.as_bytes(),
            "and none of it touched the picture"
        );
    }

    #[test]
    fn cropping_is_a_frame_that_does_nothing_until_it_is_applied() {
        let mut app = app(400, 300);
        fill_canvas(&mut app, RED);

        send(&mut app, Message::CropOpened);
        let frame = app.cropping.as_ref().expect("a frame").rect;
        assert_eq!(
            frame,
            Rect::new(0, 0, 400, 300),
            "it starts as the whole canvas"
        );

        send(
            &mut app,
            Message::CropFramingPicked(Some(sidebar::Framing::Square)),
        );
        let square = app.cropping.as_ref().unwrap().rect;
        assert_eq!(square.width(), square.height(), "1:1 means square");
        assert_eq!(square.height(), 300, "and as large as fits");
        assert_eq!(
            app.doc.size(),
            (400, 300),
            "the picture is untouched so far"
        );

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FrameGrabbed(Handle::Right)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FrameDragged(250.0, 150.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FrameReleased));
        let dragged = app.cropping.as_ref().unwrap().rect;
        assert_eq!(dragged.width(), dragged.height(), "still square");
        assert!(dragged.width() < square.width(), "and smaller than it was");

        send(&mut app, Message::CropCancelled);
        assert!(app.cropping.is_none());
        assert_eq!(app.doc.size(), (400, 300));

        send(&mut app, Message::CropOpened);
        send(
            &mut app,
            Message::CropFramingPicked(Some(sidebar::Framing::Widescreen)),
        );
        let wanted = app.cropping.as_ref().unwrap().rect;
        send(&mut app, Message::CropApplied);
        assert_eq!(app.doc.size(), (wanted.width(), wanted.height()));
        assert_eq!(pixel(&app, 4, 4), RED, "and the picture came with it");
    }

    #[test]
    fn crop_with_a_selection_crops_to_it_without_a_frame() {
        let mut app = app(32, 32);
        fill_canvas(&mut app, RED);
        drag_selection(&mut app, (4.0, 4.0), (20.0, 16.0));

        send(&mut app, Message::CropOpened);
        assert!(app.cropping.is_none(), "no frame was opened");
        assert_eq!(app.doc.size(), (16, 12), "it cropped to the selection");
    }

    #[test]
    fn the_crop_fields_move_the_frame() {
        let mut app = app(400, 300);
        send(&mut app, Message::CropOpened);
        send(&mut app, Message::CropWidthEdited("120".into()));
        send(&mut app, Message::CropHeightEdited("90".into()));

        let frame = app.cropping.as_ref().unwrap().rect;
        assert_eq!((frame.width(), frame.height()), (120, 90));

        send(&mut app, Message::CropWidthEdited(String::new()));
        assert_eq!(app.cropping.as_ref().unwrap().rect.width(), 120);
    }

    #[test]
    fn a_shape_takes_bones_and_the_canvas_adds_more() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapePicked(shapes::ShapeKind::Circle));
        drag_shape(&mut app, (20.0, 20.0), (180.0, 180.0));

        send(&mut app, Message::BonesRequested);
        let floating = app.floating.as_ref().expect("still floating");
        assert!(floating.is_closed(), "the shape came back as a loop");
        let bones = floating.points().len();
        assert_eq!(bones, crate::paint::curve::SHAPE_BONES);

        let on_the_line = app.floating.as_ref().unwrap().points()[0];
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PointAdded(on_the_line.0, on_the_line.1)),
        );
        assert_eq!(app.floating.as_ref().unwrap().points().len(), bones + 1);

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PointAdded(100.0, 100.0)),
        );
        assert_eq!(app.floating.as_ref().unwrap().points().len(), bones + 1);

        send(&mut app, Message::Canvas(gpu::Interaction::PointRemoved(0)));
        assert_eq!(app.floating.as_ref().unwrap().points().len(), bones);

        let live = app.live_drawing().expect("the style panel is up");
        assert!(
            !live.curve,
            "a shape with bones is still a shape to the panel"
        );
        assert!(live.boned && !live.bones);
    }

    #[test]
    fn a_curve_stretches_by_its_box_and_turns_by_its_dial() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(
            &mut app,
            Message::CurvePicked(crate::paint::curve::CurveKind::Curve3),
        );
        drag_shape(&mut app, (20.0, 100.0), (100.0, 100.0));

        let before = app.floating.as_ref().unwrap().points().to_vec();
        let width = app.floating.as_ref().unwrap().xform.width;

        let xform = app.floating.as_ref().unwrap().xform;
        let (gx, gy) = xform.handle_at(Handle::Right);
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(
                gpu::Grab::Resize(Handle::Right),
                gx,
                gy,
            )),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatDragged(xform.x + width * 2.0, gy)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::FloatReleased));

        let after = app.floating.as_ref().unwrap().points().to_vec();
        let span = |p: &[(f32, f32)]| p.last().unwrap().0 - p[0].0;
        assert!(
            span(&after) > span(&before) * 1.5,
            "the points did not follow the box: {} to {}",
            span(&before),
            span(&after)
        );

        let flat = app.floating.as_ref().unwrap().points().to_vec();
        let (cx, cy) = app.floating.as_ref().unwrap().xform.centre();
        let (dx, dy) = app.floating.as_ref().unwrap().xform.rotation_grip(20.0);
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatGrabbed(gpu::Grab::Rotate, dx, dy)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::FloatReleasedAt(cx + 60.0, cy)),
        );

        let turned = app.floating.as_ref().unwrap().points().to_vec();
        let vertical = |p: &[(f32, f32)]| (p.last().unwrap().1 - p[0].1).abs();
        assert!(
            vertical(&turned) > vertical(&flat) + 10.0,
            "a quarter turn should stand it up: {:?}",
            turned
        );
    }

    #[test]
    fn picking_a_colour_repaints_what_is_still_live() {
        let mut app = app(100, 100);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapeFillTypePicked(shapes::Paint::Solid));
        send(&mut app, Message::ShapeLineTypePicked(shapes::Paint::None));
        drag_shape(&mut app, (20.0, 20.0), (80.0, 80.0));

        let last = crate::ui::theme::SWATCHES.len() - 1;
        send(&mut app, Message::ColourPicked(last));
        let want = sidebar::to_bytes(crate::ui::theme::SWATCHES[last]);
        send(&mut app, Message::Deselect);
        assert_eq!(pixel(&app, 50, 50), want);
    }

    fn type_into(app: &mut App, s: &str) {
        for c in s.chars() {
            send(app, Message::TextEdited(TextAction::Insert(c)));
        }
    }

    #[test]
    fn dragging_on_the_text_tab_leaves_a_box_waiting_for_letters() {
        let mut app = app(400, 300);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (40.0, 40.0), (300.0, 140.0));

        let floating = app.floating.as_ref().expect("a text box is floating");
        assert!(matches!(floating.source, select::Source::Text(_)));
        assert!(floating.editing, "and the caret is in it");
        assert_eq!(floating.xform.width, 260.0);
        assert!(app.typing(), "the keyboard belongs to the box now");
    }

    #[test]
    fn a_text_box_is_an_outline_until_the_drag_ends() {
        let mut app = app(400, 300);
        send(&mut app, Message::TabPicked(Tab::Text));

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectBegan(40.0, 40.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(60.0, 60.0)),
        );
        assert!(app.floating.is_none(), "nothing has been made yet");
        assert_eq!(
            app.marquee(),
            Some(Rect::new(40, 40, 60, 60)),
            "just the outline"
        );

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(300.0, 140.0)),
        );
        assert_eq!(
            app.marquee(),
            Some(Rect::new(40, 40, 300, 140)),
            "which follows the drag"
        );

        send(&mut app, Message::Canvas(gpu::Interaction::SelectEnded));
        assert_eq!(app.marquee(), None, "and goes when the drag does");

        let floating = app.floating.as_ref().expect("the box the outline promised");
        assert_eq!(
            floating.xform.width, 260.0,
            "at the width of the whole drag"
        );
        assert!(floating.editing, "with the caret in it");
    }

    #[test]
    fn a_click_on_the_text_tab_leaves_a_box_of_its_own() {
        let mut app = app(400, 300);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (40.0, 40.0), (40.0, 40.0));

        let floating = app.floating.as_ref().expect("a box, not nothing at all");
        assert!(matches!(floating.source, select::Source::Text(_)));
        assert!(floating.xform.width > 40.0, "wide enough to type into");
    }

    #[test]
    fn the_panel_styles_the_selected_part_of_the_text() {
        let mut app = app(400, 200);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (10.0, 10.0), (390.0, 120.0));
        type_into(&mut app, "AB");

        send(
            &mut app,
            Message::TextEdited(TextAction::Motion(Motion::SelectLeft)),
        );
        let last = crate::ui::theme::SWATCHES.len() - 1;
        let want = sidebar::to_bytes(crate::ui::theme::SWATCHES[last]);
        send(&mut app, Message::ColourPicked(last));

        send(&mut app, Message::Deselect);
        let ink: Vec<[u8; 4]> = app
            .doc
            .pixels()
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .filter(|p| p[3] == 255)
            .collect();
        assert!(ink.contains(&want), "the selected letter took the colour");
        assert!(
            ink.contains(&[0, 0, 0, 255]),
            "and the letter next to it kept the one it had"
        );
    }

    #[test]
    fn a_colour_lands_on_the_text_about_to_be_typed() {
        let mut app = app(400, 200);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (20.0, 20.0), (380.0, 120.0));
        type_into(&mut app, "H");

        let last = crate::ui::theme::SWATCHES.len() - 1;
        let want = sidebar::to_bytes(crate::ui::theme::SWATCHES[last]);
        send(&mut app, Message::ColourPicked(last));
        assert_eq!(app.text_style.colour, want, "the panel took it");
        type_into(&mut app, "i");

        send(&mut app, Message::Deselect);
        let ink: Vec<[u8; 4]> = app
            .doc
            .pixels()
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .filter(|p| p[3] == 255)
            .collect();
        assert!(
            ink.contains(&want),
            "what was typed after came out in the new colour"
        );
        assert!(
            ink.contains(&[0, 0, 0, 255]),
            "and what was typed before kept the colour it was typed in"
        );
    }

    #[test]
    fn a_setting_with_nothing_selected_does_not_rewrite_what_is_there() {
        let written = |fiddle: bool| {
            let mut app = app(400, 200);
            send(&mut app, Message::TabPicked(Tab::Text));
            drag_shape(&mut app, (20.0, 20.0), (380.0, 120.0));
            type_into(&mut app, "Hi");
            if fiddle {
                send(&mut app, Message::TextBoldToggled);
                send(&mut app, Message::TextSizePicked(96));
                send(&mut app, Message::TextItalicToggled);
            }
            send(&mut app, Message::Deselect);
            app.doc.pixels().as_bytes().to_vec()
        };

        assert_eq!(
            written(true),
            written(false),
            "the letters already typed were rewritten"
        );
    }

    #[test]
    fn typing_goes_into_the_box_and_committing_draws_it() {
        let mut app = app(400, 300);
        send(&mut app, Message::TabPicked(Tab::Text));
        app.text_style.colour = [255, 0, 0, 255];
        drag_shape(&mut app, (20.0, 20.0), (380.0, 120.0));
        type_into(&mut app, "Hi");

        let boxed = app.floating.as_mut().unwrap().text_box().unwrap();
        assert_eq!(boxed.content(), "Hi");

        send(&mut app, Message::Deselect);
        assert!(app.floating.is_none(), "clicking away puts it down");
        let ink = app
            .doc
            .pixels()
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 8)
            .count();
        assert!(ink > 50, "the letters landed on the canvas: {ink}");
    }

    #[test]
    fn an_empty_box_leaves_nothing_behind() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (20.0, 20.0), (180.0, 80.0));
        send(&mut app, Message::Deselect);

        let ink = app
            .doc
            .pixels()
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 8)
            .count();
        assert_eq!(ink, 0, "no caret painted into the canvas");
    }

    #[test]
    fn the_committed_text_carries_no_caret() {
        let mut app = app(300, 200);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (10.0, 10.0), (290.0, 90.0));
        assert!(app.caret_on, "a fresh box shows its caret");
        send(&mut app, Message::Deselect);

        assert!(app.doc.pixels().as_bytes().iter().all(|b| *b == 0));
    }

    #[test]
    fn restyling_keeps_the_text_and_changes_how_it_is_set() {
        let mut app = app(400, 300);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (20.0, 20.0), (380.0, 120.0));
        type_into(&mut app, "Writing");

        send(&mut app, Message::TextBoldToggled);
        assert!(app.text_style.bold);
        let boxed = app.floating.as_mut().unwrap().text_box().unwrap();
        assert_eq!(boxed.content(), "Writing", "the letters survive a restyle");
    }

    #[test]
    fn a_bigger_size_makes_a_taller_box() {
        let mut app = app(600, 400);
        send(&mut app, Message::TabPicked(Tab::Text));
        send(&mut app, Message::TextSizePicked(16));
        drag_shape(&mut app, (20.0, 20.0), (580.0, 60.0));
        type_into(&mut app, "Writing Test");
        let small = app.floating.as_ref().unwrap().xform.height;

        send(&mut app, Message::TextSizePicked(72));
        let big = app.floating.as_ref().unwrap().xform.height;
        assert!(
            big > small,
            "the box grows with the letters: {small} then {big}"
        );
    }

    #[test]
    fn a_text_box_swallows_the_keyboard_and_gives_it_back() {
        let mut app = app(300, 200);
        assert!(!app.typing());
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (10.0, 10.0), (290.0, 90.0));
        assert!(app.typing());
        send(&mut app, Message::Deselect);
        assert!(
            !app.typing(),
            "and once it is down the shortcuts work again"
        );
    }

    #[test]
    fn a_plain_letter_types_and_a_shortcut_does_not() {
        use iced::keyboard::{Event, Key, Modifiers, key::Named};
        let press = |key: Key, mods: Modifiers, typed: Option<&str>| Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: mods,
            text: typed.map(|t| t.into()),
            repeat: false,
        };
        let got = |e: Event| format!("{:?}", typing(e));

        assert_eq!(
            got(press(
                Key::Character("o".into()),
                Modifiers::empty(),
                Some("o")
            )),
            "Some(TextEdited(Insert('o')))"
        );
        assert_eq!(
            got(press(
                Key::Named(Named::Backspace),
                Modifiers::empty(),
                None
            )),
            "Some(TextEdited(Backspace))"
        );
        assert_eq!(
            got(press(Key::Named(Named::Escape), Modifiers::empty(), None)),
            "Some(Deselect)",
            "escape still puts the box down"
        );
        assert_eq!(
            got(press(
                Key::Character("s".into()),
                Modifiers::COMMAND,
                Some("s")
            )),
            "Some(SaveRequested)",
            "and saving still works"
        );
        assert_eq!(
            got(press(
                Key::Character("z".into()),
                Modifiers::COMMAND,
                Some("z")
            )),
            "Some(Undo)",
            "undo belongs to the box while it is being typed into"
        );
        assert_eq!(
            got(press(
                Key::Character("z".into()),
                Modifiers::COMMAND | Modifiers::SHIFT,
                Some("z")
            )),
            "Some(Redo)"
        );
    }

    #[test]
    fn text_undo_stays_live_until_the_box_itself_is_undone() {
        let mut app = app(300, 200);
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (10.0, 10.0), (290.0, 100.0));
        type_into(&mut app, "cat");

        let content = |app: &mut App| app.floating.as_mut().unwrap().text_box().unwrap().content();
        send(&mut app, Message::Undo);
        assert_eq!(content(&mut app), "ca");
        send(&mut app, Message::Redo);
        assert_eq!(content(&mut app), "cat");

        send(&mut app, Message::Undo);
        send(&mut app, Message::Undo);
        send(&mut app, Message::Undo);
        assert_eq!(
            content(&mut app),
            "",
            "the last text edit leaves the empty box alive"
        );
        send(&mut app, Message::Undo);
        assert!(
            app.floating.is_none(),
            "the next step cancels the box itself"
        );

        send(&mut app, Message::Redo);
        assert_eq!(
            content(&mut app),
            "",
            "redo restores the cancelled box first"
        );
        send(&mut app, Message::Redo);
        assert_eq!(content(&mut app), "c", "then restores its text history");
    }

    #[test]
    fn an_empty_text_box_is_not_worth_an_undo_step() {
        let mut app = app(200, 200);
        let before = app.doc.can_undo();
        send(&mut app, Message::TabPicked(Tab::Text));
        drag_shape(&mut app, (20.0, 20.0), (180.0, 80.0));
        send(&mut app, Message::Deselect);
        assert_eq!(
            app.doc.can_undo(),
            before,
            "nothing happened, so nothing to undo"
        );
    }

    #[test]
    fn the_menu_takes_over_and_gives_the_window_back() {
        let mut app = app(200, 200);
        assert!(app.menu.is_none());
        send(&mut app, Message::MenuOpened);
        assert_eq!(app.menu, Some(None), "open, on no page in particular");
        send(&mut app, Message::MenuPagePicked(MenuPage::Settings));
        assert_eq!(app.menu, Some(Some(MenuPage::Settings)));
        send(&mut app, Message::MenuClosed);
        assert!(app.menu.is_none());
    }

    #[test]
    fn opening_the_menu_puts_down_what_is_floating() {
        let mut app = app(200, 200);
        send(&mut app, Message::TabPicked(Tab::Shapes));
        send(&mut app, Message::ShapeFillTypePicked(shapes::Paint::Solid));
        drag_shape(&mut app, (20.0, 20.0), (180.0, 180.0));
        assert!(app.floating.is_some());

        send(&mut app, Message::MenuOpened);
        assert!(
            app.floating.is_none(),
            "the shape went down rather than away"
        );
        assert!(app.doc.can_undo(), "and it is on the canvas");
    }

    #[test]
    fn new_on_an_untouched_document_does_not_ask() {
        let mut app = app(64, 48);
        assert!(!app.doc.modified);
        send(&mut app, Message::NewRequested);
        assert_eq!(
            app.doc.size(),
            app.new_canvas_size(),
            "back to a fresh canvas"
        );
        assert!(app.menu.is_none(), "and the menu closes behind it");
    }

    #[test]
    fn the_canvas_you_start_with_takes_its_size_from_the_preset() {
        let fixed = Config {
            theme: Choice::Light,
            new_canvas: NewCanvas::Fixed(1920, 1080),
            ..Config::default()
        };
        let (app, _boot) = App::boot(fixed, None, None);
        assert_eq!(
            app.doc.size(),
            (1920, 1080),
            "before a window is even measured"
        );

        let fitting = Config {
            theme: Choice::Light,
            new_canvas: NewCanvas::Fit(crate::canvas::Ratio::Square),
            ..Config::default()
        };
        let (mut app, _boot) = App::boot(fitting, None, None);
        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));
        let (w, h) = app.doc.size();
        assert_eq!(
            w, h,
            "a square preference gives a square canvas, got {w} by {h}"
        );
        assert_eq!((w, h), app.new_canvas_size());
    }

    #[test]
    fn a_document_that_has_been_touched_survives_the_first_measurement() {
        let config = Config {
            theme: Choice::Light,
            new_canvas: NewCanvas::Fit(crate::canvas::Ratio::Square),
            ..Config::default()
        };
        let (mut app, _boot) = App::boot(config, None, None);
        app.doc = Document::blank_sized(300, 200, false);
        app.doc.modified = true;

        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));
        assert_eq!(
            app.doc.size(),
            (300, 200),
            "the canvas that was there stayed"
        );
    }

    #[test]
    fn new_takes_its_size_from_the_preset() {
        let mut app = app(64, 48);
        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));

        send(
            &mut app,
            Message::NewCanvasPicked(NewCanvas::Fixed(1920, 1080)),
        );
        send(&mut app, Message::NewRequested);
        assert_eq!(app.doc.size(), (1920, 1080));

        send(
            &mut app,
            Message::NewCanvasPicked(NewCanvas::Fit(crate::canvas::Ratio::Square)),
        );
        send(&mut app, Message::NewRequested);
        let (w, h) = app.doc.size();
        assert_eq!(w, h, "a square preset gives a square canvas, got {w} x {h}");

        send(&mut app, Message::WindowResized(Size::new(1600.0, 1000.0)));
        send(&mut app, Message::NewRequested);
        assert!(
            app.doc.size().0 > w,
            "and a bigger window gives a bigger canvas"
        );
    }

    #[test]
    fn a_custom_size_is_only_taken_once_both_halves_are_numbers() {
        let mut app = app(64, 48);
        send(
            &mut app,
            Message::NewCanvasPicked(NewCanvas::Custom(100, 100)),
        );

        send(&mut app, Message::NewCanvasWidthEdited(String::new()));
        assert_eq!(
            app.config.new_canvas,
            NewCanvas::Custom(100, 100),
            "half typed, left alone"
        );

        send(&mut app, Message::NewCanvasWidthEdited("640".into()));
        send(&mut app, Message::NewCanvasHeightEdited("480".into()));
        assert_eq!(app.config.new_canvas, NewCanvas::Custom(640, 480));

        send(&mut app, Message::NewCanvasHeightEdited("nonsense".into()));
        assert_eq!(
            app.config.new_canvas,
            NewCanvas::Custom(640, 480),
            "rubbish changes nothing"
        );
    }

    #[test]
    fn the_picker_opens_on_the_colour_in_hand_and_adds_what_it_gives() {
        let mut app = app(60, 60);
        send(&mut app, Message::ColourPicked(5));
        let had = app.brush.colour;

        send(&mut app, Message::PickerOpened);
        assert_eq!(
            app.picker.as_ref().expect("open").colour(),
            had,
            "opens where you are"
        );

        send(&mut app, Message::PickerHexEdited("#123456".into()));
        send(&mut app, Message::PickerConfirmed);
        assert!(app.picker.is_none(), "and closes behind itself");
        assert_eq!(
            app.brush.colour,
            [0x12, 0x34, 0x56, 255],
            "the colour is taken"
        );
        assert_eq!(
            app.config.custom_colours,
            vec![[0x12, 0x34, 0x56, 255]],
            "and added to the palette"
        );
    }

    #[test]
    fn cancelling_the_picker_changes_nothing() {
        let mut app = app(60, 60);
        let had = app.brush.colour;
        send(&mut app, Message::PickerOpened);
        send(&mut app, Message::PickerHexEdited("#ff00ff".into()));
        send(&mut app, Message::PickerClosed);
        assert_eq!(app.brush.colour, had);
        assert!(app.config.custom_colours.is_empty());
    }

    #[test]
    fn the_gradients_only_move_while_they_are_being_dragged() {
        let mut app = app(60, 60);
        send(&mut app, Message::PickerOpened);
        let before = app.picker.as_ref().unwrap().clone();

        send(&mut app, Message::PickerFieldPicked(0.5, 0.5));
        send(&mut app, Message::PickerHuePicked(200.0));
        assert_eq!(
            app.picker.as_ref().unwrap(),
            &before,
            "nothing was held down"
        );

        send(&mut app, Message::PickerFieldPressed);
        send(&mut app, Message::PickerFieldPicked(0.5, 0.25));
        let picker = app.picker.as_ref().unwrap();
        assert_eq!((picker.saturation, picker.value), (0.5, 0.25));

        let hue = app.picker.as_ref().unwrap().hue;
        send(&mut app, Message::PickerHuePicked(200.0));
        assert_eq!(app.picker.as_ref().unwrap().hue, hue);

        send(&mut app, Message::PickerReleased);
        send(&mut app, Message::PickerFieldPicked(0.9, 0.9));
        assert_eq!(
            app.picker.as_ref().unwrap().saturation,
            0.5,
            "let go, so it stopped"
        );
    }

    #[test]
    fn the_custom_row_holds_one_row_and_no_duplicates() {
        let mut app = app(60, 60);
        for hex in [
            "#111111", "#222222", "#333333", "#444444", "#555555", "#666666", "#777777",
        ] {
            send(&mut app, Message::PickerOpened);
            send(&mut app, Message::PickerHexEdited(hex.into()));
            send(&mut app, Message::PickerConfirmed);
        }
        assert_eq!(
            app.config.custom_colours.len(),
            6,
            "six across, oldest out first"
        );
        assert_eq!(
            app.config.custom_colours[0],
            [0x22, 0x22, 0x22, 255],
            "the first went"
        );
        assert_eq!(app.config.custom_colours[5], [0x77, 0x77, 0x77, 255]);

        send(&mut app, Message::PickerOpened);
        send(&mut app, Message::PickerHexEdited("#777777".into()));
        send(&mut app, Message::PickerConfirmed);
        assert_eq!(app.config.custom_colours.len(), 6);
    }

    #[test]
    fn a_custom_colour_can_be_picked_again_from_the_row() {
        let mut app = app(60, 60);
        send(&mut app, Message::PickerOpened);
        send(&mut app, Message::PickerHexEdited("#0064b6".into()));
        send(&mut app, Message::PickerConfirmed);
        send(&mut app, Message::ColourPicked(0));
        assert_ne!(app.brush.colour, [0, 100, 182, 255]);

        send(&mut app, Message::CustomColourPicked(0));
        assert_eq!(app.brush.colour, [0, 100, 182, 255]);
    }

    #[test]
    fn the_tabs_drop_their_labels_when_there_is_no_room() {
        let mut app = app(64, 48);
        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));
        assert!(app.tabs_fit(), "a normal window has room for the labels");

        send(&mut app, Message::WindowResized(Size::new(500.0, 800.0)));
        assert!(!app.tabs_fit(), "a narrow one does not");

        let strip = (crate::ui::sidebar::TABS.len() + 1) as f32 * metrics::TOP_PANEL_BUTTON_WIDTH
            + 3.0 * metrics::TOP_PANEL_THIN_BUTTON_WIDTH;
        send(&mut app, Message::WindowResized(Size::new(strip, 800.0)));
        assert!(app.tabs_fit(), "and exactly enough room is enough");
    }

    #[test]
    fn our_own_title_bar_costs_the_canvas_its_height() {
        let mut app = app(64, 48);
        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));
        let borderless = app.viewport.height;

        app.config.decorations = true;
        app.resync_viewport();
        assert_eq!(
            app.viewport.height - borderless,
            crate::ui::titlebar::HEIGHT,
            "the canvas gets the bar's height back when the compositor draws one"
        );
    }

    #[test]
    fn the_system_title_bar_is_a_setting() {
        let mut app = app(64, 48);
        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));
        let ours = app.viewport.height;

        send(&mut app, Message::DecorationsToggled(true));
        assert!(app.config.decorations, "the setting took");
        assert_eq!(
            app.viewport.height - ours,
            crate::ui::titlebar::HEIGHT,
            "and the canvas gets our bar's height back"
        );

        send(&mut app, Message::DecorationsToggled(false));
        assert!(!app.config.decorations);
        assert_eq!(app.viewport.height, ours);
    }

    #[test]
    fn the_new_shortcuts_reach_the_right_messages() {
        use iced::keyboard::{Event, Key, Modifiers, key::Named};

        let press = |key: Key, modifiers: Modifiers| {
            shortcut(Event::KeyPressed {
                key: key.clone(),
                modified_key: key.clone(),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
            .map(|m| format!("{m:?}"))
        };

        assert_eq!(
            press(Key::Character("n".into()), Modifiers::CTRL),
            Some("NewRequested".into())
        );
        assert_eq!(
            press(Key::Named(Named::Delete), Modifiers::empty()),
            Some("DeleteFloating".into())
        );
        assert_eq!(
            press(Key::Character("]".into()), Modifiers::empty()),
            Some("ThicknessNudged(1.0)".into())
        );
        assert_eq!(
            press(Key::Character("[".into()), Modifiers::empty()),
            Some("ThicknessNudged(-1.0)".into())
        );
        assert_eq!(press(Key::Character("n".into()), Modifiers::empty()), None);
    }

    #[test]
    fn the_clipboard_shortcuts_work_inside_a_text_box() {
        use iced::keyboard::{Event, Key, Modifiers};

        let press = |character: &str, modifiers: Modifiers| {
            let key = Key::Character(character.into());
            typing(Event::KeyPressed {
                key: key.clone(),
                modified_key: key.clone(),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: Some(character.into()),
                repeat: false,
            })
            .map(|m| format!("{m:?}"))
        };

        assert_eq!(press("v", Modifiers::CTRL), Some("Paste".into()));
        assert_eq!(press("c", Modifiers::CTRL), Some("Copy".into()));
        assert_eq!(press("x", Modifiers::CTRL), Some("Cut".into()));
        assert_eq!(
            press("v", Modifiers::empty()),
            Some("TextEdited(Insert('v'))".into())
        );
    }

    #[test]
    fn delete_throws_a_selection_away_and_leaves_the_hole() {
        let mut app = app(60, 60);
        send(&mut app, Message::ToolPicked(Tool::Fill));
        click(&mut app, 30.0, 30.0);
        send(&mut app, Message::ToolPicked(Tool::Select));
        drag_selection(&mut app, (10.0, 10.0), (40.0, 40.0));
        assert!(app.floating.is_some());

        send(&mut app, Message::DeleteFloating);
        assert!(app.floating.is_none());
        assert_eq!(pixel(&app, 25, 25)[3], 0, "the hole stays a hole");
        assert_eq!(pixel(&app, 55, 55)[3], 255, "and the rest is untouched");
    }

    #[test]
    fn the_bracket_keys_move_whichever_thickness_is_in_use() {
        let mut app = app(60, 60);
        let brush = app.brush.thickness;
        send(&mut app, Message::ThicknessNudged(1.0));
        assert_eq!(app.brush.thickness, brush + 1.0);

        send(&mut app, Message::TabPicked(Tab::Shapes));
        let shape = app.shape_style.thickness;
        send(&mut app, Message::ThicknessNudged(1.0));
        assert_eq!(app.shape_style.thickness, shape + 1.0);
        assert_eq!(app.brush.thickness, brush + 1.0, "the brush was left alone");

        for _ in 0..500 {
            send(&mut app, Message::ThicknessNudged(-1.0));
        }
        assert_eq!(app.shape_style.thickness, shapes::MIN_THICKNESS);
    }

    #[test]
    fn the_theme_choice_is_kept_and_resolved() {
        let mut app = app(64, 48);
        send(&mut app, Message::ThemePicked(Choice::Dark));
        assert_eq!(app.config.theme, Choice::Dark);
        send(&mut app, Message::AccentPicked(Scheme::Classic));
        assert_eq!(app.config.accent, Scheme::Classic);

        assert_eq!(Choice::Dark.resolve(), theme::Mode::Dark);
        assert_eq!(Choice::Light.resolve(), theme::Mode::Light);
    }

    #[test]
    fn a_test_can_never_reach_the_real_settings_file() {
        let mut app = app(64, 48);
        assert!(app.config_path.is_none());
        app.save_config();
        assert!(app.status.is_empty(), "and it does not complain about it");
    }

    #[test]
    fn new_on_a_modified_document_waits_for_an_answer() {
        let mut app = app(64, 48);
        click(&mut app, 10.0, 10.0);
        assert!(app.doc.modified);
        let size = app.doc.size();

        send(&mut app, Message::NewRequested);
        assert_eq!(app.doc.size(), size, "nothing thrown away yet");

        send(&mut app, Message::NewConfirmed(Discard::Keep));
        assert_eq!(app.doc.size(), size, "cancelling keeps it");

        send(&mut app, Message::NewConfirmed(Discard::Throw));
        assert_eq!(
            app.doc.size(),
            app.new_canvas_size(),
            "and throwing it away starts over"
        );
    }

    fn start_text_box(app: &mut App) {
        send(app, Message::TabPicked(Tab::Text));
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectBegan(4.0, 4.0)),
        );
        send(
            app,
            Message::Canvas(gpu::Interaction::SelectMoved(60.0, 30.0)),
        );
        send(app, Message::Canvas(gpu::Interaction::SelectEnded));
    }

    fn text_in_hand(app: &mut App) -> Option<String> {
        Some(app.floating.as_mut()?.text_box()?.content())
    }

    #[test]
    fn pasted_text_lands_in_the_box_being_typed_into() {
        let mut app = app(200, 120);
        start_text_box(&mut app);
        for c in "ab".chars() {
            send(&mut app, Message::TextEdited(TextAction::Insert(c)));
        }

        send(&mut app, Message::Pasted(Some(Clip::Text("cd".into()))));
        assert_eq!(text_in_hand(&mut app).as_deref(), Some("abcd"));
        assert!(app.typing(), "and the box is still being typed into");
    }

    #[test]
    fn pasted_text_with_no_box_open_makes_one() {
        let mut app = app(400, 300);
        send(&mut app, Message::WindowResized(Size::new(1280.0, 800.0)));
        assert!(app.floating.is_none());

        send(
            &mut app,
            Message::Pasted(Some(Clip::Text("hello there".into()))),
        );
        assert_eq!(text_in_hand(&mut app).as_deref(), Some("hello there"));
        assert!(app.typing(), "with the caret in it, ready to be typed into");
        assert_eq!(
            app.tab,
            Tab::Text,
            "and the text panel open, since that is what is in hand"
        );
        assert_eq!(app.brush.tool, Tool::Text);

        let xform = app.floating.as_ref().unwrap().xform;
        assert!(
            xform.width > 0.0 && xform.width < 400.0,
            "box is {} wide",
            xform.width
        );
        assert!(
            xform.height > 0.0 && xform.height < 300.0,
            "box is {} tall",
            xform.height
        );
    }

    #[test]
    fn pasted_pixels_still_float_over_the_canvas() {
        let mut app = app(64, 64);
        send(
            &mut app,
            Message::Pasted(Some(Clip::Image(Rgba8::new(8, 8, BLUE)))),
        );
        assert!(app.floating.is_some());
        assert!(
            text_in_hand(&mut app).is_none(),
            "an image is not a text box"
        );
    }

    #[test]
    fn opening_over_changes_asks_before_replacing_the_document() {
        let mut app = app(64, 48);
        click(&mut app, 10.0, 10.0);
        assert!(app.unsaved());

        send(&mut app, Message::OpenRequested);
        assert!(
            app.doc.modified,
            "the answer is still coming, nothing has gone"
        );

        send(&mut app, Message::OpenConfirmed(Discard::Keep));
        assert_eq!(app.after_save, None, "cancelling asks for no file at all");

        send(&mut app, Message::OpenConfirmed(Discard::Save));
        assert_eq!(
            app.after_save,
            Some(Pending::Open),
            "the open waits for the save"
        );

        send(&mut app, Message::Saved(Ok(PathBuf::from("/tmp/x.png"))));
        assert_eq!(app.after_save, None, "and is asked for once the save lands");
    }

    #[test]
    fn opening_a_file_lets_go_of_what_was_floating() {
        let mut app = app(32, 32);
        send(
            &mut app,
            Message::Pasted(Some(Clip::Image(Rgba8::new(8, 8, BLUE)))),
        );
        assert!(app.floating.is_some());

        let opened = Rgba8::new(40, 20, RED);
        send(
            &mut app,
            Message::Opened(Ok((PathBuf::from("/tmp/y.png"), opened))),
        );
        assert_eq!(app.doc.size(), (40, 20));
        assert!(
            app.floating.is_none(),
            "the old document's paste came with it"
        );
        assert!(app.grab.is_none());
    }

    #[test]
    fn the_size_readout_follows_the_selection_being_dragged_out() {
        let mut app = app(100, 80);
        send(&mut app, Message::FreeformToggled(false));
        assert_eq!(app.readout(), None, "nothing to measure yet");

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectBegan(10.0, 10.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(40.0, 30.0)),
        );
        let (region, from) = app.readout().expect("a readout while it is being dragged");
        assert_eq!(
            (region.width(), region.height()),
            (30, 20),
            "in canvas pixels"
        );
        assert_eq!(from, (40.0, 30.0), "hanging off the point being dragged");

        send(&mut app, Message::Canvas(gpu::Interaction::SelectEnded));
        assert_eq!(app.readout(), None, "and gone once there is a selection");
    }

    #[test]
    fn the_size_readout_is_wide_enough_for_the_numbers_it_shows() {
        let mut face =
            ttf_parser::Face::parse(crate::assets::UI_FONT, 0).expect("the interface font");
        face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), 500.0);
        let per_unit = READOUT_TEXT / face.units_per_em() as f32;
        let advance = |c: char| {
            face.glyph_index(c)
                .and_then(|g| face.glyph_hor_advance(g))
                .map_or(0.0, |a| a as f32 * per_unit)
        };

        let widest = format!("{MAX_CANVAS} px");
        let drawn: f32 = widest.chars().map(advance).sum();
        let allowed = widest.len() as f32 * READOUT_GLYPH;
        assert!(
            drawn <= allowed,
            "\"{widest}\" wants {drawn}, the panel gives it {allowed}"
        );
        assert!(
            drawn > allowed * 0.6,
            "the panel is far wider than it needs: {drawn} of {allowed}"
        );

        for label in ["W:", "H:"] {
            let wide: f32 = label.chars().map(advance).sum();
            assert!(
                wide < READOUT_LABEL,
                "\"{label}\" is {wide} wide, its column is {READOUT_LABEL}"
            );
        }
    }

    #[test]
    fn the_size_readout_stays_inside_the_window() {
        let size = Size::new(80.0, 40.0);
        let bounds = Size::new(600.0, 400.0);

        let loose = readout_origin(Point::new(100.0, 100.0), size, bounds);
        assert_eq!(
            loose,
            Point::new(116.0, 112.0),
            "down and right of the pointer"
        );

        let corner = readout_origin(Point::new(596.0, 397.0), size, bounds);
        assert!(
            corner.x + size.width <= bounds.width,
            "ran off the right: {corner:?}"
        );
        assert!(
            corner.y + size.height <= bounds.height,
            "ran off the bottom: {corner:?}"
        );
        assert!(
            corner.x < 596.0 && corner.y < 397.0,
            "and it is behind the pointer"
        );
    }

    #[test]
    fn closing_with_changes_asks_before_the_window_goes() {
        let mut app = app(64, 48);
        send(&mut app, Message::WindowClosed);
        assert!(
            app.after_save.is_none(),
            "an untouched canvas closes without a word"
        );

        click(&mut app, 10.0, 10.0);
        assert!(app.unsaved());
        send(&mut app, Message::WindowClosed);
        assert!(
            app.doc.modified,
            "nothing has happened yet, the answer is still coming"
        );

        send(&mut app, Message::CloseConfirmed(Discard::Keep));
        assert!(app.doc.modified, "cancelling leaves it alone");
        assert_eq!(app.after_save, None, "and nothing is waiting on a save");

        send(&mut app, Message::CloseConfirmed(Discard::Save));
        assert_eq!(
            app.after_save,
            Some(Pending::Close),
            "the close waits for the save"
        );

        send(&mut app, Message::Saved(Err(String::new())));
        assert_eq!(app.after_save, None, "the close is off");
        assert!(app.doc.modified, "and the work is still here");
    }

    #[test]
    fn a_text_box_in_hand_is_unsaved_work() {
        let mut app = app(64, 48);
        send(&mut app, Message::TabPicked(Tab::Text));
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectBegan(4.0, 4.0)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::SelectMoved(40.0, 20.0)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::SelectEnded));
        send(&mut app, Message::TextEdited(TextAction::Insert('h')));

        assert!(!app.doc.modified, "the canvas itself is untouched");
        assert!(app.unsaved(), "but there is a box with a letter in it");
    }

    #[test]
    fn a_save_takes_what_is_floating_with_it_without_putting_it_down() {
        let mut app = app(32, 32);
        fill_canvas(&mut app, RED);
        send(
            &mut app,
            Message::Pasted(Some(Clip::Image(Rgba8::new(8, 8, BLUE)))),
        );
        assert!(app.floating.is_some());

        let written = app.for_saving();
        let (w, h) = app.doc.size();
        let middle = |image: &Rgba8, x: u32, y: u32| {
            let i = ((y * image.size().0 + x) * 4) as usize;
            let b = image.as_bytes();
            [b[i], b[i + 1], b[i + 2], b[i + 3]]
        };
        assert_eq!(written.size(), (w, h));
        assert_eq!(
            middle(&written, w / 2, h / 2),
            BLUE,
            "the paste is in the file"
        );
        assert_eq!(middle(&written, 1, 1), RED, "and the canvas around it");
        assert!(app.floating.is_some(), "and it is still in hand afterwards");
    }

    #[test]
    fn saving_from_the_new_dialog_then_starts_the_new_canvas() {
        let mut app = app(64, 48);
        click(&mut app, 10.0, 10.0);
        send(&mut app, Message::NewRequested);
        send(&mut app, Message::NewConfirmed(Discard::Save));
        assert_eq!(app.after_save, Some(Pending::Blank));

        send(&mut app, Message::Saved(Ok(PathBuf::from("/tmp/x.png"))));
        assert_eq!(
            app.doc.size(),
            app.new_canvas_size(),
            "the new canvas arrived"
        );
        assert_eq!(app.after_save, None);
    }

    #[test]
    fn acrylic_can_be_changed_from_settings() {
        let mut app = app(200, 200);

        send(&mut app, Message::AcrylicToggled(false));
        assert!(!app.config.acrylic);
        assert_eq!(theme::veiled(iced::Color::BLACK).a, 1.0);

        send(&mut app, Message::AcrylicToggled(true));
        assert!(app.config.acrylic);
        assert!(theme::veiled(iced::Color::BLACK).a < 1.0);
    }

    fn ink(floating: &Floating) -> usize {
        floating
            .pixels
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 8)
            .count()
    }

    #[test]
    fn every_tab_in_the_strip_either_opens_a_panel_or_is_inert() {
        let tabs = crate::ui::sidebar::TABS;
        let built: Vec<_> = tabs.iter().filter(|(_, _, t)| t.is_some()).collect();
        assert_eq!(tabs.len(), 5);
        assert_eq!(
            built.len(),
            tabs.len(),
            "every tab in the strip has a panel"
        );
        assert!(
            !tabs.iter().any(|(label, _, _)| label.contains("3D")),
            "nothing 3D belongs in the strip"
        );
    }

    #[test]
    fn the_panel_resizes_the_canvas_without_scaling_the_image() {
        let mut app = app(100, 100);
        send(&mut app, Message::LockAspectToggled(false));
        resize_to(&mut app, "200", "150");
        assert_eq!(app.doc.size(), (200, 150));
        assert_eq!(app.doc.pixels().as_bytes().len(), 200 * 150 * 4);
    }

    #[test]
    fn resize_image_with_canvas_scales_instead() {
        let mut app = app(100, 100);
        send(&mut app, Message::ResizeImageToggled(true));
        send(&mut app, Message::LockAspectToggled(false));
        resize_to(&mut app, "50", "50");
        assert_eq!(app.doc.size(), (50, 50));
    }

    #[test]
    fn locking_the_aspect_ratio_fills_in_the_other_field() {
        let mut app = app(200, 100);
        assert!(app.panel.lock_aspect, "on by default, as in Paint 3D");
        send(&mut app, Message::CanvasWidthEdited("400".into()));
        assert_eq!(app.panel.height, "200", "height should follow width");

        send(&mut app, Message::CanvasHeightEdited("50".into()));
        assert_eq!(app.panel.width, "100");
    }

    #[test]
    fn percent_mode_resizes_relative_to_the_current_size() {
        let mut app = app(200, 100);
        send(&mut app, Message::CanvasUnitPicked(true));
        assert_eq!(app.panel.width, "100", "percent mode starts at 100%");
        resize_to(&mut app, "50", "50");
        assert_eq!(app.doc.size(), (100, 50));
    }

    #[test]
    fn the_fields_follow_the_document_after_every_change() {
        let mut app = app(200, 100);
        send(&mut app, Message::Rotate(true));
        assert_eq!(app.doc.size(), (100, 200));
        assert_eq!(
            (app.panel.width.as_str(), app.panel.height.as_str()),
            ("100", "200")
        );

        send(&mut app, Message::Undo);
        assert_eq!(app.doc.size(), (200, 100));
        assert_eq!(
            (app.panel.width.as_str(), app.panel.height.as_str()),
            ("200", "100")
        );
    }

    #[test]
    fn rotating_and_flipping_go_through_history_as_one_step_each() {
        let mut app = app(30, 10);
        send(&mut app, Message::Rotate(false));
        send(&mut app, Message::Flip(true));
        assert_eq!(app.doc.size(), (10, 30));

        send(&mut app, Message::Undo);
        send(&mut app, Message::Undo);
        assert_eq!(app.doc.size(), (30, 10));
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn turning_transparency_off_flattens_and_can_be_undone() {
        let mut app = app(4, 4);
        send(&mut app, Message::TransparencyToggled(true));
        assert!(app.doc.transparent);

        send(&mut app, Message::TransparencyToggled(false));
        assert!(!app.doc.transparent);

        send(&mut app, Message::Undo);
        assert!(
            app.doc.transparent,
            "undo should restore the flag, not just the pixels"
        );
    }

    #[test]
    fn a_resize_drag_applies_once_at_the_end() {
        let mut app = app(100, 100);
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::ResizePreview(150, 100)),
        );
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::ResizePreview(180, 100)),
        );
        assert_eq!(
            app.doc.size(),
            (100, 100),
            "preview must not touch the document"
        );
        assert!(!app.doc.can_undo());

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::Resized(180, 100, Handle::Right)),
        );
        assert_eq!(app.doc.size(), (180, 100));
        assert_eq!(app.resize_preview, None);

        send(&mut app, Message::Undo);
        assert_eq!(app.doc.size(), (100, 100));
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn a_cancelled_drag_changes_nothing() {
        let mut app = app(100, 100);
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::ResizePreview(400, 400)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::ResizeCancelled));
        assert_eq!(app.doc.size(), (100, 100));
        assert_eq!(app.resize_preview, None);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn dragging_the_left_edge_keeps_the_image_against_the_right() {
        let mut app = app(2, 1);
        app.doc.edit().pixels_mut()[0..4].copy_from_slice(&[255, 0, 0, 255]);

        send(
            &mut app,
            Message::Canvas(gpu::Interaction::Resized(4, 1, Handle::Left)),
        );
        assert_eq!(app.doc.size(), (4, 1));
        let bytes = app.doc.pixels().as_bytes();
        assert_eq!(
            &bytes[8..12],
            &[255, 0, 0, 255],
            "content should have shifted right"
        );
        assert_eq!(&bytes[0..4], &[0, 0, 0, 0], "new area is empty");
        assert_eq!(&app.doc.flattened().as_bytes()[0..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn transparency_clears_an_untouched_canvas_but_spares_painted_white() {
        let mut app = app(4, 1);
        app.brush = Brush {
            tool: Tool::PixelPen,
            thickness: 1.0,
            opacity: 1.0,
            colour: [255, 255, 255, 255],
            ..Default::default()
        };
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PaintBegan(0.5, 0.5)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::PaintEnded));

        assert!(app.doc.has_backing());
        send(&mut app, Message::TransparencyToggled(true));

        let bytes = app.doc.pixels().as_bytes();
        assert_eq!(&bytes[0..4], &[255, 255, 255, 255], "painted white stays");
        assert_eq!(
            &bytes[12..16],
            &[0, 0, 0, 0],
            "untouched canvas goes see-through"
        );
    }

    #[test]
    fn the_transparency_toggle_loses_nothing_either_way() {
        let mut app = app(4, 4);
        app.brush = Brush {
            tool: Tool::PixelPen,
            thickness: 1.0,
            opacity: 1.0,
            colour: [10, 20, 30, 255],
            ..Default::default()
        };
        send(
            &mut app,
            Message::Canvas(gpu::Interaction::PaintBegan(2.5, 2.5)),
        );
        send(&mut app, Message::Canvas(gpu::Interaction::PaintEnded));
        let painted = app.doc.pixels().clone();

        send(&mut app, Message::TransparencyToggled(true));
        send(&mut app, Message::TransparencyToggled(false));
        assert_eq!(
            app.doc.pixels().as_bytes(),
            painted.as_bytes(),
            "round trip must be lossless"
        );
    }

    #[test]
    fn nonsense_in_the_size_fields_is_ignored() {
        let mut app = app(100, 100);
        send(&mut app, Message::LockAspectToggled(false));
        resize_to(&mut app, "banana", "12");
        assert_eq!(
            app.doc.size(),
            (100, 100),
            "a bad field should not resize anything"
        );

        resize_to(&mut app, "0", "0");
        assert_eq!(
            app.doc.size(),
            (1, 1),
            "zero clamps to the smallest real canvas"
        );
    }
}
