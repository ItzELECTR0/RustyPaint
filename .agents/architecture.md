# Architecture

## Document ownership

The document owns one non-premultiplied RGBA8 bitmap in sRGB. White is a backing behind those
pixels, not initial pixel data. Turning transparency on removes the backing without changing the
bitmap; turning it off composites onto white and is therefore a recorded edit.

There are no layers. The canvas holds committed pixels and the application may hold one live object:
a selection, sticker, shape, curve, or text box. Committing composites that object into the document.
Cancelling a lifted selection restores its hole; cancelling a newly created object simply discards
it.

Document history stores before-and-after regions for local edits and copy-on-write whole buffers for
canvas-wide changes. Live objects sit outside that history. Undo first resolves live state: text uses
its own edit journal, while another live object is cancelled before committed document history moves.

## Coordinates and input

Keep image, viewport-logical, and physical-pixel coordinates explicit. Painting and selection work
in image coordinates. iced layout uses logical pixels. wgpu uniforms and hit targets that must remain
the same visible size use physical pixels.

Dragging a new object shows only its prospective outline until release. Existing live objects remain
grabbable on every tab. A press that commits one live object and starts another is one interaction,
not a discarded click.

High-rate pointer samples for live-object transforms are coalesced until redraw. Release must flush
the last sample before completing the operation. Viewport-only animation such as marching ants and
the caret stays out of the application update loop unless application state actually changes.

## Compatibility

Configuration is a small TOML file with defaults for every field. Older files must continue to load,
unknown newer keys must be ignored, and a malformed or unwritable file must not prevent the editor
from starting. Tests must use an injected path and never read or write the user's real settings.
