# Brushes

## Per-tool settings

`Brush` is the whole brush panel, not one brush. `tool` says which one is selected and
`settings[tool]` holds thickness, opacity, hardness and the antialiasing flag for that tool alone;
the accessors read through it, so there is one copy of each value and nothing to sync when the tool
changes. Colour and tolerance stay shared, which is what Paint 3D does.

The eyedropper temporarily remembers the previous tool and restores it after a nontransparent
canvas sample. Re-selecting the eyedropper keeps that return target; choosing another tool replaces
it. Invalid samples leave the eyedropper active, and the sampling gesture never paints.

Nothing persists to the config file. Paint 3D forgets these on exit too, and a brush panel that
comes back mid-stroke-faint after a restart is worse than one that starts clean.

## Typed values

Every slider in the side panel has a box beside it, because a slider is a bad way to ask for an
exact number and the only reason the thickness needed one is the only reason any of them did.
`app::Field` names each box, converts its own unit, and maps back onto the slider's own message, so
typing goes through exactly the same handler a drag does rather than a second copy of it.

Half-typed text is one `App::typed`, not one per box: only one can hold focus. It carries the field
and the tool it was typed in, so a stale one ages out on its own rather than needing every place
that assigns `brush.tool` to clear it, and `Message::moves_a_field` drops it in one place when a
drag or a nudge takes the value back.

## Thickness limits

`MAX_THICKNESS` is only where the slider stops. `THICKNESS_CEILING` is where the brush stops, and
the field takes anything up to it. They are 200 and 20000: 200 is Paint 3D's slider, and a brush
wider than `canvas::MAX_CANVAS` has nowhere left to land, which
`a_brush_cannot_grow_wider_than_the_largest_canvas` keeps honest. `paint/` cannot name `canvas`
directly because the examples pull it in on its own.

## Strokes

Every pointer sample paints what the brush newly reaches. There is no spacing to cross and no
timer: each sample sweeps the tip from where the brush was to where it is, so a one pixel nudge of
a 200 px eraser erases the sliver it moved into and a release has nothing left to finish.

`Brush::tip` works the brush out once per stroke. Distances are measured in the tip's own frame,
where the calligraphy nib is a circle and a square pixel pen a square, and texture is looked up by
pixel because grain and scatter belong to the canvas.

`Max` brushes (marker, eraser, calligraphy, oil) give each pixel the edge at its distance from the
swept segment, the limit of stamps laid ever closer together without the scallops between them.
The previous sweep's solid core is already at full strength, so for round tips each row skips it
and a short move only touches the new crescent.

`Accumulate` brushes (watercolour, pencil, crayon) deposit a stamp's worth of optical density per
profile spacing travelled, each piece laid at its end so the last lands under the pointer. Density
is a float, so a hundred 1 px samples lay down what one 100 px sample does, give or take the one
stamp of beading the spacing always caused. Compared with the old integer accumulation, watercolour
comes out up to about ten levels darker where passes overlap.

The pixel pen presses its hard tip on every pixel a one pixel line steps through. A one pixel pen
passes each of those pixels through pixel-perfect, so corners are still seen one at a time however
fast the pointer moves. The spray can still puffs per frame, because spray builds with time held.

Coverage and what was under the stroke live in 64 px tiles made on first touch, so pressing on a
large canvas copies and allocates nothing canvas sized, and the undo step holds only changed tiles.
Each mirrored copy reports its own region so copies at opposite edges upload separately.

## Measuring strokes

`app::timings::stroke_timings` drags each brush through the real update path, keeps the previous
frame alive as the renderer does, and plans uploads with the pipeline's own rule. With a GPU adapter
it waits on real uploads; without one the upload column is CPU staging only. Missed pixels are ones
at least 1.5 px inside the swept path that a drag or release left untouched; a one pixel pen is held
to the pixel under the pointer and a stabilised stroke only to where it ends.

```sh
cargo test --release -p rustypaint stroke_timings -- --ignored --nocapture
```

Before and after strokes became sweeps, on a 4 core 2.1 GHz Xeon without an adapter (ms, mean
frame with the worst in brackets):

