# RustyPaint

<p align="center">
  <img src=".github/assets/rustypaint.png" alt="RustyPaint logo" width="160">
</p>

Paint3D but without the 3D.<br>
A spiritual successor built in Rust with only the 2D tools.

## LICENSE

[GPL-3.0-only](LICENSE)

## What is this?

Microsoft retired Paint3D without ever releasing its source code.
RustyPaint brings back its simple 2D tools that I've personally grown really used to.

It's usable for everyday edits. It includes brushes, erasing, fill, colour picking, transparency, clipboard, image stickers dragged in or opened, crop, rectangle and freeform selection nudged around or stretched with the arrow keys, Smart cutout, shapes, editable curves, adding text and basically everything that Paint3D was able to do with 2D.

It also has some QoL features like dark mode, editable curve and shape points, several pictures open at once, and a configurable new-canvas size. If it ever dies on you, it offers the unsaved work back the next time it starts. Things I wished Paint3D would've had. I also gave it a theme of its own, but the old colour scheme is also available.

It detects image types from their contents, opens several icon and obscure bitmap formats, and Save as can convert between the formats it writes.

## How do I cut something out?

Open Smart cutout, draw a box around what you want, and choose Next. You can keep the subject, keep
the background instead, or keep one colour you click out of the picture. Add and Remove then correct
the selection, and each stroke only changes what it reaches: follow the edge to nudge it, or paint
further in to take the whole area. Edges come out on whole pixels, so what you see is what you lift.
Preview it on the shaded original, a checkerboard or any colour you like, adjust the edge, then
choose Done. Undo also works while refining.

I bundle a replaceable MobileSAM model under GPLv3-compatible MIT and Apache-2.0 licences.
It runs entirely on your computer, without downloads or image uploads. Settings has a Smart Cutout
section: turn object recognition off to select by colour alone, or point it at a folder of your own.
Replacement folders need the compatible encoder and decoder described in the
[model notice](res/models/mobile-sam/NOTICE.md).

## Screenshots

| Classic Light | Rusty Dark |
|---|---|
| ![RustyPaint in Classic Light, screenshot 1](.github/assets/screenshots/classic-light-screenshot-1.png) | ![RustyPaint in Rusty Dark, screenshot 1](.github/assets/screenshots/rusty-dark-screenshot-1.png) |
| ![RustyPaint in Classic Light, screenshot 2](.github/assets/screenshots/classic-light-screenshot-2.png) | ![RustyPaint in Rusty Dark, screenshot 2](.github/assets/screenshots/rusty-dark-screenshot-2.png) |
| ![RustyPaint in Classic Light, screenshot 3](.github/assets/screenshots/classic-light-screenshot-3.png) | ![RustyPaint in Rusty Dark, screenshot 3](.github/assets/screenshots/rusty-dark-screenshot-3.png) |
| ![RustyPaint in Classic Light, screenshot 4](.github/assets/screenshots/classic-light-screenshot-4.png) | ![RustyPaint in Rusty Dark, screenshot 4](.github/assets/screenshots/rusty-dark-screenshot-4.png) |
| ![RustyPaint in Classic Light, screenshot 5](.github/assets/screenshots/classic-light-screenshot-5.png) | ![RustyPaint in Rusty Dark, screenshot 5](.github/assets/screenshots/rusty-dark-screenshot-5.png) |

## How do I install it?

On Linux and macOS, this works out what your system wants and installs it:

```sh
curl -fsSL https://raw.githubusercontent.com/ItzELECTR0/RustyPaint/main/install.sh | sh
```

It asks whether you want the stable or the experimental release, and if it cannot tell what kind of
package your system takes, it asks that too. Then it shows you what it found and waits for a yes
before it installs anything. Pass options after `-s --`, so `| sh -s -- --help` lists them,
`| sh -s -- --stable --format appimage` answers the questions up front, and `--noconfirm` means it
never asks at all.

Releases offer multiple builds. An AppImage, Arch package, Debian package, RPM, Alpine apk, Flatpak bundle, Windows setup program, and macOS DMG, each one for x86_64 and for ARM64. Windows gets an MSI too, but only for x86_64.

On Arch, install the [stable](https://aur.archlinux.org/packages/rustypaint) or [Git](https://aur.archlinux.org/packages/rustypaint-git) AUR package with your favourite helper:

For `yay`:
```sh
yay -S rustypaint
# OR
yay -S rustypaint-git
```

For `paru`:
```sh
paru -S rustypaint
# OR
paru -S rustypaint-git
```

There is also an `experimental` pre-release at the top of the releases list, built from the latest commit on `main`. It is replaced on every push, so it changes without warning and can be broken. Take it only if you want the newest thing rather than a working one.

Windows and macOS packages are not code-signed yet, so those systems will warn before opening them.

## How do I build it?

Rust 1.95 or newer is required. Linux also needs a working Vulkan driver. Windows uses Direct3D 12 and macOS uses Metal.

The default build needs a native ONNX runtime and a C++ toolchain. On Windows, first run
`./packaging/windows/build-cutout-runtime.ps1` in PowerShell 7 with Python 3.12+, CMake 3.28+
and Visual Studio 2022 C++ tools installed (`-Architecture aarch64` for ARM64).
Other supported native targets download a prebuilt runtime during compilation. Use
`--no-default-features` for the slower, pure-Rust inference backend. Alpine and Intel macOS
currently use that portable backend automatically; the bundled model works with either one.

For an optimized build:
```sh
cargo build --release -p rustypaint
```

Package for Arch Linux:
```sh
cd packaging
makepkg -sci
```

Set `_native=1` when the package will only run on the machine that builds it.

To run directly:
```sh
cargo run -p rustypaint -- path/to/image.png
```
(omit the path to start with a new canvas)

## How is it configured?

All options are available in the settings menu in the app and written to these files:

| Where? | Settings file |
|---|---|
| Linux | `~/.config/rustypaint/config.toml` |
| Flatpak | `~/.var/app/net.electris.RustyPaint/config/rustypaint/config.toml` |
| Windows | `%APPDATA%\RustyPaint\config.toml` |
| macOS | `~/Library/Application Support/RustyPaint/config.toml` |

## Where do the assets come from?

No Paint3D assets are used. I made the icons for this program; Urbanist uses the SIL Open Font
License. The bundled model and ONNX runtime have their own notices, available under About.

## Disclosure

I am not affiliated with Microsoft. RustyPaint is not endorsed or acknowledged by Microsoft as an actual successor to Paint3D. It is a community reimplementation of software they chose to discard. This project is intended to preserve the 2D functionality of Paint3D and offer it to platforms the original never did.
