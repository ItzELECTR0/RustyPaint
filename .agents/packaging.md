# Packaging and release builds

`net.electris.RustyPaint` is the stable application ID. The executable and system package name stay
`rustypaint`. Flatpak rewrites the shared desktop file's icon name to its application ID, as its
repository rules require.

The Arch `PKGBUILD` stays in `packaging/`. makepkg creates `src/` and `pkg/` beside the PKGBUILD, so
placing it at the repository root would collide with project source. `prepare()` copies a clean set
of build inputs into `$srcdir`; it must not mutate the checkout or copy makepkg scratch directories
back into themselves.

The AUR recipes live in `packaging/aur/rustypaint/` and `packaging/aur/rustypaint-git/`, ready to be
copied into their separate AUR repositories. They clone `https://github.com/ItzELECTR0/RustyPaint`
rather than borrowing the local checkout. The stable recipe pins the latest release tag manually;
the Git recipe follows HEAD and derives `pkgver` from `git describe`, matched against `v*` tags so
the rolling `experimental` tag cannot become a version.

Everyday development uses optimized dependencies and lightly optimized project code. Release builds
use thin LTO for reasonable iteration time. The `dist` profile is the package build: fat LTO and one
codegen unit. `_native=1` is an explicit local-only package option because such binaries may not run
on older CPUs. Both AUR recipes deliberately enable it because AUR packages are compiled on the
installing machine.

The workspace version is the source of truth for both crates and must not change without maintainer
direction. Keep `packaging/PKGBUILD` synchronized when a release version is chosen; version changes
are not incidental cleanup.

`.github/workflows/build-packages.yml` builds AppImage, Debian, RPM, Arch, Flatpak, Windows MSI, and
macOS DMG artifacts. It is reusable and has no triggers of its own, so the build steps have a single
owner. `release.yml` calls it for a tag beginning with `v`, creates or updates that tag's GitHub
release, and then submits the MSI to WinGet; a manual run only stores workflow artifacts.
`experimental.yml` calls it for every push to `main` and replaces the rolling `experimental`
pre-release, deleting and recreating it so GitHub lists it above the tagged releases. That build
carries the version in `packaging/PKGBUILD`, which is the last release rather than anything derived
from the commit. Windows and macOS packages remain unsigned until signing credentials are
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
