# Packaging and release builds

`net.electris.RustyPaint` is the stable application ID. The executable and system package name stay
`rustypaint`. Flatpak rewrites the shared desktop file's icon name to its application ID, as its
repository rules require.

The Arch `PKGBUILD` stays in `packaging/`. makepkg creates `src/` and `pkg/` beside the PKGBUILD, so
placing it at the repository root would collide with project source. `prepare()` copies a clean set
of build inputs into `$srcdir`; it must not mutate the checkout or copy makepkg scratch directories
back into themselves.

Everyday development uses optimized dependencies and lightly optimized project code. Release builds
use thin LTO for reasonable iteration time. The `dist` profile is the package build: fat LTO and one
codegen unit. `_native=1` is an explicit local-only package option because such binaries may not run
on older CPUs.

The workspace version is the source of truth for both crates and must not change without maintainer
direction. Keep `packaging/PKGBUILD` synchronized when a release version is chosen; version changes
are not incidental cleanup.

`.github/workflows/release.yml` builds AppImage, Debian, RPM, Arch, Flatpak, Windows MSI, and macOS
DMG artifacts. A tag beginning with `v` also creates or updates its GitHub release. Manual runs only
store workflow artifacts. Windows and macOS packages remain unsigned until signing credentials are
configured.

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
