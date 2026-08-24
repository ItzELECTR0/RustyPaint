use crate::doc::Rgba8;
use cosmic_text::{
    Attrs, Buffer, Change, Edit, Editor, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use std::sync::{LazyLock, Mutex};

pub use cosmic_text::Action;

static FONTS: LazyLock<Mutex<(FontSystem, SwashCache)>> =
    LazyLock::new(|| Mutex::new((FontSystem::new(), SwashCache::new())));

pub static FAMILIES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let guard = FONTS.lock().expect("font system");
    let mut names: Vec<String> = guard
        .0
        .db()
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
});

pub const SIZES: &[u32] = &[
    8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 40, 48, 56, 64, 72, 96, 128,
];

pub const DEFAULT_SIZE: f32 = 48.0;

pub const DEFAULT_FAMILY: &str = "sans-serif";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

impl Align {
    fn to_cosmic(self) -> cosmic_text::Align {
        match self {
            Align::Left => cosmic_text::Align::Left,
            Align::Centre => cosmic_text::Align::Center,
            Align::Right => cosmic_text::Align::Right,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub colour: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: Align,
    pub background: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: DEFAULT_FAMILY.into(),
            size: DEFAULT_SIZE,
            colour: [0, 0, 0, 255],
            bold: false,
            italic: false,
            underline: false,
            align: Align::Left,
            background: false,
        }
    }
}

impl TextStyle {
    pub fn slack(&self) -> f32 {
        (self.size * 0.25).ceil().clamp(2.0, 64.0)
    }

    pub fn line_height(&self) -> f32 {
        self.size * 1.2
    }

    fn attrs(&self) -> Attrs<'_> {
        let mut attrs = Attrs::new()
            .family(Family::Name(&self.family))
            .color(colour_of(self.colour))
            .metrics(Metrics::new(self.size, self.line_height()))
            .metadata(if self.underline { UNDERLINE } else { 0 });
        if self.bold {
            attrs = attrs.weight(cosmic_text::Weight::BOLD);
        }
        if self.italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }
        attrs
    }
}

const UNDERLINE: usize = 1;

const BACKGROUND: [u8; 4] = [255, 255, 255, 255];

const SELECTION: [u8; 4] = [0, 99, 177, 255];
const SELECTED_TEXT: [u8; 4] = [255, 255, 255, 255];

#[derive(Clone)]
struct TextState {
    editor: Editor<'static>,
    style: TextStyle,
}

enum TextEdit {
    Change(Change),
    State(TextState),
}

pub struct TextBox {
    editor: Editor<'static>,
    pub style: TextStyle,
    width: f32,
    undo: Vec<TextEdit>,
    redo: Vec<TextState>,
}

impl TextBox {
    pub fn new(style: TextStyle, width: f32) -> Self {
        let metrics = Metrics::new(style.size, style.line_height());
        let mut boxed = Self {
            editor: Editor::new(Buffer::new_empty(metrics)),
            style,
            width: width.max(1.0),
            undo: Vec::new(),
            redo: Vec::new(),
        };
        {
            let mut guard = FONTS.lock().expect("font system");
            let (fonts, _) = &mut *guard;
            let attrs = boxed.style.attrs();
            let align = boxed.style.align.to_cosmic();
            boxed.editor.with_buffer_mut(|buffer| {
                buffer.set_text(fonts, "", &attrs, Shaping::Advanced, Some(align))
            });
        }
        boxed.reshape();
        boxed
    }

    fn state(&self) -> TextState {
        TextState {
            editor: self.editor.clone(),
            style: self.style.clone(),
        }
    }

    fn restore(&mut self, state: TextState) {
        self.editor = state.editor;
        self.style = state.style;
        self.reshape();
    }

    fn begin_edit(&mut self, needs_state: bool) -> Option<TextState> {
        let before = needs_state.then(|| self.state());
        self.editor.start_change();
        before
    }

    fn finish_edit(&mut self, before: Option<TextState>) {
        let Some(change) = self
            .editor
            .finish_change()
            .filter(|change| !change.items.is_empty())
        else {
            return;
        };
        self.undo.push(match before {
            Some(state) => TextEdit::State(state),
            None => TextEdit::Change(change),
        });
        self.redo.clear();
    }

