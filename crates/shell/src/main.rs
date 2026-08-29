//! Lumen shell вЂ” С‚РѕС‡РєР° РІС…РѕРґР° Р±СЂР°СѓР·РµСЂР°.
//!
//! Р РµР¶РёРјС‹ Р·Р°РїСѓСЃРєР°:
//! - `lumen` вЂ” РѕС‚РєСЂС‹С‚СЊ РїСѓСЃС‚РѕРµ РѕРєРЅРѕ.
//! - `lumen <path.html>` вЂ” СЂР°СЃРїР°СЂСЃРёС‚СЊ С„Р°Р№Р», layout, paint, РЅР°СЂРёСЃРѕРІР°С‚СЊ РІ РѕРєРЅРµ.
//! - `lumen <http(s)://...>` вЂ” Р·Р°РіСЂСѓР·РёС‚СЊ СЃС‚СЂР°РЅРёС†Сѓ РїРѕ СЃРµС‚Рё, layout, paint.
//! - `lumen --dump-source <path-or-url>` вЂ” РїРµС‡Р°С‚СЊ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅРѕРіРѕ HTML РІ stdout.
//! - `lumen --dump-layout <path-or-url>` вЂ” РїРµС‡Р°С‚СЊ layout-РґРµСЂРµРІР° РІ stdout.
//! - `lumen --dump-display-list <path-or-url>` вЂ” РїРµС‡Р°С‚СЊ display list РІ stdout.
//! - `lumen --print-to-pdf <out.pdf> <path-or-url>` вЂ” СЃРѕС…СЂР°РЅРёС‚СЊ СЃС‚СЂР°РЅРёС†Сѓ РєР°Рє PDF (A4).
//! - `lumen --screenshot <out.png> <path-or-url>` вЂ” РґРµС‚РµСЂРјРёРЅРёСЂРѕРІР°РЅРЅС‹Р№ CPU-СЃРЅРёРјРѕРє СЃС‚СЂР°РЅРёС†С‹ РІ PNG.
//! - `lumen --trace-nav <out.json> <path-or-url>` вЂ” С‚Р°Р№РјР»Р°Р№РЅ РѕРґРЅРѕР№ РЅР°РІРёРіР°С†РёРё РІ Chrome-trace С„РѕСЂРјР°С‚Рµ (Perfetto/chrome://tracing).
//! - `lumen --devtools-port <N>` вЂ” Р·Р°РїСѓСЃС‚РёС‚СЊ DevTools WebSocket СЃРµСЂРІРµСЂ РЅР° РїРѕСЂС‚Сѓ N.
//! - `lumen --bidi-port <N>` вЂ” Р·Р°РїСѓСЃС‚РёС‚СЊ WebDriver BiDi WebSocket СЃРµСЂРІРµСЂ РЅР° РїРѕСЂС‚Сѓ N
//!   (SDC-2: РµСЃР»Рё СЃРѕРІРјРµС‰С‘РЅ СЃ РѕС‚РєСЂС‹С‚С‹Рј РѕРєРЅРѕРј вЂ” СЂРµР°Р»СЊРЅС‹Рµ navigate/eval/captureScreenshot).
//! - `lumen --mcp [url]` вЂ” MCP-СЃРµСЂРІРµСЂ (stdio) РґР»СЏ AI-Р°РіРµРЅС‚РѕРІ (Claude, Browser UseвЂ¦), headless.
//! - `lumen --mcp-port <N> [url]` вЂ” MCP-СЃРµСЂРІРµСЂ РЅР° TCP РїРѕСЂС‚Сѓ N (РѕС‚Р»Р°РґРєР° С‡РµСЂРµР· netcat), headless.
//! - `lumen --mcp-live-port <N> <path-or-url>` вЂ” MCP-СЃРµСЂРІРµСЂ РЅР° TCP РїРѕСЂС‚Сѓ N РїСЂРѕС‚РёРІ Р–РР’РћР“Рћ
//!   РѕРєРЅР° (SDC-2, `LiveWindowSession`): `screenshot`/`eval` РІРѕР·РІСЂР°С‰Р°СЋС‚ СЂРµР°Р»СЊРЅС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚.
//!
//! Dump-СЂРµР¶РёРјС‹ РЅРµ СЃРѕР·РґР°СЋС‚ РѕРєРЅР° Рё РЅРµ РёРЅРёС†РёР°Р»РёР·РёСЂСѓСЋС‚ wgpu вЂ” pipeline РїСЂРѕРіРѕРЅСЏРµС‚СЃСЏ
//! РґРѕ РЅСѓР¶РЅРѕР№ С„Р°Р·С‹, СЂРµР·СѓР»СЊС‚Р°С‚ СЃРµСЂРёР°Р»РёР·СѓРµС‚СЃСЏ Рё РїРёС€РµС‚СЃСЏ РІ stdout. РџРѕР»РµР·РЅРѕ РґР»СЏ CI
//! (Р±РµР· GPU), РѕС‚Р»Р°РґРєРё СЃР»РѕР¶РЅС‹С… СЃС‚СЂР°РЅРёС† Рё СЃСЂР°РІРЅРµРЅРёСЏ РІС‹РІРѕРґР° РјРµР¶РґСѓ РІРµСЂСЃРёСЏРјРё.
//!
//! Р’РЅРµС€РЅРёРµ CSS: `<link rel="stylesheet" href="...">` Р·Р°РіСЂСѓР¶Р°РµС‚СЃСЏ СЃ РґРёСЃРєР° РёР»Рё
//! РїРѕ СЃРµС‚Рё вЂ” РІ Р·Р°РІРёСЃРёРјРѕСЃС‚Рё РѕС‚ С‚РѕРіРѕ, РєР°РєРёРј СЃРїРѕСЃРѕР±РѕРј Р·Р°РіСЂСѓР¶РµРЅР° СЃС‚СЂР°РЅРёС†Р°.

