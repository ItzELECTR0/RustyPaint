use crate::doc::{self, Document, Rect, Rgba8};
use crate::gpu::View;
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
    let handle = rfd::AsyncFileDialog::new()
        .add_filter(i18n::dialog_images(), doc::io::READABLE)
        .set_title(i18n::open_title())
        .pick_file()
        .await
        .ok_or_else(String::new)?;

    load(handle.path().to_path_buf()).await
}

pub(super) async fn pick_and_save(
    pixels: Rgba8,
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
    doc::io::save_as(&pixels, &path, format).map(|()| path)
}

pub(super) async fn save_to(pixels: Rgba8, path: PathBuf) -> Result<PathBuf, String> {
    doc::io::save(&pixels, &path).map(|()| path)
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

    pub(super) fn save(&self) -> Task<Message> {
        let pixels = self.for_saving();
        match self.doc.path.clone() {
            Some(path) if doc::io::SaveFormat::from_path(&path).is_some() => {
                Task::perform(save_to(pixels, path), Message::Saved)
            }
            _ => self.save_as(),
        }
    }

    pub(super) fn save_as(&self) -> Task<Message> {
        let stem = self
            .doc
            .path
            .as_deref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| i18n::untitled().into());
        Task::perform(
            pick_and_save(self.for_saving(), stem, self.save_format),
            Message::Saved,
        )
    }

    // Recovery keeps the document as it stands, backing and all, rather than a flattened picture.
    pub(super) fn for_recovery(&self) -> Rgba8 {
        let Some(floating) = &self.floating else {
            return self.doc.pixels().clone();
        };
        let mut scratch = Document::from_image(self.doc.pixels().clone(), None);
        scratch.transparent = self.doc.transparent;
        floating.commit(&mut scratch);
        scratch.pixels().clone()
    }

    pub(super) fn snapshot_parked(&self, sheet: &Sheet) -> Task<Message> {
        let Some(root) = self.recovery.clone() else {
            return Task::none();
        };
        if !sheet.unsaved() {
            return Task::none();
        }
        let (id, slot) = (self.session.clone(), sheet.slot.clone());
        let pixels = sheet.for_recovery();
        Task::perform(
            async move { doc::recovery::write_document(&root, &id, &slot, &pixels) },
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
                    transparent: self.doc.transparent,
                    unsaved: self.unsaved(),
                },
                Some(i) => {
                    let sheet = &self.parked[i];
                    doc::recovery::Open {
                        slot: sheet.slot.clone(),
                        path: sheet.doc.path.clone(),
                        transparent: sheet.doc.transparent,
                        unsaved: sheet.unsaved(),
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

        let at = (self.doc.version(), self.float_version);
        if !self.unsaved() || self.snapshotted == Some(at) || self.snapshotting {
            return Task::none();
        }
        if self.last_snapshot.elapsed() < doc::recovery::SNAPSHOT_GAP {
            return Task::none();
        }

        self.snapshotting = true;
        self.last_snapshot = Instant::now();
        let (id, slot) = (self.session.clone(), self.slot.clone());
        let pixels = self.for_recovery();
        Task::perform(
            async move { doc::recovery::write_document(&root, &id, &slot, &pixels) },
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
        let mut sheets = self.collapse();
        let leaving = self.snapshot_parked(&sheets[self.active.min(sheets.len() - 1)]);
        let at = sheets.len();
        sheets.push(sheet);
        self.expand(sheets, at);
        self.record_session();
        leaving
    }

    pub(super) fn adopt_document(&mut self, doc: Document) {
        self.doc = doc;
        self.floating = None;
        self.live_redo = None;
        self.grab = None;
        self.grab_from = None;
        self.float_version += 1;
        self.view = View::fitted(self.viewport, self.doc.size());
        self.dirty = None;
        self.panel.sync(self.doc.size());
        self.status.clear();
        self.menu = None;
    }

    pub(super) fn switch_to(&mut self, tab: usize) -> Task<Message> {
        if tab == self.active || tab >= self.sheets() {
            return Task::none();
        }
        let sheets = self.collapse();
        let leaving = self.snapshot_parked(&sheets[self.active.min(sheets.len() - 1)]);
        self.expand(sheets, tab);
        self.record_session();
        leaving
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
        Task::none()
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
            let doc = if one.unsaved {
                Document::recovered(one.pixels, one.path, one.transparent)
            } else {
                Document::from_image(one.pixels, one.path)
            };
            tasks.push(self.open_document(doc, format));
        }
        tasks.push(self.switch_to(active));
        Task::batch(tasks)
    }

    pub(super) fn for_saving(&self) -> Rgba8 {
        let Some(floating) = &self.floating else {
            return self.doc.flattened();
        };
        let mut scratch = Document::from_image(self.doc.pixels().clone(), None);
        scratch.transparent = self.doc.transparent;
        floating.commit(&mut scratch);
        scratch.flattened()
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
            Pending::Close => iced::window::latest().and_then(iced::window::close),
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
        self.dirty = None;
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
        self.dirty = Some((version, touched));
    }

    pub(super) fn eyedropper(&mut self, x: f32, y: f32) {
        if let Some(colour) = fill::pick(self.doc.pixels(), x.floor() as i64, y.floor() as i64)
            && colour[3] > 0
        {
            self.brush.colour = colour;
        }
    }

    pub(super) fn flush_stroke(&mut self) {
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

    pub(super) fn can_undo(&self) -> bool {
        self.floating.is_some() || self.doc.can_undo()
    }

    pub(super) fn can_redo(&self) -> bool {
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
        self.dirty = match changed {
            Some(Some(rect)) => Some((before, rect)),
            Some(None) => None,
            None => return,
        };
        self.panel.sync(self.doc.size());
    }
}

impl Sheet {
    pub(super) fn unsaved(&self) -> bool {
        self.doc.modified() || self.floating.is_some()
    }

    pub(super) fn for_recovery(&self) -> Rgba8 {
        let Some(floating) = &self.floating else {
            return self.doc.pixels().clone();
        };
        let mut scratch = Document::from_image(self.doc.pixels().clone(), None);
        scratch.transparent = self.doc.transparent;
        floating.commit(&mut scratch);
        scratch.pixels().clone()
    }
}