    fn remember(&mut self, before: TextState) {
        self.undo.push(TextEdit::State(before));
        self.redo.clear();
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        let after = self.state();
        let applied = match &edit {
            TextEdit::Change(change) => {
                let mut reversed = change.clone();
                reversed.reverse();
                self.editor.apply_change(&reversed)
            }
            TextEdit::State(before) => {
                self.restore(before.clone());
                true
            }
        };
        if !applied {
            self.undo.push(edit);
            return false;
        }
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        self.editor.shape_as_needed(fonts, false);
        self.redo.push(after);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(after) = self.redo.pop() else {
            return false;
        };
        let before = self.state();
        self.restore(after);
        self.undo.push(TextEdit::State(before));
        true
    }

    pub fn content(&self) -> String {
        self.editor.with_buffer(|buffer| {
            buffer
                .lines
                .iter()
                .map(|line| line.text())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    pub fn is_empty(&self) -> bool {
        self.content().is_empty()
    }

    pub fn insert(&mut self, c: char, style: &TextStyle) {
        let before = self.begin_edit(self.editor.selection_bounds().is_some());
        let start = self
            .editor
            .selection_bounds()
            .map_or(self.editor.cursor(), |(s, _)| s);
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        self.editor.action(fonts, Action::Insert(c));

        let end = self.editor.cursor();
        if end.line != start.line || end.index <= start.index {
            self.editor.shape_as_needed(fonts, false);
            drop(guard);
            self.finish_edit(before);
            return;
        }
        let attrs = style.attrs();
        self.editor.with_buffer_mut(|buffer| {
            let Some(line) = buffer.lines.get_mut(start.line) else {
                return;
            };
            let to = end.index.min(line.text().len());
            let mut list = line.attrs_list().clone();
            list.add_span(start.index..to, &attrs);
            line.set_attrs_list(list);
        });

        self.editor.shape_as_needed(fonts, false);
        drop(guard);
        self.finish_edit(before);
    }

    pub fn insert_str(&mut self, text: &str, style: &TextStyle) {
        if text.is_empty() {
            return;
        }
        let before = self.begin_edit(self.editor.selection_bounds().is_some());
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        let attrs = cosmic_text::AttrsList::new(&style.attrs());
        self.editor.insert_string(text, Some(attrs));
        self.editor.shape_as_needed(fonts, false);
        drop(guard);
        self.finish_edit(before);
    }

    pub fn selected_text(&self) -> Option<String> {
        self.editor.copy_selection()
    }

    pub fn delete_selection(&mut self) -> bool {
        let before = self.begin_edit(true);
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        let deleted = self.editor.delete_selection();
        self.editor.shape_as_needed(fonts, false);
        drop(guard);
        self.finish_edit(before);
        deleted
    }

    pub fn laid_width(&self) -> f32 {
        self.editor.with_buffer(|buffer| {
            buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0, f32::max)
        })
    }

    pub fn adopt(&mut self, style: TextStyle) {
        if self.style == style {
            return;
        }
        let before = self.state();
        let align = style.align.to_cosmic();
        let line_index = self.editor.cursor().line;
        self.style = style;
        self.editor.with_buffer_mut(|buffer| {
            if let Some(line) = buffer.lines.get_mut(line_index) {
                line.set_align(Some(align));
            }
        });
        self.reshape();
        self.remember(before);
    }

    pub fn act(&mut self, action: Action) {
        let records = matches!(
            action,
            Action::Insert(_)
                | Action::Enter
                | Action::Backspace
                | Action::Delete
                | Action::Indent
                | Action::Unindent
        );
        let destructive = matches!(
            action,
            Action::Backspace | Action::Delete | Action::Indent | Action::Unindent
        ) || (matches!(action, Action::Insert(_) | Action::Enter)
            && self.editor.selection_bounds().is_some());
        let before = records.then(|| self.begin_edit(destructive)).flatten();
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        self.editor.action(fonts, action);
        self.editor.shape_as_needed(fonts, false);
        drop(guard);
        if records {
            self.finish_edit(before);
        }
    }

    pub fn set_selecting(&mut self, on: bool) {
        if on {
            if self.editor.selection() == cosmic_text::Selection::None {
                self.editor
                    .set_selection(cosmic_text::Selection::Normal(self.editor.cursor()));
            }
        } else {
            self.editor.set_selection(cosmic_text::Selection::None);
        }
    }

    pub fn select_all(&mut self) {
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        self.editor
            .set_selection(cosmic_text::Selection::Normal(cosmic_text::Cursor::new(
                0, 0,
            )));
        self.editor
            .action(fonts, Action::Motion(cosmic_text::Motion::BufferEnd));
        self.editor.shape_as_needed(fonts, false);
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn set_width(&mut self, width: f32) {
        let width = width.max(1.0);
        if (width - self.width).abs() < 0.5 {
            return;
        }
        self.width = width;
        self.reshape();
    }

    fn reshape(&mut self) {
        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        let metrics = Metrics::new(self.style.size, self.style.line_height());
        let align = self.style.align.to_cosmic();
        self.editor.with_buffer_mut(|buffer| {
            buffer.set_metrics_and_size(fonts, metrics, Some(self.width), None);
            buffer.set_wrap(fonts, cosmic_text::Wrap::WordOrGlyph);
            for line in &mut buffer.lines {
                if line.align().is_none() {
                    line.set_align(Some(align));
                }
            }
        });
        self.editor.shape_as_needed(fonts, false);
    }

    pub fn style_selection(&mut self, style: &TextStyle) -> bool {
        let Some((start, end)) = self.editor.selection_bounds() else {
            return false;
        };
        let before = self.state();

        let mut guard = FONTS.lock().expect("font system");
        let (fonts, _) = &mut *guard;
        let attrs = style.attrs();
        let align = style.align.to_cosmic();
        self.editor.with_buffer_mut(|buffer| {
            for index in start.line..=end.line {
                let Some(line) = buffer.lines.get_mut(index) else {
                    continue;
                };
                let end_of_line = line.text().len();
                let from = if index == start.line {
                    start.index.min(end_of_line)
                } else {
                    0
                };
                let to = if index == end.line {
                    end.index.min(end_of_line)
                } else {
                    end_of_line
                };
                line.set_align(Some(align));
                if from >= to {
                    continue;
                }
                let mut list = line.attrs_list().clone();
                list.add_span(from..to, &attrs);
                line.set_attrs_list(list);
            }
        });
        self.editor.shape_as_needed(fonts, false);
        drop(guard);
        self.remember(before);
        true
    }

    pub fn restyle_empty(&mut self, style: TextStyle) {
        if self.style == style {
            return;
        }
        let before = self.state();
        let fresh = Self::new(style, self.width);
        self.editor = fresh.editor;
        self.style = fresh.style;
        self.remember(before);
    }

    pub fn slack(&self) -> f32 {
        let largest = self.editor.with_buffer(|buffer| {
            buffer
                .layout_runs()
                .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.font_size))
                .fold(self.style.size, f32::max)
        });
        (largest * 0.25).ceil().clamp(2.0, 64.0)
    }

    pub fn restyled_from(text: &str, style: TextStyle, width: f32) -> Self {
        let mut boxed = Self::new(style, width);
        {
            let mut guard = FONTS.lock().expect("font system");
            let (fonts, _) = &mut *guard;
            let attrs = boxed.style.attrs();
            let align = boxed.style.align.to_cosmic();
            boxed.editor.with_buffer_mut(|buffer| {
                buffer.set_text(fonts, text, &attrs, Shaping::Advanced, Some(align));
            });
            boxed.editor.shape_as_needed(fonts, false);
        }
        boxed.act(Action::Motion(cosmic_text::Motion::BufferEnd));
        boxed
    }

    pub fn height(&self) -> f32 {
        let laid = self
            .editor
            .with_buffer(|buffer| buffer.layout_runs().map(|run| run.line_height).sum::<f32>());
        laid.max(self.style.line_height())
    }

    pub fn render(&self, width: u32, height: u32, editing: bool, caret: bool) -> Option<Rgba8> {
        let (w, h) = (width.max(1), height.max(1));
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        if self.style.background {
            for px in pixels.as_chunks_mut::<4>().0 {
                *px = BACKGROUND;
            }
        }

        let mut guard = FONTS.lock().expect("font system");
        let (fonts, cache) = &mut *guard;

        let hidden = colour_of([0, 0, 0, 0]);
        self.editor.draw(
            fonts,
            cache,
            colour_of(self.style.colour),
            if editing && caret {
                colour_of(self.style.colour)
            } else {
                hidden
            },
            if editing {
                colour_of(SELECTION)
            } else {
                hidden
            },
            if editing {
                colour_of(SELECTED_TEXT)
            } else {
                colour_of(self.style.colour)
            },
            |x, y, rw, rh, colour| {
                blend_rect(
                    &mut Target {
                        pixels: &mut pixels,
                        width: w,
                        height: h,
                    },
                    x,
                    y,
                    rw,
                    rh,
                    colour,
                );
            },
        );

        if self.style.underline {
            self.draw_underline(&mut Target {
                pixels: &mut pixels,
                width: w,
                height: h,
            });
        }

        Rgba8::from_raw(w, h, pixels)
    }

    fn draw_underline(&self, target: &mut Target<'_>) {
        let fallback = colour_of(self.style.colour);
        self.editor.with_buffer(|buffer| {
            for run in buffer.layout_runs() {
                let mut rule: Option<Rule> = None;
                for glyph in run.glyphs {
                    let colour = glyph.color_opt.unwrap_or(fallback);
                    let wanted = glyph.metadata & UNDERLINE != 0;
                    let carry_on = rule
                        .as_ref()
                        .is_some_and(|r| wanted && r.colour == colour && r.size == glyph.font_size);
                    if carry_on {
                        if let Some(r) = &mut rule {
                            r.right = glyph.x + glyph.w;
                        }
                        continue;
                    }
                    if let Some(r) = rule.take() {
                        r.draw(target, run.line_y);
                    }
                    if wanted {
                        rule = Some(Rule {
                            left: glyph.x,
                            right: glyph.x + glyph.w,
                            size: glyph.font_size,
                            colour,
                        });
                    }
                }
                if let Some(r) = rule.take() {
                    r.draw(target, run.line_y);
                }
            }
        });
    }
}

struct Rule {
    left: f32,
    right: f32,
    size: f32,
    colour: cosmic_text::Color,
}

impl Rule {
    fn draw(&self, target: &mut Target<'_>, baseline: f32) {
        let thickness = (self.size / 16.0).round().max(1.0) as u32;
        let y = baseline + self.size * 0.12;
        let width = (self.right - self.left).round().max(1.0) as u32;
        blend_rect(
            target,
            self.left.round() as i32,
            y.round() as i32,
            width,
            thickness,
            self.colour,
        );
    }
}

fn colour_of([r, g, b, a]: [u8; 4]) -> cosmic_text::Color {
    cosmic_text::Color::rgba(r, g, b, a)
}

struct Target<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
}

