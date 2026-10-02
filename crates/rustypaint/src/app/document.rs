use crate::doc::layers::Stack;
use crate::doc::{self, Document, Rect, Rgba8};
use crate::i18n;
use crate::paint::fill;

use iced::Task;
use std::path::PathBuf;

use super::*;

pub(super) fn fingerprint(pixels: &Rgba8) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pixels.size().hash(&mut hasher);
    pixels.as_bytes().hash(&mut hasher);
    hasher.finish()
}

pub(super) async fn load(path: PathBuf) -> Result<(PathBuf, Rgba8), String> {
    doc::io::load(&path).map(|pixels| (path, pixels))
}

pub(super) async fn open_file(path: PathBuf) -> Result<(PathBuf, Opening), String> {
    doc::io::open(&path).map(|loaded| (path, Opening(std::sync::Arc::new(loaded))))
}

// Messages are cloned, a whole stack of layers is not worth copying to do it.
#[derive(Clone)]
pub struct Opening(pub std::sync::Arc<doc::io::Loaded>);

impl std::fmt::Debug for Opening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Opening")
    }
}

pub(super) async fn pick_path() -> Result<PathBuf, String> {
    rfd::AsyncFileDialog::new()
        .add_filter(i18n::dialog_images(), doc::io::READABLE)
        .set_title(i18n::open_title())
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
        .ok_or_else(String::new)
}

pub(super) async fn pick_and_load() -> Result<(PathBuf, Rgba8), String> {
    let path = pick_path().await?;
    load(path).await
}

pub(super) async fn pick_and_open() -> Result<(PathBuf, Opening), String> {
    let path = pick_path().await?;
    open_file(path).await
}

pub(super) async fn pick_and_save(
    stack: Stack,
    stem: String,
    format: doc::io::SaveFormat,
) -> Result<PathBuf, String> {
    let handle = rfd::AsyncFileDialog::new()
        .add_filter(format.label(), format.extensions())
        .set_title(i18n::save_as_title())
        .set_file_name(format!("{stem}.{}", format.extension()))
        .save_file()
        .await
        .ok_or_else(String::new)?;

    let path = doc::io::with_extension(handle.path().to_path_buf(), format);
    doc::io::save_document(&stack, &path, format).map(|()| path)
}

pub(super) async fn save_to(
    stack: Stack,
    path: PathBuf,
    format: doc::io::SaveFormat,
) -> Result<PathBuf, String> {
    doc::io::save_document(&stack, &path, format).map(|()| path)
}

impl App {
    pub(super) fn document_name(&self) -> &str {
        self.doc
            .path
            .as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(i18n::untitled())
    }

    // Saving never flattens layers on its own: a layered picture that came from a flat file is
    // offered as an OpenRaster project instead, and Save As can still export a flat copy.
    pub(super) fn save(&mut self) -> Task<Message> {
        match self.save_target() {
            Some((format, path)) => {
                Task::perform(save_to(self.for_saving(), path, format), Message::Saved)
            }
            None => {
                if self.doc.layered() {
                    self.save_format = doc::io::SaveFormat::Ora;
                }
                self.save_as()
            }
        }
    }

    // Where Save can write without asking, if anywhere.
    pub(super) fn save_target(&self) -> Option<(doc::io::SaveFormat, PathBuf)> {
        let path = self.doc.path.clone().filter(|_| !self.doc.merged_only)?;
        let format = doc::io::SaveFormat::from_path(&path)?;
        (format.is_project() || !self.doc.layered()).then_some((format, path))
    }

