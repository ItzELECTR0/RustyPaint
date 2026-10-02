# Layers and projects

## The model

`Document` holds a `doc::layers::Stack`: canvas-sized layers, bottom first, the index of the active
one, and the backing flag. Every layer is exactly the canvas's size, so tools, history and the GPU
keep using canvas coordinates. The active layer is the only one tools touch, which is why
`Document::pixels` and `Document::edit` still mean what they meant when there was one bitmap: every
brush, fill, selection, shape, text box and cutout works on it unchanged.

A layer has an id, a name, a visible flag and an 8-bit opacity. Ids come from a per-document counter
and are never reused, so history can name a layer that has since been deleted. Opacity is 8 bits
because Krita stores it that way; see the format notes below.

There is no "project mode" flag. `Stack::layered` asks whether a flat picture would lose anything:
more than one layer, or a single layer that is hidden or faded. Everything that cares (Save, Save
As, recovery, Flatten) derives from that, so a picture is a project exactly while it needs to be.

## Interaction

Photoshop is the behavioural reference; Paint's layers panel is the approachability target.

- New layer goes directly above the active one and becomes active, named `Layer <id>`. Duplicate
  goes above its source as `<name> copy`. The first layer of a flat picture is `Background`.
- Delete hands over to the layer below, or the one above when the bottom layer goes. The last
  layer cannot be deleted.
- Merge down composites the active layer into the one below at both layers' opacities and leaves
  the result fully opaque, so the picture looks the same. It is refused while either layer is
  hidden, because there is no answer that keeps both what shows and what is hidden. Flatten drops
  hidden layers, which Photoshop asks about first; here undo brings them back.
- Painting, filling, selecting, drawing a shape or starting text on a hidden layer is refused with a
  status message, because nothing would show.
- The eyedropper samples what the layers show together, so it picks the colour you see. The fill
  reads only the active layer, as Photoshop's paint bucket does unless told otherwise.
- Selecting a layer is not an undo step. Adding, duplicating, deleting, moving, merging, flattening,
  hiding, renaming and opacity are. An opacity drag is one step: `set_opacity` changes the layer
  live and remembers where the stack stood, and `settle` files the step when the slider is released
  or anything else happens first.
- Undoing or redoing a pixel edit on another layer selects that layer, so the change happens where
  it can be seen and the next stroke lands on the layer that was just put back. Undoing a layer
  change restores the layer that was active before it.
- A live object (selection, paste, sticker, shape, curve, text) belongs to the layer it was made on.
  Anything that changes the active layer or the stack's structure commits it there first, the way
  picking another tool does. Visibility, opacity and renaming leave it alone.
- Crop and Smart cutout hold the stack still: the layers panel is shown but inert until they end.
  Cutout's analysis belongs to the active layer as it was, and a crop is about to change every layer.
