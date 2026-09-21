# Smart cutout

Smart cutout combines optional local MobileSAM recognition with a colour-based GrabCut fallback.
`select/cutout/workflow.rs` owns reusable analysis and mask refinement; `app/cutout.rs` owns the
bounded worker, session revisions, cancellation and refinement history. Inference never runs on
the UI thread. A cancelled in-flight inference finishes in the worker, but its result is discarded.

The bundled model is embedded, replaceable and fully offline. Its source, hashes, licences and
replacement tensor contract live in `res/models/mobile-sam/NOTICE.md`. `runtime.rs` selects native
ONNX Runtime by default, with tract for `--no-default-features`, musl and Intel macOS. Keep the two
backends on identical preprocessing, prompt coordinates and decoder outputs. Package runtime
requirements belong in `.agents/packaging.md`.

Encoding crops the box with a margin and caches its embedding. Decoder corrections reuse it;
edge-only changes reuse the raw mask too. The colour guide only ranks automatic masks while it
called between a tenth and 85% of the box foreground. Outside that band it agrees with a wrong
proposal as readily as a right one, so ranking uses the model's own scores instead, and neither
the point retry nor the low-agreement fallback runs. Independently of the guide, a proposal
covering more than three quarters of the box is penalised and one covering more than nine tenths
is rejected, and every proposal is charged for how much of the box's border band it claims. A
subject rarely lines that band while the backdrop always does, which is the only thing left to go
on when the box fills the picture and so says nothing.

A believable guide no longer overrules the model on its own. It takes a second opinion only when
the proposals are mutually exclusive rather than four versions of one answer, measured as the worst
pairwise overlap between the chosen proposal and the rest. When they all agree, a guide that
disagrees with all of them is the one that is wrong. A proposal the model itself scores below 0.70
takes the second opinion whatever the guide says. The retry prompts the guide's interior point, and
failing that the colour cut takes over and remains the correction engine for the session. These
heuristics are not proof of semantic correctness.

Colour segmentation uses Gaussian mixtures and max-flow/min-cut on a cropped, downscaled region
(longest side 560, guide 192). Accumulation uses `f64` to retain precision on large photographs.
Pixels outside the box stay background. An inner rim supplies background when the box fills the
image. Preserve holes and disconnected components rather than pruning small details automatically.

A box cut is `base`. Every drag, identified by `Dab::stroke`, then corrects it in order, so the same
stroke list always produces the same mask and undo is a truncation. A stroke list that extends the
last one carries its mask forward and only runs the new drags, which is why a correction costs one
decoder pass and not one per drag made so far; anything else replays from `base`. The latched colour
fallback stays the correction engine, so it replays too. With the model, a drag asks
`Encoded::region` what it lies on: its own box around the stroke, its dabs as positive points, the
cached embedding, and the smallest candidate the stroke covers. A proposal covering nine tenths of
the box is refused. Only the part of the change connected to painted pixels is kept, and a remove may
only remove while an add may only add. Re-prompting the model with every stroke at once is what used
to move unrelated areas.

The brush radius is the granularity dial and where the drag lands sets the gear. A drag whose paint
is already a quarter settled in the direction it asks for is straddling the edge and tracing it, so
it reaches half a radius; one laid into what it is changing reaches three. A change inside the reach
is taken whole, edges and all; one that runs further is clipped to it, which is the only time a
correction draws a boundary of its own. So the same brush nudges an edge when you follow it and
takes the panel when you lay it inside, and neither can walk off across the picture. The prompt box
is half again as wide as the reach so the model still sees the edge it should snap to. There is no
separate refining brush: it was only ever the matte, which now always runs.

The colour engine has no such notion, so it recuts with the strokes constraining labels and the same
connectivity rule keeps what the strokes reach. A correction does not retrain the colours it is
correcting. If the first cut collapses, refit from known samples: the add stroke is foreground and
the area outside the box is background. Stop iterative passes once the labelling settles.

The target chooses what the box keeps. Subject is the cut itself, Background inverts it inside the
box, and Colour thresholds distance from a sampled colour instead of running either engine. The
colour is sampled by clicking the picture while the box frame is up, which is the one press the frame
does not already use. Corrections apply to all three. Model choice and whether recognition runs at all
are application settings, not cutout ones.

