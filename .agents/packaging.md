# Packaging and release builds

`net.electris.RustyPaint` is the stable application ID. The executable and system package name stay
`rustypaint`. Flatpak rewrites the shared desktop file's icon name to its application ID, as its
repository rules require.

The Arch `PKGBUILD` stays in `packaging/`. makepkg creates `src/` and `pkg/` beside the PKGBUILD, so
placing it at the repository root would collide with project source. `prepare()` copies a clean set
of build inputs into `$srcdir`; it must not mutate the checkout or copy makepkg scratch directories
back into themselves.

`packaging/aur/rustypaint/` and `packaging/aur/rustypaint-git/` are the AUR repositories themselves,
each a git checkout of `ssh://aur@aur.archlinux.org/<name>` nested inside this one and also tracked
by it. Editing a recipe therefore needs two commits: one here and one in the nested repository, whose
own convention is `chore: update to <version>`. They clone
`https://github.com/ItzELECTR0/RustyPaint` rather than borrowing the local checkout. The stable
recipe pins the latest release tag through `#tag=v${pkgver}`; the Git recipe follows HEAD and derives
`pkgver` from `git describe`, matched against `v*` tags so the rolling `experimental` tag cannot
become a version.

`.SRCINFO` is what the AUR actually reads, so it is regenerated rather than hand-edited:

```sh
makepkg -D packaging/aur/<name> --printsrcinfo > packaging/aur/<name>/.SRCINFO
```

A recipe whose `.SRCINFO` still names the old version shows the old version on the AUR no matter what
its `PKGBUILD` says. That matters most for `rustypaint-git`: `pkgver()` recomputes the truth at build
time, but helpers compare `.SRCINFO` unless the user runs them in devel mode, so leaving it behind
means `-Syu` never offers the update.

Everyday development uses optimized dependencies and lightly optimized project code. Release builds
use thin LTO for reasonable iteration time. The `dist` profile is the package build: fat LTO and one
codegen unit. `_native=1` is an explicit local-only package option because such binaries may not run
on older CPUs. Both AUR recipes deliberately enable it because AUR packages are compiled on the
installing machine.

The workspace version is the source of truth for both crates and must not change without maintainer
direction. Version changes are not incidental cleanup, and the maintainer tests the work before the
bump is made rather than after.

## Cutting a release

One commit, `build: release <version>`, carrying every place the version is written:

- `Cargo.toml` workspace version, and `Cargo.lock` refreshed by any cargo command
- `packaging/PKGBUILD`
- `packaging/aur/rustypaint/` `PKGBUILD` and `.SRCINFO`
- `packaging/aur/rustypaint-git/` `PKGBUILD` and `.SRCINFO`
- a `releases` entry in `packaging/flatpak/net.electris.RustyPaint.metainfo.xml`, then
  `appstreamcli validate`

Then an annotated tag, `git tag -a v<version> -m "RustyPaint <version>"`, matching the existing ones.

In the release commit `rustypaint-git` takes the bare version, with no `.rN.gHASH` suffix. The suffix
names the release commit, and no commit can contain its own hash, so a hashed value is only
expressible from a later commit. `pkgver()` derives the real one at build time anyway. Ordering still
holds, and `vercmp` is the way to confirm it rather than reading the strings:

```
0.2.1.r8.gc8e5959  <  0.2.2  <  0.2.2.r0.g<release commit>
```

so later out-of-cycle syncs stay upgrades. Those syncs are `build(aur): sync recipes` here, and carry
the hashed `pkgver` because by then the release commit exists to name.

A sync commit's own `pkgver` names the commit before it, for the same reason: the value is written
into the recipe by the commit that carries it, so it can only name a commit that already exists. The
`rustypaint-git` recipe is therefore permanently one commit behind the sync that updates it, and that
is fine. `pkgver()` recomputes the truth against whatever HEAD the AUR clone lands on, and the stale
`.SRCINFO` value only has to stay lower than that so helpers still offer the update.

Pushing is the maintainer's, always, and `main` goes before the tag or `release.yml` fails its
`--verify-tag`. The AUR repositories are pushed separately:

```sh
git push origin main && git push origin v<version>
git -C packaging/aur/rustypaint push
git -C packaging/aur/rustypaint-git push
```

`.github/workflows/build-packages.yml` builds AppImage, Debian, RPM, Arch, Flatpak, Windows MSI, and
macOS DMG artifacts. It is reusable and has no triggers of its own, so the build steps have a single
owner. `release.yml` calls it for a tag beginning with `v`, creates or updates that tag's GitHub
release, and then submits the MSI to WinGet; a manual run only stores workflow artifacts.
`experimental.yml` calls it for a push to `main` and replaces the rolling `experimental`
pre-release, deleting and recreating it so GitHub lists it above the tagged releases. That build
carries the version in `packaging/PKGBUILD`, which is the last release rather than anything derived
from the commit.

It skips whatever cannot change a package. A push confined to `**.md`, `.agents/` or
`.github/assets/` never starts, and one whose tip message begins `docs` or `build: release` stops at
the `build` job, so a release is built once from its own tag instead of twice. The message test reads
only the tip of the push, so a batch ending on a documentation commit is skipped whatever came before
it and the rolling pre-release stays where it was until the next push. Windows and macOS packages remain unsigned until signing credentials are
configured.

The WinGet package identifier is `ItzELECTR0.RustyPaint` and cannot be renamed without a separate
move request to `microsoft/winget-pkgs`. `winget.yml` owns the submission and matches `\.msi$`
against the release's assets, so only the one Windows installer is sent. `release.yml` calls it for a
tag beginning with `v` once the release exists, and it takes a tag on a manual run as well, which is
how a release that could not be submitted at the time is caught up later. `experimental.yml` never
reaches it.

