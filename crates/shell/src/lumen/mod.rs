//! The `Lumen` application struct and its methods, split out of `main.rs`
//! (SPLIT-SH6).
//!
//! `main.rs` used to carry one ~7 500-line `impl Lumen` block covering every
//! theme of the shell at once. The submodules here hold that block cut by
//! theme; each is a plain `impl Lumen { … }` next to `use crate::*;`, so the
//! bodies are byte-identical to what `main.rs` held — only the module path and
//! the visibility of methods called from outside their new home differ.

mod a11y_media;
mod ai_answer;
mod automation;
mod bfcache;
mod click;
mod content_visibility;
mod cursor;
mod docking;
mod file_picker;
mod find_bar;
mod form_submit;
mod frame_form_submit;
mod frame_forms;
mod frame_links;
mod gestures;
mod hibernation;
mod hint_mode;
mod keyboard;
mod nav_state;
mod navigation;
mod newtab_page;
mod omnibox_bar;
mod page_snapshot;
mod page_views;
mod palette;
mod panel_data;
mod panel_keys;
mod pip;
mod pointer;
mod printing;
mod resize_grip;
mod scrolling;
mod session;
mod spell_menu;
mod state;
mod tabs_cmd;
mod text_input;
mod viewport;
mod viewport_sync;

pub(crate) use state::Lumen;