- Double-clicking a name edits it; Enter, or moving on to anything else, keeps the new name.
- Shortcuts are Photoshop's: Ctrl+Shift+N new layer, Ctrl+J duplicate, Ctrl+E merge down, Ctrl+] and
  Ctrl+[ move the layer up and down.

Not in the first version, with room left for each: groups, blend modes, masks, locks, text that stays
editable as a layer, content outside the canvas, drag-and-drop reordering, selecting several layers,
and per-layer export.

## History

Pixel edits name the layer they changed. `Edit::Layers` records a change to the stack as two
`Arrangement`s (properties only) plus `kept`, the pixels of layers that exist on just one side: a
deleted layer, a duplicate, the result of a flatten. A layer that was only ever added comes back
blank, so a new layer costs nothing in history. Hiding or renaming a layer therefore holds no pixels
at all; holding a copy of a buffer the canvas still owns would make the next stamp copy the whole
layer (see `.agents/brushes.md`). Merge down is an `Edit::Many` of the arrangement and the lower
layer's region, undone last to first. Whole-canvas changes (resize, rotate, flip, crop, backing)
snapshot every layer in an `Edit::Whole`.

## Saving

- Save writes straight to the document's file only when that loses nothing: an OpenRaster path, or
  a flat format for a picture that is not layered. Otherwise it asks where the project goes, with
  OpenRaster chosen. Layers are never flattened by Save.
- Save As preselects OpenRaster for layered work. Picking a flat format there exports: the file gets
  the visible picture, the tab keeps its layers, its own file and its unsaved state, and the status
  bar says so.
- Flatten is the explicit way back to a flat picture.
- A project that opened only as its merged picture (see below) has `merged_only` set: Save always
  asks for a new name, so it can never write a flat file over the original project. The flag is
  kept in the recovery index too.
- Recovery writes a layered document as `<slot>.ora` and a flat one as `<slot>.png`, deleting the
  other, so a restored tab comes back with its layers.

## The format: OpenRaster

Chosen after comparing the candidates against the first version's model.

| Format | Documented? | Who opens it | Verdict |
|---|---|---|---|
| OpenRaster (`.ora`) | Open spec: a ZIP with `stack.xml`, PNG layers, `mergedimage.png`, a thumbnail | Krita, GIMP 3 (Python plug-in), MyPaint, Pinta | Chosen |
| GIMP XCF | Only as GIMP's own notes on its internals; changes with GIMP releases | GIMP; Krita imports only | Not interchange |
| Krita KRA | Only by Krita's source; tiled LZF layer data | Krita | Not interchange |
| PSD | Adobe publishes it, but it is very large and full of legacy structure | Krita, GIMP, Affinity, Unity (flattened), Unreal (flattened) | A possible later export, not a native format |

OpenRaster stores exactly what the first version has: names, order, visibility, opacity, offsets,
the selected layer, 8-bit RGBA pixels as PNG, and a merged picture for viewers. No concrete gap was
found that would justify a `.rspt` format.

What the writer does, and why:

- `mimetype` first and stored, everything else deflated except PNGs, which are stored because they
  are already compressed. The file is written beside the target and renamed over it, so a failed
  save leaves the old project whole.
- Layers are cropped to what they hold and placed with `x`/`y`, as Krita writes them. An empty layer
  is a 1 px PNG.
- The backing is written as a bottom layer, white and `edit-locked`, marked
  `rustypaint:backing="white"` in our own namespace. Other editors have no backing, so this is what
  makes them show the same picture; reading it back turns it into the backing again. Krita drops
  the attribute on its own save, after which the layer comes back as an ordinary locked white layer.
  The picture is unchanged.
- Opacity is written as `(level + 0.25) / 255`, and `0` and `1` exactly, because Krita 5.2.2
  truncates opacity to 8 bits when it reads. A truncating reader and a rounding one both land on the
  same level. Krita's own files lose a level each time it re-opens them (127 becomes 126); ours do not.

What the reader accepts as layers: PNG layers with `svg:src-over`, and groups that can be lifted out
without changing the picture, which means visible or hidden groups at full opacity with normal
compositing. A hidden group hides its layers. Anything else (other blend modes, a faded group, SVG
or text layers, unknown elements) opens as the file's `mergedimage.png`, flat and `merged_only`,
with a status message. Content outside the canvas is trimmed, with a status message, because layers
are canvas-sized; the visible picture is unaffected.

## Interoperability, measured

Krita 5.2.2 (`krita file.ora --export --export-filename out.ora`, under Xvfb):

- Its flattened export of a RustyPaint project matches our composite to within one level per colour
  channel, with alpha exact. That holds with and without the backing.
- It keeps names (Unicode and `&` included), order, visibility, opacity, the selected layer and
  `edit-locked`. Its own files say `version="0.0.1"` and put attributes on the root stack.
- It drops unknown attributes, unreferenced files and our namespace. It keeps content outside the
  canvas through offsets, which we trim.
- It reads a group with no `isolation` attribute as `auto`, although 0.0.6 says the default is
  `isolate`. Groups are an interoperability risk as well as a feature gap.

GIMP 2.10 as packaged by Ubuntu 24.04 has no OpenRaster support, because its plug-in needs Python 2.
GIMP 3's plug-in could not be fetched from the build environment, so GIMP round trips are still to
be checked by hand: save a project with three layers (one hidden, one at 50%), open it in GIMP 3,
compare the composite, save it back and reopen it here.

Save and open timings, release build on 4 cores, `cargo test --release -p rustypaint ora_timings --
--ignored --nocapture`. The bottom layer is a noisy photo, the rest sparse strokes.

| Canvas | Layers | Project | Save | Open | Flat PNG |
|---|---|---|---|---|---|
| 1920x1080 | 5 | 12.4 MB | 112 ms | 79 ms | 5.8 MB, 37 ms |
| 4000x3000 | 5 | 71.5 MB | 626 ms | 556 ms | 33.5 MB, 246 ms |
| 6000x4000 | 3 | 139.6 MB | 1184 ms | 820 ms | 67.5 MB, 427 ms |

A project is about twice the flat PNG, because the merged picture is stored as well as the layers.
Saving runs off the interface thread, so the cost is only how long until the file is complete.

## Cost

Strokes pay nothing for layers: the canvas texture is still the one layer being painted, and the
layer thumbnails wait for a stroke to end. Against the branch point on the same machine, with real
uploads to a software Vulkan device, `stroke_timings` keeps zero whole uploads, canvas copies and
missed pixels in all 72 cases, with the median frame within 4% (noise) and a release about 0.2 ms
slower for the one thumbnail redraw.

Anything that changes the layers under or over the active one recomposites both sides on the CPU,
in parallel above a million pixels. `cargo test --release -p rustypaint layer_timings -- --ignored
--nocapture`, sparse layers over an opaque one:

| Canvas | Layers | Select | Hide another | Add | Merge down |
|---|---|---|---|---|---|
| 1920x1080 | 5 | 8 ms | 6 ms | 8 ms | 18 ms |
| 4000x3000 | 8 | 86 ms | 84 ms | 130 ms | 214 ms |
| 6000x4000 | 8 | 183 ms | 176 ms | 246 ms | 386 ms |

That is fine at ordinary sizes and a visible pause on very large, deep stacks. Tracking each layer's
content bounds, or a texture per layer composited on the GPU, are the two ways out; neither is
needed yet.

## Game engines

None of Unity, Unreal or Godot imports OpenRaster, KRA or XCF. Godot 4.7 imports BMP, DDS, KTX, EXR,
HDR, JPEG, PNG, TGA, WebP and SVG. Unity 6.6 reads PSD and TIFF but flattens layered files on import;
its separate PSD Importer package turns PSD/PSB layers into sprites. Unreal lists PSD among its
import formats, flattened. The practical path is a flattened PNG (Save As with a flat format), and
per-layer PNGs, which are still to come.
