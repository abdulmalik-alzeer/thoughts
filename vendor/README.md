# vendor/

Code from other projects that travels inside Thoughts.

## archify/

[Archify](https://github.com/tt-a1i/archify) 3.2.0, Apache-2.0 (earlier parts MIT): the program that draws
diagrams from a JSON document. `crates/thoughtsd/src/figures.rs` carries this folder inside the
daemon, unpacks it under the app's data folder and runs `bin/archify.mjs render` with the Node.js
on the user's machine. Its licences and notices (`LICENSE`, `LICENSE-MIT`, `NOTICE`,
`THIRD_PARTY_NOTICES.md`) travel with it and are unpacked beside it.

- `UPSTREAM` is the commit this copy was taken from.
- Taken from upstream's `archify/` folder, unchanged: `bin/`, `renderers/`, `schemas/`, `locales/`,
  `assets/`, `brand-marks/`, `delta/`, `explainer/`, `migrations/`, `package.json`,
  `skill-release.json`, the licence files, the `.json` files of `examples/` (not the rendered
  pages), and two files of `references/`.
- Added here: `GUIDE.md`, what an assistant reads to write diagram and chart blocks for Thoughts.

To take a newer Archify: replace those folders from the new upstream commit, write its hash to
`UPSTREAM`, check that the examples `GUIDE.md` names still exist, and run
`cargo test -p thoughtsd figures` (it draws every example with this machine's Node.js). The window
reads the drawing out of Archify's page (`ui/src/lib/figures/standalone.ts`) and builds the draw.io
file from the marks Archify puts on it (`data-node-id`, `data-edge-from`, `data-edge-to`): look at a
drawing of each type again after an update.
