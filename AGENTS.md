# RustyPaint maintenance guide

Rust 2024 workspace for the `rustypaint` editor. Preserve the 2D-only product scope, the
single-canvas document model, and the existing user-visible behavior.

Run `cargo test --workspace` after changes. Use `cargo fmt --all -- --check` and
`cargo clippy --workspace --all-targets -- -D warnings` before release work.

Read `.agents/architecture.md` when changing documents, undo, floating objects, coordinates, input
handling, dropped files, or the clipboard.

Read `.agents/rendering.md` when changing the viewport, GPU resources, shaders, redraws, or visual
tests.

Read `.agents/assets.md` when changing bundled art, fonts, or visual reference examples.

Read `.agents/cutout.md` when changing Smart cutout, colour models, graph cuts, or refinement.

Read `.agents/packaging.md` when changing Cargo profiles, versions, desktop integration, or the Arch
package.
