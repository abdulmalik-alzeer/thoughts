# glib 0.18.5, with one fix

This folder is the `glib` crate exactly as published on crates.io at version 0.18.5 (MIT, see
`LICENSE` and `COPYRIGHT`), with one change in `src/variant_iter.rs`: `VariantStrIter::impl_get`
passes its out-argument as `&mut p` instead of `&p`. That is the fix for GHSA-wrw7-89jp-8q8g
(RUSTSEC-2024-0429), which upstream released in glib 0.20.0.

Why it is here: Tauri 2 uses gtk 0.18 on Linux, and gtk 0.18 only works with glib 0.18, so the fixed
0.20 cannot be used. The root `Cargo.toml` points every use of `glib` at this folder (`[patch.crates-io]`).
It is built on Linux only.

Remove this folder and the `[patch.crates-io]` entry once Tauri no longer needs glib 0.18.
To check it against the original: download glib 0.18.5 from crates.io and compare; only
`src/variant_iter.rs` and this file differ.
