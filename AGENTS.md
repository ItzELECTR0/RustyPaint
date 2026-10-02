# RustyPaint maintenance guide

Rust 2024 workspace for the `rustypaint` editor. Preserve the 2D-only product scope, the quick
single-canvas workflow for flat pictures, layered projects as a deliberate step up from it, and the
existing user-visible behavior.

Run `cargo test --workspace` after changes. Use `cargo fmt --all -- --check` and
`cargo clippy --workspace --all-targets -- -D warnings` before release work.

Read `.agents/architecture.md` when changing documents, image file I/O, undo, floating objects,
coordinates, input handling, dropped files, the clipboard, or the side panel and tab strip.

Read `.agents/layers.md` when changing layers, the layers bar, OpenRaster projects, or how saving
treats layered work.

Read `.agents/brushes.md` when changing brush settings, thickness limits, the eraser, or how strokes
cover the canvas. Re-run its stroke timings after changing anything a stroke does per sample.

Read `.agents/rendering.md` when changing the viewport, GPU resources, shaders, redraws, or visual
tests.

Read `.agents/translations.md` when changing anything the user reads, the Fluent catalogues, or the
locale a build starts in.

Read `.agents/assets.md` when changing bundled art, fonts, or visual reference examples.

Read `.agents/cutout.md` when changing Smart cutout, colour models, graph cuts, or refinement.

Read `.agents/packaging.md` when cutting a release or changing Cargo profiles, versions, desktop
integration, the install script, or any of the distribution packages.
