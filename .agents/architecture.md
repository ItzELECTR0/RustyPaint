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

## Unsaved work

There is no stored "modified" flag. Every history entry carries a serial, the current position names
the state the canvas is in, and a save records that name; the document is modified when the position
has a different name, so undoing back to what is on disk really is no change. A serial from a
discarded redo branch or from an entry trimmed off the bottom can never be reached again, which is
what should happen. Between an edit and the commit that files it the canvas is ahead of its history,
so `touched` covers that gap and a commit clears it. A commit whose region comes out byte for byte
the same records nothing at all.

Anything that would throw away unsaved work goes through one in-window dialog rather than an OS
message box, which on Linux costs a portal round trip the editor should not have to wait for. New,
Open and closing the window all raise the same question and share the answer, and `confirm_discard`
turns the question off for people who would rather not be asked.

## Coordinates and input

Keep image, viewport-logical, and physical-pixel coordinates explicit. Painting and selection work
in image coordinates. iced layout uses logical pixels. wgpu uniforms and hit targets that must remain
the same visible size use physical pixels.

Dragging a new object shows only its prospective outline until release. Existing live objects remain
grabbable on every tab. A press that commits one live object and starts another is one interaction,
not a discarded click.

High-rate pointer samples for live-object transforms are coalesced until redraw. Release must flush
the last sample before completing the operation.

Arrow keys work the live object a pixel at a time, and what they do follows the last grip it was
dragged by: a move grip leaves them moving it, a resize grip leaves them stretching that same edge,
so the axis a side grip does not own stays still. The keyboard's own repeat is ignored and the ramp
is driven off frames instead, holding still for a moment and then climbing to a capped rate, because
the point of the keys is to be followed by eye. A stalled frame is clamped rather than paid back. Viewport-only animation such as marching ants and
the caret stays out of the application update loop unless application state actually changes.

## Dropped files and the clipboard

A dropped image floats over the canvas like a pasted one rather than replacing the document. Paste
prefers pixels on the clipboard, then a copied image file, then text, so copying a file in a file
manager inserts the picture instead of its path. File managers hand over a CRLF separated
`text/uri-list`, and the trailing carriage return has to be trimmed off each path.

winit reports dropped files on Windows, macOS and X11 but not on Wayland, so `dnd::wayland` follows
the drag by hand: a guest `wayland-client` backend over winit's `wl_display` gets its own event
queue on its own thread, binds a `wl_data_device` and accepts `text/uri-list`. Compositors that fan
drag events out to every one of a client's data devices, wlroots among them, deliver drops to it.
Hyprland instead routes to the client's first data device only, which `iced` claims for
smithay-clipboard when it opens the window, so drops never arrive there.

## Compatibility

Configuration is a small TOML file with defaults for every field. Older files must continue to load,
unknown newer keys must be ignored, and a malformed or unwritable file must not prevent the editor
from starting. Tests must use an injected path and never read or write the user's real settings.
