use super::*;
use crate::select::cutout::{
    refine::{Dab, Settings},
    workflow,
};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

type Job = Box<dyn FnOnce() + Send>;

fn worker() -> &'static std::sync::mpsc::SyncSender<Job> {
    static WORKER: OnceLock<std::sync::mpsc::SyncSender<Job>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Job>(64);
        std::thread::spawn(move || {
            for job in receiver {
                job();
            }
        });
        sender
    })
}

#[derive(Clone)]
pub(super) struct CutoutEdit {
    strokes: Vec<Dab>,
    settings: Settings,
    aim: workflow::Aim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutoutPreview {
    Overlay,
    Checkerboard,
    Colour,
}

impl CutoutPreview {
    pub const ALL: [Self; 3] = [Self::Overlay, Self::Checkerboard, Self::Colour];
}

impl std::fmt::Display for CutoutPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Overlay => crate::i18n::cutout_overlay(),
            Self::Checkerboard => crate::i18n::cutout_checkerboard(),
            Self::Colour => crate::i18n::cutout_colour(),
        })
    }
}

pub(super) fn next_cutout_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Drop for CuttingOut {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl CuttingOut {
    fn edit(&self) -> CutoutEdit {
        CutoutEdit {
            strokes: self.strokes.clone(),
            settings: self.settings,
            aim: self.aim,
        }
    }

    pub(super) fn checkpoint(&mut self) {
        self.edit_field = None;
        self.history.push(self.edit());
        if self.history.len() > 64 {
            self.history.remove(0);
        }
        self.redo.clear();
    }

    pub(super) fn step(&mut self, undo: bool) -> bool {
        let next = if undo {
            self.history.pop()
        } else {
            self.redo.pop()
        };
        let Some(next) = next else { return false };
        let current = self.edit();
        if undo {
            self.redo.push(current);
        } else {
            self.history.push(current);
        }
        self.strokes = next.strokes;
        self.settings = next.settings;
        self.aim = next.aim;
        self.edit_field = None;
        self.painting = false;
        self.previous = None;
        true
    }
}

impl App {
    pub(super) fn run_cutout(&mut self) -> Task<Message> {
        let Some(cutout) = &mut self.cutting_out else {
            return Task::none();
        };
        cutout.revision += 1;
        if cutout.busy {
            return Task::none();
        }
        self.start_cutout()
    }

    pub(super) fn resume_cutout(&mut self) -> Task<Message> {
        if self
            .cutting_out
            .as_ref()
            .is_some_and(|c| c.refining && !c.busy && !c.failed && c.revision != c.applied_revision)
        {
            self.start_cutout()
        } else {
            Task::none()
        }
    }

    fn start_cutout(&mut self) -> Task<Message> {
        let Some(cutout) = &mut self.cutting_out else {
            return Task::none();
        };
        cutout.busy = true;
        cutout.failed = false;
        cutout.refining = true;
        let (id, revision, version) = (cutout.id, cutout.revision, self.doc.version());
        let (rect, settings) = (cutout.rect, cutout.settings);
        let (strokes, previous, cancelled) = (
            cutout.strokes.clone(),
            cutout.result.clone(),
            cutout.cancelled.clone(),
        );
        let (object, path) = (cutout.object, cutout.model_path.clone());
        let request = workflow::Request {
            rect,
            object,
            aim: cutout.aim,
            path,
            strokes,
            settings,
        };
        let pixels = self.doc.pixels().clone();
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        let job = Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(String::new());
                }
                workflow::compute(&pixels, &request, previous.as_deref()).map(Arc::new)
            }))
            .unwrap_or_else(|_| Err("Smart Cutout worker failed".into()));
            let _ = sender.send(result);
        });
        if worker().try_send(job).is_err() {
            return Task::done(Message::CutoutFinished(
                id,
                revision,
                version,
                Err("Analysis queue is full; try again".into()),
            ));
        }
        Task::perform(
            async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err("Smart Cutout worker disconnected".into()))
            },
            move |result| Message::CutoutFinished(id, revision, version, result),
        )
    }

    pub(super) fn cutout_finished(
        &mut self,
        id: u64,
        revision: u64,
        version: Version,
        result: Result<Arc<workflow::ResultMask>, String>,
    ) -> Task<Message> {
        if !self.cutting_out.as_ref().is_some_and(|c| c.id == id) {
            for sheet in &mut self.parked {
                if let Some(c) = &mut sheet.cutting_out
                    && c.id == id
                {
                    c.busy = false;
                    if sheet.doc.version() != version {
                        sheet.cutting_out = None;
                    } else {
                        match result {
                            Ok(result) => {
                                c.result = Some(result.clone());
                                if c.revision == revision && !c.painting {
                                    c.mask = Some(result.mask.clone());
                                    c.applied_revision = revision;
                                    c.build_overlay(sheet.doc.pixels());
                                    sheet.float_version += 1;
                                }
                            }
                            Err(_) => {
                                c.failed = true;
                                if c.mask.is_none() {
                                    c.refining = false;
                                }
                            }
                        }
                    }
                    break;
                }
            }
            return Task::none();
        }
        let c = self.cutting_out.as_mut().unwrap();
        c.busy = false;
        if self.doc.version() != version {
            self.cutting_out = None;
            self.float_version += 1;
            return Task::none();
        }
        match result {
            Ok(result) => {
                c.result = Some(result.clone());
                if c.revision == revision && !c.painting {
                    c.mask = Some(result.mask.clone());
                    c.applied_revision = revision;
                    c.build_overlay(self.doc.pixels());
                    self.float_version += 1;
                }
            }
            Err(error) => {
                c.failed = true;
                self.status = format!("{}: {error}", crate::i18n::cutout_failed());
                if c.mask.is_none() {
                    c.refining = false;
                }
                return Task::none();
            }
        }
        if c.revision != revision && !c.painting {
            return self.start_cutout();
        }
        Task::none()
    }

    pub(super) fn cutout_field(&mut self, field: Field, value: f32) -> Task<Message> {
        let Some(c) = &mut self.cutting_out else {
            return Task::none();
        };
        if field == Field::CutoutBrush {
            c.brush = value;
            return Task::none();
        }
        if field == Field::CutoutTolerance {
            if c.aim.tolerance == value {
                return Task::none();
            }
            if c.edit_field != Some(field) {
                c.checkpoint();
                c.edit_field = Some(field);
            }
            c.aim.tolerance = value;
            return self.run_cutout();
        }
        let before = c.settings;
        let mut settings = before;
        match field {
            Field::CutoutRadius => settings.radius = value,
            Field::CutoutFeather => settings.feather = value,
            Field::CutoutSmooth => settings.smooth = value,
            Field::CutoutShift => settings.shift = value,
            _ => return Task::none(),
        }
        if before == settings {
            return Task::none();
        }
        if c.edit_field != Some(field) {
            c.checkpoint();
            c.edit_field = Some(field);
        }
        c.settings = settings;
        self.run_cutout()
    }
}