Komac only adds versions to packages that already exist, so the workflow reads
`manifests/i/ItzELECTR0/RustyPaint` from `microsoft/winget-pkgs` before doing anything. A package
that is not there yet, or a version already published, records a notice and submits nothing instead
of failing the release. Tagging therefore does not depend on the state of the WinGet side, and a run
of the workflow with the tag it skipped is all that is needed once the situation changes.

The workflow needs a `WINGET_TOKEN` repository secret holding a classic personal access token with the
`public_repo` scope, plus a fork of `microsoft/winget-pkgs` on the account that owns the token.
`GITHUB_TOKEN` cannot be used because the pull request targets another repository.

`winget-releaser` drives Komac, which downloads the MSI and reads its architecture, product code and
upgrade code, then carries the descriptive metadata forward from the previous version already in
`winget-pkgs`. That published manifest is the source of truth for the WinGet description, tags and
URLs, so this repository keeps no copy of it. The product code is a fresh random GUID per build; the
upgrade code is derived from the product name and stays stable, which is what lets WinGet recognise
an installed copy across versions.

The first version has to be submitted by hand, since there is nothing for Komac to add a version to:

```sh
komac submit <directory containing manifests/i/ItzELECTR0/RustyPaint/<version>/> --token <token>
```

## File associations

`doc::io::READABLE` decides what the app can open, and three separate files have to agree with it:
`packaging/net.electris.RustyPaint.desktop`, the `file-associations` table in
`crates/rustypaint/Cargo.toml`, and `packaging/windows/file-associations.wxs`. Three tests in
`doc/io.rs` compare them against `READABLE` and against each other, because nothing else notices
when one of them falls behind.

`role` in the packager table is `editor` for a format `SaveFormat` can write and `viewer` for one
the app can only read. macOS is the only platform that reads it, as `CFBundleTypeRole`.

`packaging/mime/net.electris.RustyPaint.xml` fills the gaps in shared-mime-info. It defines
`image/x-portable-arbitrarymap` and `image/vnd.nokia.ota-bitmap`, which the system does not know at
all, and adds `.iris`, `.rgba`, `.bw` to `image/x-sgi` and `.bm` to `image/x-xbitmap`, which it
knows under other extensions only. Everything else in the desktop entry is a type shared-mime-info
already names, so it is referenced rather than redefined. dpkg, RPM and pacman all rebuild the MIME
database from a file trigger on `/usr/share/mime/packages`, so no maintainer script is needed.

The stable AUR recipe does not install that file yet. It builds from `#tag=v${pkgver}`, and the tag
it currently pins predates the file, so the install line would fail. It joins
`packaging/aur/rustypaint/PKGBUILD` with the first release that contains `packaging/mime/`.
`rustypaint-git` follows HEAD and already has it.

cargo-packager 0.11.8 cannot do the Windows half itself. Its WiX template renders
`association.ext` while a `FileAssociation` serialises as `extensions`, so `#each` finds nothing and
the MSI silently ships with no associations at all. `packaging/windows/file-associations.wxs` does
the registration instead, wired in through `windows.wix.fragment-paths`; that path is resolved
against the directory `cargo packager` runs in, which is the repository root, not the crate. The
fragment offers every format through `OpenWithProgids` and a `RegisteredApplications` capabilities
key rather than claiming extensions outright, so installing RustyPaint never takes a file type away
from whatever already owns it. Its component GUID is fixed and must stay that way across versions.

macOS needs code as well as metadata. `CFBundleDocumentTypes` lands in the plist from the packager
table, but Finder then delivers the file as an `application:openURLs:` Apple Event rather than on
the command line, and winit 0.30 does not handle it. `src/open_with/macos.rs` registers an
`NSApplicationDelegate` of its own, which winit explicitly leaves room for, and feeds the paths into
the same subscription dropped files use. Neither the Windows nor the macOS half has been run on its
own platform.

The window carries `net.electris.RustyPaint` as its Linux application id, set in `main.rs` because
iced leaves it empty by default. Compositors match that id against the desktop file's basename to
find the window's icon, so it, the desktop file's name, and `StartupWMClass` have to stay equal; an
empty id leaves a Flatpak window with no icon in the dock at all.

`packaging/flatpak/net.electris.RustyPaint.metainfo.xml` is the AppStream data every Linux package
ships, not just the Flatpak, so software centres read it from the AppImage and RPM too. Keep it
passing `appstreamcli validate`, and add a `releases` entry when a version is tagged. Screenshots are
served from `main` on GitHub rather than pinned to a tag, so they follow the current interface.

The Flatpak manifest builds from a `dir` source pointing at the checkout because CI bundles whatever
commit it is run against, including the rolling `experimental` build. Flathub would need a pinned
`git` source with a tag and commit instead, so that switch belongs with a Flathub submission rather
than in the manifest that feeds the bundle. `runtime-version` has to move together with the
container image in `build-packages.yml`.

Cargo-packager owns the shared Linux, Windows, and macOS bundle metadata in
`crates/rustypaint/Cargo.toml`. RPM metadata lives beside it because cargo-generate-rpm reads the
crate manifest. The AUR-ready `PKGBUILD` and Flatpak manifest remain native definitions rather than
generated approximations.

Regenerate `packaging/flatpak/cargo-sources.json` whenever `Cargo.lock` changes:

```sh
python flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
```

Useful local package checks are:

```sh
cargo packager -p rustypaint --profile dist --formats appimage,deb
```

```sh
cargo generate-rpm -p crates/rustypaint --profile dist
```

```sh
flatpak-builder --force-clean build-flatpak packaging/flatpak/net.electris.RustyPaint.yml
```