mod adblock;
mod address_bar;
mod assets;
mod app;
mod animation_scheduler;
mod automation_server;
mod click_log;
mod health_log;
mod backend_factory;
mod bench_frames;
mod chrome_preview;
mod chrome_ui;
mod cli_args;
mod diag_stderr;
mod doc_extract;
mod display_list_metrics;
mod dump_mode;
mod event_sink;
mod frame_pacing;
mod js_escape;
mod layout_metrics;
mod layout_walk;
mod nav_history;
mod page_source;
mod page_state;
mod parallel_fetch;
mod resource_base;
mod stylesheets;
mod subresources;
mod view_transition;
mod window_metrics;
mod window_mode;
// Private `use` in the crate root is visible to descendants, so these lines
// double as a re-export for the `use crate::*;` of every submodule — no call
// site elsewhere in the crate had to change when SH-3a moved the bodies out.
use automation_server::{run_ipc_server, run_mcp_mode};
use assets::{INTER_FONT, SPELL_DICTS};
use event_sink::StdoutEventSink;
use frame_pacing::{RAF_MIN_INTERVAL_MS, fast_scroll_degrade_disabled, page_offset_fast_disabled};
use lumen::Lumen;
use window_mode::run_window_mode;
use page_load::{LoadEvent, STREAM_PAINT_INTERVAL_MS};
use view_transition::{ViewTransitionEvent, ViewTransitionState};
use cli_args::*;
use dump_mode::{
    PrintOptions, canvas_updates_as_images, default_pdf_output_path, do_print_to_pdf_with_opts,
    render_source_to_png, run_dump_mode, run_print_to_pdf, run_screenshot, run_trace_nav,
};
use layout_metrics::{count_layout_boxes, count_rendered_units};
use nav_history::{JsNavigateRequest, NavEntry, PendingIntercepted};
use page_source::{PageSource, RawPage, page_source_for_automation_url, resolve_js_navigation};
use subresources::{
    LoadedWebFont, PendingWebFont, fetch_and_decode_background_images, fetch_vtt_text,
    load_font_faces, rule_to_font_face,
};
// Only `tests/` calls the helper directly, so an unconditional re-export
// would be an unused import in the ordinary build (SH-3a: a re-export must
// repeat the cfg of whatever actually uses it).
#[cfg(test)]
use page_source::cache_control_no_store;
use persistent_js::PersistentJs;
#[cfg(feature = "v8")]
use persistent_js::V8PersistentJs;
// Used only from `tests/`, so an unconditional re-export would be an unused
// import in the ordinary build.
#[cfg(test)]
use chrome_ui::chrome_node_changes;
#[cfg(test)]
use dump_mode::encode_images_as_pdf;
#[cfg(all(test, feature = "v8"))]
use persistent_js::popstate_eval_source;
use lumen_bidi_server::spawn as bidi_spawn;
mod config;
mod deterministic;
mod devtools;
mod engine_bridge;
mod engine_thread;
mod download;
mod find;
mod frame_log;
mod forms;
mod frames;
mod gc_tick;
mod hints;
mod image_cache;
mod memory_poll;
mod newtab;
mod input;
mod links;
mod lumen;
mod momentum_anim;
mod notification;
mod omnibox;
mod panel_layout;
mod page_context_menu;
mod page_load;
mod page_pipeline;
mod persistent_js;
mod panels;
mod platform;
mod prefetch;
mod reader_view;
mod relayout;
mod render_thread;
mod resource_timing;
mod source_view;
mod spellcheck;
mod startup_trace;
mod storage_stores;
mod svg_image;
pub mod surface;
mod runtime;
mod scripts;
mod scroll;
mod scroll_anim;
mod extensions;
mod scrollbar;
mod session_persist;
mod tab_lifecycle;
mod tabs;
mod theme_tokens;
mod toolbar;
mod tracks;
mod zoom;
mod network_service;

