# Rendering

The viewport is an iced shader widget backed by one wgpu pipeline, uniform buffer, and canvas
texture. Pan, zoom, caret animation, and marching ants should require uniform updates only.

The canvas texture is keyed by document version. A dirty rectangle may be uploaded only when it
describes every change since the version already on the GPU; the first frame, a skipped version, or a
shape change requires a full upload. This invariant keeps brush strokes on large images inexpensive.

The Rust uniform struct and `src/gpu/shaders/viewport.wgsl` must agree on every field offset, not just
total size. The pipeline tests intentionally verify both. Floating previews render at canvas
resolution so zoomed pixels and the object about to be committed match.

Selection outlines use physical-pixel metrics. Their phase comes from elapsed time, and dash edges
blend across fractional pixels so movement remains smooth on high-refresh displays. The rectangular
marquee and the alpha-edge outline share the same renderer but have different geometry.

Offscreen GPU tests create and destroy devices. They are serialized because concurrent device
teardown has crashed Mesa without identifying a failing test. Returning no adapter skips a visual
test; a rendered mismatch fails it.