While a cutout is open it is modal against anything that would move the picture out from under it:
select all, cut and paste do nothing, and the canvas resize grips stay hidden even on the Canvas tab,
because resizing is a document edit that the cutout's own undo cannot reach. The refinement overlay
occupies the live-object slot. Completing the operation lifts the cut into a shaped selection;
optional background fill runs after the lift so its result is not cut out again. Crop and Smart
cutout share frame geometry but not state transitions.

The preview sits the cut on the shaded original, a checkerboard, or a colour chosen from the panel
itself, which is its own colour and never the paint one. The shaded preview is what carries the
outline, because there its alpha is the selection turned inside out. `FloatingFrame::shaded` says so,
and the shader reads the alpha inverted and ignores the texture border, which would otherwise be
traced as the edge of the shading rather than of the cut. Outlines are measured a screen pixel at a
time rather than an image pixel, so they keep their width at any zoom; a lasso selection outline
shares that path.

Full-resolution matting estimates local foreground/background colours in an uncertain boundary band
whose width is the detail radius. It always runs, and its answer is then thresholded and unioned
with the cut, so the edge is whole pixels and the matte may only add coverage, never take it. That
is one behaviour rather than a hard-edge switch: bypassing the matte was what used to lose the
boundary it had found. Smooth blurs and re-thresholds, so it rounds corners while staying hard;
shift grows or shrinks; feather blurs last and is the only thing that leaves partial alpha. Add/remove strokes apply last at full resolution, with the latest stroke
winning. Source alpha is multiplied only when lifting. Optional decontamination changes foreground
RGB, not its source alpha. Never modify the original while previewing.

Jobs carry session ID, revision and document version. Stale results cannot repaint a replacement
document; coalesced changes rerun after the current job. Parked tabs keep their session and resume
pending work on activation. End a brush stroke before parking. Undo/redo restores refinement state
before document history; slider drags are one refinement edit.

`cargo test --workspace` exercises cancellation, parked tabs, cache reuse, source alpha, model
failure, lifting and undo. `cargo test --workspace --no-default-features cutout` checks the portable
backend. The Apache-2.0 truck fixture also exercises real model inference.

`cutout-model` accepts an image and four box coordinates and writes masks/composites under
`target/cutout`. `cutout-stroke` takes the same box plus a stroke (x0 y0 x1 y1 radius adding) and
reports what that one correction changed: how many separate regions, how far each sits from the
stroke, and how much of it the stroke never reached. Anything in that last number is a bug. `cutout-bench` accepts a GrabCut dataset directory, baseline `cutout` executable,
baseline output directory and result directory. It compares identical boxes, derived from reference
bounds plus a 5% image margin, and ignores uncertain reference pixels. Reference masks never enter
segmentation. Keep external datasets and generated results out of Git.

It wants `data_GT` holding the photographs and `boundary_GT` holding same-named `.bmp` trimaps of
0, 128 and 255. Microsoft's original download is gone, so build the directory from the Oxford
`iseg` set at `robots.ox.ac.uk/~vgg/data/iseg/`: `images.tgz` and `images-gt.tgz` carry 49 of the
50 GrabCut images with the trimaps intact, everything but `124080`. Converting its PNG ground truth
to BMP is the only step, and the boxes it yields match the ones the original produced.

`cutout-bench` also dabs at the middle of the largest remaining mistake, up to `CORRECTIONS` times,
which is the closest thing there is to measuring correction effort. Over the 49 GrabCut images the
Oxford iseg set carries, the mean goes 92.88 with no dab, 94.39 after one and 95.42 after three,
with no image made worse. The border and consensus rules took the no-dab mean up from 89.59 and
improved nine images without moving any of the other forty backwards.

`cross` is the weakest case left, and not for want of ranking: the model offers two clean proposals
of the sky and two dithered ones of the churchyard, so picking the right side means picking a mask
that is 19% boundary pixels. `banana1` is next, where all four proposals score near zero and the
colour cut is genuinely the best answer available.

Quality is not established by passing unit tests. Inspect contrasting composites and report mask
overlap, missed foreground, retained background, correction effort and latency. Hair, translucency,
severe model mistakes and platform performance remain explicit evaluation work, not parity claims.