fn blend_rect(
    target: &mut Target<'_>,
    x: i32,
    y: i32,
    rw: u32,
    rh: u32,
    colour: cosmic_text::Color,
) {
    let (pixels, w, h) = (&mut *target.pixels, target.width, target.height);
    let alpha = colour.a() as u32;
    if alpha == 0 {
        return;
    }
    let src = [colour.r(), colour.g(), colour.b()];
    for row in 0..rh as i32 {
        let py = y + row;
        if py < 0 || py >= h as i32 {
            continue;
        }
        for col in 0..rw as i32 {
            let px = x + col;
            if px < 0 || px >= w as i32 {
                continue;
            }
            let i = (py as usize * w as usize + px as usize) * 4;
            let under = u32::from(pixels[i + 3]);
            let out = alpha + under * (255 - alpha) / 255;
            if out == 0 {
                continue;
            }
            for c in 0..3 {
                let s = u32::from(src[c]) * alpha;
                let u = u32::from(pixels[i + c]) * under * (255 - alpha) / 255;
                pixels[i + c] = ((s + u) / out) as u8;
            }
            pixels[i + 3] = out as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(pixels: &Rgba8) -> usize {
        pixels
            .as_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 8)
            .count()
    }

    fn typed(text: &str, style: TextStyle, width: f32) -> TextBox {
        TextBox::restyled_from(text, style, width)
    }

    #[test]
    fn a_selected_part_takes_the_style_on_its_own() {
        use cosmic_text::Motion;

        let mut boxed = TextBox::restyled_from("AB", TextStyle::default(), 400.0);
        boxed.act(Action::Motion(Motion::BufferEnd));
        boxed.set_selecting(true);
        boxed.act(Action::Motion(Motion::Left));

        let red = TextStyle {
            colour: [255, 0, 0, 255],
            size: 96.0,
            ..TextStyle::default()
        };
        assert!(
            boxed.style_selection(&red),
            "there was a selection to style"
        );
        let taller = boxed.height();

        let pixels = boxed.render(400, 200, false, false).unwrap();
        let count = |want: [u8; 3]| {
            pixels
                .as_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] == 255 && [p[0], p[1], p[2]] == want)
                .count()
        };
        assert!(
            count([255, 0, 0]) > 20,
            "the selected letter did not go red"
        );
        assert!(count([0, 0, 0]) > 20, "the other one went red as well");

        let plain = TextBox::restyled_from("AB", TextStyle::default(), 400.0);
        assert!(
            taller > plain.height() + 10.0,
            "the line did not grow with the bigger run"
        );
    }

    #[test]
    fn with_nothing_selected_there_is_nothing_to_style() {
        let mut boxed = TextBox::restyled_from("AB", TextStyle::default(), 400.0);
        assert!(!boxed.style_selection(&TextStyle::default()));
    }

    #[test]
    fn typing_puts_the_characters_in() {
        let mut boxed = TextBox::new(TextStyle::default(), 400.0);
        assert!(boxed.is_empty());
        for c in "Hi".chars() {
            boxed.act(Action::Insert(c));
        }
        assert_eq!(boxed.content(), "Hi");
    }

    #[test]
    fn a_paste_goes_in_where_the_caret_is() {
        let style = TextStyle::default();
        let mut boxed = typed("ad", style.clone(), 400.0);
        boxed.act(Action::Motion(cosmic_text::Motion::Left));
        boxed.insert_str("bc", &style);
        assert_eq!(boxed.content(), "abcd");
    }

    #[test]
    fn a_paste_with_line_breaks_in_it_makes_lines() {
        let style = TextStyle::default();
        let mut boxed = TextBox::new(style.clone(), 400.0);
        boxed.insert_str("one\ntwo\nthree", &style);
        assert_eq!(boxed.content(), "one\ntwo\nthree");
    }

    #[test]
    fn a_paste_takes_the_style_the_panel_is_set_to() {
        let plain = TextStyle::default();
        let mut boxed = typed("a", plain.clone(), 400.0);
        let thin = ink(&boxed.render(400, 200, false, false).expect("drawn"));

        let bold = TextStyle {
            bold: true,
            ..plain
        };
        boxed.insert_str("aaaa", &bold);
        let heavy = ink(&boxed.render(400, 200, false, false).expect("drawn"));
        assert!(
            heavy > thin * 4,
            "the pasted run is no heavier: {heavy} against {thin}"
        );
    }

    #[test]
    fn the_selection_can_be_read_out_and_taken_out() {
        let style = TextStyle::default();
        let mut boxed = typed("hello", style, 400.0);
        assert_eq!(boxed.selected_text(), None, "nothing is selected yet");

        boxed.select_all();
        assert_eq!(boxed.selected_text().as_deref(), Some("hello"));
        assert!(boxed.delete_selection());
        assert_eq!(boxed.content(), "");
    }

    #[test]
    fn the_laid_width_is_the_widest_line_not_the_wrap_width() {
        let style = TextStyle::default();
        let boxed = typed("hi", style, 4000.0);
        let width = boxed.laid_width();
        assert!(width > 0.0, "two letters have some width");
        assert!(
            width < 400.0,
            "but nothing like the width they were laid out at: {width}"
        );
    }

    #[test]
    fn backspace_takes_one_back_off() {
        let mut boxed = TextBox::new(TextStyle::default(), 400.0);
        for c in "Hi!".chars() {
            boxed.act(Action::Insert(c));
        }
        boxed.act(Action::Backspace);
        assert_eq!(boxed.content(), "Hi");
    }

    #[test]
    fn undo_restores_deleted_text_with_its_style() {
        let red = TextStyle {
            colour: [255, 0, 0, 255],
            bold: true,
            ..TextStyle::default()
        };
        let mut boxed = TextBox::new(TextStyle::default(), 400.0);
        boxed.insert('A', &red);
        let before = boxed.render(400, 100, false, false).unwrap();

        boxed.act(Action::Backspace);
        assert!(boxed.is_empty());
        assert!(boxed.undo());
        assert_eq!(boxed.content(), "A");
        assert_eq!(
            boxed.render(400, 100, false, false).unwrap().as_bytes(),
            before.as_bytes(),
            "undo restored the run attributes as well as its character"
        );

        assert!(boxed.redo());
        assert!(boxed.is_empty());
    }

    #[test]
    fn an_edit_that_did_nothing_adds_no_undo_step() {
        let mut boxed = TextBox::new(TextStyle::default(), 400.0);
        boxed.act(Action::Backspace);
        assert!(!boxed.undo());
    }

    #[test]
    fn enter_starts_a_second_line() {
        let mut boxed = TextBox::new(TextStyle::default(), 400.0);
        boxed.act(Action::Insert('a'));
        boxed.act(Action::Enter);
        boxed.act(Action::Insert('b'));
        assert_eq!(boxed.content(), "a\nb");
        assert!(
            boxed.height() > boxed.style.line_height(),
            "two lines are taller than one"
        );
    }

    #[test]
    fn a_narrow_box_wraps_rather_than_running_off_the_end() {
        let style = TextStyle {
            size: 16.0,
            ..Default::default()
        };
        let wide = typed(
            "the quick brown fox jumps over the lazy dog",
            style.clone(),
            600.0,
        );
        let narrow = typed("the quick brown fox jumps over the lazy dog", style, 120.0);
        assert!(
            narrow.height() > wide.height(),
            "the narrow box needs more lines"
        );
        assert_eq!(narrow.content(), wide.content());
    }

    #[test]
    fn resizing_rewraps_instead_of_stretching() {
        let style = TextStyle {
            size: 16.0,
            ..Default::default()
        };
        let mut boxed = typed("the quick brown fox jumps over the lazy dog", style, 600.0);
        let wide = boxed.height();
        boxed.set_width(120.0);
        assert!(boxed.height() > wide);
    }

    #[test]
    fn text_is_actually_drawn() {
        let boxed = typed("Writing Test", TextStyle::default(), 400.0);
        let drawn = boxed.render(400, 80, false, false).unwrap();
        assert!(
            ink(&drawn) > 200,
            "the letters should cover a fair few pixels"
        );
    }

    #[test]
    fn an_empty_box_draws_nothing_once_it_is_put_down() {
        let boxed = TextBox::new(TextStyle::default(), 200.0);
        let drawn = boxed.render(200, 60, false, true).unwrap();
        assert_eq!(ink(&drawn), 0, "no caret and no letters");
    }

    #[test]
    fn the_caret_shows_while_editing_and_not_after() {
        let boxed = TextBox::new(TextStyle::default(), 200.0);
        let on = boxed.render(200, 60, true, true).unwrap();
        let off = boxed.render(200, 60, true, false).unwrap();
        assert!(ink(&on) > 0, "the caret is up on this blink");
        assert_eq!(ink(&off), 0, "and down on the next");
    }

    #[test]
    fn background_fill_paints_the_whole_box() {
        let style = TextStyle {
            background: true,
            ..Default::default()
        };
        let boxed = typed("a", style, 100.0);
        let drawn = boxed.render(100, 60, false, false).unwrap();
        assert_eq!(ink(&drawn), 100 * 60, "every pixel is opaque");
        assert_eq!(&drawn.as_bytes()[0..4], &BACKGROUND);
    }

    #[test]
    fn bold_covers_more_than_regular() {
        let plain = typed("Writing", TextStyle::default(), 400.0);
        let bold = typed(
            "Writing",
            TextStyle {
                bold: true,
                ..Default::default()
            },
            400.0,
        );
        let a = ink(&plain.render(400, 80, false, false).unwrap());
        let b = ink(&bold.render(400, 80, false, false).unwrap());
        assert!(b > a, "bold should be heavier: {a} then {b}");
    }

    #[test]
    fn underline_adds_a_rule_below_the_letters() {
        let style = TextStyle {
            underline: true,
            ..Default::default()
        };
        let plain = typed("Writing", TextStyle::default(), 400.0);
        let ruled = typed("Writing", style, 400.0);
        let a = ink(&plain.render(400, 80, false, false).unwrap());
        let b = ink(&ruled.render(400, 80, false, false).unwrap());
        assert!(b > a, "the rule should add ink: {a} then {b}");
    }

    #[test]
    fn alignment_moves_the_letters_across_the_box() {
        let where_ink_starts = |align: Align| {
            let style = TextStyle {
                size: 16.0,
                align,
                ..Default::default()
            };
            let drawn = typed("ab", style, 300.0)
                .render(300, 40, false, false)
                .unwrap();
            let bytes = drawn.as_bytes();
            (0..300).find(|x| (0..40).any(|y| bytes[((y * 300 + x) as usize) * 4 + 3] > 8))
        };
        let left = where_ink_starts(Align::Left).expect("left drew something");
        let centre = where_ink_starts(Align::Centre).expect("centre drew something");
        let right = where_ink_starts(Align::Right).expect("right drew something");
        assert!(left < centre && centre < right, "{left} {centre} {right}");
    }

    #[test]
    fn there_is_at_least_one_font_to_pick() {
        assert!(!FAMILIES.is_empty(), "no fonts installed?");
    }
}
