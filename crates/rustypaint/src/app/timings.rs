// Measures rather than asserts, so it only runs with `--release --ignored --nocapture`.

use super::*;
use crate::gpu::{CanvasFrame, Upload, plan_upload};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SMALL: (u32, u32) = Document::DEFAULT_SIZE;
const LARGE: (u32, u32) = (6000, 4000);

#[derive(Clone, Copy)]
enum Movement {
    Nudge,
    SlowShort,
    RapidLong,
    Reversals,
}

impl Movement {
    const ALL: [Movement; 4] = [
        Movement::Nudge,
        Movement::SlowShort,
        Movement::RapidLong,
        Movement::Reversals,
    ];

    fn name(self) -> &'static str {
        match self {
            Movement::Nudge => "1px nudges",
            Movement::SlowShort => "slow short",
            Movement::RapidLong => "rapid long",
            Movement::Reversals => "reversals",
        }
    }

    // Pointer samples in image coordinates, and how many of them arrive between two frames.
    fn samples(self, canvas: (u32, u32)) -> (Vec<(f32, f32)>, usize) {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let (cx, cy) = (w / 2.0 + 0.3, h / 2.0 + 0.3);
        match self {
            Movement::Nudge => ((0..=8).map(|i| (cx + i as f32, cy)).collect(), 1),
            Movement::SlowShort => (
                (0..=48)
                    .map(|i| (cx + i as f32, cy + (i / 3) as f32))
                    .collect(),
                4,
            ),
            Movement::RapidLong => (
                (0..=64)
                    .map(|i| {
                        let t = i as f32 / 64.0;
                        let bow = (t * std::f32::consts::PI).sin() * h * 0.1;
                        (w * (0.1 + 0.8 * t), h * (0.2 + 0.6 * t) + bow)
                    })
                    .collect(),
                16,
            ),
            Movement::Reversals => {
                let mut at = (cx, cy);
                let mut heading = 0.0f32;
                let mut points = vec![at];
                for i in 1..=160 {
                    if i % 12 == 0 {
                        heading += if (i / 12) % 2 == 0 {
                            std::f32::consts::PI
                        } else {
                            2.4
                        };
                    }
                    at = (
                        (at.0 + heading.cos() * 6.0).clamp(1.0, w - 1.0),
                        (at.1 + heading.sin() * 6.0).clamp(1.0, h - 1.0),
                    );
                    points.push(at);
                }
                (points, 8)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Tip {
    tool: Tool,
    thickness: f32,
    antialiased: bool,
    stabilizer: f32,
}

impl Tip {
    const fn of(tool: Tool, thickness: f32) -> Self {
        Self {
            tool,
            thickness,
            antialiased: false,
            stabilizer: 0.0,
        }
    }

    fn name(self) -> String {
        let mut name = format!("{:?} {}", self.tool, self.thickness);
        if self.antialiased {
            name.push_str(" aa");
        }
        if self.stabilizer > 0.0 {
            name.push_str(&format!(" s{}", self.stabilizer));
        }
        name
    }

    fn brush(self) -> Brush {
        let mut brush = Brush {
            tool: self.tool,
            colour: [20, 20, 20, 255],
            ..Brush::default()
        };
        brush.set_thickness(self.thickness);
        brush.set_antialiased(self.antialiased);
        brush.set_stabilizer(self.stabilizer);
        brush
    }

    // How far in from the swept edge a pixel has to be before it must be painted.
    fn inside(self) -> f32 {
        let brush = self.brush();
        let profile = brush.profile();
        brush.stamp_radius() * profile.aspect.min(1.0) - 1.5
    }
}

const TIPS: [Tip; 9] = [
    Tip::of(Tool::Marker, 12.0),
    Tip::of(Tool::Marker, 200.0),
    Tip::of(Tool::Eraser, 12.0),
    Tip {
        antialiased: true,
        ..Tip::of(Tool::Eraser, 200.0)
    },
    Tip::of(Tool::Calligraphy, 60.0),
    Tip::of(Tool::Watercolour, 120.0),
    Tip::of(Tool::Pencil, 40.0),
    Tip::of(Tool::PixelPen, 1.0),
    Tip {
        stabilizer: 0.8,
        ..Tip::of(Tool::Marker, 12.0)
    },
];

struct Gpu {
    uploaded: u64,
    scratch: Vec<u8>,
    staged: Vec<u8>,
    real: Option<(wgpu::Device, wgpu::Queue, crate::gpu::ViewportPipeline)>,
}

impl Gpu {
    fn new() -> Self {
        Self {
            uploaded: u64::MAX,
            scratch: Vec::new(),
            staged: Vec::new(),
            real: real_device(),
        }
    }

    // Brings the texture up to the frame and returns the bytes that took and how long it took.
    fn present(&mut self, frame: &CanvasFrame) -> (usize, bool, Duration) {
        let plan = plan_upload(self.uploaded, frame.size, frame.version, frame.dirty);
        self.uploaded = frame.version;
        let bytes = match plan {
            Upload::Nothing => 0,
            Upload::Whole => frame.pixels.len(),
            Upload::Region(rect) => rect.area() * 4,
        };

        let t = Instant::now();
        if let Some((device, queue, pipeline)) = &mut self.real {
            pipeline.sync_canvas(
                device,
                queue,
                frame.size,
                frame.version,
                frame.dirty,
                &frame.pixels,
            );
            queue.submit([]);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        } else {
            match plan {
                Upload::Nothing => {}
                Upload::Whole => {
                    self.scratch.clear();
                    self.scratch.extend_from_slice(&frame.pixels);
                }
                Upload::Region(rect) => {
                    let (span, stride) = (rect.width() as usize * 4, frame.size.0 as usize * 4);
                    self.staged.clear();
                    for y in rect.rows() {
                        let start = y as usize * stride + rect.x0 as usize * 4;
                        self.staged
                            .extend_from_slice(&frame.pixels[start..start + span]);
                    }
                    self.scratch.clear();
                    self.scratch.extend_from_slice(&self.staged);
                }
            }
        }
        std::hint::black_box(&self.scratch);
        (bytes, matches!(plan, Upload::Whole), t.elapsed())
    }
}

fn real_device() -> Option<(wgpu::Device, wgpu::Queue, crate::gpu::ViewportPipeline)> {
    use iced::widget::shader::Pipeline;

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    let pipeline =
        crate::gpu::ViewportPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    Some((device, queue, pipeline))
}

#[derive(Default)]
struct Tally {
    samples: Vec<Duration>,
    frames: Vec<Duration>,
    uploads: Vec<usize>,
    whole: usize,
    copies: usize,
    missed_during: usize,
    missed_after: usize,
    press: Duration,
    release: Duration,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn mean(v: &[Duration]) -> Duration {
    v.iter().sum::<Duration>() / v.len().max(1) as u32
}

fn fresh(canvas: (u32, u32), tip: Tip) -> App {
    let config = Config {
        theme: crate::ui::theme::Choice::Light,
        cutout_object: false,
        ..Config::default()
    };
    let (mut app, _boot) = App::boot(config, None, None, None);
    app.doc = if tip.tool == Tool::Eraser {
        Document::from_image(Rgba8::new(canvas.0, canvas.1, [90, 140, 200, 255]), None)
    } else {
        Document::blank_sized(canvas.0, canvas.1, false)
    };
    app.panel = CanvasPanel::new(app.doc.size());
    app.viewport = Size::new(1280.0, 800.0);
    app.brush = tip.brush();
    app
}

fn painted(now: &[u8], before: &[u8], i: usize, erase: bool) -> bool {
    if erase {
        now[i + 3] < before[i + 3]
    } else {
        now[i..i + 4] != before[i..i + 4]
    }
}

// Untouched pixels at least `inside` from the edge of the brush swept from `a` to `b`.
fn missed(
    app: &App,
    before: &[u8],
    erase: bool,
    (a, b): ((f32, f32), (f32, f32)),
    inside: f32,
    seen: &mut [bool],
) -> usize {
    let (w, h) = app.doc.size();
    let now = app.doc.pixels().as_bytes();
    let x0 = (a.0.min(b.0) - inside).floor().max(0.0) as u32;
    let y0 = (a.1.min(b.1) - inside).floor().max(0.0) as u32;
    let x1 = ((a.0.max(b.0) + inside).ceil().max(0.0) as u32).min(w);
    let y1 = ((a.1.max(b.1) + inside).ceil().max(0.0) as u32).min(h);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let mut count = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let t = if length > 0.0 {
                (((px - a.0) * dx + (py - a.1) * dy) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = (px - a.0 - dx * t).hypot(py - a.1 - dy * t);
            let index = y as usize * w as usize + x as usize;
            if d <= inside && !seen[index] && !painted(now, before, index * 4, erase) {
                seen[index] = true;
                count += 1;
            }
        }
    }
    count
}

fn run(gpu: &mut Gpu, canvas: (u32, u32), tip: Tip, movement: Movement) -> Tally {
    let mut app = fresh(canvas, tip);
    let (points, per_frame) = movement.samples(canvas);
    let erase = tip.tool == Tool::Eraser;
    let before = app.doc.pixels().as_bytes().to_vec();
    let inside = tip.inside();
    let cells = tip.tool.snaps_to_pixels() && inside < 0.5;
    let area = canvas.0 as usize * canvas.1 as usize;
    let mut tally = Tally::default();

    // The renderer keeps the last frame's primitive until it draws the next one.
    let mut held = Some(app.frame());
    gpu.present(held.as_ref().unwrap());

    let pointer = |app: &App, at: (f32, f32)| -> usize {
        let (w, h) = app.doc.size();
        let (x, y) = (at.0.floor(), at.1.floor());
        if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
            return 0;
        }
        let i = (y as usize * w as usize + x as usize) * 4;
        usize::from(!painted(app.doc.pixels().as_bytes(), &before, i, erase))
    };

    let mut previous = points[0];
    let batches: Vec<&[(f32, f32)]> = points.chunks(per_frame).collect();
    for (n, batch) in batches.iter().enumerate() {
        let buffer = Arc::as_ptr(&app.doc.pixels().bytes_arc());
        let mut spent = Duration::ZERO;
        for (k, &at) in batch.iter().enumerate() {
            let message = if n == 0 && k == 0 {
                gpu::Interaction::PaintBegan(at.0, at.1)
            } else {
                gpu::Interaction::PaintMoved(at.0, at.1)
            };
            let t = Instant::now();
            let _ = app.update(Message::Canvas(message));
            let took = t.elapsed();
            spent += took;
            if n == 0 && k == 0 {
                tally.press = took;
            } else {
                tally.samples.push(took);
            }

            if tip.stabilizer <= 0.0 {
                tally.missed_during += if cells {
                    pointer(&app, at)
                } else {
                    let mut seen = vec![false; area];
                    missed(&app, &before, erase, (previous, at), inside, &mut seen)
                };
            }
            previous = at;
        }
        if Arc::as_ptr(&app.doc.pixels().bytes_arc()) != buffer {
            tally.copies += 1;
        }

        let t = Instant::now();
        let frame = app.frame();
        let view = t.elapsed();
        let (bytes, whole, upload) = gpu.present(&frame);
        held = Some(frame);
        if n == 0 {
            tally.press += view + upload;
        }
        tally.frames.push(spent + view + upload);
        tally.uploads.push(bytes);
        tally.whole += usize::from(whole);
    }

    let t = Instant::now();
    let _ = app.update(Message::Canvas(gpu::Interaction::PaintEnded));
    let frame = app.frame();
    let (bytes, whole, upload) = gpu.present(&frame);
    tally.release = t.elapsed();
    tally.uploads.push(bytes);
    tally.whole += usize::from(whole);
    drop(held);
    let _ = upload;

    let mut seen = vec![false; area];
    if cells {
        tally.missed_after = points.iter().map(|at| pointer(&app, *at)).sum();
    } else if tip.stabilizer > 0.0 {
        // A stabilised stroke cuts corners on purpose, so only where it ends is owed.
        let end = *points.last().unwrap();
        tally.missed_after = missed(&app, &before, erase, (end, end), inside, &mut seen);
    } else {
        for pair in points.windows(2) {
            tally.missed_after +=
                missed(&app, &before, erase, (pair[0], pair[1]), inside, &mut seen);
        }
    }
    tally
}

#[test]
#[ignore = "measures rather than asserts; run with --release --ignored --nocapture"]
fn stroke_timings() {
    let mut gpu = Gpu::new();
    println!(
        "uploads: {}",
        if gpu.real.is_some() {
            "written to a real texture and waited on"
        } else {
            "no adapter, so CPU staging copies only"
        }
    );
    println!(
        "{:<11} {:<22} {:<11} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9} {:>5} {:>5} {:>7} {:>7}",
        "canvas",
        "tip",
        "movement",
        "press",
        "sample",
        "sample",
        "frame",
        "frame",
        "release",
        "upload",
        "upload",
        "whole",
        "copy",
        "missed",
        "missed"
    );
    println!(
        "{:<11} {:<22} {:<11} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} {:>9} {:>5} {:>5} {:>7} {:>7}",
        "",
        "",
        "",
        "ms",
        "mean ms",
        "max ms",
        "mean ms",
        "max ms",
        "ms",
        "mean KiB",
        "max KiB",
        "",
        "",
        "drag",
        "release"
    );
    for canvas in [SMALL, LARGE] {
        for tip in TIPS {
            for movement in Movement::ALL {
                let tally = run(&mut gpu, canvas, tip, movement);
                let uploads = &tally.uploads;
                let kib = |b: usize| b as f64 / 1024.0;
                println!(
                    "{:<11} {:<22} {:<11} {:>7.2} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.2} {:>9.1} {:>9.1} {:>5} {:>5} {:>7} {:>7}",
                    format!("{}x{}", canvas.0, canvas.1),
                    tip.name(),
                    movement.name(),
                    ms(tally.press),
                    ms(mean(&tally.samples)),
                    ms(tally.samples.iter().copied().max().unwrap_or_default()),
                    ms(mean(&tally.frames)),
                    ms(tally.frames.iter().copied().max().unwrap_or_default()),
                    ms(tally.release),
                    kib(uploads.iter().sum::<usize>() / uploads.len().max(1)),
                    kib(uploads.iter().copied().max().unwrap_or_default()),
                    tally.whole,
                    tally.copies,
                    if tip.stabilizer > 0.0 {
                        "-".to_string()
                    } else {
                        tally.missed_during.to_string()
                    },
                    tally.missed_after,
                );
            }
        }
    }
}
