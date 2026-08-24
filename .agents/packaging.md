# Packaging and release builds

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
