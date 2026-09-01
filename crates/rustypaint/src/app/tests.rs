use super::*;
use crate::gpu::Handle;
use crate::select::Xform;
use iced::Point;
use iced::time::Duration;

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
    let (mut app, _boot) = App::boot(config, None, None, None);
    app.doc = Document::blank_sized(width, height, false);
    app.panel = CanvasPanel::new(app.doc.size());
    app.viewport = Size::new(800.0, 600.0);
    app
}

fn recovering(dir: &std::path::Path) -> App {
    let config = Config {
        theme: Choice::Light,
        ..Config::default()
    };
    let (mut app, _boot) = App::boot(config, None, Some(dir.to_path_buf()), None);
    app.viewport = Size::new(800.0, 600.0);
    app
}

fn recovery_scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rustypaint-app-recovery-{name}-{}",
        crate::doc::recovery::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn work_left_behind_by_a_dead_editor_is_offered_back() {
    let dir = recovery_scratch("offered");
    let pixels = Rgba8::new(7, 5, [1, 2, 3, 255]);
    crate::doc::recovery::write(&dir, "gone", &pixels, None, false).unwrap();

    let mut app = recovering(&dir);
    assert!(app.offering, "the dialog opens on its own at launch");
    assert_eq!(app.recovered.len(), 1);

    send(&mut app, Message::RecoveryAnswered(true));
    assert!(!app.offering);
    assert_eq!(app.doc.size(), (7, 5));
    assert_eq!(app.doc.pixels(), &pixels);
    assert!(app.unsaved(), "recovered work has still never been saved");
    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "what came back is no longer waiting"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_recovered_document_remembers_the_file_it_came_from() {
    let dir = recovery_scratch("path");
    let file = std::env::temp_dir().join("portrait.png");
    let pixels = Rgba8::new(2, 2, [9, 9, 9, 255]);
    crate::doc::recovery::write(&dir, "gone", &pixels, Some(&file), true).unwrap();

    let mut app = recovering(&dir);
    send(&mut app, Message::RecoveryAnswered(true));
    assert_eq!(app.doc.path.as_deref(), Some(file.as_path()));
    assert!(app.doc.transparent, "the backing came back too");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn turning_the_offer_down_throws_all_of_it_away() {
    let dir = recovery_scratch("declined");
    let pixels = Rgba8::new(3, 3, [4, 4, 4, 255]);
    crate::doc::recovery::write(&dir, "one", &pixels, None, false).unwrap();
    crate::doc::recovery::write(&dir, "two", &pixels, None, false).unwrap();

    let mut app = recovering(&dir);
    assert_eq!(app.recovered.len(), 2);
    send(&mut app, Message::RecoveryAnswered(false));
    assert!(!app.offering);
    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "declining clears the lot rather than asking again forever"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn recovering_brings_every_abandoned_document_back_as_its_own_tab() {
    let dir = recovery_scratch("queue");
    crate::doc::recovery::write(&dir, "one", &Rgba8::new(3, 3, [4, 4, 4, 255]), None, false)
        .unwrap();
    crate::doc::recovery::write(&dir, "two", &Rgba8::new(5, 5, [6, 6, 6, 255]), None, false)
        .unwrap();

    let mut app = recovering(&dir);
    send(&mut app, Message::RecoveryAnswered(true));
    assert_eq!(app.sheets(), 2, "both came back, neither had to wait");
    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "and nothing is still sitting on disk"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_clean_launch_asks_nothing() {
    let dir = recovery_scratch("clean");
    let app = recovering(&dir);
    assert!(!app.offering);
    assert!(app.recovered.is_empty());
}

#[test]
fn saving_clears_the_snapshot_that_was_covering_the_work() {
    let dir = recovery_scratch("saved");
    let mut app = recovering(&dir);
    let file = std::env::temp_dir().join("done.png");
    crate::doc::recovery::write(&dir, &app.recovery_id, app.doc.pixels(), None, false).unwrap();

    send(&mut app, Message::Saved(Ok(file)));
    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "work that reached disk needs no snapshot"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_running_editor_never_offers_its_own_work_back_to_itself() {
    let dir = recovery_scratch("selfheld");
    let mut app = recovering(&dir);
    app.doc.edit().pixels_mut()[0] = 7;
    crate::doc::recovery::write(&dir, &app.recovery_id, app.doc.pixels(), None, false).unwrap();

    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "the lock this editor holds says the work is still being worked on"
    );

    let id = app.recovery_id.clone();
    drop(app);
    let found = crate::doc::recovery::abandoned(&dir);
    assert_eq!(found.len(), 1, "and the moment it dies the work is offered");
    assert_eq!(found[0].id, id);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_parked_tab_keeps_its_own_snapshot_alive() {
    let dir = recovery_scratch("parked");
    let mut app = recovering(&dir);
    app.doc.edit().pixels_mut()[0] = 7;
    let first = app.recovery_id.clone();
    send(&mut app, Message::NewRequested);

    assert_eq!(app.sheets(), 2);
    assert_ne!(app.recovery_id, first, "each sheet has its own identity");
    crate::doc::recovery::write(&dir, &first, app.doc.pixels(), None, false).unwrap();
    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "a parked tab is still held by this editor"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_document_with_nothing_in_it_writes_no_snapshot() {
    let dir = recovery_scratch("untouched");
    let mut app = recovering(&dir);
    let _ = app.snapshot();
    assert!(
        crate::doc::recovery::abandoned(&dir).is_empty(),
        "an untouched canvas is not work"
    );
}

#[test]
fn a_snapshot_keeps_the_document_rather_than_a_flattened_picture() {
    let mut app = app(2, 1);
    app.doc = Document::blank_sized(2, 1, false);
    app.doc.edit().pixels_mut()[..4].copy_from_slice(&[255, 0, 0, 128]);

    assert_eq!(
        app.for_recovery().as_bytes()[..4],
        [255, 0, 0, 128],
        "half-transparent pixels come back exactly as they were"
    );
    assert_eq!(
        app.for_saving().as_bytes()[3],
        255,
        "saving composites onto the backing, which is why recovery cannot reuse it"
    );
}

#[test]
fn unsaved_work_keeps_its_snapshot_where_it_is() {
    let dir = recovery_scratch("kept");
    let mut app = recovering(&dir);
    app.doc.edit().pixels_mut()[0] = 7;
    crate::doc::recovery::write(&dir, &app.recovery_id, app.doc.pixels(), None, false).unwrap();

    let _ = app.snapshot();
    assert!(
        dir.join(format!("{}.png", app.recovery_id)).exists(),
        "a snapshot is only cleared once the work behind it is safe"
    );
    std::fs::remove_dir_all(&dir).unwrap();
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
fn undoing_the_work_leaves_nothing_to_ask_about() {
    let mut app = app(8, 8);
    app.brush.tool = Tool::Fill;
    app.brush.colour = [255, 0, 0, 255];
    click(&mut app, 4.0, 4.0);
    assert!(app.unsaved());

    send(&mut app, Message::Undo);
    assert!(!app.unsaved(), "there is no difference left to save");

    send(&mut app, Message::Redo);
    assert!(app.unsaved(), "and redoing brings it back");
}

#[test]
fn a_fill_that_paints_what_is_already_there_is_not_unsaved_work() {
    let mut app = app(8, 8);
    app.brush.tool = Tool::Fill;
    app.brush.colour = [0, 0, 0, 0];

    click(&mut app, 4.0, 4.0);
    assert!(!app.unsaved(), "the canvas was already transparent");
    assert!(!app.doc.can_undo(), "and it is not an undo step");
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
        !app.doc.modified() || app.doc.can_undo(),
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
    assert!(!app.doc.modified());

    drag_selection(&mut app, (2.0, 2.0), (8.0, 8.0));
    assert!(app.doc.modified(), "lifting touched the canvas");
    send(&mut app, Message::Undo);
    assert!(
        !app.doc.modified(),
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

fn xform(app: &App) -> Xform {
    app.floating.as_ref().unwrap().xform
}

fn grab_grip(app: &mut App, grab: gpu::Grab) {
    let at = match grab {
        gpu::Grab::Resize(handle) => xform(app).handle_at(handle),
        _ => xform(app).centre(),
    };
    send(
        app,
        Message::Canvas(gpu::Interaction::FloatGrabbed(grab, at.0, at.1)),
    );
    send(app, Message::Canvas(gpu::Interaction::FloatReleased));
}

fn selection(width: u32, height: u32) -> App {
    let mut app = app(width, height);
    fill_canvas(&mut app, [255, 0, 0, 255]);
    drag_selection(&mut app, (20.0, 20.0), (80.0, 60.0));
    assert!(app.floating.is_some(), "there is a selection to nudge");
    app
}

#[test]
fn an_arrow_walks_a_selection_a_pixel_at_a_time() {
    let mut app = selection(200, 200);
    let was = xform(&app);

    send(&mut app, Message::NudgeStarted(Arrow::Left));
    assert_eq!(xform(&app).x, was.x - 1.0);
    assert_eq!(xform(&app).y, was.y, "and nothing else moves");

    send(&mut app, Message::NudgeEnded(Arrow::Left));
    send(&mut app, Message::NudgeStarted(Arrow::Down));
    assert_eq!(xform(&app).y, was.y + 1.0);
    assert_eq!(xform(&app).width, was.width, "moving is not stretching");
}

#[test]
fn arrows_stretch_the_edge_that_was_last_dragged() {
    let mut app = selection(200, 200);
    grab_grip(&mut app, gpu::Grab::Resize(gpu::Handle::Left));
    let was = xform(&app);

    send(&mut app, Message::NudgeStarted(Arrow::Left));
    let out = xform(&app);
    assert!(
        (out.width - was.width - 1.0).abs() < 0.01,
        "left grows the left edge outwards, got {}",
        out.width
    );
    assert!(
        (out.height - was.height).abs() < 0.01,
        "the height is not its axis"
    );

    send(&mut app, Message::NudgeEnded(Arrow::Left));
    send(&mut app, Message::NudgeStarted(Arrow::Right));
    assert!(
        (xform(&app).width - was.width).abs() < 0.01,
        "and right takes it back"
    );

    send(&mut app, Message::NudgeEnded(Arrow::Right));
    send(&mut app, Message::NudgeStarted(Arrow::Up));
    assert!(
        (xform(&app).height - was.height).abs() < 0.01,
        "up and down have nothing to do on a side grip"
    );
}

#[test]
fn the_top_grip_gives_the_arrows_the_other_axis() {
    let mut app = selection(200, 200);
    grab_grip(&mut app, gpu::Grab::Resize(gpu::Handle::Top));
    let was = xform(&app);

    send(&mut app, Message::NudgeStarted(Arrow::Up));
    assert!((xform(&app).height - was.height - 1.0).abs() < 0.01);

    send(&mut app, Message::NudgeEnded(Arrow::Up));
    send(&mut app, Message::NudgeStarted(Arrow::Left));
    assert!((xform(&app).width - was.width).abs() < 0.01);
}

#[test]
fn moving_it_with_the_mouse_puts_the_arrows_back_to_moving() {
    let mut app = selection(200, 200);
    grab_grip(&mut app, gpu::Grab::Resize(gpu::Handle::Left));
    grab_grip(&mut app, gpu::Grab::Move);
    let was = xform(&app);

    send(&mut app, Message::NudgeStarted(Arrow::Left));
    assert_eq!(xform(&app).x, was.x - 1.0);
    assert!((xform(&app).width - was.width).abs() < 0.01);
}

#[test]
fn arrows_are_idle_with_nothing_in_hand() {
    let mut app = app(64, 48);
    send(&mut app, Message::NudgeStarted(Arrow::Left));
    assert!(app.nudge.is_none(), "there is nothing to walk about");
}

#[test]
fn a_held_arrow_waits_then_speeds_up_to_a_cap() {
    let mut app = selection(400, 400);
    send(&mut app, Message::NudgeStarted(Arrow::Right));
    let base = Instant::now();
    let tapped = xform(&app).x;

    let mut at = 0.0_f32;
    let run = |app: &mut App, to: f32, at: &mut f32| {
        while *at < to {
            send(app, Message::NudgeTick(base + Duration::from_secs_f32(*at)));
            *at += 1.0 / 60.0;
        }
    };

    run(&mut app, NUDGE_DELAY - 0.02, &mut at);
    assert_eq!(xform(&app).x, tapped, "a tap does not run away on its own");

    let before_slow = xform(&app).x;
    run(&mut app, NUDGE_DELAY + 0.5, &mut at);
    let slow = xform(&app).x - before_slow;
    assert!(slow > 0.0, "it does get going, moved {slow}");

    run(&mut app, 4.0, &mut at);
    let before_fast = xform(&app).x;
    run(&mut app, 4.5, &mut at);
    let fast = xform(&app).x - before_fast;
    assert!(fast > slow, "it speeds up: {slow} then {fast}");
    assert!(
        fast <= NUDGE_FAST * 0.5 + 1.0,
        "but never past the cap, {fast} in half a second"
    );
}

#[test]
fn letting_go_of_a_different_key_does_not_stop_the_held_one() {
    let mut app = selection(200, 200);
    send(&mut app, Message::NudgeStarted(Arrow::Right));
    send(&mut app, Message::NudgeEnded(Arrow::Left));
    assert!(app.nudge.is_some());
    send(&mut app, Message::NudgeEnded(Arrow::Right));
    assert!(app.nudge.is_none());
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
    assert!(app.cutting_out.as_ref().unwrap().mask.as_ref().unwrap()[30 * w as usize + 40] > 128);

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
    assert_eq!(app.menu, Some(MenuPage::About), "open, on the About page");
    send(&mut app, Message::MenuPagePicked(MenuPage::Settings));
    assert_eq!(app.menu, Some(MenuPage::Settings));
    send(&mut app, Message::MenuClosed);
    assert!(app.menu.is_none());
}

#[test]
fn save_as_chooses_the_format_before_the_native_dialog() {
    let mut app = app(200, 200);
    send(&mut app, Message::SaveAsRequested);
    assert_eq!(app.menu, Some(MenuPage::SaveAs));

    send(
        &mut app,
        Message::SaveFormatPicked(doc::io::SaveFormat::Jpeg),
    );
    assert_eq!(app.save_format, doc::io::SaveFormat::Jpeg);
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
fn new_on_an_untouched_canvas_just_closes_the_menu() {
    let mut app = app(64, 48);
    send(&mut app, Message::MenuOpened);
    assert!(!app.doc.modified());
    send(&mut app, Message::NewRequested);
    assert_eq!(
        app.sheets(),
        1,
        "there was already a blank canvas to work on"
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
    let (app, _boot) = App::boot(fixed, None, None, None);
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
    let (mut app, _boot) = App::boot(fitting, None, None, None);
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
    let (mut app, _boot) = App::boot(config, None, None, None);
    app.doc = Document::blank_sized(300, 200, false);
    app.doc.edit();

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
    click(&mut app, 1.0, 1.0);
    send(&mut app, Message::NewRequested);
    assert_eq!(app.doc.size(), (1920, 1080));

    send(
        &mut app,
        Message::NewCanvasPicked(NewCanvas::Fit(crate::canvas::Ratio::Square)),
    );
    click(&mut app, 1.0, 1.0);
    send(&mut app, Message::NewRequested);
    let (w, h) = app.doc.size();
    assert_eq!(w, h, "a square preset gives a square canvas, got {w} x {h}");

    send(&mut app, Message::WindowResized(Size::new(1600.0, 1000.0)));
    click(&mut app, 1.0, 1.0);
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
fn a_custom_colour_can_be_edited_in_place() {
    let mut app = app(60, 60);
    for hex in ["#112233", "#445566"] {
        send(&mut app, Message::PickerOpened);
        send(&mut app, Message::PickerHexEdited(hex.into()));
        send(&mut app, Message::PickerConfirmed);
    }

    send(&mut app, Message::CustomColourMenuOpened(0));
    assert_eq!(app.custom_colour_menu, Some(0));
    send(&mut app, Message::CustomColourEditRequested(0));
    assert_eq!(
        app.picker.as_ref().unwrap().colour(),
        [0x11, 0x22, 0x33, 255]
    );
    send(&mut app, Message::PickerHexEdited("#abcdef".into()));
    send(&mut app, Message::PickerConfirmed);

    assert_eq!(
        app.config.custom_colours,
        vec![[0xab, 0xcd, 0xef, 255], [0x44, 0x55, 0x66, 255]]
    );
    assert_eq!(app.brush.colour, [0xab, 0xcd, 0xef, 255]);
}

#[test]
fn a_custom_colour_can_be_removed_from_its_menu() {
    let mut app = app(60, 60);
    for hex in ["#112233", "#445566"] {
        send(&mut app, Message::PickerOpened);
        send(&mut app, Message::PickerHexEdited(hex.into()));
        send(&mut app, Message::PickerConfirmed);
    }

    send(&mut app, Message::CustomColourMenuOpened(0));
    send(&mut app, Message::CustomColourRemoved(0));
    assert_eq!(app.config.custom_colours, vec![[0x44, 0x55, 0x66, 255]]);
    assert!(app.custom_colour_menu.is_none());
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
fn new_leaves_the_open_document_where_it_is_and_adds_a_tab() {
    let mut app = app(64, 48);
    click(&mut app, 10.0, 10.0);
    assert!(app.doc.modified());
    let size = app.doc.size();

    send(&mut app, Message::NewRequested);
    assert_eq!(app.sheets(), 2, "a second document joined the window");
    assert_eq!(
        app.asking, None,
        "nothing was thrown away, so nothing to ask"
    );
    assert_eq!(
        app.doc.size(),
        app.new_canvas_size(),
        "the new one is in front"
    );

    send(&mut app, Message::TabSelected(0));
    assert_eq!(
        app.doc.size(),
        size,
        "the first document is exactly as it was"
    );
    assert!(app.doc.modified());
}

#[test]
fn turning_the_warning_off_takes_you_at_your_word() {
    let mut app = app(64, 48);
    click(&mut app, 10.0, 10.0);
    assert!(app.unsaved());

    send(&mut app, Message::ConfirmDiscardToggled(false));
    send(&mut app, Message::NewRequested);
    assert_eq!(app.asking, None, "nothing to answer");
    assert_eq!(
        app.doc.size(),
        app.new_canvas_size(),
        "the canvas went without a word"
    );
}

#[test]
fn the_dialog_checkbox_is_the_setting() {
    let mut app = app(64, 48);
    assert!(app.config.confirm_discard);
    send(&mut app, Message::ConfirmDiscardToggled(false));
    assert!(!app.config.confirm_discard, "ticking it off is the setting");
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
fn opening_a_file_joins_the_window_rather_than_replacing_what_is_open() {
    let mut app = app(64, 48);
    click(&mut app, 10.0, 10.0);
    assert!(app.unsaved());

    let opened = Rgba8::new(9, 7, [1, 2, 3, 255]);
    send(
        &mut app,
        Message::Opened(Ok((PathBuf::from("/tmp/x.png"), opened))),
    );
    assert_eq!(app.sheets(), 2);
    assert_eq!(app.doc.size(), (9, 7), "the opened file is in front");
    assert_eq!(
        app.asking, None,
        "nothing was at risk, so nothing was asked"
    );

    send(&mut app, Message::TabSelected(0));
    assert!(
        app.doc.modified(),
        "the work that was already there survived"
    );
}

#[test]
fn a_file_opened_over_an_untouched_canvas_takes_its_place() {
    let mut app = app(64, 48);
    let opened = Rgba8::new(9, 7, [1, 2, 3, 255]);
    send(
        &mut app,
        Message::Opened(Ok((PathBuf::from("/tmp/x.png"), opened))),
    );
    assert_eq!(app.sheets(), 1, "an empty canvas is a slot, not work");
    assert_eq!(app.doc.size(), (9, 7));
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
fn opening_a_file_from_the_menu_returns_to_the_canvas() {
    let mut app = app(32, 32);
    send(&mut app, Message::MenuOpened);
    send(&mut app, Message::MenuPagePicked(MenuPage::Open));
    send(&mut app, Message::OpenRequested);
    assert_eq!(app.menu, Some(MenuPage::Open));

    send(
        &mut app,
        Message::Opened(Ok((
            PathBuf::from("/tmp/opened.png"),
            Rgba8::new(40, 20, RED),
        ))),
    );

    assert!(app.menu.is_none());
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
    let mut face = ttf_parser::Face::parse(crate::assets::UI_FONT, 0).expect("the interface font");
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
        app.doc.modified(),
        "nothing has happened yet, the answer is still coming"
    );

    send(&mut app, Message::DiscardAnswered(Discard::Keep));
    assert!(app.doc.modified(), "cancelling leaves it alone");
    assert_eq!(app.after_save, None, "and nothing is waiting on a save");

    send(&mut app, Message::WindowClosed);
    send(&mut app, Message::DiscardAnswered(Discard::Save));
    assert_eq!(
        app.after_save,
        Some(Pending::Close),
        "the close waits for the save"
    );

    send(&mut app, Message::Saved(Err(String::new())));
    assert_eq!(app.after_save, None, "the close is off");
    assert!(app.doc.modified(), "and the work is still here");
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

    assert!(!app.doc.modified(), "the canvas itself is untouched");
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
fn saving_from_the_close_dialog_then_lets_the_tab_go() {
    let mut app = app(64, 48);
    click(&mut app, 10.0, 10.0);
    send(&mut app, Message::NewRequested);
    click(&mut app, 5.0, 5.0);
    assert_eq!(app.sheets(), 2);

    send(&mut app, Message::TabClosed(1));
    assert_eq!(app.asking, Some(Pending::Tab), "the dialog is up");
    assert_eq!(app.sheets(), 2, "nothing thrown away yet");

    send(&mut app, Message::DiscardAnswered(Discard::Save));
    assert_eq!(app.after_save, Some(Pending::Tab));

    send(&mut app, Message::Saved(Ok(PathBuf::from("/tmp/x.png"))));
    assert_eq!(app.sheets(), 1, "the tab went once its work was safe");
    assert_eq!(app.after_save, None);
}

#[test]
fn closing_the_only_tab_closes_the_window_instead() {
    let mut app = app(64, 48);
    click(&mut app, 10.0, 10.0);
    send(&mut app, Message::TabClosed(0));
    assert_eq!(
        app.asking,
        Some(Pending::Close),
        "that is the window closing"
    );
}

#[test]
fn stepping_wraps_round_the_tabs_in_both_directions() {
    let mut app = app(64, 48);
    click(&mut app, 10.0, 10.0);
    send(&mut app, Message::NewRequested);
    click(&mut app, 5.0, 5.0);
    send(&mut app, Message::NewRequested);
    assert_eq!(app.sheets(), 3);
    assert_eq!(app.active, 2);

    send(&mut app, Message::TabStepped(1));
    assert_eq!(app.active, 0, "past the end comes back to the first");
    send(&mut app, Message::TabStepped(-1));
    assert_eq!(app.active, 2, "and back the other way");
}

#[test]
fn a_closed_tab_leaves_the_order_of_the_rest_alone() {
    let mut app = app(11, 11);
    click(&mut app, 5.0, 5.0);
    send(
        &mut app,
        Message::Opened(Ok((
            PathBuf::from("/tmp/middle.png"),
            Rgba8::new(22, 22, [0, 0, 0, 255]),
        ))),
    );
    send(
        &mut app,
        Message::Opened(Ok((
            PathBuf::from("/tmp/last.png"),
            Rgba8::new(33, 33, [0, 0, 0, 255]),
        ))),
    );
    assert_eq!(app.sheets(), 3);

    send(&mut app, Message::TabSelected(1));
    assert_eq!(app.doc.size(), (22, 22));
    send(&mut app, Message::TabClosed(1));
    assert_eq!(app.sheets(), 2);

    send(&mut app, Message::TabSelected(0));
    assert_eq!(app.doc.size(), (11, 11), "the first is still the first");
    send(&mut app, Message::TabSelected(1));
    assert_eq!(app.doc.size(), (33, 33), "and the last is still the last");
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
fn the_panel_resizes_the_canvas_around_its_centre() {
    let mut app = app(5, 5);
    let middle = (2 * 5 + 2) * 4;
    app.doc.edit().pixels_mut()[middle..middle + 4].copy_from_slice(&[255, 0, 0, 255]);
    send(&mut app, Message::LockAspectToggled(false));

    resize_to(&mut app, "3", "3");
    assert_eq!(pixel(&app, 1, 1), [255, 0, 0, 255]);

    resize_to(&mut app, "6", "6");
    assert_eq!(
        pixel(&app, 2, 2),
        [255, 0, 0, 255],
        "the odd spare pixel goes to the right and bottom"
    );
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
    assert_eq!(
        (app.panel.width.as_str(), app.panel.height.as_str()),
        ("150", "100")
    );
    send(
        &mut app,
        Message::Canvas(gpu::Interaction::ResizePreview(180, 100)),
    );
    assert_eq!(
        (app.panel.width.as_str(), app.panel.height.as_str()),
        ("180", "100")
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
    assert_eq!(
        (app.panel.width.as_str(), app.panel.height.as_str()),
        ("100", "100")
    );
    assert!(!app.doc.can_undo());
}

#[test]
fn a_resize_preview_keeps_percent_fields_in_percent() {
    let mut app = app(200, 100);
    send(&mut app, Message::CanvasUnitPicked(true));
    send(
        &mut app,
        Message::Canvas(gpu::Interaction::ResizePreview(301, 75)),
    );
    assert_eq!(
        (app.panel.width.as_str(), app.panel.height.as_str()),
        ("150.5", "75")
    );
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
