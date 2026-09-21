# Assets

Everything used by the running editor is embedded from `res/`. The icons, tool drawings, shapes, and
application icon are project-owned. Urbanist is licensed under the SIL Open Font License recorded in
`res/fonts/OFL.txt`. A fresh build must require only the files tracked in the repository.

Smart cutout model assets and their provenance are in `res/models/mobile-sam/`; runtime notices
are in `res/licenses/onnxruntime/`. Both are embedded in the About licence view. See
`.agents/cutout.md` for the replacement-model contract and test fixtures.

Examples generate visual evidence: `icon-sheet`, `brush-sheet`, `curve-sheet`, `shape-sheet`,
`text-sheet`, and `theme-sheet`. Keep generated artifacts out of Git. The SVGs in `res/` are source;
the application icon PNGs are deliberate generated deliverables for desktop integration.
