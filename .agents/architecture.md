# Architecture

## Application module

`app/` splits the editor by concern rather than by type. `mod.rs` owns `App`, `Message`, and the
state that has no better home; `update.rs` handles messages; `input.rs` turns pointer, key, and
shortcut events into them; `live.rs` owns the live object's whole life from creation to commit,
including crop and Smart cutout; `document.rs` covers saving, history, and whole-canvas pixel work;
`view.rs` builds the widget tree.

Methods on `App` are spread across those files, so anything used outside the file it lives in needs
`pub(super)`, which reaches every `app` submodule and no further. The tests are still one module
because they were written against the whole application and reach freely across it.

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

## Several documents

The open document's state stays on `App` itself, so every tool keeps reaching for `self.doc` and
friends directly. Other open documents are parked as `Sheet` values and swapped in whole. `collapse`
puts the open document back into the tab order and hands over the full list; `expand` takes one back
out. Every tab operation is written as collapse, change the list, expand, so the ordering rules live
in one place instead of being spread through index arithmetic.

`open_in` decides whether a second document joins this window or gets its own. A window is a second
process rather than a second iced window, which keeps the single-window shell and gives each
document its own crash-recovery identity for free.

New and Open no longer replace what is open, so neither one can throw work away and neither asks. An
untouched blank canvas is treated as a slot rather than as work, so the first file opened takes it
over instead of leaving an empty tab behind. Closing a tab switches to it first, which is what lets
the save prompt act on the right document without a second code path.

Each sheet carries its own recovery identity. A parked sheet cannot change, so it is snapshotted once
as it is parked and only has its stamp touched afterwards. Closing the window with unsaved work in a
parked tab leaves that snapshot behind on purpose: the next launch offers it back.

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

## Crash recovery

Unsaved work is snapshotted into `recovery/` beside the settings file: the document's own pixels as
PNG, plus a small TOML file holding the file it came from, whether it has a backing, and a stamp.
The pixels are the document rather than the flattened picture `for_saving` produces, so a restore
puts back exactly what was on screen, live object committed and all.

The stamp is what separates a crashed session from a running one. A live editor rewrites it on every
beat even when nothing changed, so a snapshot older than `STALE_AFTER` belongs to an editor that is
gone. That is why the beat has to keep running while idle, and why `touch` exists as a cheaper
alternative to writing the image again. A launch offers the newest abandoned snapshot; declining
clears them all, recovering takes one and leaves the rest for the next launch. Nothing is deleted
that has not either been recovered or explicitly thrown away.

Saving, and anything that goes through `carry_on`, clears the running session's snapshot, because
the work behind it is either on disk or deliberately gone.

The beat runs on its own thread feeding a channel, because iced's `thread-pool` executor has no
interval helper and `iced::time::every` needs the `tokio` or `smol` backend.

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

## Image files

Opening guesses the format from the file header before falling back to its extension. `image` owns
the standard codecs, `image-extras` registers the read-only obscure codecs, and ICNS is handled
directly so its largest embedded icon can also be decoded when it uses JPEG 2000. Multi-image files
still enter the single-canvas document as one image.

`doc::io::SaveFormat` is the source of truth for Save As choices, extensions, and encoders. The
selected format overrides an old filename extension, and the native dialog receives a matching
default name and filter so Linux and macOS do not need to report which filter was selected.

## Compatibility

Configuration is a small TOML file with defaults for every field. Older files must continue to load,
unknown newer keys must be ignored, and a malformed or unwritable file must not prevent the editor
from starting. Tests must use an injected path and never read or write the user's real settings.
