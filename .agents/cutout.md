# Smart cutout

Smart cutout is a GrabCut-style segmentation pipeline: Gaussian-mixture colour models supply unary
costs and a max-flow/min-cut graph supplies the label boundary. Model accumulation uses `f64`; large
photographs overflow useful precision with `f32` sums.

The first run works on a downscaled image, then projects the mask back to full resolution. Pixels
outside the requested box are always background. When the box reaches the image edge, an inner rim
provides background samples. Work should remain limited to the box and its margin rather than the
entire photograph.

Add/remove strokes constrain labels and normally recut with the existing models, so a correction
does not retrain the colours it is correcting. If the first cut collapses, refit from known samples:
the add stroke is foreground and the area outside the box is background. Stop iterative passes once
the labelling settles.

The refinement overlay occupies the live-object slot. Completing the operation lifts the cut into a
shaped selection; optional background fill runs after the lift so its result is not cut out again.
Crop and Smart cutout share frame geometry but not state transitions.
