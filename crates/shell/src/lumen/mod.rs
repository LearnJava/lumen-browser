//! The `Lumen` application struct and its methods, split out of `main.rs`
//! (SPLIT-SH6).
//!
//! `main.rs` used to carry one ~7 500-line `impl Lumen` block covering every
//! theme of the shell at once. The submodules here hold that block cut by
//! theme; each is a plain `impl Lumen { … }` next to `use crate::*;`, so the
//! bodies are byte-identical to what `main.rs` held — only the module path and
//! the visibility of methods called from outside their new home differ.

mod ai_answer;
mod automation;
mod content_visibility;
mod docking;
mod hibernation;
mod page_snapshot;
mod page_views;
mod palette;
mod panel_data;
mod scrolling;
mod session;
mod spell_menu;
mod tabs_cmd;
mod viewport;