    pub(super) fn save_as(&self) -> Task<Message> {
        let stem = self
            .doc
            .path
            .as_deref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| i18n::untitled().into());
        let save = pick_and_save(self.for_saving(), stem, self.save_format);
        if self.exporting() {
            Task::perform(save, Message::Exported)
        } else {
            Task::perform(save, Message::Saved)
        }
    }

    // A flat format chosen for layered work writes a copy and leaves the tab as it is.
    pub(super) fn exporting(&self) -> bool {
        self.doc.layered() && !self.save_format.is_project()
    }

    // Recovery keeps the document as it stands, backing and layers and all.
    pub(super) fn for_recovery(&self) -> Stack {
        with_live(&self.doc, self.floating.as_ref())
    }

    pub(super) fn snapshot_parked(&self, sheet: &Sheet) -> Task<Message> {
        let Some(root) = self.recovery.clone() else {
            return Task::none();
        };
        if !sheet.unsaved() {
            return Task::none();
        }
        let (id, slot) = (self.session.clone(), sheet.slot.clone());
        let stack = sheet.for_recovery();
        Task::perform(
            async move { doc::recovery::write_document(&root, &id, &slot, &stack) },
            Message::ParkedSnapshotted,
        )
    }

    // The index is the tab order, so it is rewritten whenever that order changes.
    pub(super) fn record_session(&self) {
        let Some(root) = &self.recovery else {
            return;
        };
        let open: Vec<doc::recovery::Open> = (0..self.sheets())
            .map(|tab| match self.parked_at(tab) {
                None => doc::recovery::Open {
                    slot: self.slot.clone(),
                    path: self.doc.path.clone(),
                    transparent: self.doc.transparent(),
                    unsaved: self.unsaved(),
                    merged_only: self.doc.merged_only,
                },
                Some(i) => {
                    let sheet = &self.parked[i];
                    doc::recovery::Open {
                        slot: sheet.slot.clone(),
                        path: sheet.doc.path.clone(),
                        transparent: sheet.doc.transparent(),
                        unsaved: sheet.unsaved(),
                        merged_only: sheet.doc.merged_only,
                    }
                }
            })
            .collect();
        let _ = doc::recovery::write_index(root, &self.session, &open, self.active);
    }

    pub(super) fn snapshot(&mut self) -> Task<Message> {
        let Some(root) = self.recovery.clone() else {
            return Task::none();
        };
        // The index says which pictures were at risk, so it is rewritten the moment that changes.
        if self.recorded_unsaved != self.unsaved() {
            self.recorded_unsaved = self.unsaved();
            self.record_session();
        }

        let at = (self.doc.revision(), self.float_version);
        if !self.unsaved() || self.snapshotted == Some(at) || self.snapshotting {
            return Task::none();
        }
        // The encoder shares the canvas, so the stroke's next stamp would have to copy all of it.
        if self.stroke.is_some() {
            return Task::none();
        }
        if self.last_snapshot.elapsed() < doc::recovery::SNAPSHOT_GAP {
            return Task::none();
        }

        self.snapshotting = true;
        self.last_snapshot = Instant::now();
        let (id, slot) = (self.session.clone(), self.slot.clone());
        let stack = self.for_recovery();
        Task::perform(
            async move { doc::recovery::write_document(&root, &id, &slot, &stack) },
            move |result| Message::Snapshotted(at, result),
        )
    }

    // The whole session goes: work thrown away on purpose is not work to restore. Recovery is put
    // down with it so nothing on the way out writes the session back.
    pub(super) fn forget_session(&mut self) {
        if let Some(root) = self.recovery.take() {
            doc::recovery::clear(&root, &self.session);
        }
        self.snapshotted = None;
    }

    // A blank document nobody has touched is a slot rather than work, so a file takes it over.
    pub(super) fn open_document(
        &mut self,
        doc: Document,
        format: doc::io::SaveFormat,
    ) -> Task<Message> {
        if self.untouched() {
            self.save_format = format;
            self.adopt_document(doc);
            return Task::none();
        }
        let sheet = self.new_sheet(doc, format);
        self.add_sheet(sheet)
    }

    pub(super) fn add_sheet(&mut self, sheet: Sheet) -> Task<Message> {
        let cutout = self.finish_cutout_stroke();
        let mut sheets = self.collapse();
        let leaving = self.snapshot_parked(&sheets[self.active.min(sheets.len() - 1)]);
        let at = sheets.len();
        sheets.push(sheet);
        self.expand(sheets, at);
        self.record_session();
        Task::batch([leaving, cutout])
    }

    pub(super) fn adopt_document(&mut self, doc: Document) {
        self.cutting_out = None;
        self.doc = doc;
        self.floating = None;
        self.live_redo = None;
        self.grab = None;
        self.grab_from = None;
        self.float_version += 1;
        self.view = self.fitted(self.doc.size());
        self.damage.clear();
        self.panel.sync(self.doc.size());
        self.status.clear();
        self.menu = None;
    }

    pub(super) fn switch_to(&mut self, tab: usize) -> Task<Message> {
        if tab == self.active || tab >= self.sheets() {
            return Task::none();
        }
        let cutout = self.finish_cutout_stroke();
        let sheets = self.collapse();
        let leaving = self.snapshot_parked(&sheets[self.active.min(sheets.len() - 1)]);
        self.expand(sheets, tab);
        self.record_session();
        Task::batch([leaving, cutout, self.resume_cutout()])
    }

    fn finish_cutout_stroke(&mut self) -> Task<Message> {
        if self.cutting_out.as_ref().is_some_and(|c| c.painting) {
            if let Some(c) = &mut self.cutting_out {
                c.painting = false;
                c.previous = None;
            }
            self.run_cutout()
        } else {
            Task::none()
        }
    }

    pub(super) fn close_tab(&mut self) -> Task<Message> {
        let mut sheets = self.collapse();
        if sheets.len() < 2 {
            self.expand(sheets, self.active);
            return Task::none();
        }
        sheets.remove(self.active);
        let next = self.active.min(sheets.len() - 1);
        self.expand(sheets, next);
        self.record_session();
        self.resume_cutout()
    }

    pub(super) fn elsewhere(&self, path: Option<&std::path::Path>) -> Task<Message> {
        let Ok(exe) = std::env::current_exe() else {
            return Task::none();
        };
        let mut command = std::process::Command::new(exe);
        if let Some(path) = path {
            command.arg(path);
        }
        match command.spawn() {
            Ok(child) => {
                std::thread::spawn(move || drop(child.wait_with_output()));
                Task::none()
            }
            Err(e) => Task::done(Message::ParkedSnapshotted(Err(
                i18n::error_cannot_open_window(&e.to_string()),
            ))),
        }
    }

    pub(super) fn restore(&mut self, session: doc::recovery::Session) -> Task<Message> {
        let mut tasks = Vec::new();
        let active = session.active;
        for one in session.documents {
            let format = one
                .path
                .as_deref()
                .and_then(doc::io::SaveFormat::from_path)
                .unwrap_or_default();
            let mut doc = if one.unsaved {
                Document::recovered(one.stack, one.path)
            } else {
                Document::from_stack(one.stack, one.path)
            };
            doc.merged_only = one.merged_only;
            tasks.push(self.open_document(doc, format));
        }
        tasks.push(self.switch_to(active));
        Task::batch(tasks)
    }

    pub(super) fn for_saving(&self) -> Stack {
        with_live(&self.doc, self.floating.as_ref())
    }

    pub(super) fn discarding(&mut self, pending: Pending) -> Task<Message> {
        let dirty = match pending {
            Pending::Close => self.any_unsaved(),
            _ => self.unsaved(),
        };
        if dirty && self.config.confirm_discard {
            self.asking = Some(pending);
            return Task::none();
        }
        self.carry_on(pending)
    }

    pub(super) fn carry_on(&mut self, pending: Pending) -> Task<Message> {
        match pending {
            Pending::Close => iced::exit(),
            Pending::Tab => self.close_tab(),
        }
    }

    pub(super) fn unsaved(&self) -> bool {
        self.doc.modified() || self.floating.is_some()
    }

    pub(super) fn untouched(&self) -> bool {
        !self.doc.modified() && self.doc.path.is_none() && !self.doc.can_undo()
    }

    pub(super) fn selection_rect(&self) -> Option<Rect> {
        self.floating.as_ref()?.xform.bounds(self.doc.size())
    }

    pub(super) fn selected_pixels(&self) -> Option<Rgba8> {
        Some(self.floating.as_ref()?.pixels.clone())
    }

    pub(super) fn erase_selection(&mut self) {
        let Some(floating) = self.floating.take() else {
            return;
        };
        self.grab = None;
        self.grab_from = None;
        if let Some(hole) = floating.lifted_from {
            self.doc.commit("Cut", hole, floating.backup());
        }
        self.damage.clear();
    }

    pub(super) fn bucket(&mut self, x: f32, y: f32) {
        let before = self.doc.pixels().clone();
        let version = self.doc.version();
        let (colour, tolerance) = (self.brush.colour, self.brush.tolerance);

        let mut filled = before.clone();
        let Some(touched) = fill::flood(
            &mut filled,
            x.floor() as i64,
            y.floor() as i64,
            colour,
            tolerance,
        ) else {
            return;
        };
        *self.doc.edit() = filled;
        self.doc.commit("Fill", touched, &before);
        self.damage.record(version, self.doc.version(), [touched]);
    }

    pub(super) fn eyedropper(&mut self, x: f32, y: f32) {
        if let Some(colour) = self.doc.sample(x.floor() as i64, y.floor() as i64)
            && colour[3] > 0
        {
            self.brush.colour = colour;
            if let Some(tool) = self.pipette_return.take() {
                self.brush.tool = tool;
            }
        }
    }

    pub(super) fn flush_stroke(&mut self) {
        let before = self.doc.version();
        let Some(stroke) = &mut self.stroke else {
            return;
        };
        let rects = stroke.flush(&mut self.doc);
        self.damage.record(before, self.doc.version(), rects);
    }

    pub(super) fn can_undo(&self) -> bool {
        if let Some(c) = &self.cutting_out {
            return !c.history.is_empty();
        }
        self.floating.is_some() || self.doc.can_undo()
    }

    pub(super) fn can_redo(&self) -> bool {
        if let Some(c) = &self.cutting_out {
            return !c.redo.is_empty();
        }
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

    pub(super) fn step_history(&mut self, undo: bool) {
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
        match changed {
            Some(Some(rect)) => self.damage.record(before, self.doc.version(), [rect]),
            Some(None) => self.damage.clear(),
            None => return,
        }
        self.panel.sync(self.doc.size());
    }
}

impl Sheet {
    pub(super) fn unsaved(&self) -> bool {
        self.doc.modified() || self.floating.is_some()
    }

    pub(super) fn for_recovery(&self) -> Stack {
        with_live(&self.doc, self.floating.as_ref())
    }
}

// The stack as it would be with the live object committed into the layer it belongs to.
fn with_live(doc: &Document, floating: Option<&select::Floating>) -> Stack {
    let Some(floating) = floating else {
        return doc.stack().clone();
    };
    let mut scratch = doc.detached();
    floating.commit(&mut scratch);
    scratch.stack().clone()
}