// SPLIT SH-5: helpers that used to live at the bottom of this file.
use crate::display_list_metrics::{
    build_split_placeholder, content_height_of, content_width_of, next_dl_epoch, paint_ordered,
};
use crate::app::about_to_wait::PendingWait;
use crate::doc_extract::{
    DynamicCssBase, extract_style_blocks, extract_title, inline_style_fingerprint,
    stylesheet_link_fingerprint, window_title,
};
use crate::input::dnd::{DND_THRESHOLD, DndState};
use crate::page_pipeline::{
    JsLayoutSnapshot, LayoutSource, LoadedPage, RenderOutcome, dispatch_preload_hints,
    parse_and_layout, render_bytes,
};
use crate::scripts::{
    collect_inline_scripts, collect_scripts_ordered, resolve_script_sources,
    run_scripts_with_dom,
};
#[cfg(test)]
use crate::scripts::{ParserInsertLog, ResolvedScript, ScriptSource, run_scripts};
use crate::frames::FrameHandle;
use crate::frames::{apply_iframe_sandbox_gates, base_url_string, load_frame_sub_documents};
#[cfg(test)]
use crate::frames::{fetch_frame_subresources, frame_access_allowed};
use crate::chrome_ui::ContentAreaDetachment;
use crate::engine_bridge::{
    EngineCommit, EngineJsState, route_eval_js, route_query_js, route_task_js,
    spawn_engine_thread_if_enabled,
};
// Only `tests/` calls the pure decision behind `engine_thread_enabled` directly,
// so an unconditional re-export would be an unused import in the ordinary build.
#[cfg(test)]
use crate::engine_bridge::engine_thread_enabled_from;
use crate::frame_log::{compose_outcome_label, frame_log_nanos, frame_phase_ms};
use crate::relayout::{
    ContentVisibilityChange, collect_cv_auto, diff_cv_state, meta_initial_scale, relayout_page,
    system_font_faces,
};
use crate::storage_stores::{
    idb_store_for_base, idb_store_for_url, ls_store_for_base, lumen_idb_dir, ss_store_for_base,
    sw_store_for_base,
};
#[cfg(test)]
use crate::chrome_ui::{restore_content_area, take_content_area};
use crate::layout_walk::{collect_box_styles, find_video_source, promote_will_change_layers};
use crate::window_metrics::{FullscreenPoll, content_layout_viewport, decide_fullscreen_poll};
use crate::page_state::{PARKED_PAGES_MAX, PageSnapshot, ParkedPage};
use crate::subresources::{decode_image, fetch_and_decode_images, fetch_image_bytes};
use crate::stylesheets::{
    inline_css_imports, link_media_matches, load_linked_stylesheets, print_media_context,
    screen_media_context,
};
#[cfg(test)]
use crate::stylesheets::{collect_link_hrefs, contains_ignore_ascii_case};
use crate::parallel_fetch::parallel_map;
use crate::resource_base::{ResolvedResource, ResourceBase, SW_FETCH_INTERCEPTOR};
use crate::input::keybindings::{KeyCommand, TypeableField, keybinding_for};
use crate::input::winit_events::{css_cursor_to_winit, cursor_icon_for_hover, winit_modifiers_state};
#[cfg(feature = "v8")]
use crate::js_escape::js_string_literal;
use crate::js_escape::{escape_js_string, escape_js_string_char};
use crate::panels::doc_pip_os_window::DocPipOsWindow;
use crate::panels::pip_os_window::PipOsWindow;
use crate::scroll::metrics::{LINE_STEP_CSS_PX, clamp_scroll, page_step};
use crate::session_persist::{dom_blob_of, should_restore_session, source_url_string};
use crate::tab_lifecycle::state::TabState;
use crate::tabs::containers::origin_of_url;
use std::cell::Cell;
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use lumen_core::event::{Event, FetchPriority, SubresourceKind};
use lumen_core::ext::{DisplayColorProfile, EventSink, HyphenationProvider, NullHyphenationProvider, SpellChecker as _, SuspendedHeap};
use lumen_core::geom::{Point, Rect, Size};
use lumen_core::ColorSpace;
use lumen_encoding::KnuthLiangHyphenation;
use lumen_devtools::DevToolsServer;
use lumen_driver::BrowserSession;
use lumen_knowledge::HistoryFts;
use lumen_storage::session_export::{self, ExportedTab, SessionFile};
use lumen_storage::{BfCache, BfCacheEntry, BfCachePayload, FrozenPage, History, SearchHistory};
use lumen_dom::{
    Document, NodeData, NodeId, check_form_gate, check_navigation_gate,
    collect_iframes, check_popup_gate,
    DomPosition, Range, delete_range, insert_text_at, locate_text_offset_range, node_text_content,
};
use std::collections::HashMap;
use lumen_layout::{LayoutBox, Mat4, PaintOrder, SnapContainer, StackingTree, TransitionScheduler};
use lumen_layout::{StartingStyleTracker, compute_style_from_declarations, resolve_starting_style};
use lumen_layout::{collect_scroll_containers, collect_snap_containers, find_scroll_container_at, find_snap_target, set_scroll_position};
#[cfg(feature = "v8")]
use lumen_layout::{collect_computed_styles, collect_custom_properties, collect_layout_rects};
use lumen_layout::apply_intrinsic_size;
use lumen_layout::style::{ComputedStyle, ScrollBehavior};
use lumen_layout::computed_style_to_map;
use lumen_paint::{
    build_display_list_ordered, build_display_list_ordered_with_anim_split, hit_test, DisplayList,
    RenderBackend,
};
use lumen_driver::{
    AutomationCommand, AutomationHandle, AutomationReply, AutomationRequest, BoxModel, ConsoleEntry,
    ConsoleLevel as DriverConsoleLevel, InterceptedRequest, NetworkEntry as DriverNetworkEntry,
    WaitCondition,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{CursorGrabMode, CursorIcon, Window, WindowId};

fn main() -> ExitCode {
    // BUG-770: install the non-blocking stderr sink before anything can print.
    // A parent that captures stderr as a pipe and stops reading it used to
    // block whichever thread called `eprintln!` next вЂ” with the UI thread that
    // froze the whole window mid-run. No-op unless stderr really is a pipe.
    diag_stderr::install();
    let code = run_cli();
    // PERF-12: closes the startup accounting вЂ” see `startup_trace::log_exit`.
    // Before `diag_stderr::flush`, so the line survives the bounded wait below.
    startup_trace::log_exit();
    // The writer thread is detached, so the tail of the log would be lost when
    // `main` returns. Bounded wait: a parent that never reads must not turn
    // process exit into a second hang.
    diag_stderr::flush(std::time::Duration::from_secs(2));
    code
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod navigate_by;
