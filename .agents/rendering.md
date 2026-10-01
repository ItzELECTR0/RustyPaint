# Rendering

The viewport is an iced shader widget backed by one wgpu pipeline, uniform buffer, and canvas
texture. Pan, zoom, caret animation, and marching ants should require uniform updates only.

The canvas texture is keyed by document version. Regions may be uploaded only when they describe
every change since the version already on the GPU; the first frame, a version the application no
longer remembers, or a shape change requires a full upload. This keeps brush strokes on large images
cheap.

The application cannot see which version the renderer has, so each frame carries `gpu::Damage`, a
short log of recently changed regions. The renderer uploads the ones after its own version and falls
back to the whole canvas when the log does not reach back that far. Changes the application cannot
name clear the log. Document versions come from one process-wide counter, so a version the log knows
is always this document's and two same-sized tabs can never be mistaken for each other.

Nearby regions are merged into one upload only while that costs at most half again their area, so
mirrored copies at opposite edges of the same rows do not become a band across the canvas.

The primitive releases the canvas once it is uploaded. The renderer keeps the last primitive until
the next draw, and holding the pixels that long would make the next brush stamp copy all of them.

The optional pixel grid is a shader overlay toggled from the zoom toolbar. Its spacing follows canvas
pixels, its line width follows physical screen pixels, and it draws below editing handles. The
toggle is a saved viewing preference; it never changes document pixels or history. Automatic mode
sets visibility at 800% and above when zoom changes. A manual toggle lasts until the next zoom
change; disabling automatic mode leaves the current visibility alone.

The Rust uniform struct and `src/gpu/shaders/viewport.wgsl` must agree on every field offset, not just
total size. The pipeline tests intentionally verify both. Floating previews render at canvas
resolution so zoomed pixels and the object about to be committed match.

The live blur box is the exception to the floating texture path. Compute shaders cache the selected
algorithm over the canvas, keyed by canvas version and every blur setting; their kernels and
per-pass rounding mirror the CPU commit path. Moving, resizing, and rotating only change the box
uniforms and reveal that cached texture; never rebuild or upload a blurred floating bitmap during
pointer movement.

Selection outlines use physical-pixel metrics. Their phase comes from elapsed time, and dash edges
blend across fractional pixels so movement remains smooth on high-refresh displays. The rectangular
marquee and the alpha-edge outline share the same renderer but have different geometry.

`app::view::Outline` is an `iced` canvas stacked over the shader, and anything with words in it
belongs there rather than in `viewport.wgsl`: the shader has no glyphs, and putting them there would
mean an atlas and a second font path for two small readouts. The lasso being drawn, the selection
size readout, and the rotation dial all live in it. Its geometry is in logical pixels and stays a
fixed size on screen, so a readout is as legible at 800% as at 9%.

The rotation dial uses the drag's original pivot and current target angle. Curve redraws bake the
rotation into their points and replace the bitmap bounds, so those bounds cannot drive the dial.

Offscreen GPU tests create and destroy devices. They are serialized because concurrent device
teardown has crashed Mesa without identifying a failing test. Returning no adapter skips a visual
test; a rendered mismatch fails it.