| Canvas | Brush and movement | Press | Frame | Release | Whole uploads, copies | Missed during, after |
|---|---|---|---|---|---|---|
| 1152x648 | Marker 200, 1 px nudges | 2.5 -> 0.9 | 0.28 (2.5) -> 0.14 (0.9) | 0.03 -> 0.06 | 0, 1 -> 0, 0 | 3283, 1012 -> 0, 0 |
| 1152x648 | Eraser 200, slow short drag | 2.0 -> 0.69 | 0.46 (2.0) -> 0.21 (0.8) | 0.08 -> 0.09 | 2, 3 -> 0, 0 | 58149, 2103 -> 0, 0 |
| 1152x648 | Pixel pen 1, 1 px nudges | 0.4 -> 0.01 | 0.45 (0.57) -> 0.002 (0.012) | 0.01 -> 0 | 7, 8 -> 0, 0 | 8, 1 -> 0, 0 |
| 6000x4000 | Marker 12, slow short drag | 73 -> 0.08 | 86 (88) -> 0.058 (0.17) | 6.6 -> 0.04 | 12, 13 -> 0, 0 | 0, 0 -> 0, 0 |
| 6000x4000 | Marker 200, rapid long drag | 66 -> 2.8 | 114 (135) -> 8.9 (12) | 76 -> 2.8 | 4, 5 -> 0, 0 | 86646, 520 -> 0, 0 |
| 6000x4000 | Eraser 200, reversals | 68 -> 0.73 | 73 (89) -> 0.35 (1.3) | 7.5 -> 0.24 | 18, 19 -> 0, 0 | 66873, 1763 -> 0, 0 |
| 6000x4000 | Watercolour 120, rapid long drag | 73 -> 2.2 | 133 (154) -> 16 (20) | 78 -> 1.9 | 4, 5 -> 0, 0 | 13324, 0 -> 0, 0 |
| 6000x4000 | Pencil 40, reversals | 84 -> 0.14 | 78 (103) -> 0.67 (0.9) | 8.9 -> 0.12 | 20, 21 -> 0, 0 | 296, 15 -> 0, 0 |

The rapid long drag crosses 4800 px in 64 samples at 16 a frame, about 90 000 canvas px/s. Only wide
accumulating brushes get near a frame there, because each sample is still ten overlapping stamps.

## The stabiliser

The brush chases the pointer instead of being it: each sample moves it a fraction of the way there,
and `Brush::follow` is that fraction. Full strength still leaves five percent of the gap, so the
line always converges rather than trailing forever behind a fast hand.

Releasing calls `Stroke::settle`, which walks the brush the rest of the way to the last place the
pointer was and then draws to it exactly, because a stroke that stops short of where the hand
stopped looks like a dropped input. Without the stabiliser the brush is already there, so settling
paints nothing. Only `extend` is smoothed, so the spray can, which puffs at the pointer directly, is
unaffected and does not show the slider.

It starts at zero. A stabiliser that was on by default would change how every existing brush feels.

## The pixel pen

The tip is round by default, which is what Paint 3D does, and a square one is on a picker beside it.
`Profile` carries the choice as `square`, and it swaps the stamp's distance for a Chebyshev one so a
pen wider than one pixel lays down a block instead of a dot. `Settings::square_tip` is per tool like
the rest, and `Brush::profile` only lets it reach a profile when the tool snaps to pixels, so it
cannot leak into the round brushes.

Pixel-perfect drops the doubled corner where a thin line turns, so a staircase stays one pixel
thick. A corner is only known once the pixel after it arrives, so the stamp goes down and is taken
back rather than held: holding it would leave the pixel under a resting cursor missing. Taking one
back is zeroing a single coverage slot, which is why `Brush::drops_corners` also asks for a tip that
covers one pixel. Zeroing restores what was under the stroke, so a stroke that crosses its own
corner loses that earlier pass; strokes that double back on themselves are rare enough to leave it.

Pixel-perfect is per tool too and starts on, and only tools that snap to pixels show it. The
panel keeps its explanation on a tooltip, because the difference is easy to see and hard to name.

## Eraser hardness

Paint 3D's eraser is a plain hard disc at every size. Measured off `screenshot24` to `screenshot26`
in the reference set, the erased alpha area is within a percent of pi r squared for 5, 9, 10, 15,
30, 50, 100, 150 and 200 px, and the radial edge is one partial pixel wide. There is no feathering
in it at all.

The one real difference is below 10 px, where it stops antialiasing and lays down a binary mask.
That size threshold is not reproduced, because a 5 px eraser that is jagged only because it is small
is a bug wearing a feature's clothes. The look itself is on a checkbox instead, at any size, and the
checkbox starts off, so the eraser is crisp until asked otherwise and only matches the screenshots
at 10 px and up once antialiasing is turned on.

`Brush::edge` keeps the rim fixed at half a pixel past the stamp radius and walks the solid core
inwards as hardness drops, so softening an eraser never changes the area it reaches. Hardness 1
leaves exactly the one-pixel band that antialiasing needs, which is why it comes out identical to
the screenshots. Hardness 0 ramps from the centre, which is softer than Paint 3D could ever go.

The ramp is linear at full hardness and smoothstepped at zero, blended by `1 - hardness` between.
Linear is what correct coverage antialiasing wants at the hard end; a linear cone at the soft end
would leave a visible point at the tip and a crease at the rim.

Turning antialiasing off takes the whole edge back to a threshold at the stamp radius, so it answers
hardness rather than combining with it: thresholding a soft ramp would just make a hard disc of about
half the size, which is not what anyone asking for a crunchy eraser wants. The panel shows the
hardness slider only while antialiasing is on rather than leaving a control that does nothing.

Only tools where `edge_is_tunable` is true read hardness or antialiasing. The painting brushes keep the
fixed-width `Profile::feather` that gives each of them its character, so a hardness slider on them
would be fighting their profiles rather than tuning them.
