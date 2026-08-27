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
mod window_metrics;
// Private `use` in the crate root is visible to descendants, so these lines
// double as a re-export for the `use crate::*;` of every submodule — no call
// site elsewhere in the crate had to change when SH-3a moved the bodies out.
use automation_server::{run_ipc_server, run_mcp_mode};
use cli_args::*;
use dump_mode::{
    PrintOptions, canvas_updates_as_images, default_pdf_output_path, do_print_to_pdf_with_opts,
    render_source_to_png, run_dump_mode, run_print_to_pdf, run_screenshot, run_trace_nav,
};
use layout_metrics::{count_layout_boxes, count_rendered_units};
use nav_history::{JsNavigateRequest, NavEntry};
use page_source::{PageSource, RawPage, page_source_for_automation_url, resolve_js_navigation};
use subresources::{
    fetch_and_decode_background_images, fetch_vtt_text, load_font_faces, rule_to_font_face,
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
    window_title,
};
use crate::input::dnd::{DND_THRESHOLD, DndState};
use crate::page_pipeline::{dispatch_preload_hints, render_bytes};
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
use crate::engine_bridge::{EngineCommit, EngineJsState, route_eval_js, route_query_js, route_task_js};
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

/// РЎРѕР±С‹С‚РёРµ РѕС‚ background-РїРѕС‚РѕРєР° Р·Р°РіСЂСѓР·РєРё СЃС‚СЂР°РЅРёС†С‹ РІ event loop.
///
/// Р—Р°РіСЂСѓР·РєР° СЂР°Р·Р±РёС‚Р° РЅР° С‡РµС‚С‹СЂРµ С„Р°Р·С‹: (0) `EarlyPreloadHints` вЂ” С…РёРЅС‚С‹ РёР· РїРµСЂРІС‹С…
/// Р±Р°Р№С‚ HTML РґР»СЏ СЂР°РЅРЅРµРіРѕ СЃС‚Р°СЂС‚Р° subresource fetch-РѕРІ; (1) chunks СЃС‹СЂС‹С… Р±Р°Р№С‚ РґР»СЏ
/// РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ РїР°СЂСЃРёРЅРіР° Рё РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹С… РєР°РґСЂРѕРІ С‡РµСЂРµР·
/// `IncrementalTreeBuilder::feed_bytes`; (2) `LoadDone` вЂ” РІСЃРµ Р±Р°Р№С‚С‹ РґРѕСЃС‚СѓРїРЅС‹,
/// Р·Р°РїСѓСЃРєР°РµРј РїРѕР»РЅС‹Р№ pipeline (CSS + РёР·РѕР±СЂР°Р¶РµРЅРёСЏ); (3) `LoadError` вЂ” РѕС€РёР±РєР° fetch.
enum LoadEvent {
    /// No-op wake-up (SDC-2). `winit`'s `ControlFlow::Wait` genuinely parks
    /// the event loop until an OS window event, a scheduled `WaitUntil`
    /// deadline, or a proxied user event arrives вЂ” an `AutomationCommand`
    /// enqueued from a BiDi/MCP thread is none of those, so without this the
    /// loop could sit parked indefinitely and never drain it.
    /// `AutomationHandle::execute` sends this through `load_proxy` right
    /// after queuing a command; `user_event` below does nothing with it вЂ”
    /// merely *receiving* a proxied event is what interrupts `Wait` and
    /// triggers the next `about_to_wait` (where automation commands are
    /// actually drained).
    AutomationWake,
    /// Subresource-С…РёРЅС‚С‹ РёР· РїРµСЂРІРѕРіРѕ chunk HTML (HTML LS В§13.2.6.4.7
    /// В«Speculative HTML parsingВ»). РћС‚РїСЂР°РІР»СЏСЋС‚СЃСЏ Р”Рћ РїРµСЂРІРѕРіРѕ `HtmlChunk`,
    /// С‡С‚РѕР±С‹ sink РјРѕРі РЅР°С‡Р°С‚СЊ Р·Р°РіСЂСѓР¶Р°С‚СЊ CSS/С€СЂРёС„С‚С‹ РµС‰С‘ РІ РїСЂРѕС†РµСЃСЃРµ РїР°СЂСЃРёРЅРіР°.
    /// Р”РµРґСѓРїР»РёРєР°С†РёСЏ СЃ С„РёРЅР°Р»СЊРЅС‹РјРё С…РёРЅС‚Р°РјРё РёР· `LoadDone` вЂ” С‡РµСЂРµР·
    /// `preload_dispatched` РІ `Lumen`.
    /// РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1): РёРґРµРЅС‚РёС„РёРєР°С‚РѕСЂ load-С†РёРєР»Р°,
    /// РїСЂРёСЃРІРѕРµРЅРЅС‹Р№ РІ `reload`/`resumed`. `user_event` РѕС‚Р±СЂР°СЃС‹РІР°РµС‚ СЃРѕР±С‹С‚РёРµ, РµСЃР»Рё
    /// РµРіРѕ generation РЅРµ СЃРѕРІРїР°РґР°РµС‚ СЃ `Lumen::load_generation` вЂ” Р·Р°С‰РёС‚Р° РѕС‚
    /// СѓСЃС‚Р°СЂРµРІС€РёС… СЃРѕР±С‹С‚РёР№ РіРѕРЅРєРё РЅР°РІРёРіР°С†РёР№ (Р±С‹СЃС‚СЂС‹Р№ back/forward РёР»Рё РєР»РёРє РїРѕ РґРІСѓРј
    /// СЃСЃС‹Р»РєР°Рј РїРѕРґСЂСЏРґ), РєРѕС‚РѕСЂС‹Рµ РёРЅР°С‡Рµ РїРѕРґРјРµС€Р°Р»Рё Р±С‹ DOM/CSS РїСЂРѕС€Р»РѕР№ СЃС‚СЂР°РЅРёС†С‹.
    EarlyPreloadHints(Vec<lumen_html_parser::PreloadHint>, ResourceBase, u64),
    /// BUG-757: Р±Р°Р·Р° РґРѕРєСѓРјРµРЅС‚Р° СЃС‚Р°Р»Р° РёР·РІРµСЃС‚РЅР° Рё РѕС‚Р»РёС‡Р°РµС‚СЃСЏ РѕС‚ Р·Р°РїСЂРѕС€РµРЅРЅРѕРіРѕ
    /// Р°РґСЂРµСЃР° (СЃРµСЂРІРµСЂ РѕС‚РІРµС‚РёР» СЂРµРґРёСЂРµРєС‚РѕРј). РћС‚РїСЂР°РІР»СЏРµС‚СЃСЏ РёР· streaming-РїРѕС‚РѕРєР°,
    /// РєР°Рє С‚РѕР»СЊРєРѕ С‚РµР»Рѕ РїРѕС‚РµРєР»Рѕ СЃ С„РёРЅР°Р»СЊРЅРѕРіРѕ hop-Р° вЂ” С‚Рѕ РµСЃС‚СЊ Р”Рћ С‚РѕРіРѕ, РєР°Рє
    /// С‡Р°СЃС‚РёС‡РЅС‹Р№ DOM РЅР°С‡РЅС‘С‚ Р·Р°РєР°Р·С‹РІР°С‚СЊ РєР°СЂС‚РёРЅРєРё Рё С€СЂРёС„С‚С‹, РєРѕС‚РѕСЂС‹Рµ UI-РїРѕС‚РѕРє
    /// СЂРµР·РѕР»РІРёС‚ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ Р±Р°Р·С‹. РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    DocumentBase(ResourceBase, u64),
    /// РћС‡РµСЂРµРґРЅРѕР№ chunk СЃС‹СЂС‹С… Р±Р°Р№С‚ HTML. UTF-8 РіСЂР°РЅРёС†С‹ РЅРµ РІС‹СЂР°РІРЅРёРІР°СЋС‚СЃСЏ вЂ”
    /// `IncrementalTreeBuilder::feed_bytes` Р±СѓС„РµСЂРёР·СѓРµС‚ РЅРµР·Р°РІРµСЂС€С‘РЅРЅС‹Рµ
    /// code-point-С‹ РІРЅСѓС‚СЂРё. РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    HtmlChunk(Vec<u8>, u64),
    /// CSS Р·Р°РіСЂСѓР¶РµРЅ РїР°СЂР°Р»Р»РµР»СЊРЅС‹Рј РїРѕС‚РѕРєРѕРј РґР»СЏ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹С… streaming-РєР°РґСЂРѕРІ.
    /// РњС‘СЂРґР¶РёС‚СЃСЏ РІ `Lumen::stream_sheet` Рё РїСЂРёРјРµРЅСЏРµС‚СЃСЏ РІ `paint_partial_dom`.
    /// РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    CssLoaded(Box<lumen_css_parser::Stylesheet>, u64),
    /// PH1-2c: РєР°СЂС‚РёРЅРєР° `<img>` РґРµРєРѕРґРёСЂРѕРІР°РЅР° РїР°СЂР°Р»Р»РµР»СЊРЅС‹Рј РїРѕС‚РѕРєРѕРј РІРѕ РІСЂРµРјСЏ
    /// streaming. Р РµРіРёСЃС‚СЂРёСЂСѓРµС‚СЃСЏ РІ renderer-Рµ РїРѕ РєР»СЋС‡Сѓ `src` Рё РІС‹Р·С‹РІР°РµС‚ redraw вЂ”
    /// РєР°СЂС‚РёРЅРєРё РїРѕСЏРІР»СЏСЋС‚СЃСЏ РїРѕ РјРµСЂРµ РїСЂРёС…РѕРґР°, Р° РЅРµ СЂР°Р·РѕРј РІ С„РёРЅР°Р»СЊРЅРѕРј `LoadDone`.
    /// Р”Р»СЏ Р°РЅРёРјРёСЂРѕРІР°РЅРЅРѕРіРѕ GIF `animated` РЅРµСЃС‘С‚ РІСЃРµ РєР°РґСЂС‹ (С‚РёРєР°СЋС‚СЃСЏ РІ
    /// `RedrawRequested`); `image` вЂ” РЅСѓР»РµРІРѕР№ РєР°РґСЂ РґР»СЏ РЅРµРјРµРґР»РµРЅРЅРѕР№ РѕС‚СЂРёСЃРѕРІРєРё.
    ImageDecoded {
        src: String,
        image: Box<lumen_image::Image>,
        animated: Option<Box<lumen_image::AnimatedGif>>,
    },
    /// PH3-19: web-С€СЂРёС„С‚ РёР· @font-face url() РґРµРєРѕРґРёСЂРѕРІР°РЅ РІ С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ.
    /// Р РµРіРёСЃС‚СЂРёСЂСѓРµС‚СЃСЏ РІ FontRegistry + MultiFontMeasurer Рё РІС‹Р·С‹РІР°РµС‚ relayout вЂ”
    /// С‚РµРєСЃС‚ РїРѕСЏРІР»СЏРµС‚СЃСЏ РІ fallback-С€СЂРёС„С‚Рµ СЃСЂР°Р·Сѓ, РїРѕРґРјРµРЅСЏРµС‚СЃСЏ РїРѕ РїСЂРёС…РѕРґСѓ (FOUT).
    FontLoaded {
        family: String,
        weight: u16,
        style: lumen_core::FontStyle,
        unicode_range: Vec<lumen_font::UnicodeRange>,
        bytes: Vec<u8>,
    },
    /// Р’СЃРµ Р±Р°Р№С‚С‹ РїРѕР»СѓС‡РµРЅС‹ вЂ” РґР»СЏ С„РёРЅР°Р»СЊРЅРѕРіРѕ РїРѕР»РЅРѕРіРѕ pipeline.
    /// РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    LoadDone(RawPage, u64),
    /// РћС€РёР±РєР° РїСЂРё Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹. РџРѕСЃР»РµРґРЅРµРµ РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    LoadError(String, u64),
    /// BUG-171 СЌС‚Р°Рї 2: С„РёРЅР°Р»СЊРЅС‹Р№ pipeline (parse в†’ JS в†’ fetch РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ в†’
    /// layout) РІС‹РїРѕР»РЅРµРЅ РЅР° С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ; РіРѕС‚РѕРІС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚ РїСЂРёРјРµРЅСЏРµС‚СЃСЏ РЅР°
    /// UI-РїРѕС‚РѕРєРµ (`apply_loaded_page`) Р±РµР· Р±Р»РѕРєРёСЂРѕРІРєРё event loop. РџРѕСЃР»РµРґРЅРµРµ
    /// РїРѕР»Рµ вЂ” generation РЅР°РІРёРіР°С†РёРё (U-1).
    RenderDone(Box<RenderOutcome>, u64),
}

/// Р“РѕС‚РѕРІС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚ С„РёРЅР°Р»СЊРЅРѕРіРѕ pipeline: display-list-СЃС‚СЂР°РЅРёС†Р°, РёСЃС‚РѕС‡РЅРёРє РґР»СЏ
/// relayout Рё Р¶РёРІРѕР№ JS-С…СЌРЅРґР» (РµСЃР»Рё РІРєР»СЋС‡С‘РЅ QuickJS). РўРёРї-Р°Р»РёР°СЃ, С‡С‚РѕР±С‹ РІС‹РЅРµСЃС‚Рё
/// СЃР»РѕР¶РЅСѓСЋ С‚СЂРѕР№РєСѓ РёР· СЃРёРіРЅР°С‚СѓСЂ (`render_bytes`, `RenderOutcome`).
type RenderedPage = (LoadedPage, LayoutSource, Option<Arc<dyn PersistentJs>>);

/// BUG-171 СЌС‚Р°Рї 2: СЂРµР·СѓР»СЊС‚Р°С‚ С„РёРЅР°Р»СЊРЅРѕРіРѕ off-UI-thread СЂРµРЅРґРµСЂР° (`render_bytes`),
/// РїРµСЂРµСЃС‹Р»Р°РµРјС‹Р№ РЅР°Р·Р°Рґ РЅР° UI-РїРѕС‚РѕРє С‡РµСЂРµР· `LoadEvent::RenderDone`.
///
/// Р’СЃРµ РїРѕР»СЏ `Send`: `LoadedPage`/`LayoutSource` вЂ” РѕР±С‹С‡РЅС‹Рµ РґР°РЅРЅС‹Рµ; `js_ctx` вЂ”
/// С…СЌРЅРґР» QuickJS (`Send + Sync` РїРѕ ADR-014, СЃРѕР·РґР°РЅ РЅР° СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєРµ);
/// `preload_dispatched` РІСЂРµРјРµРЅРЅРѕ Р·Р°Р±СЂР°РЅ РёР· `Lumen` РЅР° РІСЂРµРјСЏ СЂРµРЅРґРµСЂР° (РѕРЅ РµРіРѕ
/// РґРµРґСѓРїР»РёС†РёСЂСѓРµС‚) Рё РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ РґР»СЏ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёСЏ.
struct RenderOutcome {
    /// Р“РѕС‚РѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р° + РёСЃС‚РѕС‡РЅРёРє layout + Р¶РёРІРѕР№ JS-С…СЌРЅРґР»; Р»РёР±Рѕ С‚РµРєСЃС‚ РѕС€РёР±РєРё
    /// (`Box<dyn Error>` РЅРµ `Send`, РїРѕСЌС‚РѕРјСѓ РєРѕРЅРІРµСЂС‚РёСЂСѓРµС‚СЃСЏ РІ `String`).
    result: Result<RenderedPage, String>,
    /// РќР°Р±РѕСЂ СѓР¶Рµ СЂР°Р·РѕСЃР»Р°РЅРЅС‹С… preload-С…РёРЅС‚РѕРІ, Р·Р°Р±СЂР°РЅРЅС‹Р№ РёР·
    /// `Lumen::preload_dispatched` РЅР° РІСЂРµРјСЏ СЂРµРЅРґРµСЂР°.
    preload_dispatched: std::collections::HashSet<String>,
}

/// PH3-19: РґРµСЃРєСЂРёРїС‚РѕСЂ @font-face url()-РёСЃС‚РѕС‡РЅРёРєР°, РµС‰С‘ РЅРµ Р·Р°РіСЂСѓР¶РµРЅРЅРѕРіРѕ РІ РїР°РјСЏС‚СЊ.
/// РҐСЂР°РЅРёС‚СЃСЏ РІ `ParsedPage` / `LoadedPage`; `apply_loaded_page` СЃРїР°РІРЅРёС‚
/// С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє fetch+decode РґР»СЏ РєР°Р¶РґРѕРіРѕ, СЂРµР·СѓР»СЊС‚Р°С‚ вЂ” `LoadEvent::FontLoaded`.
struct PendingWebFont {
    /// CSS `font-family` РґРµСЃРєСЂРёРїС‚РѕСЂ.
    family: String,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-weight (400 = normal, 700 = bold).
    weight: u16,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-style.
    style: lumen_core::FontStyle,
    /// РЎС‹СЂР°СЏ СЃС‚СЂРѕРєР° `unicode-range` РґРµСЃРєСЂРёРїС‚РѕСЂР° (None в†’ РїРѕРєСЂС‹РІР°РµС‚ РІСЃРµ РєРѕРґРїРѕРёРЅС‚С‹).
    unicode_range_str: Option<String>,
    /// URL РґР»СЏ fetch (@font-face `src: url(...)`).
    url: String,
}

/// PH3-19: web-С€СЂРёС„С‚, СѓР¶Рµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Р№ Рё РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Р№ РїРѕСЃР»Рµ `FontLoaded`.
/// РЎРїРёСЃРѕРє С…СЂР°РЅРёС‚СЃСЏ РІ `Lumen::web_fonts` Рё РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РґР»СЏ РїРµСЂРµСЃР±РѕСЂРєРё
/// `MultiFontMeasurer` РїСЂРё РєР°Р¶РґРѕРј relayout вЂ” РёРЅР°С‡Рµ resize/scroll-reflow
/// С‚РµСЂСЏРµС‚ web-РјРµС‚СЂРёРєРё Рё РѕС‚РєР°С‚С‹РІР°РµС‚СЃСЏ Рє Inter.
// weight/style С…СЂР°РЅСЏС‚СЃСЏ РґР»СЏ Р±СѓРґСѓС‰РµРіРѕ CSS font-matching (РїРѕ weight/style РґРµСЃРєСЂРёРїС‚РѕСЂР°Рј @font-face).
// Clone: ADR-016 M2.2 вЂ” off-thread relayout Р·Р°С…РІР°С‚С‹РІР°РµС‚ РІР»Р°РґРµСЋС‰РёР№ СЃРЅРёРјРѕРє web-С€СЂРёС„С‚РѕРІ.
#[derive(Clone)]
#[allow(dead_code)]
struct LoadedWebFont {
    /// CSS `font-family` РґРµСЃРєСЂРёРїС‚РѕСЂ.
    family: String,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-weight.
    weight: u16,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-style.
    style: lumen_core::FontStyle,
    /// Р”РёР°РїР°Р·РѕРЅС‹ Unicode РёР· @font-face `unicode-range` РґРµСЃРєСЂРёРїС‚РѕСЂР°.
    unicode_range: Vec<lumen_font::UnicodeRange>,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ sfnt-Р±Р°Р№С‚С‹ (TrueType / OTF РїРѕСЃР»Рµ WOFF/WOFF2-СЂР°СЃРїР°РєРѕРІРєРё).
    bytes: Vec<u8>,
}

/// Р Р°Р·РјРµСЂ РѕРґРЅРѕРіРѕ HTML-chunk РїСЂРё СЂР°Р·Р±РёРІРєРµ РґР»СЏ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ РїР°СЂСЃРёРЅРіР°.
const STREAM_CHUNK_BYTES: usize = 8 * 1024;
/// РњРёРЅРёРјР°Р»СЊРЅС‹Р№ РёРЅС‚РµСЂРІР°Р» РјРµР¶РґСѓ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹РјРё РєР°РґСЂР°РјРё РїСЂРё streaming (РјСЃ) вЂ” ~60 Р“С†.
const STREAM_PAINT_INTERVAL_MS: u128 = 16;
/// Minimum interval between rAF batches (ms) вЂ” vsync gate at 60 Hz.
///
/// Prevents `requestAnimationFrame` from firing more than once per display frame
/// when `RedrawRequested` is delivered at higher frequency (e.g. from scroll events).
const RAF_MIN_INTERVAL_MS: f64 = 1000.0 / 60.0;

/// EventSink, РєРѕС‚РѕСЂС‹Р№ РїРµС‡Р°С‚Р°РµС‚ СЃРµС‚РµРІС‹Рµ СЃРѕР±С‹С‚РёСЏ РІ stdout вЂ” СЌС‚Рѕ Рё РµСЃС‚СЊ
/// В«network logВ» Phase 0, СЂРµР°Р»РёР·СѓСЋС‰РёР№ РїСЂРёРЅС†РёРї в„–4 В«РєР°Р¶РґС‹Р№ РёСЃС…РѕРґСЏС‰РёР№ Р±Р°Р№С‚
/// РІРёРґРµРЅВ». РџРѕР·Р¶Рµ Р·Р°РјРµРЅРёС‚СЃСЏ РЅР° СЃС‚СЂСѓРєС‚СѓСЂРёСЂРѕРІР°РЅРЅС‹Р№ UI-Р»РѕРіРіРµСЂ.
struct StdoutEventSink;

impl EventSink for StdoutEventSink {
    fn emit(&self, event: &Event) {
        // РЎРµС‚РµРІРѕР№ Р»РѕРі РёРґС‘С‚ РІ stderr, С‡С‚РѕР±С‹ stdout dump-СЂРµР¶РёРјРѕРІ РѕСЃС‚Р°РІР°Р»СЃСЏ С‡РёСЃС‚С‹Рј
        // (РЅР° РЅС‘Рј вЂ” С‚РѕР»СЊРєРѕ СЃРµСЂРёР°Р»РёР·РѕРІР°РЅРЅС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚ pipeline-Р°). Р’ РѕРєРѕРЅРЅРѕРј
        // СЂРµР¶РёРјРµ СЂР°Р·РЅРёС†Р° РЅРµРІРёРґРёРјР°: РѕР±Р° РїРѕС‚РѕРєР° РїРѕРїР°РґР°СЋС‚ РІ С‚РµСЂРјРёРЅР°Р».
        match event {
            Event::RequestStarted { url, .. } => eprintln!("в†’ GET {url}"),
            Event::RequestCompleted { url, status, .. } => eprintln!("в†ђ {status} {url}"),
            Event::RequestBlocked { url, reason, .. } => eprintln!("вњ— {url} ({reason})"),
            Event::RequestFailed { url, stage, reason, .. } => {
                eprintln!("вњ— {url} ({}: {reason})", stage.as_str());
            }
            Event::SubresourceHintFound { url, kind, priority } => {
                let label = match kind {
                    SubresourceKind::Stylesheet => "css",
                    SubresourceKind::Script => "js",
                    SubresourceKind::Image => "img",
                    SubresourceKind::Font => "font",
                    SubresourceKind::Preconnect { dns_only: true } => "dns-prefetch",
                    SubresourceKind::Preconnect { dns_only: false } => "preconnect",
                    SubresourceKind::Other { .. } => "preload",
                };
                let prio = match priority {
                    FetchPriority::High => "high",
                    FetchPriority::Medium => "medium",
                    FetchPriority::Low => "low",
                };
                eprintln!("в¤· preload {label} [{prio}] {url}");
            }
            Event::FormSubmit { method, action, body, .. } => {
                if body.is_empty() {
                    eprintln!("вЉў form {method} {action}");
                } else {
                    eprintln!("вЉў form {method} {action} body={body}");
                }
            }
            _ => {}
        }
    }
}

/// Bundled-С€СЂРёС„С‚: СЃС‚Р°С‚РёС‡РµСЃРєРёР№ Inter v4.1 Regular (~411 РљР‘),
/// SIL OFL 1.1, СЃРј. assets/fonts/OFL.txt.
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{CursorGrabMode, CursorIcon, Window, WindowId};

/// `true`, РµСЃР»Рё fast-scroll РґРµРіСЂР°РґР°С†РёСЏ РѕС‚РєР»СЋС‡РµРЅР°
/// (`LUMEN_NO_FAST_SCROLL_DEGRADE=1`). Р”РёР°РіРЅРѕСЃС‚РёРєР°: A/B РїРѕРІРµРґРµРЅРёСЏ Рё СЃРєРѕСЂРѕСЃС‚Рё
/// РЅР° РѕРґРЅРѕРј Р±РёРЅР°СЂРЅРёРєРµ (РїР°С‚С‚РµСЂРЅ `LUMEN_NO_SCROLL_COMPOSITOR`).
fn fast_scroll_degrade_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_FAST_SCROLL_DEGRADE").is_ok_and(|v| v == "1")
    })
}

/// `true`, РµСЃР»Рё С„Р°СЃС‚-РїР°СЃ СЃС‚СЂР°РЅРёС‡РЅРѕРіРѕ СЃРјРµС‰РµРЅРёСЏ РѕС‚РєР»СЋС‡С‘РЅ
/// (`LUMEN_NO_PAGE_OFFSET=1`) Рё С€РµР»Р» СЃРЅРѕРІР° Р·Р°РІРѕСЂР°С‡РёРІР°РµС‚ display list РІ
/// `PushTransform` РєР°Р¶РґС‹Р№ РєР°РґСЂ.
///
/// BUG-405 СЃСЂРµР· 38: СЂС‹С‡Р°Рі Р·Р°РІРµРґС‘РЅ СЂР°РґРё РёРЅС‚РµСЂР»РёРІРµРґ-A/B РЅР° РћР”РќРћРњ Р±РёРЅР°СЂРЅРёРєРµ
/// (`scripts/build_phase_census.py --arms offset`) вЂ” РёРЅР°С‡Рµ РїР»РµС‡Рё В«РґРѕВ» Рё
/// В«РїРѕСЃР»РµВ» РїСЂРёС€Р»РѕСЃСЊ Р±С‹ РјРµСЂРёС‚СЊ СЂР°Р·РЅС‹РјРё СЃР±РѕСЂРєР°РјРё, С‡С‚Рѕ `docs/perf-method.md`
/// Р·Р°РїСЂРµС‰Р°РµС‚. Р—Р°РѕРґРЅРѕ СЌС‚Рѕ РѕС‚РєР°С‚ РЅР° СЃР»СѓС‡Р°Р№, РµСЃР»Рё С„Р°СЃС‚-РїР°СЃ РІСЃРєСЂРѕРµС‚ РґРµС„РµРєС‚ РІ
/// РєР°РєРѕРј-С‚Рѕ Р±СЌРєРµРЅРґРµ.
fn page_offset_fast_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_PAGE_OFFSET").is_ok_and(|v| v == "1"))
}

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

/// Parses the command line and dispatches to the chosen `CliMode`.
///
/// Split out of `main` so that every `return` inside it вЂ” including the early
/// argument-error paths вЂ” is followed by `diag_stderr::flush`.
fn run_cli() -> ExitCode {
    // Opt-in visual profiler (В§14.3, BUG-284): `Client::start()` spawns the
    // background thread that connects to (or is discovered by) a running
    // Tracy GUI app вЂ” https://github.com/wolfpld/tracy. Must be started
    // before any `lumen_core::tracy_zone!` spans fire, so this is the very
    // first thing main() does. No-op unless built with `--features tracy`.
    #[cfg(feature = "tracy")]
    let _tracy_client = tracy_client::Client::start();
    #[cfg(feature = "tracy")]
    eprintln!("[tracy] РїСЂРѕС„РёР»РёСЂРѕРІС‰РёРє Р°РєС‚РёРІРµРЅ вЂ” РѕС‚РєСЂРѕР№ Tracy GUI, С‡С‚РѕР±С‹ СѓРІРёРґРµС‚СЊ С‚Р°Р№РјР»Р°Р№РЅ");

    // Anchor for launch->first-frame timing (В§4 score table) вЂ” before any work.
    bench_frames::mark_process_start();
    // PERF-12: fixed-startup stopwatch. Must precede the config load and the
    // argument parse below, which are the phases it measures; it also switches
    // the tracer on for `--trace-nav`, so that startup lands on the timeline
    // instead of ahead of its origin.
    let startup = startup_trace::Startup::begin();
    let cfg_phase = startup.phase("config-load");
    // Load the fingerprint profile (9F.1) once, before any network or JS setup.
    // Absent config в†’ engine defaults, so behaviour is unchanged out of the box.
    let mut startup_profile = config::load().unwrap_or_default();
    // BUG-295: automation sessions (BiDi / MCP) use an in-memory HTTP cache, never
    // the persistent on-disk one. The disk cache is keyed by URL and survives across
    // runs, so on the fixed ports an automation server reuses (e.g. wptserve's
    // 8000/8001) a resource fetched in one run is replayed stale in the next вЂ” even
    // after the served file changed on disk. That silently broke
    // `tests/wpt/run_smoke.py`: the first run (before the wptrunner `env_options` fix
    // served the right file) cached the wrong `testharnessreport.js` with its
    // `Cache-Control: max-age=3600`, and every later run kept serving that stale copy
    // from disk, setting the wrong result global forever, so the harness timed out no
    // matter what else was fixed. In-memory cache = fresh per process, deterministic.
    // This must be decided BEFORE `init_global` вЂ” the profile `OnceLock` is set-once,
    // so a later `init_global` is a no-op вЂ” hence a raw arg scan here rather than
    // reusing the `extract_*` parsers below.
    if std::env::args()
        .any(|a| matches!(a.as_str(), "--bidi-port" | "--mcp-live-port" | "--mcp" | "--mcp-port"))
    {
        startup_profile.no_persistent_state = true;
    }
    config::init_global(startup_profile);
    drop(cfg_phase);

    let arg_phase = startup.phase("arg-parse");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (devtools_port, rest_args) = match extract_devtools_port(&args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (bidi_port, rest_args) = match extract_bidi_port(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (mcp_live_port, rest_args) = match extract_mcp_live_port(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (import_session, rest_args) = match extract_import_session(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° --import-session: {err}");
            return ExitCode::FAILURE;
        }
    };
    let (no_scrollbar, rest_args) = extract_no_scrollbar(&rest_args);
    let (maximized, rest_args) = extract_maximized(&rest_args);
    let (click_log_flag, rest_args) = extract_click_log(&rest_args);
    click_log::init(click_log_flag);
    // PERF-6: session health journal. Turned on by `--activity-log`/`--click-log`
    // (shared surface), the dedicated `--health-log`, or `LUMEN_HEALTH_LOG=1`.
    let (health_log_flag, rest_args) = extract_health_log(&rest_args);
    health_log::init(click_log_flag || health_log_flag);
    let (det_cfg, rest_args) = deterministic::extract_deterministic(&rest_args);
    let (viewport_override, rest_args) = extract_viewport_override(&rest_args);
    let (pdf_output, rest_args) = extract_print_to_pdf(&rest_args);
    let (screenshot_output, rest_args) = extract_screenshot(&rest_args);
    let (trace_nav_output, rest_args) = extract_trace_nav(&rest_args);
    let (mcp_mode, rest_args) = extract_mcp_mode(&rest_args);
    let (use_network_service, rest_args) = extract_network_service(&rest_args);
    let (ipc_server, rest_args) = extract_ipc_server(&rest_args);
    let (proxy, rest_args) = match extract_proxy(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° --proxy: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Р•СЃР»Рё РїСЂРѕРєСЃРё РїРµСЂРµРґР°РЅ РІ РєРѕРјР°РЅРґРЅРѕР№ СЃС‚СЂРѕРєРµ, РїРµСЂРµРѕРїСЂРµРґРµР»РёС‚СЊ РєРѕРЅС„РёРі.
    if let Some(proxy_str) = proxy {
        let mut cfg = config::global().clone();
        cfg.proxy = Some(proxy_str);
        config::init_global(cfg);
    }

    let (tor_port, rest_args) = extract_tor_mode(&rest_args);

    // --tor: РїРµСЂРµРєР»СЋС‡РёС‚СЊ РЅР° РїСЂРѕС„РёР»СЊ TorBrowser + SOCKS5 + Р±РµР· РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРіРѕ С…СЂР°РЅРёР»РёС‰Р°.
    if let Some(port) = tor_port {
        if !check_tor_connectivity(port) {
            eprintln!(
                "lumen --tor: Tor-РґРµРјРѕРЅ РЅРµРґРѕСЃС‚СѓРїРµРЅ РЅР° 127.0.0.1:{port} вЂ” \
                 Р·Р°РїСѓСЃС‚РёС‚Рµ Tor РїРµСЂРµРґ Р·Р°РїСѓСЃРєРѕРј Lumen"
            );
            return ExitCode::FAILURE;
        }
        let mut cfg = config::global().clone();
        cfg.http_profile = lumen_network::HttpProfile::TorBrowser;
        cfg.socks5_proxy = Some(format!("socks5://127.0.0.1:{port}"));
        cfg.no_persistent_state = true;
        config::init_global(cfg);
        eprintln!(
            "lumen: Tor-СЂРµР¶РёРј Р°РєС‚РёРІРёСЂРѕРІР°РЅ (socks5://127.0.0.1:{port}, \
             РїСЂРѕС„РёР»СЊ TorBrowser, Р±РµР· РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРіРѕ С…СЂР°РЅРёР»РёС‰Р°)"
        );
    }

    let cli = if let Some(output) = pdf_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::PrintToPdf { source, output }
    } else if let Some(output) = screenshot_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::Screenshot { source, output }
    } else if let Some(output) = trace_nav_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::TraceNav { source, output }
    } else if let Some(port) = ipc_server {
        CliMode::IpcServer { port }
    } else if let Some(mcp) = mcp_mode {
        CliMode::Mcp(mcp)
    } else {
        match parse_cli(&rest_args) {
            Ok(m) => m,
            Err(err) => {
                eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
                print_usage();
                return ExitCode::FAILURE;
            }
        }
    };

    drop(arg_phase);
    let svc_phase = startup.phase("services-init");

    if let Some(port) = devtools_port
        && let Err(e) = DevToolsServer::spawn(port)
    {
        eprintln!("РћС€РёР±РєР° Р·Р°РїСѓСЃРєР° DevTools РЅР° РїРѕСЂС‚Сѓ {port}: {e}");
        return ExitCode::FAILURE;
    }

    // SDC-2: automation channel created here (not inside run_window_mode) so
    // BiDi/MCP front-ends spawned below get a handle that stays valid once the
    // live window's event loop starts draining `automation_rx`. Without an
    // open window (e.g. --dump/--screenshot/--mcp combined with --bidi-port),
    // the receiver is simply never drained and calls through the handle time out.
    let (automation_cmd_tx, automation_rx) =
        std::sync::mpsc::channel::<AutomationRequest>();
    let automation_handle = AutomationHandle::new(automation_cmd_tx.clone());

    if let Some(port) = bidi_port
        && let Err(e) = bidi_spawn(port, automation_handle.clone())
    {
        eprintln!("РћС€РёР±РєР° Р·Р°РїСѓСЃРєР° BiDi РЅР° РїРѕСЂС‚Сѓ {port}: {e}");
        return ExitCode::FAILURE;
    }

    if let Some(port) = mcp_live_port
        && let Err(e) = lumen_mcp::spawn_live(port, automation_handle.clone())
    {
        eprintln!("РћС€РёР±РєР° Р·Р°РїСѓСЃРєР° MCP (live) РЅР° РїРѕСЂС‚Сѓ {port}: {e}");
        return ExitCode::FAILURE;
    }

    let blocked_log = Arc::new(std::sync::Mutex::new(
        panels::shields_panel::BlockedLog::default(),
    ));
    let network_log = Arc::new(std::sync::Mutex::new(
        devtools::network_panel::NetworkLog::default(),
    ));
    // Sink chain: StdoutEventSink в†’ NetworkLogSink в†’ ResourceTimingSink в†’
    // ShieldCountSink. Each wrapper forwards to its inner sink, so all four
    // observe every event вЂ” the Resource Timing capture (BUG-839) is a tap, not
    // a filter.
    let event_sink: Arc<dyn EventSink> = Arc::new(panels::shields_panel::ShieldCountSink {
        inner: Arc::new(resource_timing::ResourceTimingSink {
            inner: Arc::new(devtools::network_panel::NetworkLogSink {
                inner: Arc::new(StdoutEventSink),
                log: Arc::clone(&network_log),
            }),
        }),
        log: Arc::clone(&blocked_log),
    });

    // PH1-4: Р—Р°РїСѓСЃС‚РёС‚СЊ СЃРµС‚РµРІРѕР№ СЃРµСЂРІРёСЃ РєР°Рє РґРѕС‡РµСЂРЅРёР№ РїСЂРѕС†РµСЃСЃ (РµСЃР»Рё --network-service).
    // РҐРµРЅРґР» Р¶РёРІС‘С‚ РґРѕ РєРѕРЅС†Р° main() вЂ” РїСЂРё РґСЂРѕРїРµ СѓР±РёРІР°РµС‚ РґРѕС‡РµСЂРЅРёР№ РїСЂРѕС†РµСЃСЃ.
    // _transport С…СЂР°РЅРёС‚ Arc, С‡С‚РѕР±С‹ РЅРµ РґСЂРѕРїРЅСѓС‚СЊ IPC-СЃРѕРµРґРёРЅРµРЅРёРµ РґРѕ РєРѕРЅС†Р° СЃРµСЃСЃРёРё.
    let (_network_svc, _transport) = if use_network_service {
        match network_service::NetworkServiceHandle::spawn() {
            Ok((handle, transport)) => {
                eprintln!("lumen: СЃРµС‚РµРІРѕР№ СЃРµСЂРІРёСЃ Р·Р°РїСѓС‰РµРЅ (PH1-4, --network-service)");
                (Some(handle), Some(transport))
            }
            Err(e) => {
                eprintln!("lumen: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ СЃРµС‚РµРІРѕР№ СЃРµСЂРІРёСЃ: {e}");
                eprintln!("lumen: РїСЂРѕРґРѕР»Р¶Р°СЋ СЃРѕ РІСЃС‚СЂРѕРµРЅРЅС‹Рј HttpClient");
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // --import-session РїРµСЂРµРѕРїСЂРµРґРµР»СЏРµС‚ РёСЃС‚РѕС‡РЅРёРє СЃС‚СЂР°РЅРёС†С‹ Рё РЅР°С‡Р°Р»СЊРЅС‹Р№ scroll.
    let (cli, initial_scroll) = match import_session {
        Some((session_source, scroll)) => (CliMode::OpenWindow(session_source), scroll),
        None => (cli, (0.0_f32, 0.0_f32)),
    };

    drop(svc_phase);
    startup.dispatch(cli.mode_name());

    match cli {
        CliMode::Dump { source, kind } => {
            run_dump_mode(&source, kind, event_sink, viewport_override)
        }
        CliMode::OpenWindow(source) => run_window_mode(source, event_sink, blocked_log, network_log, initial_scroll, no_scrollbar, maximized, det_cfg, viewport_override, automation_handle, automation_cmd_tx, automation_rx, bidi_port.is_some() || mcp_live_port.is_some()),
        CliMode::PrintToPdf { source, output } => run_print_to_pdf(&source, &output, event_sink),
        CliMode::Screenshot { source, output } => {
            run_screenshot(&source, &output, event_sink, viewport_override)
        }
        CliMode::TraceNav { source, output } => run_trace_nav(&source, &output, event_sink),
        CliMode::Mcp(mcp) => run_mcp_mode(mcp),
        CliMode::IpcServer { port } => run_ipc_server(port, event_sink),
    }
}

/// Collect the concatenated text content of `id`'s subtree (SDC-2 `Query` support).
fn collect_automation_text(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children {
        collect_automation_text(doc, child, out);
    }
}

/// Convert a `lumen_a11y::AXNode` into the driver's public `A11yNode` reply
/// type (SDC-2 `A11yTree` support). Mirrors `lumen_driver`'s own private
/// conversion in `session.rs`/`winit_session.rs` вЂ” kept local here since the
/// shell has no dependency the other direction.
fn automation_ax_node(ax: &lumen_a11y::AXNode) -> lumen_driver::A11yNode {
    let state = lumen_driver::A11yState {
        disabled: ax.state.disabled,
        checked: ax.state.checked,
        expanded: ax.state.expanded,
        hidden: ax.state.hidden,
        selected: ax.state.selected,
        pressed: ax.state.pressed,
        required: ax.state.required,
        readonly: ax.state.readonly,
        invalid: ax.state.invalid,
        level: ax.state.level,
    };
    lumen_driver::A11yNode {
        node_id: ax.node_id.index() as u32,
        role: ax.role.as_str().to_owned(),
        name: ax.name.clone(),
        description: ax.description.clone(),
        placeholder: ax.placeholder.clone(),
        state,
        children: ax.children.iter().map(automation_ax_node).collect(),
    }
}

/// P3-spell СЃСЂРµР· 2: СЃР»РѕРІР°СЂРё Hunspell, Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ С„РѕРЅРѕРІС‹Рј РїРѕС‚РѕРєРѕРј РїСЂРё СЃС‚Р°СЂС‚Рµ
/// РѕРєРЅР° РёР· `data/spell/` (`spellcheck::load_dictionaries`). Р”Рѕ Р·Р°РІРµСЂС€РµРЅРёСЏ
/// Р·Р°РіСЂСѓР·РєРё `get()` РІРѕР·РІСЂР°С‰Р°РµС‚ `None` Рё СЃРїРµР»Р»-С‡РµРє РјРѕР»С‡РёС‚.
static SPELL_DICTS: std::sync::OnceLock<spellcheck::MultiDictionary> = std::sync::OnceLock::new();

/// Whether the ADR-016 engine thread should be spawned.
///
/// **ADR-023 (default flip 2026-07-28): now enabled by default.** ADR-016's
/// M0вЂ“M4.1 stages all landed behind `LUMEN_ENGINE_THREAD=1` and each one was
/// accepted as byte-identical with the flag off, so the flag had become a
/// finished-but-unused feature. Leaving it off kept every `relayout()` on the
/// UI thread, which is what makes real sites hang on load: a page with N
/// `@font-face` files pays N serialized full relayouts before its first frame
/// (measured on lenta.ru вЂ” 9 fonts, ~300вЂ“700 ms each, first frame ~6.7 s в†’ ~3.6 s
/// with the thread on; see `bugs/BUG-274-OPEN.md`, СЃСЂРµР· 2026-07-28).
///
/// Rollback (same flag-strategy idiom as ADR-018's V8 cutover and ADR-021's
/// chrome flip): `LUMEN_NO_ENGINE_THREAD=1` вЂ” or `LUMEN_ENGINE_THREAD=0` for
/// callers already setting the historical variable вЂ” restores the fully
/// synchronous UI-thread behaviour.
///
/// Deliberately **not** tied to `--deterministic`: `graphic_tests/run.py`
/// launches with `--deterministic --viewport 1024x720`, so forcing the thread
/// off there would mean the pixel gate never exercises the shipped default.
fn engine_thread_enabled() -> bool {
    let opt_out = std::env::var("LUMEN_NO_ENGINE_THREAD").ok();
    let legacy = std::env::var("LUMEN_ENGINE_THREAD").ok();
    engine_thread_enabled_from(opt_out.as_deref(), legacy.as_deref())
}

/// Pure decision behind [`engine_thread_enabled`], split out so the precedence
/// rules are unit-testable: reading the real environment from a test is
/// process-global and races the rest of the (parallel) test binary.
///
/// `opt_out` is `LUMEN_NO_ENGINE_THREAD`, `legacy` is the historical
/// `LUMEN_ENGINE_THREAD`. The opt-out wins over everything; otherwise only an
/// explicit `LUMEN_ENGINE_THREAD=0` disables the thread. A leftover
/// `LUMEN_ENGINE_THREAD=1` from before the ADR-023 flip keeps working and now
/// simply agrees with the default.
fn engine_thread_enabled_from(opt_out: Option<&str>, legacy: Option<&str>) -> bool {
    if opt_out == Some("1") {
        return false;
    }
    legacy != Some("0")
}

/// ADR-016 M2.2: РїРѕРґРЅРёРјР°РµС‚ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє, РµСЃР»Рё РѕРЅ РЅРµ РѕС‚РєР»СЋС‡С‘РЅ СЏРІРЅРѕ
/// ([`engine_thread_enabled`] вЂ” СЃ ADR-023 РІРєР»СЋС‡С‘РЅ РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ). Р’
/// M2.2 С‡РµСЂРµР· РїРѕС‚РѕРє РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓРµС‚СЃСЏ off-thread layout РґР»СЏ async-С‚СЂРёРіРіРµСЂРѕРІ
/// (РїРѕРєР° вЂ” debounce-Р·СѓРј): [`Lumen::submit_relayout_job`] С€Р»С‘С‚ Р·Р°РґР°РЅРёРµ, РїРѕС‚РѕРє
/// СЃС‡РёС‚Р°РµС‚ [`EngineCommit`] Рё РєР»Р°РґС‘С‚ РІ latest-wins СЃР»РѕС‚, РѕС‚РєСѓРґР° РµРіРѕ Р·Р°Р±РёСЂР°РµС‚
/// [`Lumen::poll_engine_commit`]. РџСЂРё СЃР±РѕРµ СЃС‚Р°СЂС‚Р° РїРѕС‚РѕРєР° Р»РѕРіРёСЂСѓРµРј Рё РѕС‚РєР°С‚С‹РІР°РµРјСЃСЏ
/// РЅР° `None` (РєР°Рє РѕР±С‹С‡РЅРѕ, Р±РµР· РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ `relayout()`).
fn spawn_engine_thread_if_enabled()
-> Option<engine_thread::EngineThread<EngineCommit, EngineJsState>> {
    if !engine_thread_enabled() {
        return None;
    }
    // ADR-016 M2.2c-2b: РїРѕС‚РѕРє РІР»Р°РґРµРµС‚ `EngineJsState` (Р±СѓРґСѓС‰РµРµ СЃРёРґРµРЅСЊРµ `Document`
    // + `js_ctx`); СЃС‚Р°СЂС‚СѓРµС‚ РїСѓСЃС‚С‹Рј (`EngineJsState::default()` С‡РµСЂРµР· `spawn()`),
    // Р·Р°РїРѕР»РЅСЏРµС‚СЃСЏ `sync_engine_js_state` РїСЂРё РїРµСЂРІРѕР№ Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹.
    match engine_thread::EngineThread::<EngineCommit, EngineJsState>::spawn() {
        Ok(engine) => {
            eprintln!(
                "[engine-thread] Р·Р°РїСѓС‰РµРЅ (ADR-023 РґРµС„РѕР»С‚, M2.2 off-thread layout; \
                 РѕС‚РєР°С‚ вЂ” LUMEN_NO_ENGINE_THREAD=1)"
            );
            Some(engine)
        }
        Err(e) => {
            eprintln!("[engine-thread] РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ: {e}; РїСЂРѕРґРѕР»Р¶Р°РµРј Р±РµР· РЅРµРіРѕ");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
fn run_window_mode(
    source: PageSource,
    event_sink: Arc<dyn EventSink>,
    blocked_log: Arc<std::sync::Mutex<panels::shields_panel::BlockedLog>>,
    network_log: Arc<std::sync::Mutex<devtools::network_panel::NetworkLog>>,
    initial_scroll: (f32, f32),
    no_scrollbar: bool,
    maximized: bool,
    deterministic: deterministic::DetConfig,
    viewport_override: Option<(f32, f32)>,
    automation_handle: AutomationHandle,
    automation_cmd_tx: std::sync::mpsc::Sender<AutomationRequest>,
    automation_rx: std::sync::mpsc::Receiver<AutomationRequest>,
    automation_mode: bool,
) -> ExitCode {
    println!("Lumen v{} вЂ” Phase 2 (Interactive) complete", env!("CARGO_PKG_VERSION"));

    // Wire navigator.clipboard to the OS clipboard (task #26). Process-global,
    // installed once; the JS bindings _lumen_clipboard_read/_write forward here.
    #[cfg(feature = "v8")]
    lumen_js::set_clipboard_provider(std::sync::Arc::new(
        platform::clipboard::PlatformClipboard,
    ));

    // Wire navigator.mediaDevices.getUserMedia({audio}) to the platform audio
    // capture backend (PH3-3). Process-global; installed before any JS context starts.
    #[cfg(feature = "v8")]
    lumen_js::set_audio_capture_provider(std::sync::Arc::new(
        platform::audio_capture::PlatformAudioCapture,
    ));

    // P3-spell СЃСЂРµР· 2: СЃР»РѕРІР°СЂРё Hunspell РіСЂСѓР·СЏС‚СЃСЏ С„РѕРЅРѕРј (СЂР°Р·РІРѕСЂР°С‡РёРІР°РЅРёРµ Р°С„С„РёРєСЃРѕРІ
    // Р±РѕР»СЊС€РёС… СЃР»РѕРІР°СЂРµР№ Р·Р°РЅРёРјР°РµС‚ СЃРµРєСѓРЅРґС‹) вЂ” СЃС‚Р°СЂС‚ РѕРєРЅР° РЅРµ Р¶РґС‘С‚.
    std::thread::spawn(|| {
        let dicts = spellcheck::load_dictionaries(&spellcheck::spell_data_dir());
        if !dicts.is_empty() {
            use lumen_core::ext::SpellChecker;
            println!("Spell: СЃР»РѕРІР°СЂРё Р·Р°РіСЂСѓР¶РµРЅС‹ ({})", dicts.locale());
        }
        let _ = SPELL_DICTS.set(dicts);
    });

    // Wire HTMLAudioElement play/pause/seek to the platform audio playback
    // backend (PH3-11). Process-global; installed before any JS context starts.
    #[cfg(feature = "v8")]
    lumen_js::set_audio_playback_provider(std::sync::Arc::new(
        platform::audio_player::PlatformAudioPlayer::new(),
    ));

    // Wire Screen Wake Lock API to the platform backend (PH3-13).
    // Prevents the display from sleeping while JS holds an active WakeLockSentinel.
    #[cfg(feature = "v8")]
    lumen_js::set_wake_lock_provider(std::sync::Arc::new(
        platform::wake_lock::PlatformWakeLock::new(),
    ));

    // Wire Screen Capture API to the platform backend (PH3-17).
    // Enables navigator.mediaDevices.getDisplayMedia() to capture the primary monitor.
    #[cfg(feature = "v8")]
    lumen_js::set_screen_capture_provider(std::sync::Arc::new(
        platform::screen_capture::PlatformScreenCapture,
    ));

    // Wire HTMLVideoElement GIF playback store (PH3-12).
    // The same Arc is shared with JS native bindings and the shell's render tick.
    #[cfg(feature = "v8")]
    let video_gif_store = {
        let store = std::sync::Arc::new(lumen_js::VideoGifStore::default());
        lumen_js::set_video_gif_store(store.clone());
        store
    };
    #[cfg(not(feature = "v8"))]
    let video_gif_store: std::sync::Arc<lumen_js::VideoGifStore> =
        std::sync::Arc::new(lumen_js::VideoGifStore::default());

    // Wire the TextTrack store (P3-webvtt slice 4) вЂ” mirrors parsed `<track>`
    // cues into the JS `video.textTracks` API. Same Arc shared with bindings.
    #[cfg(feature = "v8")]
    let text_track_store = {
        let store = std::sync::Arc::new(lumen_js::TextTrackStore::default());
        lumen_js::set_text_track_store(store.clone());
        store
    };
    #[cfg(not(feature = "v8"))]
    let text_track_store: std::sync::Arc<lumen_js::TextTrackStore> =
        std::sync::Arc::new(lumen_js::TextTrackStore::default());

    // Apply the fingerprint profile's navigator/screen/timezone values (9F.1).
    // Process-global; consumed by lumen_js when each page's JS context spins up.
    #[cfg(feature = "v8")]
    config::global().install_navigator();

    // Install + enable the process-global ad-block filter (consulted by every
    // HttpClient on all fetch paths). Matches the initial tab's default (on);
    // the per-tab checkbox flips it via lumen_network::set_global_adblock_enabled.
    // Returns the persistent store; offline-first (cached lists / bundled fallback).
    let adblock_store = config::init_adblock();

    // Background refresh of external filter lists (EasyList/EasyPrivacy):
    // conditional GET of any list past its ~4-day expiry, then hot-swap the
    // reparsed filter. Best-effort вЂ” network errors keep the cached version;
    // panics are isolated to this thread and never crash the browser.
    {
        let store = std::sync::Arc::clone(&adblock_store);
        let http = config::global().apply_http(lumen_network::HttpClient::new());
        std::thread::Builder::new()
            .name("adblock-refresh".to_owned())
            .spawn(move || {
                if adblock::refresh(&store, &http) {
                    let count = adblock::load_and_install(&store);
                    eprintln!("adblock: lists updated, filter hot-swapped ({count} rules)");
                }
            })
            .ok();
    }

    // Streaming pipeline: РѕРєРЅРѕ СЃРѕР·РґР°С‘С‚СЃСЏ РЅРµРјРµРґР»РµРЅРЅРѕ, Р·Р°РіСЂСѓР·РєР° СЃС‚Р°СЂС‚СѓРµС‚
    // РїРѕСЃР»Рµ `resumed` РІ background-РїРѕС‚РѕРєРµ. Р”Рѕ РїСЂРёС…РѕРґР° РґР°РЅРЅС‹С… СЂРёСЃСѓРµРј РїСѓСЃС‚СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    let event_loop = match EventLoop::<LoadEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(err) => {
            eprintln!("РќРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ event loop: {err}");
            return ExitCode::FAILURE;
        }
    };
    let load_proxy = event_loop.create_proxy();
    // SDC-1b/SDC-2: automation command channel for BiDi/MCP/graphic_tests control.
    // Created by main() (not here) so front-ends spawned before the window
    // exists (bidi_spawn) already hold a valid handle вЂ” see call site.
    //
    // Attach the wake callback now that `load_proxy` exists: without it, a
    // command enqueued from a BiDi/MCP thread has no way to interrupt a
    // parked `ControlFlow::Wait` event loop (no OS event, timer, or redraw
    // is inherently triggered by an mpsc send from an unrelated thread) and
    // could sit undrained indefinitely. `set_wake` updates the shared cell
    // every clone of `automation_handle` вЂ” including the ones already handed
    // to `bidi_spawn`/`lumen_mcp::spawn_live` in `main()` вЂ” points to.
    {
        let wake_proxy = load_proxy.clone();
        automation_handle.set_wake(std::sync::Arc::new(move || {
            let _ = wake_proxy.send_event(LoadEvent::AutomationWake);
        }));
    }
    let (input_tx, input_rx) = input::channel();
    let (read_later_tx, read_later_rx) =
        std::sync::mpsc::channel::<(String, String, Vec<u8>)>();

    // DS-14: persistent profile registry вЂ” first run seeds the 4 default
    // profiles and makes the first one ("Р›РёС‡РЅС‹Р№") active. On later runs the
    // registry already has rows and an active pointer, so this block is a
    // no-op past the `count() == 0` check (persists across restart).
    let profiles_registry = {
        let path = adblock::browser_data_dir().join("profiles.db");
        let reg = lumen_storage::ProfileRegistry::open(&path).unwrap_or_else(|e| {
            eprintln!(
                "profiles: cannot open {} ({e}); using in-memory store",
                path.display()
            );
            lumen_storage::ProfileRegistry::open_in_memory()
                .expect("in-memory profiles always opens")
        });
        if reg.count().unwrap_or(0) == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            for (name, slug, _color) in panels::profile_menu::DEFAULT_PROFILES {
                let _ = reg.create(name, &format!("profiles/{slug}/"), "", now);
            }
            if let Ok(Some(first)) = reg.get_by_name(panels::profile_menu::DEFAULT_PROFILES[0].0) {
                let _ = reg.set_active(Some(first.id));
            }
        }
        reg
    };
    let profile_entries: Vec<panels::profile_menu::ProfileEntry> = profiles_registry
        .list_all()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, p)| panels::profile_menu::ProfileEntry {
            id: p.id,
            name: p.name.clone(),
            color: panels::profile_menu::color_for_profile(&p.name, i),
        })
        .collect();
    let active_profile_id = profiles_registry.active().ok().flatten().map(|p| p.id);

    let mut app = Lumen {
        display_list: Vec::new(),
        display_list_epoch: 1,
        tile_grid: lumen_paint::TileGrid::default_size(),
        display_list_cache: lumen_paint::DisplayListCache::new(),
        title: None,
        pending_images: Vec::new(),
        page_font_registry: Arc::new(lumen_font::FontRegistry::new()),
        web_fonts: Vec::new(),
        source,
        event_sink,
        modifiers: ModifiersState::empty(),
        window: None,
        display_color_profile: platform::display_color_profile::PlatformDisplayColorProfile::new(),
        renderer: None,
        chrome_doc: Some(lumen_chrome::parse_document(chrome_preview::HTML)),
        chrome_layout: None,
        chrome_page_host_rect: None,
        chrome_hovered_nid: None,
        chrome_active_nid: None,
        chrome_omni_input_rect: None,
        chrome_sidebar_collapsed: false,
        chrome_settings_section: "general".to_owned(),
        chrome_animation_scheduler: animation_scheduler::AnimationScheduler::new(),
        chrome_transition_scheduler: TransitionScheduler::new(),
        chrome_prev_styles: HashMap::new(),
        chrome_content_area_detached: None,
        chrome_prev_cascade_styles: lumen_layout::CascadeStyles::default(),
        chrome_prev_interactive: (None, None, None),
        chrome_prev_viewport: None,
        chrome_prev_forced_colors: false,
        chrome_anim_frame: None,
        runtime: runtime::EventLoop::new(),
        animation_scheduler: animation_scheduler::AnimationScheduler::new(),
        transition_scheduler: TransitionScheduler::new(),
        starting_style_tracker: StartingStyleTracker::new(),
        prev_styles: HashMap::new(),
        page_prev_cascade_styles: None,
        page_prev_interactive: (None, None, None),
        anim_frame: None,
        layout_box: None,
        last_frame_scroll_y: 0.0,
        scroll_velocity: 0.0,
        fast_scroll: false,
        page_tracks: tracks::PageTracks::default(),
        snap_containers: Vec::new(),
        scroll_containers: Vec::new(),
        epoch: std::time::Instant::now(),
        last_raf_batch_ms: -RAF_MIN_INTERVAL_MS,
        last_mem_report_s: 0.0,
        frame_stats: lumen_paint::FrameStats::new(),
        engine_stats: lumen_paint::FrameStats::new(),
        last_frame_fp: None,
        scroll_cache: lumen_paint::ScrollCache::default_overscan(),
        find: find::FindState::default(),
        address_bar: address_bar::AddressBarState::default(),
        hint: hints::HintState::default(),
        scroll_y: initial_scroll.1,
        scroll_x: initial_scroll.0,
        content_height: 0.0,
        content_width: 0.0,
        cv_skipped: Vec::new(),
        cv_relevant: std::collections::HashSet::new(),
        cv_auto_state: std::collections::HashMap::new(),
        cv_events: Vec::new(),
        dark_mode: false,
        cursor_position: None,
        pending_pointer_moves: Vec::new(),
        hovered_nid: None,
        active_nid: None,
        scroll_drag: None,
        scroll_anim: None,
        momentum_anim: None,
        touchpad_vel: (0.0, 0.0),
        touchpad_vel_time_ms: 0.0,
        last_cursor_icon: None,
        layout_source: None,
        pending_reload: Rc::new(Cell::new(false)),
        pending_js_navigate: None,
        load_proxy,
        stream_builder: None,
        stream_last_paint: std::time::Instant::now(),
        stream_sheet: lumen_css_parser::Stylesheet::default(),
        stream_layout_seeded: false,
        preload_dispatched: std::collections::HashSet::new(),
        stream_images_requested: std::collections::HashSet::new(),
        stream_image_sizes: HashMap::new(),
        stream_image_sizes_dirty: false,
        pending_restore_scroll: None,
        pending_pageshow_persisted: false,
        pending_post_reload_traversal: None,
        traversal_crossed_document: false,
        load_generation: 0,
        document_base: None,
        engine_thread: spawn_engine_thread_if_enabled(),
        engine_job_generation: 0,
        engine_applied_generation: 0,
        ime_composing: None,
        bfcache: BfCache::new(16),
        frozen_styles: HashMap::new(),
        parked_pages: Vec::new(),
        nav_back: Vec::new(),
        nav_fwd: Vec::new(),
        nav_key_counter: 0,
        current_nav_key: "nav-0".to_string(),
        pending_intercepted: None,
        form_state: HashMap::new(),
        validation_tooltip: None,
        color_picker_node: None,
        date_picker_node: None,
        date_picker_year: 0,
        date_picker_month: 0,
        select_dropdown_node: None,
        ls_storage: HashMap::new(),
        ss_storage: HashMap::new(),
        idb_dir: lumen_idb_dir(),
        sw_backend: Arc::new(std::sync::Mutex::new(lumen_storage::store::InMemoryStorage::new())),
        sw_worker_store: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        cache_store: Arc::new(
            lumen_storage::CacheStorage::open_in_memory().expect("cache_store init"),
        ),
        cookie_jar: Arc::new(
            lumen_storage::CookieJar::open_in_memory().expect("cookie_jar init"),
        ),
        // DS-16: Anonymous profile's own ephemeral cookie jar вЂ” kept
        // separate from `cookie_jar` so Anonymous browsing never mixes
        // cookies with Personal/Work/Guest. Reset to a fresh instance every
        // time Anonymous becomes the active profile вЂ” see
        // `active_cookie_jar`/`ProfileMenuHit::SwitchTo`.
        anonymous_cookie_jar: Arc::new(
            lumen_storage::CookieJar::open_in_memory().expect("anonymous_cookie_jar init"),
        ),
        js_ctx: None,
        js_present: false,
        raf_pending_flag: None,
        dom_dirty_flag: None,
        raf_task_inflight: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        raf_drain_gate: false,
        no_scrollbar,
        maximized,
        first_paint_delivered: false,
        first_contentful_paint_delivered: false,
        load_failed: false,
        load_error_message: None,
        nav_start: None,
        history_fts: HistoryFts::open_in_memory().expect("history_fts init"),
        notes_store: lumen_knowledge::Notes::open_in_memory().expect("notes_store init"),
        search_history: SearchHistory::open_in_memory().expect("search_history init"),
        next_history_id: 1,
        hyp_provider: Arc::new(KnuthLiangHyphenation::new()),
        animated_gifs: HashMap::new(),
        gif_last_frame: HashMap::new(),
        video_gif_last_frame: HashMap::new(),
        video_gif_frames: HashMap::new(),
        frames: Vec::new(),
        video_gif_store,
        text_track_store,
        image_cache: lumen_image::ImageDecodeCache::new(),
        automation_rx,
        automation_cmd_tx,
        pending_waits: Vec::new(),
        input_rx,
        input_tx,
        focused_node: None,
        downloads: download::DownloadManager::new(),
        tab_strip: tabs::strip::TabStrip::new(),
        container_store: tabs::containers::ContainerStore::new(),
        bg_tabs: HashMap::new(),
        hibernated_tabs: HashMap::new(),
        tab_snapshots: lumen_storage::TabSnapshotStore::open_in_memory()
            .expect("tab_snapshots in-memory"),
        t2_store: lumen_storage::SleepingTabStore::open_in_memory()
            .expect("t2_store in-memory"),
        t2_restore_start_ms: None,
        session_store: session_persist::open_store(),
        lifecycle_mgr: {
            let mut mgr = tab_lifecycle::TabLifecycleManager::new(
                tab_lifecycle::TierTimeouts::default(),
                8, // max 8 non-hibernated background tabs
            );
            // Register the initial blank tab (id=0) as the active tab.
            mgr.open_tab(0);
            mgr
        },
        lifecycle_last_tick: std::time::Instant::now(),
        split_view: None,
        vim_mode: None,
        vertical_tabs: panels::vertical_tabs::VerticalTabsPanel::new(),
        tree_tabs: panels::tree_tabs::TreeTabsPanel::new(),
        workspace_panel: panels::workspace_panel::WorkspacePanel::new(),
        workspaces: lumen_storage::Workspaces::open_in_memory()
            .expect("workspaces in-memory"),
        profile_menu: {
            let mut pm = panels::profile_menu::ProfileMenuPanel::new();
            pm.set_entries(profile_entries);
            pm.set_active(active_profile_id);
            pm
        },
        profiles: profiles_registry,
        shields: panels::shields_panel::ShieldsPanel::new(blocked_log),
        permission: panels::permission_panel::PermissionPanel::new(),
        sidebar: panels::sidebar_panel::SidebarPanel::new(),
        sidebar_source: None,
        ai_panel: panels::ai_panel::AiPanel::new(),
        panel_layout: panel_layout::PanelLayout::load(),
        panel_resize: None,
        note_viewer: panels::note_viewer::NoteViewerPanel::new(),
        ai_backend: Box::new(lumen_core::NullAiBackend),
        bookmarks: lumen_storage::Bookmarks::open_in_memory().expect("bookmarks in-memory"),
        bookmark_panel: panels::bookmark_panel::BookmarkPanel::new(),
        tab_groups: lumen_storage::TabGroups::open_in_memory().expect("tab_groups in-memory"),
        history_store: History::open_in_memory().expect("history_store in-memory"),
        history_panel: panels::history_panel::HistoryPanel::new(),
        command_palette: panels::command_palette::CommandPalette::new(),
        focus: panels::focus_panel::FocusModePanel::new(),
        pip: panels::pip_window::PipWindow::new(),
        pip_controller: panels::pip_os_window::PipController::new(),
        pip_os: None,
        doc_pip_controller: panels::doc_pip_os_window::DocPipController::new(),
        doc_pip_os: None,
        gesture: input::gesture::GestureRecognizer::new(),
        omnibox_aliases: lumen_storage::OmniboxAliases::open_in_memory()
            .expect("omnibox_aliases init"),
        newtab_tiles: {
            let path = adblock::browser_data_dir().join("newtab_tiles.db");
            lumen_storage::NewtabTiles::open(&path).unwrap_or_else(|e| {
                eprintln!(
                    "newtab_tiles: cannot open {} ({e}); using in-memory store",
                    path.display()
                );
                lumen_storage::NewtabTiles::open_in_memory()
                    .expect("in-memory newtab_tiles always opens")
            })
        },
        notes: Vec::new(),
        read_later_store: lumen_knowledge::ReadLater::open_in_memory()
            .expect("read_later in-memory"),
        read_later_panel: panels::read_later_panel::ReadLaterPanel::new(),
        read_later_rx,
        read_later_tx,
        cookie_banner_dismiss: true,
        gc_tick: gc_tick::GcTick::new(),
        memory_poll: memory_poll::MemoryPollTick::new(memory_poll::platform_source()),
        cache_registry: lumen_core::ext::CacheRegistry::new(),
        deterministic,
        viewport_override,
        devtools_console: devtools::console_panel::ConsolePanel::new(),
        dom_inspector: devtools::inspector::DomInspectorPanel::new(),
        network_panel: devtools::network_panel::NetworkPanel::new(std::sync::Arc::clone(
            &network_log,
        )),
        privacy: panels::privacy_panel::PrivacyPanel::new(network_log),
        a11y_store: lumen_storage::A11yPrefs::open_in_memory()
            .expect("a11y_prefs in-memory"),
        a11y_panel: panels::a11y_panel::A11yPanel::new(),
        platform_bridge: lumen_a11y::platform::platform_bridge(),
        print_panel: panels::print_panel::PrintPanel::new(),
        settings_store: {
            let path = adblock::browser_data_dir().join("settings.db");
            lumen_storage::BrowserSettings::open(&path).unwrap_or_else(|e| {
                eprintln!(
                    "settings: cannot open {} ({e}); using in-memory store",
                    path.display()
                );
                lumen_storage::BrowserSettings::open_in_memory()
                    .expect("in-memory settings always opens")
            })
        },
        settings_panel: panels::settings_panel::SettingsPanel::new(),
        adblock_store: std::sync::Arc::clone(&adblock_store),
        shortcuts_panel: {
            let ks = lumen_storage::KeyboardShortcuts::open_in_memory()
                .expect("shortcuts in-memory");
            panels::shortcuts_panel::ShortcutsPanel::new(&ks.all())
        },
        fallbacks_preloaded: false,
        zoom_factor: zoom::ZOOM_DEFAULT,
        laid_out_zoom_factor: zoom::ZOOM_DEFAULT,
        pending_zoom_relayout: None,
        display_url: None,
        current_history_state_json: String::from("null"),
        fullscreen_nid: None,
        fullscreen_resize_pending: None,
        view_transition: None,
        archive: tabs::archive::TabArchive::new(),
        restore_spinner_start_ms: None,
        resize_active: None,
        tab_drag: None,
        dnd_state: None,
        tab_context_menu: tabs::context_menu::TabContextMenu::default(),
        page_context_menu: page_context_menu::PageContextMenu::default(),
        spell_user_words: spellcheck::load_user_words(&spellcheck::user_words_path()),
        spell_ignored: std::collections::HashSet::new(),
        shell_theme: panels::themes::ShellTheme::default(),
        reader_original_source: None,
        cert_info: None,
        cert_panel: panels::cert_panel::CertPanel::new(),
    };
    // BUG-411: seed the shields fallback from the persisted "Р‘Р»РѕРєРёСЂРѕРІР°С‚СЊ
    // СЂРµРєР»Р°РјСѓ" setting and push it at the process-global filter, which
    // `config::init_adblock` deliberately leaves off. Before this the setting
    // was write-only вЂ” nothing read `BrowserSettings::shields_enabled` back вЂ”
    // and after CC-15 removed the in-tab checkbox there was no reachable UI
    // that enabled filtering at all.
    //
    // BUG-800: `LUMEN_NO_ADBLOCK=1` overrides the persisted default to off.
    // EasyList's 100K+ rules false-positive on WPT's own test-infra request
    // shapes (e.g. `common/security-features/subresource/document.py?...
    // action=purge...`) вЂ” the request is silently blocked, the navigation
    // that depended on it fails without an error (BUG-438), and the stale
    // document poisons the next result. `tools/wptrunner/wptrunner/browsers/
    // lumen.py` sets this for every automation-launched process.
    {
        let no_adblock = std::env::var_os("LUMEN_NO_ADBLOCK").is_some();
        let on = !no_adblock && app.settings_store.shields_enabled();
        app.shields.set_default_enabled(on);
        app.sync_adblock_filter();
    }
    // PH3-20: install the session-global Service Worker fetch interceptor.
    // It shares the same `sw_worker_store` + `cache_store` the page runtime uses,
    // so an activated SW serves cache-first responses to subresource/`fetch()`
    // requests. The SQLite `ServiceWorkers` store is an empty in-memory instance:
    // the shell keeps SW registrations in-memory, and the interceptor routes via
    // `sw_worker_store` (scope-prefix match) independently of it.
    {
        let interceptor = lumen_storage::ServiceWorkerInterceptor::new(
            Arc::new(
                lumen_storage::ServiceWorkers::open_in_memory().expect("sw registry init"),
            ),
            Arc::clone(&app.cache_store),
        )
        .with_sw_workers(Arc::clone(&app.sw_worker_store));
        let _ = SW_FETCH_INTERCEPTOR
            .set(Arc::new(interceptor) as Arc<dyn lumen_core::ext::FetchInterceptor>);
    }
    // Restore the previous session only when launched without an explicit page
    // (no file/url argument and no --import-session), so we never clobber an
    // argv-requested page. Sets the active tab's source before `run_app`, so the
    // streaming load in `resumed` picks it up.
    //
    // Also skipped in automation mode (BUG-296): an automation driver's own
    // `browsingContext.navigate` races a leftover `last_session.db` tab (saved
    // by a prior interactive run from the same working directory вЂ” the session
    // store's on-disk file is a bare CWD-relative path, see `session_persist.rs`)
    // restoring into the same top-level context, sometimes landing *after* the
    // driver's navigate and silently leaving `window`/`document` pointed at the
    // stale page. `lumen --bidi-port`/`--mcp-live-port` are documented as
    // opening an empty window (`print_usage`'s "РїСѓСЃС‚РѕРµ РѕРєРЅРѕ") вЂ” automation
    // callers always drive their own first navigation, so restoring a session
    // here would violate that contract even without the race.
    if should_restore_session(&app.source, automation_mode) {
        app.restore_session();
    }
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("РћС€РёР±РєР° event loop: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Р РµР·СѓР»СЊС‚Р°С‚ Р·Р°РіСЂСѓР·РєРё СЃС‚СЂР°РЅРёС†С‹: С‡С‚Рѕ СЂРёСЃРѕРІР°С‚СЊ Рё РєР°Рє РЅР°Р·РІР°С‚СЊ РѕРєРЅРѕ.
/// Р Р°СЃС€РёСЂСЏРµС‚СЃСЏ: favicon, current URL, scroll state вЂ” РїРѕР·Р¶Рµ.
struct LoadedPage {
    display_list: DisplayList,
    title: Option<String>,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ `<img src="вЂ¦">` РґР»СЏ GPU upload С‡РµСЂРµР·
    /// `Renderer::register_image`. РљР»СЋС‡ вЂ” raw src attribute value (С‚РѕС‚ Р¶Рµ,
    /// С‡С‚Рѕ РїРѕРїР°РґР°РµС‚ РІ `DisplayCommand::DrawImage.src`), С‡С‚РѕР±С‹ render-side
    /// РјРѕРі СЃРґРµР»Р°С‚СЊ lookup Р±РµР· РѕС‚РґРµР»СЊРЅРѕР№ РЅРѕСЂРјР°Р»РёР·Р°С†РёРё URL. `Arc<Image>` (BUG-272
    /// СЃСЂРµР· 17): СЂР°Р·РґРµР»СЏРµС‚ РїРёРєСЃРµР»Рё СЃ `IMAGE_CACHE`/`register_image`, РЅРµ РєРѕРїРёСЂСѓРµС‚.
    images: Vec<(String, Arc<lumen_image::Image>)>,
    /// Multi-frame GIF animations decoded at load time. Keyed by the same src URL
    /// as `DrawImage.src`. Frame 0 of each entry is already in `images` so the
    /// renderer has a valid texture on first paint; subsequent frames are uploaded
    /// on each `RedrawRequested` tick via `Lumen::animated_gifs`.
    animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` вЂ” registered with JS
    /// after page load via `_lumen_init_lazy_images` for proximity-based loading.
    #[allow(dead_code)] // read only inside #[cfg(feature = "v8")] blocks
    lazy_pairs: Vec<(u32, String)>,
    /// Layout-РґРµСЂРµРІРѕ СЃС‚СЂР°РЅРёС†С‹ вЂ” РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ animation scheduler-РѕРј.
    layout_box: lumen_layout::LayoutBox,
    /// РџСЂРѕРІР°Р№РґРµСЂ С€СЂРёС„С‚РѕРІ СЃ @font-face local()-РёСЃС‚РѕС‡РЅРёРєР°РјРё СЃС‚СЂР°РЅРёС†С‹.
    /// РџРµСЂРµРґР°С‘С‚СЃСЏ СЂРµРЅРґРµСЂСѓ С‡РµСЂРµР· `set_font_provider` РїСЂРё apply_loaded_page.
    /// PH3-19: РєРѕРЅРєСЂРµС‚РЅС‹Р№ С‚РёРї (РЅРµ С‚СЂРµР№С‚-РѕР±СЉРµРєС‚), С‡С‚РѕР±С‹ `apply_loaded_page`
    /// РјРѕРі РґРёРЅР°РјРёС‡РµСЃРєРё РґРѕСЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ web-С€СЂРёС„С‚С‹ С‡РµСЂРµР· `register_from_bytes`.
    font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: @font-face url()-РёСЃС‚РѕС‡РЅРёРєРё, РµС‰С‘ РЅРµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ РІ РјРѕРјРµРЅС‚ РїРµСЂРІРѕРіРѕ
    /// layout-Р°. `apply_loaded_page` СЃРїР°РІРЅРёС‚ С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє РґР»СЏ РєР°Р¶РґРѕРіРѕ;
    /// СЂРµР·СѓР»СЊС‚Р°С‚ РїСЂРёС…РѕРґРёС‚ РєР°Рє `LoadEvent::FontLoaded` в†’ relayout СЃ FOUT.
    pending_web_fonts: Vec<PendingWebFont>,
    /// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ РѕС‚ JS (location.href= Рё С‚.Рї.), РІС‹РїРѕР»РЅРµРЅРЅС‹Р№
    /// РІ РїСЂРѕС†РµСЃСЃРµ Р·Р°РіСЂСѓР·РєРё. РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚СЃСЏ РІ `about_to_wait`.
    js_navigate: Option<JsNavigateRequest>,
    /// P3-webvtt СЃСЂРµР· 3: WebVTT-cues РїРѕ РєР°Р¶РґРѕРјСѓ `<video>` СЃС‚СЂР°РЅРёС†С‹.
    page_tracks: tracks::PageTracks,
    /// BUG-480 СЃСЂРµР· 1: Р¶РёРІС‹Рµ sub-РґРѕРєСѓРјРµРЅС‚С‹ `<iframe>` вЂ” РґРµСЂР¶Р°С‚ JS-РєРѕРЅС‚РµРєСЃС‚С‹
    /// Рё DOM РґРµС‚РµР№ РґРѕ Р·Р°РјРµРЅС‹ СЃС‚СЂР°РЅРёС†С‹.
    frames: Vec<FrameHandle>,
}

impl LoadedPage {
    fn empty() -> Self {
        Self {
            display_list: DisplayList::new(),
            title: None,
            images: Vec::new(),
            animated_gifs: Vec::new(),
            lazy_pairs: Vec::new(),
            layout_box: lumen_layout::LayoutBox {
                node: NodeId::from_index(0),
                rect: Rect::ZERO,
                style: std::sync::Arc::new(lumen_layout::style::ComputedStyle::root()),
                kind: lumen_layout::BoxKind::Block,
                children: Vec::new(),
                col_span: 1,
                row_span: 1,
                svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                dirty: lumen_layout::DirtyBits::CLEAN,
                origin: lumen_layout::BoxOrigin { node: None, role: lumen_layout::BoxRole::Placeholder },
            },
            font_registry: Arc::new(lumen_font::FontRegistry::new()),
            pending_web_fonts: Vec::new(),
            js_navigate: None,
            page_tracks: tracks::PageTracks::default(),
            frames: Vec::new(),
        }
    }
}

/// Р РµР·СѓР»СЊС‚Р°С‚ С„Р°Р· `decode в†’ parse в†’ layout` вЂ” РѕР±С‰Р°СЏ С‡Р°СЃС‚СЊ РґР»СЏ РѕРєРѕРЅРЅРѕРіРѕ Рё
/// dump-СЂРµР¶РёРјРѕРІ. РџРѕР»СЏ РІР»Р°РґРµСЋС‚ СЃРІРѕРёРјРё РґР°РЅРЅС‹РјРё вЂ” РЅРµС‚ СЃСЃС‹Р»РѕРє РЅР°СЂСѓР¶Сѓ.
struct ParsedPage {
    /// Parsed DOM вЂ” shared with JS closures via Arc so event handlers can
    /// mutate the document without rebuilding the entire page.
    document: Arc<Mutex<Document>>,
    stylesheet: lumen_css_parser::Stylesheet,
    layout: LayoutBox,
    title: Option<String>,
    rule_count: usize,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ РёР·РѕР±СЂР°Р¶РµРЅРёСЏ, РЅР°Р№РґРµРЅРЅС‹Рµ РїСЂРё РѕР±С…РѕРґРµ DOM. РЎРј. [`LoadedPage::images`].
    images: Vec<(String, Arc<lumen_image::Image>)>,
    /// Multi-frame GIF animations found in the DOM. See [`LoadedPage::animated_gifs`].
    animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` elements вЂ” skipped by
    /// the eager fetch pass; registered with JS `_lumen_init_lazy_images` after load.
    lazy_pairs: Vec<(u32, String)>,
    /// Subresource-С…РёРЅС‚С‹, РЅР°Р№РґРµРЅРЅС‹Рµ preload-СЃРєР°РЅРµСЂРѕРј Р”Рћ DOM-РїР°СЂСЃРёРЅРіР°.
    /// Source-order: РїРµСЂРІС‹Рµ С…РёРЅС‚С‹ РІР°Р¶РЅРµРµ (РёС… fetch СЃС‚Р°СЂС‚СѓРµС‚ РїРµСЂРІС‹Рј).
    preload_hints: Vec<lumen_html_parser::PreloadHint>,
    /// Decoded UTF-8 HTML source вЂ” stored for bfcache snapshot.
    html_source: String,
    /// @font-face local()-С€СЂРёС„С‚С‹ + СЃРёСЃС‚РµРјРЅС‹Рµ С€СЂРёС„С‚С‹. РџРµСЂРµРґР°С‘С‚СЃСЏ СЂРµРЅРґРµСЂСѓ.
    /// PH3-19: РєРѕРЅРєСЂРµС‚РЅС‹Р№ `FontRegistry` (РЅРµ С‚СЂРµР№С‚-РѕР±СЉРµРєС‚) РґР»СЏ РґРѕСЂРµРіРёСЃС‚СЂР°С†РёРё
    /// web-С€СЂРёС„С‚РѕРІ РїРѕСЃР»Рµ `FontLoaded` Р±РµР· РґР°СѓРЅРєР°СЃС‚Р°.
    font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: @font-face url()-РёСЃС‚РѕС‡РЅРёРєРё, РµС‰С‘ РЅРµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ; РїРµСЂРµРґР°СЋС‚СЃСЏ РІ
    /// `LoadedPage` Рё РґР°Р»РµРµ РІ С„РѕРЅРѕРІС‹Рµ РїРѕС‚РѕРєРё С‡РµСЂРµР· `apply_loaded_page`.
    pending_web_fonts: Vec<PendingWebFont>,
    /// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ, РІС‹СЃС‚Р°РІР»РµРЅРЅС‹Р№ JS РІРѕ РІСЂРµРјСЏ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ.
    js_navigate: Option<JsNavigateRequest>,
    /// Persistent JS context (V8) kept alive after page load so that
    /// event handlers registered via `addEventListener` continue to work.
    /// `None` when the v8 feature is disabled or script init failed.
    ///
    /// ADR-016 M2.2c-2b: `Arc` (РЅРµ `Box`), С‡С‚РѕР±С‹ С…СЌРЅРґР» РјРѕР¶РЅРѕ Р±С‹Р»Рѕ СЂР°Р·РґРµР»РёС‚СЊ СЃ
    /// РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј (`EngineJsState`) РЅР° РІСЂРµРјСЏ РјРёРіСЂР°С†РёРё `js_ctx` РЅР° РЅРµРіРѕ.
    js_ctx: Option<Arc<dyn PersistentJs>>,
    /// P3-webvtt СЃСЂРµР· 3: WebVTT-cues, Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ РёР· `<track>` РєР°Р¶РґРѕРіРѕ `<video>`.
    page_tracks: tracks::PageTracks,
    /// BUG-743: РЅРµРёР·РјРµРЅСЏРµРјР°СЏ С‡Р°СЃС‚СЊ CSS + РѕС‚РїРµС‡Р°С‚РѕРє РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>`,
    /// С‡С‚РѕР±С‹ РїРѕР·РґРЅСЏСЏ РІСЃС‚Р°РІРєР° Р»РёСЃС‚Р° РїРµСЂРµСЃРѕР±СЂР°Р»Р° РєР°СЃРєР°Рґ Р±РµР· СЃРµС‚Рё.
    dynamic_css: DynamicCssBase,
    /// BUG-480 СЃСЂРµР· 1: Р¶РёРІС‹Рµ sub-РґРѕРєСѓРјРµРЅС‚С‹ `<iframe>` СЌС‚РѕР№ СЃС‚СЂР°РЅРёС†С‹.
    frames: Vec<FrameHandle>,
}

/// РСЃС‚РѕС‡РЅРёРє РґР»СЏ РїРѕРІС‚РѕСЂРЅРѕРіРѕ layout Р±РµР· РїРѕРІС‚РѕСЂРЅРѕР№ Р·Р°РіСЂСѓР·РєРё/РїР°СЂСЃРёРЅРіР°.
/// РҐСЂР°РЅРёС‚СЃСЏ РІ `Lumen`; РѕР±РЅРѕРІР»СЏРµС‚СЃСЏ С‚РѕР»СЊРєРѕ РїСЂРё reload/load РЅРѕРІРѕР№ СЃС‚СЂР°РЅРёС†С‹.
struct LayoutSource {
    /// DOM вЂ” shared with the persistent JS runtime via Arc<Mutex> so that
    /// JS event handlers can mutate it between repaints.
    document: Arc<Mutex<Document>>,
    /// Parsed stylesheet, shared as an immutable `Arc` snapshot (ADR-016 M2.2b):
    /// off-thread relayout jobs clone the handle (`Arc::clone`) instead of deep-
    /// cloning the whole `Stylesheet` on every submit. Replaced wholesale on
    /// reload/thaw, never mutated in place.
    stylesheet: Arc<lumen_css_parser::Stylesheet>,
    /// Decoded HTML source captured after encoding detection. Used by bfcache
    /// to restore the page without a network round-trip.
    #[allow(dead_code)]
    html_source: Option<String>,
    /// `Cache-Control: no-store` on the response that produced this page.
    /// Checked by [`Lumen::bfcache_eligible`] on navigate-away; `true` routes
    /// the page to the HTML-snapshot bfcache fallback instead of a full
    /// freeze. `false` for non-network sources (file/thaw/sidebar/hibernate
    /// restore) вЂ” no header to check, so the page is treated as cacheable.
    cache_control_no_store: bool,
    /// BUG-743: С‡Р°СЃС‚СЊ CSS, РЅРµ Р·Р°РІРёСЃСЏС‰Р°СЏ РѕС‚ РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>`, РїР»СЋСЃ РѕС‚РїРµС‡Р°С‚РѕРє
    /// С‚РµС… Р±Р»РѕРєРѕРІ, РёР· РєРѕС‚РѕСЂС‹С… СЃРѕР±СЂР°РЅ С‚РµРєСѓС‰РёР№ [`Self::stylesheet`]. `Some` РЅР°
    /// РѕР±С‹С‡РЅРѕРј РїСѓС‚Рё Р·Р°РіСЂСѓР·РєРё; `None` РЅР° РїСѓС‚СЏС… РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёСЏ (bfcache-thaw,
    /// СЂР°Р·РјРѕСЂРѕР·РєР° РІРєР»Р°РґРєРё, sidebar), РіРґРµ РёСЃС…РѕРґРЅС‹Рµ С‡Р°СЃС‚Рё CSS РЅРµ СЃРѕС…СЂР°РЅРµРЅС‹ вЂ” С‚Р°Рј
    /// РєР°СЃРєР°Рґ РІРµРґС‘С‚ СЃРµР±СЏ РєР°Рє РґРѕ BUG-743 Рё РїРѕР·РґРЅРёР№ `<style>` РЅРµ РїРѕРґС…РІР°С‚С‹РІР°РµС‚СЃСЏ.
    dynamic_css: Option<DynamicCssBase>,
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    hp: &dyn HyphenationProvider,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    dark_mode: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    cross_origin_isolated: bool,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    target: lumen_core::ColorSpace,
    media_print: bool,
) -> Result<ParsedPage, Box<dyn Error>> {
    // РљРѕРґРёСЂРѕРІРєСѓ РѕРїСЂРµРґРµР»СЏРµРј РїРѕ BOM -> <meta charset> -> СЌРІСЂРёСЃС‚РёРєРµ. Р­С‚Рѕ РїРѕРєСЂС‹РІР°РµС‚
    // Рё UTF-8 (Р±РѕР»СЊС€РёРЅСЃС‚РІРѕ), Рё СЃС‚Р°СЂС‹Рµ cp1251 / koi8-r / cp866 С„Р°Р№Р»С‹.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    eprintln!("РљРѕРґРёСЂРѕРІРєР°: {}", encoding.name());

    // Preload-СЃРєР°РЅРµСЂ Р·Р°РїСѓСЃРєР°РµС‚СЃСЏ Р”Рћ DOM-РїР°СЂСЃРёРЅРіР° (HTML LS В§13.2.6.4.7).
    // `preload_seen` вЂ” cross-call dedup: РµСЃР»Рё streaming СѓР¶Рµ РѕС‚РїСЂР°РІРёР» <head>-С…РёРЅС‚С‹
    // С‡РµСЂРµР· EarlyPreloadHints, С„РёРЅР°Р»СЊРЅС‹Р№ scan РїСЂРѕРїСѓСЃС‚РёС‚ РёС… Рё РґРѕР±Р°РІРёС‚ С‚РѕР»СЊРєРѕ РЅРѕРІС‹Рµ
    // (body-images, lazy-loaded resources Рё С‚.Рї.).
    let preload_hints = lumen_html_parser::scan_preload_hints(&source);
    dispatch_preload_hints(&preload_hints, base, sink, preload_seen);

    let mut doc = {
        let _s = lumen_core::trace::span("parse-html", "parse");
        lumen_html_parser::parse(&source)
    };
    // BUG-358: stamp the document with what it was actually decoded as / served
    // as, so `document.characterSet`/`charset`/`inputEncoding`/`contentType`
    // read real per-load state instead of `undefined`.
    doc.set_character_set(encoding.canonical_name().to_string());
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or(ct).trim();
        if !mime.is_empty() {
            doc.set_content_type(mime.to_string());
        }
    }
    let title = extract_title(&doc);

    // Р“РµР№С‚ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ: top-level РґРѕРєСѓРјРµРЅС‚ РЅРµ sandboxed.
    // QuickJS + install_dom РґР°СЋС‚ СЃРєСЂРёРїС‚Р°Рј РїРѕР»РЅС‹Р№ РґРѕСЃС‚СѓРї Рє DOM-РґРµСЂРµРІСѓ.
    // fetch_provider РїСЂРѕР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ window.fetch(); ws_provider вЂ” РІ new WebSocket();
    // sse_provider вЂ” РІ new EventSource(). Р’СЃРµ С‚СЂРё РёСЃРїРѕР»СЊР·СѓСЋС‚ РѕРґРёРЅ HttpClient.
    let (fetch_provider, ws_provider, sse_provider) = match base {
        ResourceBase::Url(_) => {
            let client = base.http_client_for_subresource(Arc::clone(sink), cookie_jar.clone());
            let arc_client = Arc::new(client);
            let fp: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsFetchProvider>);
            let wp: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsWebSocketProvider>);
            let sp: Option<Arc<dyn lumen_core::ext::JsSseProvider>> =
                Some(arc_client as Arc<dyn lumen_core::ext::JsSseProvider>);
            (fp, wp, sp)
        }
        ResourceBase::File(_) => (None, None, None),
    };
    // URL СЃС‚СЂР°РЅРёС†С‹ РґР»СЏ РёРЅРёС†РёР°Р»РёР·Р°С†РёРё window.location РІ JS.
    let page_url = base_url_string(base);
    // Extension content scripts: collect JS sources that match the page URL.
    let ext_registry = extensions::ExtensionRegistry::load();
    let ext_scripts = ext_registry.content_scripts_for_url(&page_url);
    // BUG-164: collect classic + module scripts in document order and fetch
    // external `<script src>` bodies via the subresource fetcher, so SPA
    // bundles execute (lenta.ru owlBundle.js etc.), not just inline scripts.
    let (classic_scripts, module_scripts) = {
        let _s = lumen_core::trace::span("fetch-scripts", "net");
        let mut classic_items = Vec::new();
        let mut module_items = Vec::new();
        collect_scripts_ordered(&doc, doc.root(), &mut classic_items, &mut module_items);
        (
            resolve_script_sources(&classic_items, base, sink, cookie_jar.clone()),
            resolve_script_sources(&module_items, base, sink, cookie_jar.clone()),
        )
    };
    let run_scripts_span = lumen_core::trace::span("run-scripts", "script");
    // BUG-480 СЃСЂРµР· 1: РєР»РѕРЅС‹ РїСЂРѕРІР°Р№РґРµСЂРѕРІ/С…СЂР°РЅРёР»РёС‰ РґР»СЏ sub-РґРѕРєСѓРјРµРЅС‚РѕРІ <iframe> вЂ”
    // РѕСЃРЅРѕРІРЅС‹Рµ СѓС…РѕРґСЏС‚ РІ run_scripts_with_dom РїРѕ Р·РЅР°С‡РµРЅРёСЋ.
    let (frame_fp, frame_wp, frame_sp) =
        (fetch_provider.clone(), ws_provider.clone(), sse_provider.clone());
    let (frame_ls, frame_ss, frame_idb) =
        (ls_store.clone(), ss_store.clone(), idb_backend.clone());
    let (frame_sw, frame_sww, frame_cache) =
        (sw_backend.clone(), sw_worker_store.clone(), cache_backend.clone());
    let frame_cookie_jar = cookie_jar.clone();
    let (doc_arc, js_nav, js_ctx) = run_scripts_with_dom(
        doc,
        lumen_core::SandboxFlags::empty(),
        &page_url,
        fetch_provider,
        ws_provider,
        sse_provider,
        ls_store,
        ss_store,
        idb_backend,
        sw_backend,
        sw_worker_store,
        cache_backend,
        cookie_banner_dismiss,
        deterministic,
        cross_origin_isolated,
        &ext_scripts,
        classic_scripts,
        module_scripts,
        false,
    );
    drop(run_scripts_span);
    // HTML LS В§8.2.3 вЂ” after HTML parse + inline scripts: readyState в†’ "interactive"
    // + DOMContentLoaded event. Fires before images/fonts are decoded.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        js.notify_dom_content_loaded();
    }

    // CSS Selectors L4 В§9.6 `:target`: set current target from URL fragment so
    // the matcher has the correct target_id before style cascade in layout.
    let page_fragment = if let ResourceBase::Url(u) = base {
        lumen_core::url::Url::parse(u)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned))
    } else {
        None
    };
    {
        let mut d = doc_arc.lock().unwrap();
        d.set_target(page_fragment.as_deref());
        // Р“РµР№С‚ РѕС‚РїСЂР°РІРєРё С„РѕСЂРј: Phase 0 вЂ” top-level РґРѕРєСѓРјРµРЅС‚ РЅРµ sandboxed.
        check_form_gate(&d, lumen_core::SandboxFlags::empty());
        // Р“РµР№С‚ РЅР°РІРёРіР°С†РёРё: Phase 0 вЂ” top-level РґРѕРєСѓРјРµРЅС‚ РЅРµ sandboxed.
        check_navigation_gate(&d, lumen_core::SandboxFlags::empty());
        // РџСЂРёРјРµРЅСЏРµРј sandbox-РѕРіСЂР°РЅРёС‡РµРЅРёСЏ РёР· <iframe sandbox> СЌР»РµРјРµРЅС‚РѕРІ.
        // Phase 0: iframe sub-РґРѕРєСѓРјРµРЅС‚С‹ РЅРµ Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ вЂ” РїСЂРёРјРµРЅСЏРµРј РіРµР№С‚С‹
        // Рє СЃР°РјРѕРјСѓ iframe-СЌР»РµРјРµРЅС‚Сѓ, Р»РѕРіРёСЂСѓРµРј РѕРіСЂР°РЅРёС‡РµРЅРёСЏ РґР»СЏ Р±СѓРґСѓС‰РµРіРѕ Phase 1.
        apply_iframe_sandbox_gates(&d);
    }

    // BUG-480 СЃСЂРµР· 1: Р·Р°РіСЂСѓР·РєР° sub-РґРѕРєСѓРјРµРЅС‚РѕРІ <iframe>. Р›РѕРєРё РІРЅСѓС‚СЂРё С„СѓРЅРєС†РёРё
    // РєРѕСЂРѕС‚РєРёРµ вЂ” СЃРєСЂРёРїС‚С‹ РґРµС‚РµР№ Рё `load` С…РѕСЃС‚Р° РёРґСѓС‚ Р±РµР· СѓРґРµСЂР¶Р°РЅРёСЏ РґРµСЂРµРІР°.
    // РЎСЂРµР· 3: РґРѕРєСѓРјРµРЅС‚/Р±Р°Р·Р° СЃС‚СЂР°РЅРёС†С‹ РїРµСЂРµРґР°СЋС‚СЃСЏ Рё РєР°Рє top вЂ” Сѓ С„СЂРµР№РјРѕРІ
    // РїРµСЂРІРѕРіРѕ СѓСЂРѕРІРЅСЏ parent === top, РіР»СѓР±Р¶Рµ top РІСЃРµРіРґР° РєРѕСЂРµРЅСЊ.
    // РЎСЂРµР· 11: СЌРєСЂР°РЅРЅС‹Р№ media-РіРµР№С‚ `<link>` Рё РІСЊСЋРїРѕСЂС‚ picker-Р° РєР°СЂС‚РёРЅРѕРә вЂ”
    // С‚Рµ Р¶Рµ, СЃ РєР°РєРёРјРё СЃС‚СЂР°РЅРёС†Р° РіСЂСѓР·РёС‚ СЃРІРѕРё РїРѕРґСЂРµСЃСѓСЂСЃС‹ (print-РіРµР№С‚
    // С„СЂРµР№РјР°Рј РЅРµ РЅСѓР¶РµРЅ — РїРµС‡Р°С‚СЊ PDF РїРѕРґ-РґРѕРєСѓРјРµРЅС‚РѕРІ РІРЅРµ СЃСЂРµР·Р°).
    let frames = {
        let _s = lumen_core::trace::span("fetch-iframes", "net");
        load_frame_sub_documents(
            &doc_arc,
            0,
            base,
            &doc_arc,
            base,
            &screen_media_context(viewport, dark_mode),
            viewport,
            sink,
            frame_cookie_jar,
            frame_fp,
            frame_wp,
            frame_sp,
            frame_ls,
            frame_ss,
            frame_idb,
            frame_sw,
            frame_sww,
            frame_cache,
            cookie_banner_dismiss,
            deterministic,
            cross_origin_isolated,
            js_ctx.as_ref(),
        )
    };

    // Fetch + decode <img src>. Р”РѕР»Р¶РЅРѕ РёРґС‚Рё Р”Рћ layout, РїРѕС‚РѕРјСѓ С‡С‚Рѕ intrinsic
    // dimensions РёР· РґРµРєРѕРґРёСЂРѕРІР°РЅРЅРѕРіРѕ РёР·РѕР±СЂР°Р¶РµРЅРёСЏ РїСЂРѕСЃС‚Р°РІР»СЏСЋС‚СЃСЏ РєР°Рє HTML
    // presentational hints (width/height attribute) Рё РїРѕС‚РѕРј РїРѕРґС…РІР°С‚С‹РІР°СЋС‚СЃСЏ
    // style cascade. Errors silently РїСЂРѕРїСѓСЃРєР°СЋС‚СЃСЏ вЂ” Р±РёС‚Р°СЏ РєР°СЂС‚РёРЅРєР° РЅРµ РІР°Р»РёС‚
    // РІСЃСЋ СЃС‚СЂР°РЅРёС†Сѓ, layout РЅР°СЂРёСЃСѓРµС‚ СЃРµСЂС‹Р№ placeholder.
    // loading="lazy" РёР·РѕР±СЂР°Р¶РµРЅРёСЏ РІРѕР·РІСЂР°С‰Р°СЋС‚СЃСЏ РІ lazy_pairs Рё РЅРµ Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ СЃРµР№С‡Р°СЃ.
    let (images, animated_gifs, lazy_pairs) = {
        let _s = lumen_core::trace::span("fetch-images", "net");
        let mut d = doc_arc.lock().unwrap();
        fetch_and_decode_images(&mut d, base, sink, viewport, cookie_jar.clone(), target)
    };

    // P3-webvtt СЃСЂРµР· 3: Р·Р°РіСЂСѓР·РєР° WebVTT-СЃСѓР±С‚РёС‚СЂРѕРІ РёР· <track> РєР°Р¶РґРѕРіРѕ <video>.
    // РћС€РёР±РєРё С„РµС‚С‡Р°/РїР°СЂСЃРёРЅРіР° РЅРµ РІР°Р»СЏС‚ СЃС‚СЂР°РЅРёС†Сѓ вЂ” РІРёРґРµРѕ РїСЂРѕСЃС‚Рѕ РѕСЃС‚Р°С‘С‚СЃСЏ Р±РµР· cues.
    let page_tracks = {
        let d = doc_arc.lock().unwrap();
        tracks::load_video_tracks(&d, &|src| {
            fetch_vtt_text(src, base, sink, cookie_jar.clone())
        })
    };

    // Register decoded <img> bitmaps with the JS runtime so Canvas 2D
    // drawImage(imgElement, вЂ¦) can read the pixels. Collect nidв†’url from DOM
    // (same traversal fetch_and_decode_images used), join with decoded images by
    // URL, and share the decoded `Arc<Image>` into img_bitmap_store on the JS thread.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        let img_reqs = {
            let d = doc_arc.lock().unwrap();
            lumen_layout::collect_image_requests(&d, viewport)
        };
        // BUG-272 СЃСЂРµР· 20: share the decoded `Arc<Image>` with the JS canvas
        // drawImage store instead of eagerly copying an RGBA8 buffer per image.
        // The store converts to RGBA8 lazily, only for images a canvas actually
        // draws вЂ” images never used as a drawImage source cost zero extra bytes.
        let url_to_img: std::collections::HashMap<&str, &std::sync::Arc<lumen_image::Image>> =
            images.iter().map(|(url, img)| (url.as_str(), img)).collect();
        let bitmaps: Vec<(u32, std::sync::Arc<lumen_image::Image>)> = img_reqs
            .iter()
            .filter_map(|req| {
                let img = url_to_img.get(req.url.as_str())?;
                Some((req.node_id.index() as u32, std::sync::Arc::clone(img)))
            })
            .collect();
        if !bitmaps.is_empty() {
            js.register_img_bitmaps(bitmaps);
        }
    }

    // Р’СЃС‚СЂРѕРµРЅРЅС‹Рµ <style> + РІРЅРµС€РЅРёРµ <link rel=stylesheet>.
    let (css, dynamic_css, link_outcomes) = {
        let _s = lumen_core::trace::span("fetch-css", "net");
        let d = doc_arc.lock().unwrap();
        let link_media_ctx = if media_print {
            print_media_context(viewport, dark_mode)
        } else {
            screen_media_context(viewport, dark_mode)
        };
        // РРЅР»Р°Р№РЅРѕРІС‹Рµ <style>: РёС… `@import` СЂРµР·РѕР»РІСЏС‚СЃСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ Р±Р°Р·С‹
        // РґРѕРєСѓРјРµРЅС‚Р° (CSS-SPECS В§@import). Р’РЅРµС€РЅРёРµ <link> СЂРµР·РѕР»РІСЏС‚ СЃРѕР±СЃС‚РІРµРЅРЅС‹Рµ
        // `@import` РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ СЃРІРѕРµРіРѕ URL РІРЅСѓС‚СЂРё load_linked_stylesheets.
        let inline = extract_style_blocks(&d);
        let mut css = inline_css_imports(
            &inline,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
            &mut std::collections::HashSet::new(),
            0,
        );
        // BUG-743: РІСЃС‘, С‡С‚Рѕ РЅРµ РїСЂРёС€Р»Рѕ РёР· РёРЅР»Р°Р№РЅРѕРІС‹С… <style>, РѕС‚РєР»Р°РґС‹РІР°РµС‚СЃСЏ
        // РѕС‚РґРµР»СЊРЅРѕ вЂ” С‚Р°Рє РїРѕР·РґРЅРёР№ РґРёРЅР°РјРёС‡РµСЃРєРёР№ <style> РїРµСЂРµСЃРѕР±РёСЂР°РµС‚ РєР°СЃРєР°Рґ Р±РµР·
        // РµРґРёРЅРѕРіРѕ СЃРµС‚РµРІРѕРіРѕ Р·Р°РїСЂРѕСЃР°. `inline_css_imports` РІРѕР·РІСЂР°С‰Р°РµС‚
        // `<РёРјРїРѕСЂС‚С‹> + <РёСЃС…РѕРґРЅС‹Р№ С‚РµРєСЃС‚>`, РїРѕСЌС‚РѕРјСѓ РїСЂРµС„РёРєСЃ = РІСЃС‘ РґРѕ С…РІРѕСЃС‚Р°.
        let imports_prefix = css[..css.len() - inline.len()].to_owned();
        let (linked, link_outcomes) = load_linked_stylesheets(
            &d,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
        );
        css.push_str(&linked);
        let dyn_css = DynamicCssBase {
            imports_prefix,
            linked,
            inline_fp: inline_style_fingerprint(&d),
        };
        (css, dyn_css, link_outcomes)
    };

    // BUG-804: HTML LS В§4.6.7 В«process the linked resourceВ» вЂ” РєР°Р¶РґС‹Р№
    // `<link rel=stylesheet>` РѕР±СЏР·Р°РЅ СЃРѕРѕР±С‰РёС‚СЊ СЃС‚СЂР°РЅРёС†Рµ `load` РёР»Рё `error`.
    // РћС‚С‡С‘С‚ СѓС…РѕРґРёС‚ РѕС‚СЃСЋРґР°, Р° РЅРµ РёР· С€РёРјР°: Р»РёСЃС‚ РіСЂСѓР·РёС‚ РїСЂРѕС…РѕРґ РІС‹С€Рµ, Рё С‚РѕР»СЊРєРѕ РѕРЅ
    // Р·РЅР°РµС‚ РёСЃС…РѕРґ вЂ” РїРѕРІС‚РѕСЂРЅС‹Р№ С„РµС‚С‡ РёР· JS РґР°Р» Р±С‹ РІС‚РѕСЂРѕР№ Р·Р°РїСЂРѕСЃ Рё РІСЃС‘ СЂР°РІРЅРѕ РЅРµ
    // РѕС‚Р»РёС‡РёР» Р±С‹ В«Р»РёСЃС‚ РІ РєР°СЃРєР°РґРµВ» РѕС‚ В«Р±Р°Р№С‚С‹ РїСЂРёС€Р»РёВ». Р­Р»РµРјРµРЅС‚, РєРѕС‚РѕСЂС‹Р№ СѓР¶Рµ
    // РѕС‚С‡РёС‚Р°Р»СЃСЏ СЃР°Рј (РІСЃС‚Р°РІР»РµРЅРЅС‹Р№ СЃРєСЂРёРїС‚РѕРј вЂ” РѕРЅ РїСЂРѕС…РѕРґРёС‚ С‡РµСЂРµР·
    // `_lumen_link_prepare` Р•Р©РЃ Р”Рћ СЌС‚РѕРіРѕ РїСЂРѕС…РѕРґР°, СЃРєСЂРёРїС‚С‹ РІС‹РїРѕР»РЅСЏСЋС‚СЃСЏ СЂР°РЅСЊС€Рµ),
    // РѕС‚СЃРµРєР°РµС‚СЃСЏ РѕР±С‰РёРј РїРµСЂ-СѓР·Р»РѕРІС‹Рј С„Р»Р°РіРѕРј РЅР° JS-СЃС‚РѕСЂРѕРЅРµ.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx
        && !link_outcomes.is_empty()
    {
        use std::fmt::Write as _;
        let mut arg = String::with_capacity(link_outcomes.len() * 8 + 40);
        arg.push_str("_lumen_deliver_parser_link_events([");
        for (i, (node, ok)) in link_outcomes.iter().enumerate() {
            if i > 0 {
                arg.push(',');
            }
            let _ = write!(arg, "{},{}", node.index(), u8::from(*ok));
        }
        arg.push_str("]);");
        js.eval_js(&arg);
    }

    let sheet = {
        let _s = lumen_core::trace::span("parse-css", "parse");
        lumen_css_parser::parse(&css)
    };

    // PH3-19: @font-face Р·Р°РіСЂСѓР·РєР° СЂР°Р·РґРµР»РµРЅР° РЅР° РґРІР° РїСЂРѕС…РѕРґР°.
    // local()-РёСЃС‚РѕС‡РЅРёРєРё Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ СЃРёРЅС…СЂРѕРЅРЅРѕ (РёР· СЃРёСЃС‚РµРјРЅРѕРіРѕ РёРЅРґРµРєСЃР°, Р±С‹СЃС‚СЂРѕ).
    // url()-РёСЃС‚РѕС‡РЅРёРєРё вЂ” С‚РѕР»СЊРєРѕ СЃРѕР±РёСЂР°РµРј РІ pending_web_fonts; С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє
    // fetch+decode СЃРїР°РІРЅРёС‚СЃСЏ РІ apply_loaded_page в†’ РїРµСЂРІС‹Р№ paint РЅРµ Р¶РґС‘С‚ СЃРµС‚Рё.
    let (font_registry, pending_web_fonts) = {
        // PERF-12: this stretch вЂ” @font-face resolution through to the measurer's
        // system faces below вЂ” was the single largest unnamed hole in the
        // `--trace-nav` waterfall (114 ms of a 128 ms `navigation` on
        // samples/page.html, against a `layout` span of 0.6 ms). It is dominated
        // by the lazy system-font index build that PERF-11 caches.
        let _s = lumen_core::trace::span("font-faces", "font");
        load_font_faces(&sheet.font_faces, base, sink, cookie_jar.clone())
    };

    // Populate document.fonts with FontFace objects from @font-face rules.
    // local() вЂ” immediately Loaded; url() вЂ” Loading (Р±СѓРґРµС‚ Loaded РїРѕ FontLoaded).
    {
        let mut d = doc_arc.lock().unwrap();
        for rule in &sheet.font_faces {
            let mut font_face = rule_to_font_face(rule);
            // local() rules already resolved вЂ” mark Loaded; url() rules stay Loading.
            let has_local = rule.sources.iter().any(|s| {
                s.kind == lumen_css_parser::FontFaceSourceKind::Local
                    && font_registry.face_bytes_for_family(&rule.family).is_some()
            });
            if has_local {
                font_face.status = lumen_dom::FontFaceStatus::Loaded;
            }
            d.fonts_mut().add(font_face);
        }
    }

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("РѕС€РёР±РєР° СЂР°Р·Р±РѕСЂР° С€СЂРёС„С‚Р°: {e}"))?;
    // РњРЅРѕРіРѕС€СЂРёС„С‚РѕРІС‹Р№ РёР·РјРµСЂРёС‚РµР»СЊ: Inter РєР°Рє fallback + СѓР¶Рµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ local()-СЃРµРјСЊРё.
    // url()-СЃРµРјСЊРё РґРѕР±Р°РІСЏС‚СЃСЏ РїРѕР·Р¶Рµ С‡РµСЂРµР· FontLoaded + relayout_with_web_fonts.
    let mut measurer = lumen_paint::MultiFontMeasurer::new(&font)
        .map_err(|e| format!("РѕС€РёР±РєР° РјРµС‚СЂРёРє С€СЂРёС„С‚Р°: {e}"))?;
    // BUG-128: СЃРёСЃС‚РµРјРЅС‹Рµ face-С‹ вЂ” С‚Рµ Р¶Рµ, С‡С‚Рѕ РІС‹Р±РµСЂРµС‚ СЂРµРЅРґРµСЂ.
    {
        // PERF-11/PERF-12: `system_font_faces()` is where the lazy system font
        // index is built on first use вЂ” hundreds of files parsed, once per
        // process. Named separately from `font-faces` so the trace attributes
        // the cost to the index rather than to @font-face handling.
        let _s = lumen_core::trace::span("system-fonts", "font");
        measurer.set_system_faces(system_font_faces());
    }
    for rule in &sheet.font_faces {
        if !rule.family.is_empty()
            && let Some(bytes) = font_registry.face_bytes_for_family(&rule.family)
        {
            // CSS Fonts L4 В§5.1: РїРµСЂРµРґР°С‘Рј unicode-range РёР· @font-face РґРµСЃРєСЂРёРїС‚РѕСЂР°.
            let ranges = rule.unicode_range.as_deref()
                .map(lumen_font::parse_unicode_ranges)
                .unwrap_or_default();
            measurer.register_family_with_ranges(&rule.family, bytes, ranges);
        }
    }
    let font_provider = Arc::new(font_registry);

    // BUG-270: РїРµС‡Р°С‚СЊ РІ PDF С„РёР»СЊС‚СЂСѓРµС‚ РєР°СЃРєР°Рґ РїРѕ media_type="print" С‡РµСЂРµР·
    // sticky thread-local. Р¤Р»Р°Рі per-pass, РїРѕСЌС‚РѕРјСѓ СЃР±СЂР°СЃС‹РІР°РµРј СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ layout,
    // С‡С‚РѕР±С‹ РїРѕСЃР»РµРґСѓСЋС‰РёРµ СЌРєСЂР°РЅРЅС‹Рµ РїСЂРѕС…РѕРґС‹ РЅР° СЌС‚РѕРј Р¶Рµ РїРѕС‚РѕРєРµ РЅРµ РЅР°СЃР»РµРґРѕРІР°Р»Рё print.
    lumen_layout::set_print_media(media_print);
    let layout = {
        let _s = lumen_core::trace::span("layout", "layout");
        let d = doc_arc.lock().unwrap();
        lumen_layout::layout_measured_hyp(&d, &sheet, viewport, &measurer, hp, dark_mode)
    };
    lumen_layout::set_print_media(false);

    // CSS Backgrounds L3 В§3.10 вЂ” СЃРѕР±РёСЂР°РµРј `background-image: url(...)` СѓР¶Рµ
    // РїРѕСЃР»Рµ layout-Р° (РєР°СЂС‚РёРЅРєРё С„РѕРЅР° РЅРµ РІР»РёСЏСЋС‚ РЅР° СЂР°СЃС‡С‘С‚ РєРѕСЂРѕР±РѕРє). Р”РµРєРѕРґРёСЂСѓРµРј
    // Рё РґРѕР±Р°РІР»СЏРµРј Рє `images` С‚РµРј Р¶Рµ РєР»СЋС‡РѕРј, С‡С‚Рѕ СЌРјРёС‚С‚РµСЂ РєР»Р°РґС‘С‚ РІ
    // `DisplayCommand::DrawBackgroundImage.src`.
    let mut images = images;
    {
        let _s = lumen_core::trace::span("fetch-bg-images", "net");
        for (src, image) in fetch_and_decode_background_images(&layout, base, sink, cookie_jar.clone(), target) {
            images.push((src, image));
        }
    }

    let rule_count = sheet.rules.len();
    Ok(ParsedPage {
        document: doc_arc,
        stylesheet: sheet,
        layout,
        title,
        rule_count,
        images,
        animated_gifs,
        lazy_pairs,
        preload_hints,
        html_source: source,
        font_registry: font_provider,
        pending_web_fonts,
        js_navigate: js_nav,
        js_ctx,
        page_tracks,
        dynamic_css,
        frames,
    })
}

// в”Ђв”Ђ Window + Renderer в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

struct Lumen {
    display_list: DisplayList,
    /// Р’РµСЂСЃРёСЏ [`Self::display_list`] РґР»СЏ СЂРµРЅРґРµСЂРµСЂР° (BUG-405 СЃСЂРµР· 39).
    ///
    /// РњРµРЅСЏРµС‚СЃСЏ РїСЂРё РљРђР–Р”РћРњ РёР·РјРµРЅРµРЅРёРё СЃРїРёСЃРєР° вЂ” Рё Р·Р°РјРµРЅРµ С†РµР»РёРєРѕРј, Рё РїСЂР°РІРєРµ РЅР°
    /// РјРµСЃС‚Рµ, вЂ” РїРѕСЌС‚РѕРјСѓ СЃРїРёСЃРѕРє РјРµРЅСЏСЋС‚ С‚РѕР»СЊРєРѕ С‡РµСЂРµР· [`Self::set_display_list`]
    /// Рё [`Self::display_list_mut`], Р° РЅРµ РїСЂРёСЃРІР°РёРІР°РЅРёРµРј РїРѕР»СЋ. РџРѕРєР° РІРµСЂСЃРёСЏ С‚Р°
    /// Р¶Рµ, СЂРµРЅРґРµСЂРµСЂ РїРµСЂРµРёСЃРїРѕР»СЊР·СѓРµС‚ СЃРІС‘СЂС‚РєСѓ content-С‡Р°СЃС‚Рё РєР°РґСЂРѕРІС‹С… С…СЌС€РµР№ РІРјРµСЃС‚Рѕ
    /// РѕР±С…РѕРґР° РІСЃРµРіРѕ СЃРїРёСЃРєР°; РїСЂРѕРїСѓС‰РµРЅРЅС‹Р№ Р±Р°РјРї РїРѕРєР°Р·Р°Р» Р±С‹ СѓСЃС‚Р°СЂРµРІС€РёРµ РїРёРєСЃРµР»Рё.
    /// РќРёРєРѕРіРґР° РЅРµ `0`: РЅРѕР»СЊ Р·Р°СЂРµР·РµСЂРІРёСЂРѕРІР°РЅ Р·Р° В«РІРµСЂСЃРёСЏ РЅРµРёР·РІРµСЃС‚РЅР°В».
    display_list_epoch: u64,
    /// Tile-based dirty-rect tracker. Updated on every display-list change via
    /// [`lumen_paint::TileGrid::update_from_diff`]. Dirty tiles are re-rendered
    /// on the next frame; clean tiles reuse the previous output (Phase 2).
    tile_grid: lumen_paint::TileGrid,
    /// Per-subtree display-list cache. Keyed by stacking-context root `NodeId`.
    /// Hit on a matching `content_hash` в†’ skip re-traversing the layout tree for
    /// that subtree. Registered with `cache_registry` so OS memory-pressure
    /// events evict it via `EvictableCache::on_memory_pressure` (EE-4).
    display_list_cache: lumen_paint::DisplayListCache,
    title: Option<String>,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ `<img>` СЂРµСЃСѓСЂСЃС‹. Р”Рѕ СЃРѕР·РґР°РЅРёСЏ Renderer-Р° вЂ” С…СЂР°РЅСЏС‚СЃСЏ
    /// РІ Vec Рё Р·Р°Р»РёРІР°СЋС‚СЃСЏ РІ GPU РІ `resumed`; РїРѕСЃР»Рµ вЂ” register_image РёРґС‘С‚
    /// РЅР°РїСЂСЏРјСѓСЋ РІ `reload`. РќР° РїРµСЂРµС…РѕРґР°С… РјРµР¶РґСѓ СЃС‚СЂР°РЅРёС†Р°РјРё РѕС‡РёС‰Р°РµС‚СЃСЏ С‡РµСЂРµР·
    /// `Renderer::clear_images` + РїРµСЂРµСѓСЃС‚Р°РЅРѕРІРєР°. `Arc<Image>` (BUG-272 СЃСЂРµР· 17).
    pending_images: Vec<(String, Arc<lumen_image::Image>)>,
    /// PH3-19: СЂРµРµСЃС‚СЂ С€СЂРёС„С‚РѕРІ С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ (local() + web-С€СЂРёС„С‚С‹, РїСЂРёС€РµРґС€РёРµ
    /// С‡РµСЂРµР· `FontLoaded`). РҐСЂР°РЅРёС‚СЃСЏ РѕС‚РґРµР»СЊРЅРѕ РѕС‚ `Arc<dyn FontProvider>` РІ renderer-Рµ,
    /// С‡С‚РѕР±С‹ `user_event(FontLoaded)` РјРѕРі РґРѕСЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ С€СЂРёС„С‚ С‡РµСЂРµР·
    /// `register_from_bytes` Р±РµР· РґР°СѓРЅРєР°СЃС‚Р°, Р° Р·Р°С‚РµРј РѕР±РЅРѕРІРёС‚СЊ СЂРµРЅРґРµСЂРµСЂ РѕРґРЅРѕР№ СЃС‚СЂРѕРєРѕР№.
    /// РЎР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РЅР° РєР°Р¶РґСѓСЋ РЅР°РІРёРіР°С†РёСЋ РІРјРµСЃС‚Рµ СЃ `web_fonts`.
    page_font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: web-С€СЂРёС„С‚С‹ С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹, СѓР¶Рµ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ РёР· @font-face url().
    /// РСЃРїРѕР»СЊР·СѓСЋС‚СЃСЏ РґР»СЏ РїРµСЂРµСЃР±РѕСЂРєРё `MultiFontMeasurer` РїСЂРё РєР°Р¶РґРѕРј relayout (resize,
    /// scroll, JS DOM mutation) вЂ” Р±РµР· С…СЂР°РЅРµРЅРёСЏ Р·РґРµСЃСЊ resize-relayout С‚РµСЂСЏР» Р±С‹
    /// web-РјРµС‚СЂРёРєРё Рё РѕС‚РєР°С‚С‹РІР°Р»СЃСЏ Рє Inter.  РћС‡РёС‰Р°РµС‚СЃСЏ РЅР° РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё.
    web_fonts: Vec<LoadedWebFont>,
    source: PageSource,
    event_sink: Arc<dyn EventSink>,
    modifiers: ModifiersState,
    window: Option<Arc<Window>>,
    /// Detected target `ColorSpace` for the active display.
    /// Populated at startup from the OS (Windows WCS/DXGI/EDID query).
    /// Defaults to `ColorSpace::Srgb` when the display profile is unknown or
    /// the OS query fails вЂ” making the whole wide-gamut pipeline a no-op on
    /// sRGB-only hardware.
    #[allow(dead_code)] // РїРѕС‚СЂРµР±РёС‚РµР»СЊ РїРѕСЏРІРёС‚СЃСЏ РїСЂРё P3 wiring (ph3-color-management Step 1)
    display_color_profile: platform::display_color_profile::PlatformDisplayColorProfile,
    renderer: Option<Box<dyn RenderBackend>>,
    /// CC-4: chrome document + stylesheet, parsed once at startup via
    /// [`lumen_chrome::parse_document`] from `chrome_preview::HTML` вЂ” the same
    /// bytes `build.rs` already CSS-gated. Only relaid out on resize
    /// ([`Lumen::relayout_chrome_host`]); the asset has no dynamic content yet
    /// (`ChromeModel` DOM mutation is CC-6), so nothing else invalidates it.
    ///
    /// CC-15-6: always `Some` since the `LUMEN_LEGACY_CHROME` rollback flag was
    /// deleted вЂ” the `Option` is now only the shape every accessor already reads
    /// through, not a live "no engine chrome" mode.
    chrome_doc: Option<(lumen_dom::Document, lumen_css_parser::Stylesheet)>,
    /// CC-4: `LayoutBox` + display list of the last `relayout_chrome_host` pass,
    /// painted at the front of `overlay_buf` every frame (legacy panels/tab-bar/
    /// toolbar still draw over it, painter's order). `None` until the first
    /// resize after startup provides a window size. `#contentArea` вЂ” the
    /// design reference's placeholder for tab content, doubling as the
    /// brief's "`#page-host`" вЂ” is pruned out of this tree entirely (not just
    /// its children) before painting, so neither its demo markup nor its own
    /// `background:var(--surface-0)` fill can end up on top of the real page
    /// painted separately at [`Self::chrome_page_host_rect`]'s rect.
    chrome_layout: Option<(lumen_layout::LayoutBox, lumen_paint::DisplayList)>,
    /// CC-4: `#contentArea`'s rect, captured from the layout tree right
    /// before [`Self::relayout_chrome_host`] prunes that node out вЂ” replaces
    /// the legacy `left_dock()` width / `toolbar::CHROME_H` pair at the two
    /// render-time page-offset call sites. `None` until the first chrome
    /// layout exists (mirrors `chrome_layout`).
    chrome_page_host_rect: Option<Rect>,
    /// CC-5 (docs/tasks/p1-css-chrome.md): hovered node in `chrome_layout`'s
    /// tree, or `None` when the pointer isn't over the chrome's own opaque
    /// area (or off the flag). Set from `WindowEvent::CursorMoved`; feeds
    /// `:hover` into the next [`Self::relayout_chrome_host`] pass. Kept
    /// separate from [`Self::hovered_nid`] (the page's own hover state) for
    /// the same reason `relayout_chrome_host` explicitly resets the
    /// interactive thread-locals rather than inheriting them вЂ” the two
    /// documents' hover state must never leak into each other's layout pass.
    chrome_hovered_nid: Option<NodeId>,
    /// CC-5: pressed node in `chrome_layout`'s tree вЂ” mirrors
    /// [`Self::chrome_hovered_nid`] but for `:active`, set from
    /// `WindowEvent::MouseInput` press.
    chrome_active_nid: Option<NodeId>,
    /// CC-7 (docs/tasks/p1-css-chrome.md): `#omniInput`'s post-layout rect
    /// from the last [`Self::relayout_chrome_host`] pass вЂ” the anchor for the
    /// hand-painted caret overlay (editing itself stays owned by the legacy
    /// `address_bar::AddressBarState`, no native caret exists for `<input>`
    /// yet). `None` off the flag or before the first chrome layout.
    chrome_omni_input_rect: Option<Rect>,
    /// CC-8 (docs/tasks/p1-css-chrome.md): `true` collapses the vertical
    /// sidebar to its icon rail (`#sidebar.collapsed`, `--sidebar-w-collapsed`
    /// in the asset). Toggled by `ChromeAction::ToggleSidebar`
    /// (`.sb-collapse` button). Independent of [`Self::vertical_tabs`]'s own
    /// `visible` flag вЂ” that one picks vertical vs. horizontal layout,
    /// this one narrows the vertical sidebar without hiding it.
    chrome_sidebar_collapsed: bool,
    /// CC-10b (docs/tasks/p1-css-chrome.md): `data-section` slug of the
    /// active `#view-settings` tab (`"general"`/`"privacy"`/`"appearance"`/
    /// `"sync"`/`"ext"`/`"qa"`). Engine-chrome-only UI state вЂ” the design's 6
    /// sections don't line up with `SettingsPanel::SettingsSection`'s 7 (see
    /// `lumen_chrome::ChromeSettingsModel` doc comment), so this is a
    /// separate field rather than a projection of the legacy enum. Set by
    /// `ChromeAction::SetSettingsSection`.
    chrome_settings_section: String,
    /// CC-11 (docs/tasks/p1-css-chrome.md): CSS Animations scheduler for the
    /// chrome document вЂ” a separate instance from [`Self::animation_scheduler`]
    /// because `chrome_doc` and the page `Document` number `NodeId`s
    /// independently (both start at 0), so a shared scheduler would collide
    /// entries between the two trees. Ticked on every `RedrawRequested`
    /// alongside the page scheduler.
    /// Unlike the page scheduler, never `.clear()`-ed: `chrome_doc`'s nodes
    /// persist for the process lifetime (no reload/navigation equivalent for
    /// chrome), so clearing on every [`Self::relayout_chrome_host`] call вЂ”
    /// which happens far more often than page relayouts (any hover/click) вЂ”
    /// would restart `infinite` animations (the spinner) on every interaction.
    chrome_animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CC-11: CSS Transitions scheduler for the chrome document вЂ” mirrors
    /// [`Self::transition_scheduler`] but keyed against `chrome_doc`'s own
    /// `NodeId` space (see [`Self::chrome_animation_scheduler`] doc comment).
    /// `sync()` runs at the end of [`Self::relayout_chrome_host`] (chrome's
    /// post-layout point, mirroring `apply_relayout_result`'s page-side
    /// sync); `tick()` runs on every `RedrawRequested`.
    chrome_transition_scheduler: TransitionScheduler,
    /// CC-11: computed styles from the previous [`Self::relayout_chrome_host`]
    /// pass вЂ” needed by [`Self::chrome_transition_scheduler`]'s `sync()` to
    /// detect which properties changed. Mirrors [`Self::prev_styles`] for the
    /// chrome tree.
    chrome_prev_styles: HashMap<NodeId, ComputedStyle>,
    /// BUG-341 S5/S22: what the previous pass's [`take_content_area`] removed
    /// from [`Self::chrome_layout`].
    ///
    /// The incremental graft needs the *pristine* (pre-pruning) tree of the
    /// previous pass: it matches children **by index**, and pruning
    /// `#contentArea` shifts every sibling after it (see BUG-341's "attempted
    /// mitigation" note, which hit exactly that). S5 met this by keeping a
    /// whole second copy of the tree (`chrome_prev_pristine_layout =
    /// layout.clone()`), which the S22 census priced at 0.16-0.40 ms per
    /// cycle вЂ” the largest item left in a chrome interaction, and ~40 % of a
    /// hover cycle. S22 keeps the *difference* instead: the pruning is
    /// recorded here and undone by [`restore_content_area`] at the top of the
    /// next pass, so the live tree in [`Self::chrome_layout`] becomes the
    /// basis and no copy is made at all. Sound because nothing mutates
    /// `chrome_layout` between passes вЂ” it is read-only until
    /// [`Self::relayout_chrome_host`] replaces it wholesale.
    chrome_content_area_detached: Option<ContentAreaDetachment>,
    /// BUG-341 S5: the per-node `ComputedStyle` cascade cache
    /// ([`lumen_layout::CounterMap::styles`]) from the previous pass вЂ”
    /// `RestyleDelta::prev_styles` for the next incremental cascade. Distinct
    /// from [`Self::chrome_prev_styles`] (CC-11's transition-sync snapshot,
    /// collected from post-layout `LayoutBox`es *after* `font-size-adjust` has
    /// mutated them in place) вЂ” the cascade cache must be the pre-layout,
    /// pre-adjust styles the cascade itself produced, or the incremental
    /// cascade's `incr == full` correctness gate (BUG-341 brief В§4) would
    /// compare against the wrong reference.
    chrome_prev_cascade_styles: lumen_layout::CascadeStyles,
    /// BUG-341 S5: `(hover, focus, active)` node ids from the previous pass вЂ”
    /// `restyle_root_set_for_state_change`'s `prev` argument for each axis, so
    /// a hover/focus/active transition can compute its conservative dirty
    /// root-set (brief В§4).
    chrome_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    /// BUG-341 S5: viewport size the previous pass laid out at вЂ” a resize
    /// invalidates the previous tree's geometry for `graft_geometry` purposes,
    /// so a viewport change forces the full-layout path regardless of what
    /// `bind_model_tracked` reports touched.
    chrome_prev_viewport: Option<Size>,
    /// BUG-341 S5: Forced Colors Mode state ([`lumen_layout::forced_colors_active`])
    /// the previous pass ran under. Not part of `ChromeModel` (it's a
    /// thread-local accessibility preference, not shell UI state), but it does
    /// feed the cascade вЂ” a change here must force a full recompute (the
    /// `bind_model_tracked` diff cannot see it, since it never touches `doc`),
    /// or the incremental path would reuse `chrome_prev_cascade_styles`
    /// computed under the wrong Forced-Colors state.
    chrome_prev_forced_colors: bool,
    /// CC-11: last computed animation/transition frame for the chrome
    /// document. `None` when the chrome flag is off or nothing is currently
    /// animating. Only the compositor-offloadable properties (opacity,
    /// transform, color, background-color) are applied вЂ” same limitation as
    /// [`Self::anim_frame`] for the page, since `width` transitions
    /// (`#sidebar`, `.dl-progress-fill`) aren't in the Phase-0 animatable
    /// property table (`TransitionScheduler::sync`) and stay unanimated.
    chrome_anim_frame: Option<lumen_layout::AnimationFrame>,
    /// HTML event loop runtime. РќР° РєР°Р¶РґРѕР№ РёС‚РµСЂР°С†РёРё winit-loop (AboutToWait)
    /// РІС‹РїРѕР»РЅСЏРµС‚СЃСЏ РѕРґРЅР° task, РЅР° RedrawRequested вЂ” run_rendering_step
    /// (РІС‹Р·С‹РІР°РµС‚ rAF-callback-Рё), РЅР° WindowEvent::Resized вЂ”
    /// deliver_observer_records(Resize).
    runtime: runtime::EventLoop,
    /// CSS Animations timeline scheduler вЂ” С‚РёРєР°РµС‚СЃСЏ РЅР° РєР°Р¶РґРѕРј RedrawRequested.
    /// РҐСЂР°РЅРёС‚ start-time РґР»СЏ РєР°Р¶РґРѕР№ Р·Р°РїСѓС‰РµРЅРЅРѕР№ Р°РЅРёРјР°С†РёРё Рё РІС‹С‡РёСЃР»СЏРµС‚
    /// РёРЅС‚РµСЂРїРѕР»РёСЂРѕРІР°РЅРЅС‹Рµ Р·РЅР°С‡РµРЅРёСЏ. РћС‡РёС‰Р°РµС‚СЃСЏ РїСЂРё load/reload.
    animation_scheduler: animation_scheduler::AnimationScheduler,
    /// CSS Transitions scheduler вЂ” reactive; РѕР±РЅР°СЂСѓР¶РёРІР°РµС‚ РёР·РјРµРЅРµРЅРёСЏ computed-style
    /// РјРµР¶РґСѓ РґРІСѓРјСЏ relayout-Р°РјРё Рё РёРЅС‚РµСЂРїРѕР»РёСЂСѓРµС‚ Р·РЅР°С‡РµРЅРёСЏ per-frame.
    /// `sync()` РІС‹Р·С‹РІР°РµС‚СЃСЏ РїРѕСЃР»Рµ РєР°Р¶РґРѕРіРѕ layout-РѕР±РЅРѕРІР»РµРЅРёСЏ; `tick()` вЂ” РЅР° РєР°Р¶РґРѕРј
    /// RedrawRequested РІРјРµСЃС‚Рµ СЃ animation_scheduler. РћС‡РёС‰Р°РµС‚СЃСЏ РїСЂРё load/reload.
    transition_scheduler: TransitionScheduler,
    /// Tracks nodes that are "entering" the document (inserted or display:noneв†’visible)
    /// so that `@starting-style` rules can provide the before-change style for their
    /// entry transitions (CSS Transitions L2 В§3.4). Consumed in `relayout()`.
    starting_style_tracker: StartingStyleTracker,
    /// Computed styles РїСЂРµРґС‹РґСѓС‰РµРіРѕ layout-РґРµСЂРµРІР° вЂ” РЅСѓР¶РЅС‹ `transition_scheduler.sync()`
    /// РґР»СЏ РѕРїСЂРµРґРµР»РµРЅРёСЏ РёР·РјРµРЅРёРІС€РёС…СЃСЏ СЃРІРѕР№СЃС‚РІ. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїРѕСЃР»Рµ РєР°Р¶РґРѕРіРѕ layout.
    prev_styles: HashMap<NodeId, ComputedStyle>,
    /// BUG-341 S7: `CounterMap::styles()` cascade cache from the last
    /// [`Self::try_relayout_raf_incremental`] call that took the restyle-aware
    /// path (`layout_mutation_incremental_restyle`) вЂ” the `RestyleDelta::prev_styles`
    /// basis for the *next* such call. `None` whenever `layout_box` was set by
    /// any other producer (`relayout()`, tab switch, page load, hibernate
    /// restore, streaming layout, вЂ¦), since a stale cache would silently derive
    /// the wrong dirty-root set against a `layout_box` it does not match вЂ”
    /// `try_relayout_raf_incremental` falls back to the existing
    /// full-cascade-plus-graft path (`layout_mutation_incremental`) whenever
    /// this is `None`.
    page_prev_cascade_styles: Option<lumen_layout::CascadeStyles>,
    /// Interactive state (`hovered_nid`/`focused_node`/`active_nid`) at the
    /// moment `page_prev_cascade_styles` was captured вЂ” the `prev` side of the
    /// next call's `restyle_root_set_for_state_change`. Only meaningful when
    /// `page_prev_cascade_styles` is `Some`.
    page_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    /// РџРѕСЃР»РµРґРЅРёР№ РІС‹С‡РёСЃР»РµРЅРЅС‹Р№ РєР°РґСЂ Р°РЅРёРјР°С†РёР№. `None` вЂ” СЃС‚СЂР°РЅРёС†Р° РЅРµ Р·Р°РіСЂСѓР¶РµРЅР°
    /// РёР»Рё РЅРµС‚ Р°РєС‚РёРІРЅС‹С… Р°РЅРёРјР°С†РёР№.
    anim_frame: Option<lumen_layout::AnimationFrame>,
    /// Layout-РґРµСЂРµРІРѕ С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ вЂ” РЅСѓР¶РµРЅ scheduler-Сѓ РґР»СЏ РѕР±С…РѕРґР° СѓР·Р»РѕРІ
    /// Рё РёР·РІР»РµС‡РµРЅРёСЏ animation-longhands. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїСЂРё load/reload/relayout.
    layout_box: Option<lumen_layout::LayoutBox>,
    /// P3-webvtt СЃСЂРµР· 3: WebVTT-cues С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹ (`<video>` в†’ cues).
    page_tracks: tracks::PageTracks,
    /// CSS Scroll Snap L1 containers collected from `layout_box` after every
    /// layout update. Used by `start_smooth_scroll` / `scroll_x_by` to apply
    /// snap positions. Empty when `layout_box` is `None` or the page has no
    /// `scroll-snap-type` declarations. Cleared on navigation, recomputed on
    /// relayout / tab switch.
    snap_containers: Vec<SnapContainer>,
    /// Overflow scroll containers collected from `layout_box` after every layout
    /// update. Used by `MouseWheel` handler to route wheel events into the correct
    /// overflow container instead of always scrolling the page. Also used to fire
    /// `scroll` events after position changes. Cleared on navigation, recomputed on
    /// relayout / tab switch.
    scroll_containers: Vec<lumen_layout::ScrollContainer>,
    /// Р­РїРѕС…Р° РґР»СЏ rAF-timestamp-РѕРІ РІ РјРёР»Р»РёСЃРµРєСѓРЅРґР°С… РѕС‚ СЃС‚Р°СЂС‚Р° shell-Р°
    /// (DOMHighResTimeStamp вЂ” HTML В§8.1.5.1: В«timestamp passed to callback
    /// should be the current high resolution timeВ»).
    epoch: std::time::Instant,
    /// Timestamp (ms from `epoch`) of the last `requestAnimationFrame` batch fire.
    ///
    /// Used by the vsync gate: rAF callbacks fire at most once per `RAF_MIN_INTERVAL_MS`
    /// (~16.67 ms, 60 Hz). Initialized to `-RAF_MIN_INTERVAL_MS` so the first frame
    /// fires immediately.
    last_raf_batch_ms: f64,
    /// TEMP BUG-272 diagnostics: epoch seconds of the last memory report.
    last_mem_report_s: f64,
    /// РЎРµСЃСЃРёРѕРЅРЅС‹Р№ Р°РєРєСѓРјСѓР»СЏС‚РѕСЂ РІСЂРµРјС‘РЅ РєР°РґСЂРѕРІ (`LUMEN_FRAME_LOG`, M0.1 ADR-016).
    /// РќР°РїРѕР»РЅСЏРµС‚СЃСЏ С‚РѕР»СЊРєРѕ РїСЂРё РІРєР»СЋС‡С‘РЅРЅРѕРј frame-log; СЃРІРѕРґРєР° p50/p95/p99
    /// РїРµС‡Р°С‚Р°РµС‚СЃСЏ РїРѕ РєР°РґР°РЅСЃСѓ `LUMEN_MEM_REPORT` Рё РѕРґРёРЅ СЂР°Р· РЅР° РІС‹С…РѕРґРµ.
    frame_stats: lumen_paint::FrameStats,
    /// ADR-016 M2.0: СЃРµСЃСЃРёРѕРЅРЅС‹Р№ Р°РєРєСѓРјСѓР»СЏС‚РѕСЂ РІСЂРµРјРµРЅРё `relayout()` РЅР° UI-РїРѕС‚РѕРєРµ
    /// (СЃС‚РёР»СЊ + layout + СЃР±РѕСЂРєР° display-list + РґРѕСЃС‚Р°РІРєР° JS-observer'РѕРІ). РљР°Р¶РґС‹Р№
    /// РёРЅС‚РµСЂР°РєС‚РёРІРЅС‹Р№ relayout (DOM-РјСѓС‚Р°С†РёСЏ РёР· JS, hover/focus, resize, С‚РёРє
    /// Р°РЅРёРјР°С†РёРё, content-visibility) СЃРµРіРѕРґРЅСЏ Р±Р»РѕРєРёСЂСѓРµС‚ UI-РїРѕС‚РѕРє вЂ” СЌС‚Рѕ Рё РµСЃС‚СЊ С‚Р°
    /// СЂР°Р±РѕС‚Р°, РєРѕС‚РѕСЂСѓСЋ M2 СѓРЅРѕСЃРёС‚ РЅР° РѕС‚РґРµР»СЊРЅС‹Р№ engine-РїРѕС‚РѕРє. РќР°РїРѕР»РЅСЏРµС‚СЃСЏ С‚РѕР»СЊРєРѕ
    /// РїСЂРё РІРєР»СЋС‡С‘РЅРЅРѕРј `LUMEN_FRAME_LOG` (РєР°Рє `frame_stats`), СЃРІРѕРґРєР°
    /// `ENGINE_SUMMARY` РїРµС‡Р°С‚Р°РµС‚СЃСЏ РїРѕ РєР°РґР°РЅСЃСѓ `LUMEN_MEM_REPORT` Рё РѕРґРёРЅ СЂР°Р· РЅР°
    /// РІС‹С…РѕРґРµ вЂ” РґР°С‘С‚ before/after С‡РёСЃР»Р°, РЅР° РєРѕС‚РѕСЂС‹Рµ СЃРѕС€Р»СЋС‚СЃСЏ СЃР»РµРґСѓСЋС‰РёРµ СЃСЂРµР·С‹ M2.
    engine_stats: lumen_paint::FrameStats,
    /// ADR-016 M0.5: split fingerprint (content-hash + scroll/page offset) of the
    /// previously presented frame. Used only when `LUMEN_FRAME_LOG` is on: each
    /// frame is classified against it (`Identical`/`OffsetOnly`/`ContentChanged`)
    /// and the delta is logged, so the scroll-vs-content frame mix is measurable
    /// before M3 turns `OffsetOnly` into an actual blit fast path. `None` until
    /// the first logged frame.
    last_frame_fp: Option<lumen_paint::FrameFingerprint>,
    /// ADR-016 M3.2: retained scroll-band bookkeeping (the pure decision brain
    /// from M3.0/M3.1). Fed the layout content hash + scroll offset + viewport
    /// each frame to classify it as blit / blit+expose / repaint against the
    /// cached overscan band. Currently drives only the `LUMEN_FRAME_LOG`
    /// instrumentation (M3.2.0 вЂ” measure the real-content band mix before the GL
    /// blit path acts on it); the femtovg backend does not yet own a content
    /// surface, so normal runs pay nothing. Invalidated on navigation
    /// ([`Lumen::reset_to_blank_tab`]); resize/nav content changes also fall out
    /// naturally because the content hash folds surface size.
    scroll_cache: lumen_paint::ScrollCache,
    /// РЎРѕСЃС‚РѕСЏРЅРёРµ Ctrl+F. РћС‚РєСЂС‹С‚ Р»Рё bar, С‚РµРєСѓС‰РёР№ query Рё РёРЅРґРµРєСЃ Р°РєС‚РёРІРЅРѕРіРѕ
    /// СЃРѕРІРїР°РґРµРЅРёСЏ. РЎРѕРґРµСЂР¶РёРјРѕРµ РїРѕРёСЃРєР° РЅРµ СЃРѕС…СЂР°РЅСЏРµС‚СЃСЏ РјРµР¶РґСѓ reload-Р°РјРё
    /// (close() РїРѕР»РЅРѕСЃС‚СЊСЋ РѕС‡РёС‰Р°РµС‚ state); СЌС‚Рѕ СЃРѕР·РЅР°С‚РµР»СЊРЅРѕ: РїРѕСЃР»Рµ reload
    /// display list РґСЂСѓРіРѕР№, Рё СЃС‚Р°СЂС‹Рµ РїРѕР·РёС†РёРё СЃРѕРІРїР°РґРµРЅРёР№ СѓР¶Рµ РЅРµРІР°Р»РёРґРЅС‹.
    find: find::FindState,
    /// РЎРѕСЃС‚РѕСЏРЅРёРµ Ctrl+L Р°РґСЂРµСЃРЅРѕР№ СЃС‚СЂРѕРєРё. РћС‚РєСЂС‹С‚ Р»Рё Р±Р°СЂ Рё С‚РµРєСѓС‰РёР№ РІРІРѕРґ.
    /// Р—Р°РєСЂС‹РІР°РµС‚СЃСЏ РїСЂРё РЅР°РІРёРіР°С†РёРё (commit) Рё РїСЂРё Esc.
    address_bar: address_bar::AddressBarState,
    /// Click-hint overlay: vimium-style kbd-РЅР°РІРёРіР°С†РёСЏ РїРѕ РєР»РёРєР°Р±РµР»СЊРЅС‹Рј СЌР»РµРјРµРЅС‚Р°Рј.
    /// РћС‚РєСЂС‹РІР°РµС‚СЃСЏ РєР»Р°РІРёС€РµР№ F; Р·Р°РєСЂС‹РІР°РµС‚СЃСЏ Escape, СѓСЃРїРµС€РЅРѕР№ Р°РєС‚РёРІР°С†РёРµР№,
    /// РѕС‚РєСЂС‹С‚РёРµРј find/address bar РёР»Рё РїРµСЂРµС…РѕРґРѕРј РЅР° РґСЂСѓРіСѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    hint: hints::HintState,
    /// РўРµРєСѓС‰РµРµ РІРµСЂС‚РёРєР°Р»СЊРЅРѕРµ СЃРјРµС‰РµРЅРёРµ СЃС‚СЂР°РЅРёС†С‹ (CSS px). 0 вЂ” РІРµСЂС… РґРѕРєСѓРјРµРЅС‚Р°.
    /// Р Р°СЃС‚С‘С‚ РІРЅРёР·, РєР»Р°РјРїРёС‚СЃСЏ РІ `[0, max(0, content_height в€’ viewport_height)]`.
    /// РќР° load/reload СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ 0.
    scroll_y: f32,
    /// РўРµРєСѓС‰РµРµ РіРѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅРѕРµ СЃРјРµС‰РµРЅРёРµ СЃС‚СЂР°РЅРёС†С‹ (CSS px). 0 вЂ” Р»РµРІС‹Р№ РєСЂР°Р№.
    /// Р Р°СЃС‚С‘С‚ РІРїСЂР°РІРѕ, РєР»Р°РјРїРёС‚СЃСЏ РІ `[0, max(0, content_width в€’ viewport_width)]`.
    /// РќР° load/reload СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ 0.
    scroll_x: f32,
    /// `scroll_y` РїСЂРµРґС‹РґСѓС‰РµРіРѕ `RedrawRequested` вЂ” РґР»СЏ РѕС†РµРЅРєРё СЃРєРѕСЂРѕСЃС‚Рё СЃРєСЂРѕР»Р»Р°
    /// (fast-scroll РґРµРіСЂР°РґР°С†РёСЏ, EXPERIMENT.md В§2 СЃСЂРµР· 2).
    last_frame_scroll_y: f32,
    /// EMA-СЃРєРѕСЂРѕСЃС‚СЊ СЃРєСЂРѕР»Р»Р° РІ CSS px/РєР°РґСЂ (СЃРіР»Р°Р¶РёРІР°РµС‚ СЂР°Р·РѕРІС‹Рµ wheel-СЂС‹РІРєРё).
    scroll_velocity: f32,
    /// Р РµР¶РёРј Р±С‹СЃС‚СЂРѕРіРѕ СЃРєСЂРѕР»Р»Р°: С‚РёРєРё CSS-Р°РЅРёРјР°С†РёР№/GIF/video-GIF Р·Р°РјРѕСЂРѕР¶РµРЅС‹,
    /// РєРѕРЅС‚РµРЅС‚ scroll-СЃС‚Р°Р±РёР»РµРЅ, РєР°РґСЂС‹ СѓС…РѕРґСЏС‚ РІ page-compose HIT.
    fast_scroll: bool,
    /// РџРѕР»РЅР°СЏ РІС‹СЃРѕС‚Р° РєРѕРЅС‚РµРЅС‚Р° РІ CSS px вЂ” `max(rect.y + rect.height)` РїРѕ
    /// С‚РµРєСѓС‰РµРјСѓ display list-Сѓ. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїРѕСЃР»Рµ load/reload. 0 вЂ” РЅРµС‚ РєРѕРЅС‚РµРЅС‚Р°.
    content_height: f32,
    /// РџРѕР»РЅР°СЏ С€РёСЂРёРЅР° РєРѕРЅС‚РµРЅС‚Р° РІ CSS px вЂ” `max(rect.x + rect.width)` РїРѕ
    /// С‚РµРєСѓС‰РµРјСѓ display list-Сѓ. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РїРѕСЃР»Рµ load/reload. 0 вЂ” РЅРµС‚ РєРѕРЅС‚РµРЅС‚Р°.
    content_width: f32,
    /// CSS Containment L3 В§4.4 (BB-4): `(node, top_y)` РїРѕРґРґРµСЂРµРІСЊРµРІ, РїСЂРѕРїСѓС‰РµРЅРЅС‹С…
    /// РїРѕСЃР»РµРґРЅРёРј layout-РїСЂРѕС…РѕРґРѕРј РёР·-Р·Р° `content-visibility: auto` РІРЅРµ СЂР°СЃС€РёСЂРµРЅРЅРѕРіРѕ
    /// viewport. top_y вЂ” СЃС‚СЂР°РЅРёС†Р°-РєРѕРѕСЂРґРёРЅР°С‚С‹ (scroll 0) СЃС…Р»РѕРїРЅСѓС‚РѕРіРѕ Р±РѕРєСЃР°.
    /// РћР±РЅРѕРІР»СЏРµС‚СЃСЏ РІ `refresh_cv_state` РїРѕСЃР»Рµ РєР°Р¶РґРѕР№ СЃРјРµРЅС‹ `layout_box`.
    cv_skipped: Vec<(NodeId, f32)>,
    /// Ratchet-РЅР°Р±РѕСЂ auto-СѓР·Р»РѕРІ, СЃС‚Р°РІС€РёС… relevant (РІРѕС€Р»Рё РІ СЂР°СЃС€РёСЂРµРЅРЅС‹Р№ viewport
    /// РїСЂРё СЃРєСЂРѕР»Р»Рµ): РїСЂРѕРєРёРґС‹РІР°РµС‚СЃСЏ РІ layout С‡РµСЂРµР· `set_cv_relevant`, С‚Р°РєРёРµ СѓР·Р»С‹
    /// Р±РѕР»СЊС€Рµ РЅРµ РїСЂРѕРїСѓСЃРєР°СЋС‚СЃСЏ. РЎР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РїСЂРё Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹.
    cv_relevant: std::collections::HashSet<NodeId>,
    /// Skipped-СЃРѕСЃС‚РѕСЏРЅРёРµ **РєР°Р¶РґРѕРіРѕ** `content-visibility: auto` СѓР·Р»Р° РїСЂРѕС€Р»РѕРіРѕ
    /// РїСЂРѕС…РѕРґР° вЂ” Р±Р°Р·Р° РґРёС„С„Р° (BUG-852). РћС‚РґРµР»СЊРЅРѕ РѕС‚ `cv_skipped`, РєРѕС‚РѕСЂС‹Р№ РґРµСЂР¶РёС‚
    /// С‚РѕР»СЊРєРѕ РїСЂРѕРїСѓС‰РµРЅРЅС‹Рµ Рё С‚РѕР»СЊРєРѕ СЂР°РґРё ratchet-Р°: В«СѓР·Р»Р° РІ РєР°СЂС‚Рµ РЅРµС‚В» Рё В«СѓР·РµР» РЅРµ
    /// РїСЂРѕРїСѓС‰РµРЅВ» вЂ” СЂР°Р·РЅС‹Рµ РІРµС‰Рё, Рё РёРјРµРЅРЅРѕ РЅР° РїРµСЂРІРѕРј РґРµСЂР¶РёС‚СЃСЏ СЃРѕР±С‹С‚РёРµ РїРµСЂРІРѕРіРѕ
    /// РЅР°Р±Р»СЋРґРµРЅРёСЏ.
    cv_auto_state: std::collections::HashMap<NodeId, bool>,
    /// РћС‡РµСЂРµРґСЊ shell-СЃРѕР±С‹С‚РёР№ `ContentVisibilityChange` вЂ” РґРёС„С„С‹ skipped-СЃРѕСЃС‚РѕСЏРЅРёСЏ
    /// РјРµР¶РґСѓ layout-РїСЂРѕС…РѕРґР°РјРё. Р”СЂРµРЅРёСЂСѓРµС‚СЃСЏ СЂР°Р· РІ РєР°РґСЂ РІ `RedrawRequested` Рё
    /// СѓС…РѕРґРёС‚ РІ JS РєР°Рє `contentvisibilityautostatechange`. РљР°Рї 256 Р·Р°РїРёСЃРµР№.
    cv_events: Vec<ContentVisibilityChange>,
    /// OS-level `prefers-color-scheme` preference. `true` вЂ” СЃРёСЃС‚РµРјР° РІ С‚С‘РјРЅРѕР№ С‚РµРјРµ.
    /// Р§РёС‚Р°РµС‚СЃСЏ РёР· winit `Window::theme()` РїСЂРё СЃРѕР·РґР°РЅРёРё РѕРєРЅР° Рё РѕР±РЅРѕРІР»СЏРµС‚СЃСЏ РЅР°
    /// `WindowEvent::ThemeChanged`. РџСЂРѕРєРёРґС‹РІР°РµС‚СЃСЏ РІ JS `matchMedia` С‡РµСЂРµР·
    /// `deliver_media_query_changes(.., self.dark_mode)`. Default `false` (light)
    /// РґРѕ СЃРѕР·РґР°РЅРёСЏ РѕРєРЅР° Рё РІ headless/deterministic-СЂРµР¶РёРјР°С… (СЃС‚Р°Р±РёР»СЊРЅРѕСЃС‚СЊ snapshot-РѕРІ).
    dark_mode: bool,
    /// Per-tab user zoom factor (100% = 1.0). Changed via Ctrl+= / Ctrl+- / Ctrl+0.
    ///
    /// Combined with `<meta viewport initial-scale>` to compute the effective CSS
    /// layout viewport: `effective = physical / (meta_scale * zoom_factor)`.
    /// Resets to 1.0 on tab switch (stored in `PageSnapshot` for background tabs).
    zoom_factor: f32,
    /// Zoom factor the current display list was laid out at (ADR-016 M0.3).
    ///
    /// Transform-first zoom lets `zoom_factor` diverge from this between a
    /// Ctrl+/-/0 press and the debounced relayout; the backend previews the gap
    /// via `set_preview_scale(zoom_factor / laid_out_zoom_factor)`. Every
    /// `relayout()` re-syncs it to `zoom_factor` (the display list then matches
    /// the requested zoom, so no preview scale is needed).
    laid_out_zoom_factor: f32,
    /// Pending debounced relayout deadline for transform-first zoom (M0.3).
    ///
    /// Set on each Ctrl+/-/0 press to `now + ZOOM_RELAYOUT_DEBOUNCE_MS`; a fresh
    /// press pushes it later so a burst reflows only once. `about_to_wait`
    /// folds it into the `WaitUntil` deadline and runs `relayout()` when it
    /// elapses. `None` when no zoom preview is in flight.
    pending_zoom_relayout: Option<std::time::Instant>,
    /// РџРѕСЃР»РµРґРЅСЏСЏ РёР·РІРµСЃС‚РЅР°СЏ РїРѕР·РёС†РёСЏ РєСѓСЂСЃРѕСЂР° РІ **physical** РїРёРєСЃРµР»СЏС… (РѕС‚ winit).
    /// `None` РїРѕРєР° РєСѓСЂСЃРѕСЂ РЅРµ РІРѕС€С‘Р» РІ РѕРєРЅРѕ. РљРѕРЅРІРµСЂС‚РёСЂСѓРµС‚СЃСЏ РІ CSS px С‡РµСЂРµР·
    /// `scale_factor()` РЅРµРїРѕСЃСЂРµРґСЃС‚РІРµРЅРЅРѕ РІ hit-test / drag callback-Р°С….
    cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    /// Ph3 pointer-events-l3: CSS-pixel `(x, y)` samples from `CursorMoved`
    /// queued since the last flush, in chronological order. Pointer Events
    /// Level 3 В§4.1 "coalesced events" вЂ” multiple raw OS samples can arrive
    /// before the next paint; `flush_pointer_moves` turns the whole batch
    /// into one `pointermove` dispatch with the rest exposed via
    /// `getCoalescedEvents()`. Flushed once per `about_to_wait` tick, or
    /// earlier вЂ” right before a hover-boundary crossing or `pointerdown`/
    /// `pointerup` вЂ” so event order stays chronological.
    pending_pointer_moves: Vec<(f32, f32)>,
    /// DOM node currently under the mouse pointer (CSS `:hover` target).
    /// Updated on every `CursorMoved`; triggers relayout when it changes so
    /// `:hover` rules re-evaluate. `None` when cursor is outside the content area.
    hovered_nid: Option<NodeId>,
    /// DOM node whose mouse button is currently held down (CSS `:active` target).
    /// Set on `MouseInput(Pressed)`, cleared on `MouseInput(Released)`.
    active_nid: Option<NodeId>,
    /// РђРєС‚РёРІРЅС‹Р№ drag scrollbar-thumb-Р°: `Some` РїРѕРєР° Р·Р°Р¶Р°С‚Р° Р»РµРІР°СЏ РєРЅРѕРїРєР° РїРѕСЃР»Рµ
    /// click-Р° РїРѕ thumb-Сѓ. `MouseInput Released` РёР»Рё `CursorLeft` СЃР±СЂР°СЃС‹РІР°СЋС‚
    /// РІ `None`. РЎРЅР°РїС€РѕС‚ `(start_scroll_y, start_mouse_y)` С„РёРєСЃРёСЂРѕРІР°РЅ РЅР° РјРѕРјРµРЅС‚
    /// РЅР°С‡Р°Р»Р° drag-Р° вЂ” СЌС‚Рѕ РґР°С‘С‚ В«Р·Р°РєСЂРµРїР»С‘РЅРЅС‹Р№ РїРѕРґ РїР°Р»СЊС†РµРјВ» thumb (СЃС‚Р°РЅРґР°СЂС‚РЅС‹Р№
    /// scrollbar UX).
    scroll_drag: Option<scrollbar::ScrollDrag>,
    /// РђРєС‚РёРІРЅР°СЏ smooth-scroll Р°РЅРёРјР°С†РёСЏ РґР»СЏ keyboard / wheel / page-jump /
    /// find-scroll-to-match. `None` вЂ” `scroll_y` СЃС‚Р°С†РёРѕРЅР°СЂРµРЅ РёР»Рё РјРµРЅСЏРµС‚СЃСЏ
    /// РёРЅСЃС‚Р°РЅС‚РЅРѕ (drag, reload). РџСЂРё live-Р°РЅРёРјР°С†РёРё `RedrawRequested` С‚РёРєР°РµС‚
    /// РµС‘ С‡РµСЂРµР· `advance_scroll_anim` Рё РїСЂРѕСЃРёС‚ РµС‰С‘ РѕРґРёРЅ redraw РґРѕ Р·Р°РІРµСЂС€РµРЅРёСЏ.
    scroll_anim: Option<scroll_anim::ScrollAnim>,
    /// Momentum (kinetic) scroll: Р·Р°РїСѓСЃРєР°РµС‚СЃСЏ РїСЂРё `TouchPhase::Ended` СЃ
    /// РЅРµРЅСѓР»РµРІРѕР№ СЃРєРѕСЂРѕСЃС‚СЊСЋ РѕС‚ С‚Р°С‡РїР°РґР°. РўРёРєР°РµС‚СЃСЏ С‡РµСЂРµР· `advance_momentum`
    /// РІ `RedrawRequested`. `None` вЂ” РЅРµС‚ Р°РєС‚РёРІРЅРѕР№ РёРЅРµСЂС†РёРё.
    momentum_anim: Option<momentum_anim::MomentumAnim>,
    /// РњРіРЅРѕРІРµРЅРЅР°СЏ СЃРєРѕСЂРѕСЃС‚СЊ С‚Р°С‡РїР°РґР° РѕС‚ РїРѕСЃР»РµРґРЅРёС… `PixelDelta`-СЃРѕР±С‹С‚РёР№
    /// (CSS px / ms). РћР±РЅРѕРІР»СЏРµС‚СЃСЏ EWMA-С„РёР»СЊС‚СЂРѕРј. РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РїСЂРё
    /// `TouchPhase::Ended` РґР»СЏ Р·Р°РїСѓСЃРєР° `momentum_anim`.
    touchpad_vel: (f32, f32),
    /// Timestamp РїРѕСЃР»РµРґРЅРµРіРѕ `PixelDelta`-СЃРѕР±С‹С‚РёСЏ РґР»СЏ СЂР°СЃС‡С‘С‚Р° dt РІ EWMA.
    touchpad_vel_time_ms: f64,
    /// РџРѕСЃР»РµРґРЅРёР№ РІС‹СЃС‚Р°РІР»РµРЅРЅС‹Р№ cursor icon вЂ” С‡С‚РѕР±С‹ РїСЂРё РєР°Р¶РґРѕРј CursorMoved (Р° СЌС‚Рѕ
    /// СЃРѕС‚РЅРё СЃРѕР±С‹С‚РёР№ РІ СЃРµРєСѓРЅРґСѓ РїСЂРё Р°РєС‚РёРІРЅРѕРј РґРІРёР¶РµРЅРёРё РјС‹С€Рё) РЅРµ РґС‘СЂРіР°С‚СЊ
    /// `Window::set_cursor` РЅР°РїСЂР°СЃРЅРѕ. `None` вЂ” РµС‰С‘ РЅРµ РІС‹СЃС‚Р°РІР»СЏР»Рё (init).
    last_cursor_icon: Option<CursorIcon>,
    /// DOM + stylesheet РґР»СЏ relayout Р±РµР· РїРѕРІС‚РѕСЂРЅРѕРіРѕ fetch/parse. РћР±РЅРѕРІР»СЏРµС‚СЃСЏ
    /// РїСЂРё РєР°Р¶РґРѕРј load/reload. `None` вЂ” СЃС‚СЂР°РЅРёС†Р° РЅРµ Р·Р°РіСЂСѓР¶РµРЅР° (Empty source).
    layout_source: Option<LayoutSource>,
    /// Р¤Р»Р°Рі В«РЅСѓР¶РЅРѕ reload РїРѕСЃР»Рµ С‚РµРєСѓС‰РµРіРѕ about_to_waitВ». РЈСЃС‚Р°РЅР°РІР»РёРІР°РµС‚СЃСЏ
    /// closure-РѕРј РІРЅСѓС‚СЂРё queue_task вЂ” СЌС‚Рѕ РµРґРёРЅСЃС‚РІРµРЅРЅС‹Р№ СЃРїРѕСЃРѕР± СЃРѕРѕР±С‰РёС‚СЊ
    /// Lumen-Сѓ РёР· task-closure (РєРѕС‚РѕСЂР°СЏ `+ 'static` Рё РЅРµ РІР»Р°РґРµРµС‚ `&mut self`).
    pending_reload: Rc<Cell<bool>>,
    /// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ РѕС‚ JS (location.href=, assign, replace, reload),
    /// Р·Р°С…РІР°С‡РµРЅРЅС‹Р№ РІРѕ РІСЂРµРјСЏ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ СЃС‚СЂР°РЅРёС†С‹. РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚СЃСЏ
    /// РІ `about_to_wait` РїРѕСЃР»Рµ РїРµСЂРІРѕРіРѕ СЂРµРЅРґРµСЂР° Р·Р°РіСЂСѓР¶РµРЅРЅРѕР№ СЃС‚СЂР°РЅРёС†С‹.
    pending_js_navigate: Option<JsNavigateRequest>,
    /// Proxy РґР»СЏ РѕС‚РїСЂР°РІРєРё LoadEvent РёР· background-РїРѕС‚РѕРєР° Р·Р°РіСЂСѓР·РєРё РІ event loop.
    load_proxy: EventLoopProxy<LoadEvent>,
    /// РРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅС‹Р№ HTML-РїР°СЂСЃРµСЂ вЂ” Р°РєС‚РёРІРµРЅ РІРѕ РІСЂРµРјСЏ streaming load.
    /// `None` РґРѕ РїРµСЂРІРѕРіРѕ HtmlChunk РёР»Рё РїРѕСЃР»Рµ LoadDone/LoadError.
    stream_builder: Option<lumen_html_parser::IncrementalTreeBuilder>,
    /// РњРѕРјРµРЅС‚ РїРѕСЃР»РµРґРЅРµРіРѕ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅРѕРіРѕ РєР°РґСЂР° РїСЂРё streaming вЂ” РґР»СЏ throttling.
    stream_last_paint: std::time::Instant,
    /// CSS-С‚Р°Р±Р»РёС†Р° РёР· РїР°СЂР°Р»Р»РµР»СЊРЅС‹С… РїРѕС‚РѕРєРѕРІ Р·Р°РіСЂСѓР·РєРё CSS (PH1-2). РџСЂРёРјРµРЅСЏРµС‚СЃСЏ
    /// РІ `paint_partial_dom` РІРјРµСЃС‚Рѕ РїСѓСЃС‚РѕР№ С‚Р°Р±Р»РёС†С‹. РЎР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РЅР° РєР°Р¶РґС‹Р№
    /// РЅРѕРІС‹Р№ СЃС‚СЂР°РЅРёС‡РЅС‹Р№ load.
    stream_sheet: lumen_css_parser::Stylesheet,
    /// PH1-2b: `true` РєРѕРіРґР° `layout_box` СЃРѕРґРµСЂР¶РёС‚ РґРµСЂРµРІРѕ, РїРѕСЃС‚СЂРѕРµРЅРЅРѕРµ РёР· С‚РµРєСѓС‰РµРіРѕ
    /// streaming-DOM (РІР°Р»РёРґРЅС‹Р№ РёСЃС‚РѕС‡РЅРёРє РґР»СЏ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕРіРѕ graft). `false` РІ
    /// РЅР°С‡Р°Р»Рµ РЅРѕРІРѕР№ РЅР°РІРёРіР°С†РёРё вЂ” РїРµСЂРІС‹Р№ РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹Р№ РєР°РґСЂ РґРµР»Р°РµС‚ РїРѕР»РЅС‹Р№ layout Рё
    /// В«Р·Р°СЃРµРІР°РµС‚В» РґРµСЂРµРІРѕ; РїРѕСЃР»РµРґСѓСЋС‰РёРµ РєР°РґСЂС‹ СЂРµР»РµР№Р°СѓС‚СЏС‚ РёРЅРєСЂРµРјРµРЅС‚Р°Р»СЊРЅРѕ.
    stream_layout_seeded: bool,
    /// URL subresource-С…РёРЅС‚РѕРІ, СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… РІ sink РІРѕ РІСЂРµРјСЏ streaming
    /// (`EarlyPreloadHints`). Р¤РёРЅР°Р»СЊРЅС‹Р№ `dispatch_preload_hints` РІ `LoadDone`
    /// РїСЂРѕРїСѓСЃРєР°РµС‚ URL РёР· СЌС‚РѕРіРѕ РЅР°Р±РѕСЂР° вЂ” Р±РµР· РґСѓР±Р»РµР№ РІ stderr Рё Р±РµР· РїРѕРІС‚РѕСЂРЅС‹С…
    /// fetch-С‚СЂРёРіРіРµСЂРѕРІ РїСЂРё СЂРµР°Р»СЊРЅРѕРј РїР°СЂР°Р»Р»РµР»СЊРЅРѕРј prefetch. РћС‡РёС‰Р°РµС‚СЃСЏ РІ РЅР°С‡Р°Р»Рµ
    /// РєР°Р¶РґРѕРіРѕ РЅРѕРІРѕРіРѕ СЃС‚СЂР°РЅРёС‡РЅРѕРіРѕ load.
    preload_dispatched: std::collections::HashSet<String>,
    /// PH1-2c: РєР»СЋС‡Рё `src` РєР°СЂС‚РёРЅРѕРє, СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… РІ background-РїРѕС‚РѕРєРё
    /// РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ РІРѕ РІСЂРµРјСЏ С‚РµРєСѓС‰РµРіРѕ streaming-load. Р”РµРґСѓРї РјРµР¶РґСѓ
    /// РїСЂРѕРјРµР¶СѓС‚РѕС‡РЅС‹РјРё РєР°РґСЂР°РјРё `paint_partial_dom`, С‡С‚РѕР±С‹ РєР°Р¶РґС‹Р№ `<img>`
    /// Р·Р°РіСЂСѓР¶Р°Р»СЃСЏ РѕРґРёРЅ СЂР°Р·. РћС‡РёС‰Р°РµС‚СЃСЏ РІ РЅР°С‡Р°Р»Рµ РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё.
    stream_images_requested: std::collections::HashSet<String>,
    /// BUG-735: intrinsic-СЂР°Р·РјРµСЂС‹ `src` в†’ `(width, height)` РІСЃРµС… РєР°СЂС‚РёРЅРѕРє,
    /// РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹С… streaming/РґРёРЅР°РјРёС‡РµСЃРєРёРј РїСѓС‚С‘Рј РІ С‚РµРєСѓС‰РµР№ РЅР°РІРёРіР°С†РёРё.
    /// РљР°СЂС‚Р° Р¶РёРІС‘С‚ РґРѕ РєРѕРЅС†Р° РЅР°РІРёРіР°С†РёРё (Р° РЅРµ РґСЂРµРЅРёСЂСѓРµС‚СЃСЏ Р·Р° РїСЂРѕС…РѕРґ), РїРѕС‚РѕРјСѓ С‡С‚Рѕ
    /// `stream_images_requested` РґРµРґСѓРїР»РёС†РёСЂСѓРµС‚ Р·Р°РїСЂРѕСЃ РїРѕ URL: СѓР·РµР» СЃ С‚РµРј Р¶Рµ
    /// `src`, РґРѕР±Р°РІР»РµРЅРЅС‹Р№ СЃРєСЂРёРїС‚РѕРј РїРѕР·Р¶Рµ, СЃРІРѕРµРіРѕ `ImageDecoded` СѓР¶Рµ РЅРµ РїРѕР»СѓС‡РёС‚,
    /// Рё СЂР°Р·РјРµСЂ РµРјСѓ РјРѕР¶РµС‚ РґР°С‚СЊ С‚РѕР»СЊРєРѕ СЌС‚Р° РєР°СЂС‚Р°.
    stream_image_sizes: HashMap<String, (u32, u32)>,
    /// BUG-735: РІ РєР°СЂС‚Сѓ [`Self::stream_image_sizes`] РїРѕРїР°Р» РЅРѕРІС‹Р№ СЂР°Р·РјРµСЂ вЂ”
    /// РЅР° Р±Р»РёР¶Р°Р№С€РµРј РєР°РґСЂРµ РЅСѓР¶РЅРѕ СЂР°Р·РЅРµСЃС‚Рё РµРіРѕ РїРѕ `<img>` Рё, РµСЃР»Рё DOM РёР·РјРµРЅРёР»СЃСЏ,
    /// СЃРґРµР»Р°С‚СЊ СЂРµР»РµР№Р°СѓС‚. Р¤Р»Р°Рі РєРѕР°Р»РµСЃС†РёСЂСѓРµС‚ РїР°С‡РєСѓ РґРµРєРѕРґРѕРІ (СЃРѕС‚РЅСЏ РєР°СЂС‚РёРЅРѕРє = РѕРґРёРЅ
    /// РїСЂРѕС…РѕРґ, Р° РЅРµ СЃРѕС‚РЅСЏ СЂРµР»РµР№Р°СѓС‚РѕРІ).
    stream_image_sizes_dirty: bool,
    /// U-1: scroll offset to restore once the in-flight navigation completes.
    /// Set by back/forward navigation before kicking off an async (streaming)
    /// reload; consumed in `apply_loaded_page` (and the sync fallback in
    /// `reload`) after the page resets scroll to the top. `None` for ordinary
    /// navigations (they stay at 0,0). Needed because navigation is no longer
    /// synchronous вЂ” the old code set `scroll_x/y` right after `reload()`
    /// returned, but the scroll reset now happens later, at `LoadEvent::LoadDone`.
    pending_restore_scroll: Option<(f32, f32)>,
    /// Bfcache (HTML LS В§8.6): `.persisted` flag for the `pageshow` event fired
    /// after the next page load completes. Set `true` by `navigate_back`/
    /// `navigate_forward` when the destination is restored from bfcache,
    /// consumed (and reset to `false`) in `apply_loaded_page` right after
    /// `notify_window_loaded`. `false` for ordinary fresh loads.
    pending_pageshow_persisted: bool,
    /// Same-document (`pushState`) state JSON + display URL to apply once an
    /// in-flight reload completes. Set by `navigate_back`/`navigate_forward`
    /// when a multi-step `history.go(n)` traversal (`navigate_by`) silently
    /// shuttled through a full-document entry before landing on a
    /// same-document entry вЂ” the currently loaded document is not the one
    /// that entry belongs to, so `popstate`/the URL update must wait for the
    /// correct document to actually finish loading. `None` for the
    /// overwhelmingly common case (destination belongs to the already-loaded
    /// document); consumed in `apply_loaded_page`.
    pending_post_reload_traversal: Option<(String, Option<String>)>,
    /// Set by `navigate_by` immediately before calling `navigate_back`/
    /// `navigate_forward` when the multi-step shuffle passed through a
    /// full-document entry en route to the destination. Consumed (reset to
    /// `false`) at the top of both functions; direct callers (single-step
    /// Alt+Left/Right, not routed through `navigate_by`) always see `false`,
    /// matching their existing single-hop behavior.
    traversal_crossed_document: bool,
    /// U-1: monotonic navigation generation. Bumped on every async navigation
    /// (`reload` when a window exists) and on the initial streaming load. Each
    /// streaming `LoadEvent` carries the generation it was spawned under;
    /// `user_event` drops events whose generation is stale (a superseded
    /// navigation), so a slow earlier load can't paint over a newer page.
    load_generation: u64,
    /// BUG-757: СЂРµР°Р»СЊРЅР°СЏ Р±Р°Р·Р° С‚РµРєСѓС‰РµРіРѕ РґРѕРєСѓРјРµРЅС‚Р° Рё generation РЅР°РІРёРіР°С†РёРё, РІ
    /// РєРѕС‚РѕСЂРѕР№ РѕРЅР° РїРѕР»СѓС‡РµРЅР°. Р—Р°РїРѕР»РЅСЏРµС‚СЃСЏ, РєРѕРіРґР° СЃРµСЂРІРµСЂ СѓРІС‘Р» Р·Р°РїСЂРѕСЃ СЂРµРґРёСЂРµРєС‚РѕРј:
    /// `self.source` С…СЂР°РЅРёС‚ Р—РђРџР РћРЁР•РќРќР«Р™ Р°РґСЂРµСЃ, Рё РїРѕРґСЂРµСЃСѓСЂСЃС‹ С‡Р°СЃС‚РёС‡РЅРѕРіРѕ DOM
    /// (РєР°СЂС‚РёРЅРєРё, `@font-face`) СѓС…РѕРґРёР»Рё Р±С‹ РѕС‚ РЅРµРіРѕ. РџР°СЂР° СЃ generation РІРјРµСЃС‚Рѕ
    /// СЃР±СЂРѕСЃР° РЅР° РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё вЂ” СѓСЃС‚Р°СЂРµРІС€Р°СЏ Р±Р°Р·Р° РїСЂРѕСЃС‚Рѕ РїРµСЂРµСЃС‚Р°С‘С‚
    /// РїРѕРґС…РѕРґРёС‚СЊ (СЃРј. [`Self::document_resource_base`]).
    document_base: Option<(ResourceBase, u64)>,
    /// ADR-016 M2.2: РґРѕР»РіРѕР¶РёРІСѓС‰РёР№ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. `Some` С‚РѕР»СЊРєРѕ РїСЂРё
    /// `LUMEN_ENGINE_THREAD=1`; РёРЅР°С‡Рµ `None` Рё РїРѕРІРµРґРµРЅРёРµ shell РЅРµРёР·РјРµРЅРЅРѕ (РІРµСЃСЊ
    /// relayout СЃРёРЅС…СЂРѕРЅРЅС‹Р№). Р§РµСЂРµР· РЅРµРіРѕ РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓРµС‚СЃСЏ off-thread layout
    /// async-С‚СЂРёРіРіРµСЂРѕРІ (РїРѕРєР° вЂ” debounce-Р·СѓРј): `submit_relayout_job` С€Р»С‘С‚ Р·Р°РґР°РЅРёРµ,
    /// `poll_engine_commit` Р·Р°Р±РёСЂР°РµС‚ РіРѕС‚РѕРІС‹Р№ [`EngineCommit`]. Р”СЂРѕРї РїСЂРё Р·Р°РІРµСЂС€РµРЅРёРё
    /// С€Р»С‘С‚ `Shutdown` Рё РґР¶РѕР№РЅРёС‚.
    ///
    /// ADR-016 M2.2c-2b: РїРѕС‚РѕРє С‚Р°РєР¶Рµ РІР»Р°РґРµРµС‚ РїРµСЂСЃРёСЃС‚РµРЅС‚РЅС‹Рј СЃРѕСЃС‚РѕСЏРЅРёРµРј
    /// [`EngineJsState`] (`Document` + С…СЌРЅРґР» `js_ctx`) вЂ” СЃРёРґРµРЅСЊРµ РґР»СЏ РїРµСЂРµРЅРѕСЃР° JS РЅР°
    /// РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. Р—Р°РїРѕР»РЅСЏРµС‚СЃСЏ С‡РµСЂРµР· `sync_engine_js_state` РїСЂРё СЃРјРµРЅРµ СЃС‚СЂР°РЅРёС†С‹.
    engine_thread: Option<engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    /// ADR-016 M2.2: generation РїРѕСЃР»РµРґРЅРµРіРѕ **РїСЂРёРјРµРЅС‘РЅРЅРѕРіРѕ** relayout-СЂРµР·СѓР»СЊС‚Р°С‚Р°.
    /// Off-thread Р·Р°РґР°РЅРёРµ СЃС‡РёС‚Р°РµС‚СЃСЏ В«РІ РїРѕР»С‘С‚РµВ» (РЅСѓР¶РµРЅ poll-Р±СѓРґРёР»СЊРЅРёРє), РїРѕРєР°
    /// `engine_job_generation != engine_applied_generation`. РЎРёРЅС…СЂРѕРЅРЅС‹Р№
    /// `relayout()` РІС‹СЃС‚Р°РІР»СЏРµС‚ РёС… СЂР°РІРЅС‹РјРё (off-thread Р·Р°РґР°РЅРёРµ РЅРµ Р¶РґС‘С‚СЃСЏ);
    /// `poll_engine_commit` РїСЂРѕРґРІРёРіР°РµС‚ СЌС‚Рѕ РїРѕР»Рµ РїСЂРёРјРµРЅС‘РЅРЅС‹Рј `commit.generation`.
    engine_applied_generation: u64,
    /// ADR-016 M2.2: РјРѕРЅРѕС‚РѕРЅРЅС‹Р№ РЅРѕРјРµСЂ async-relayout Р·Р°РґР°РЅРёСЏ. Р Р°СЃС‚С‘С‚ РїСЂРё РєР°Р¶РґРѕР№
    /// РїРѕСЃС‚Р°РЅРѕРІРєРµ off-thread Р·Р°РґР°РЅРёСЏ (`submit_relayout_job`) **Рё** РїСЂРё РєР°Р¶РґРѕРј
    /// СЃРёРЅС…СЂРѕРЅРЅРѕРј `relayout()` вЂ” С‚Р°Рє СЂРµР·СѓР»СЊС‚Р°С‚ СѓР¶Рµ РїРѕСЃС‚Р°РІР»РµРЅРЅРѕРіРѕ, РЅРѕ РµС‰С‘ РЅРµ
    /// РїСЂРёРјРµРЅС‘РЅРЅРѕРіРѕ off-thread Р·Р°РґР°РЅРёСЏ РѕРїРѕР·РЅР°С‘С‚СЃСЏ РєР°Рє СѓСЃС‚Р°СЂРµРІС€РёР№
    /// (`commit.generation != engine_job_generation`) Рё СЂРѕРЅСЏРµС‚СЃСЏ РІ
    /// `poll_engine_commit`. Latest-wins/generation-guard РЅР° СЃС‚РѕСЂРѕРЅРµ РїРѕС‚РѕРєР° вЂ”
    /// [`engine_thread`].
    engine_job_generation: u64,
    /// РўРµРєСѓС‰РёР№ IME preedit-С‚РµРєСЃС‚. `Some` вЂ” composition-СЃРµСЃСЃРёСЏ Р°РєС‚РёРІРЅР°,
    /// `None` вЂ” РЅРµС‚ Р°РєС‚РёРІРЅРѕРіРѕ IME РІРІРѕРґР°.
    ime_composing: Option<String>,
    /// In-memory bfcache вЂ” HTML snapshots keyed by URL for instant back/forward
    /// restoration without a network round-trip (HTML Living Standard В§8.6).
    bfcache: BfCache,
    /// Parsed stylesheets of frozen bfcache pages, keyed by URL.
    /// Kept shell-side because `Stylesheet` is not serializable.
    /// Pruned lazily against `bfcache.has_frozen`.
    frozen_styles: HashMap<String, lumen_css_parser::Stylesheet>,
    /// Pages kept alive (JS runtime included) for back/forward restoration,
    /// keyed by URL вЂ” see [`ParkedPage`]. Capped at [`PARKED_PAGES_MAX`];
    /// a `Vec` rather than a map because eviction is oldest-first.
    parked_pages: Vec<(String, ParkedPage)>,
    /// Navigation history stack вЂ” pages the user navigated away from.
    /// Top = most recent previous page.
    nav_back: Vec<NavEntry>,
    /// Forward history stack вЂ” pages the user went back from.
    /// Top = most recently visited "forward" page.
    nav_fwd: Vec<NavEntry>,
    /// Monotonic counter for Navigation API entry keys.
    /// Incremented on each new entry so `key` is unique across the session.
    nav_key_counter: u64,
    /// Key of the page currently displayed. Assigned when the page becomes
    /// current; preserved across back/forward navigation so `commit_nav_state`
    /// emits a stable key for the current entry (BUG-256 uniqueness invariant).
    current_nav_key: String,
    /// Pending intercepted navigation awaiting handler completion.
    pending_intercepted: Option<PendingIntercepted>,
    /// Runtime form control state (value, checked) keyed by NodeId.
    /// Persists for the lifetime of the current page; cleared on load/reload.
    form_state: forms::FormState,
    /// Active validation tooltip: (anchor_rect_in_doc_space, message).
    /// Displayed as a viewport-locked overlay. Dismissed on next click.
    validation_tooltip: Option<(Rect, String)>,
    /// NodeId of the `<input type="color">` whose picker is currently open.
    /// The picker overlay is viewport-locked; clicking a swatch closes it.
    color_picker_node: Option<NodeId>,
    /// NodeId of the `<input type="date/datetime-local/time/month/week">` whose
    /// calendar picker overlay is open. `None` when no picker is visible.
    date_picker_node: Option<NodeId>,
    /// Calendar year currently displayed in the open date picker (1-based).
    date_picker_year: i32,
    /// Calendar month currently displayed in the open date picker (1-based, 1=January).
    date_picker_month: u8,
    /// NodeId of the `<select>` whose dropdown is currently open.
    /// The dropdown overlay is viewport-locked; clicking an option closes it.
    select_dropdown_node: Option<NodeId>,
    /// Persistent `localStorage` partitions keyed by origin (scheme+host+port).
    /// Each entry survives page reloads within the same session.
    /// Partitioned by origin to enforce Same-Origin Policy for storage access.
    ls_storage: HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    /// `sessionStorage` partitions of the *active tab*, keyed by the same origin
    /// string as [`Self::ls_storage`] (BUG-836).
    ///
    /// HTML LS В§12.2 binds session storage to the browsing context: it must
    /// survive every navigation of this tab and never reach another one, so the
    /// map travels in the tab snapshot and is emptied for a newly opened tab вЂ”
    /// unlike `ls_storage`, nothing here is ever persisted.
    ss_storage: HashMap<String, Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    /// Directory for per-origin IndexedDB SQLite files (`{sha256(eTLD+1)[:16]}.db`).
    /// `None` в†’ ephemeral in-memory store per page (headless / tests).
    /// `Some(dir)` в†’ each origin gets its own SQLite file in `dir`; data persists
    /// across page reloads and is shared across tabs of the same origin.
    idb_dir: Option<std::path::PathBuf>,
    /// Shared backend for Service Worker registration persistence. A per-origin
    /// `SwStore` is built over this for each page load so SW registrations survive
    /// page navigations within the session (same pattern as `idb_backend`).
    sw_backend: Arc<std::sync::Mutex<dyn lumen_core::ext::StorageBackend>>,
    /// Live SW execution thread registry (PH3-20: SW fetch interception).
    ///
    /// Shared between `QuickJsRuntime` (populates via `_lumen_sw_activate_script`)
    /// and `ServiceWorkerInterceptor` (reads when routing network fetch requests).
    sw_worker_store: lumen_core::ext::SwWorkerStore,
    /// Session-scoped Cache API store (PH3-20). Shared between the page's `caches`
    /// API and activating SW execution threads: the SW serves cache-first
    /// responses from entries the page previously cached into this store. Also the
    /// fallback cache consulted by `ServiceWorkerInterceptor`. In-memory SQLite.
    cache_store: Arc<lumen_storage::CacheStorage>,
    /// Session-scoped cookie jar. Shared across all `HttpClient` instances so
    /// `Set-Cookie` headers received on one hop (including 3xx redirects) are
    /// sent back on subsequent requests to the same domain. In-memory in Phase 0;
    /// wired to a per-profile SQLite file in Phase 2. Used for every profile
    /// except the ephemeral Anonymous one вЂ” see [`Self::anonymous_cookie_jar`]
    /// and [`Self::active_cookie_jar`] (DS-16).
    cookie_jar: Arc<lumen_storage::CookieJar>,
    /// Anonymous profile's own cookie jar (DS-16, В§9.3 ADR-020) вЂ” kept out of
    /// [`Self::cookie_jar`] so cookies set while browsing as Anonymous never
    /// leak into Personal/Work/Guest and vice versa. Reset to a fresh
    /// in-memory instance every time Anonymous becomes the active profile
    /// (`ProfileMenuHit::SwitchTo`), so it never carries state from a
    /// previous Anonymous session either вЂ” true ephemerality within the
    /// running process, not just isolation.
    anonymous_cookie_jar: Arc<lumen_storage::CookieJar>,
    /// Live JS context for the current page вЂ” keeps event listeners active after
    /// initial script execution. `None` when `v8` feature is disabled or
    /// no scripts were registered. Must be dropped before `layout_source` on
    /// navigation to release Arc clones held in JS closures.
    ///
    /// ADR-016 M2.2c-2d (21): `Arc` (РЅРµ `Box`), РїРѕС‚РѕРјСѓ С‡С‚Рѕ С…СЌРЅРґР»РѕРј С‚РµРїРµСЂСЊ РІР»Р°РґРµРµС‚
    /// **Р»РёР±Рѕ** UI-СЃС‚РѕСЂРѕРЅР°, **Р»РёР±Рѕ** РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. РџРѕРґ С„Р»Р°РіРѕРј
    /// (`LUMEN_ENGINE_THREAD=1`) `Arc` Р¶РёРІС‘С‚ РІ [`EngineJsState::js`], Р° СЌС‚Рѕ РїРѕР»Рµ вЂ”
    /// `None`; Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) `Arc` Р·РґРµСЃСЊ, РєР°Рє РїСЂРµР¶РґРµ. Р’Р»Р°РґРµРЅРёРµ Р·Р°РґР°С‘С‚
    /// [`Self::set_js_ctx`], СЃРЅРёРјР°РµС‚ вЂ” [`Self::take_js_ctx`]. В«Р•СЃС‚СЊ Р»Рё JS?В» С‡РёС‚Р°Р№С‚Рµ
    /// РёР· [`Self::js_present`], Р° РЅРµ РёР· `self.js_ctx.is_some()`.
    js_ctx: Option<Arc<dyn PersistentJs>>,
    /// ADR-016 M2.2c-2d: UI-СЃС‚РѕСЂРѕРЅРЅРёР№ С„Р»Р°Рі В«Р°РєС‚РёРІРЅР°СЏ РІРєР»Р°РґРєР° РёРјРµРµС‚ JS-СЂР°РЅС‚Р°Р№РјВ»,
    /// СЃРѕРїСЂРѕРІРѕР¶РґР°СЋС‰РёР№ РєР°Р¶РґРѕРµ РїСЂРёСЃРІР°РёРІР°РЅРёРµ С…СЌРЅРґР»Р° (С‡РµСЂРµР· [`Self::set_js_ctx`] Рё
    /// snapshot save/restore). РћС‚РґРµР»СЏРµС‚ СЂРµС€РµРЅРёРµ В«РµСЃС‚СЊ Р»Рё JS?В» РѕС‚ С‚РѕРіРѕ, РєР°РєР°СЏ
    /// СЃС‚РѕСЂРѕРЅР° РґРµСЂР¶РёС‚ `Arc`: РіРµР№С‚С‹ (`if self.js_present`) С‡РёС‚Р°СЋС‚ РµРіРѕ РІРјРµСЃС‚Рѕ
    /// `self.js_ctx.is_some()`, РїРѕСЌС‚РѕРјСѓ РѕСЃС‚Р°СЋС‚СЃСЏ РІРµСЂРЅС‹ Рё РєРѕРіРґР° РїРѕРґ С„Р»Р°РіРѕРј СЃР°Рј `Arc`
    /// СѓРµС…Р°Р» РЅР° РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє (`state.js`), РѕСЃС‚Р°РІРёРІ `self.js_ctx == None`.
    js_present: bool,
    /// ADR-016 M2.3: UI-side lock-free clone of the JS runtime's rAF-pending
    /// flag (`Some` only when the active tab has a `v8` handle **and** the
    /// engine thread is enabled вЂ” the only mode that needs it). Read directly on
    /// the UI thread to schedule rAF turns without a blocking engine `query`
    /// that would serialize the winit thread behind an in-flight JS turn.
    /// Kept in lockstep with the handle by [`Self::set_js_ctx`]; `None` off the
    /// flag, so the byte-identical single-thread path never consults it.
    raf_pending_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// ADR-016 M2.3: UI-side lock-free clone of the JS runtime's DOM-dirty flag
    /// (companion to [`Self::raf_pending_flag`]). Consumed on the UI thread to
    /// trigger an asynchronous relayout after an off-thread rAF turn mutated the
    /// DOM, instead of a synchronous read blocked behind that turn.
    dom_dirty_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// ADR-016 M2.3: `true` while a `run_animation_frame` batch dispatched to the
    /// engine thread is still executing. Set by the UI thread before firing the
    /// (fire-and-forget) rAF `task`, cleared by that task on completion. Guards
    /// against piling a fresh 200 ms rAF turn onto the engine FIFO every 16 ms
    /// scroll frame: while set, the scroll/redraw path presents the retained
    /// display list and skips the JS pump, keeping the UI thread responsive.
    /// Only ever set under `LUMEN_ENGINE_THREAD=1`; stays `false` off the flag.
    raf_task_inflight: Arc<std::sync::atomic::AtomicBool>,
    /// ADR-016 M2.3: reserve one `about_to_wait` pass for draining the deferred
    /// value-returning JS queues after each rAF turn completes, before firing the
    /// next one. Set when a turn is fired, consumed on the first non-inflight pass
    /// afterwards (which then runs the [`Self::drain_query_js`] drains with the
    /// engine free). Without it a continuous rAF loop would re-fire every pass and
    /// permanently starve notifications/popups/console. UI-thread only, flag-on
    /// only (stays `false` off the flag).
    raf_drain_gate: bool,
    /// When true the vertical scrollbar overlay is suppressed entirely.
    /// Set by `--no-scrollbar` CLI flag; used by graphic test pipeline to
    /// avoid scrollbar pixels contaminating the diff against Edge headless.
    no_scrollbar: bool,
    /// When true the window is created maximized (`--maximized` CLI flag;
    /// live perf audit runs full-screen so the user can watch rendering).
    maximized: bool,
    /// Guards for PerformancePaintTiming entries (W3C Paint Timing В§2).
    /// `true` once the entry has been delivered to JS so we don't double-fire.
    first_paint_delivered: bool,
    /// `true` once `first-contentful-paint` has been delivered to JS.
    first_contentful_paint_delivered: bool,
    /// `true` when the current navigation finished in a network/HTTP error
    /// (`LoadError` / final-render `Err`) rather than a loaded document. A
    /// settled error IS "done loading": `check_wait_condition` treats it as
    /// `DocumentReady` so a `wait{document_ready}` (MCP/BiDi) resolves at once
    /// instead of hanging until its deadline when there is no JS context and no
    /// prior `layout_box` to fall back on (BUG-308). Reset to `false` at the
    /// start of every navigation; per-tab (saved/restored via `PageSnapshot`).
    load_failed: bool,
    /// Human-readable reason for `load_failed` (BUG-438) вЂ” the `LoadError`
    /// message or the final-render `Err`'s `Display`. Surfaced to
    /// `AutomationCommand::Wait{DocumentReady|NetworkIdle}` callers (BiDi
    /// `browsingContext.navigate`, MCP `wait`) as an `AutomationReply::Error`
    /// instead of the settled-error `Ack` BUG-308 used to send вЂ” a failed
    /// load must not be reported as a successful navigation. `None` whenever
    /// `load_failed` is `false`; reset together with it.
    load_error_message: Option<String>,
    /// Instant at which the current navigation began (set in `reload()`).
    /// Used to compute `duration` for the W3C Navigation Timing entry.
    nav_start: Option<std::time::Instant>,
    /// FTS5-РёРЅРґРµРєСЃ РїРѕ С‚РµРєСЃС‚Сѓ РїРѕСЃРµС‰С‘РЅРЅС‹С… СЃС‚СЂР°РЅРёС† вЂ” РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ omnibox (@history).
    /// In-memory РІ Phase 0; РІ Phase 2 РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РёР· РїСЂРѕС„РёР»СЊРЅРѕР№ Р‘Р”.
    history_fts: HistoryFts,
    /// РҐСЂР°РЅРёР»РёС‰Рµ РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёС… Р·Р°РјРµС‚РѕРє (В§12.2) вЂ” omnibox `@notes <query>`.
    /// In-memory РІ Phase 0; РІ Phase 2 РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РёР· РїСЂРѕС„РёР»СЊРЅРѕР№ Р‘Р”.
    notes_store: lumen_knowledge::Notes,
    /// РСЃС‚РѕСЂРёСЏ РїРѕРёСЃРєРѕРІС‹С… Р·Р°РїСЂРѕСЃРѕРІ РґР»СЏ prefix-match autocomplete РІ omnibox.
    /// In-memory РІ Phase 0; РІ Phase 2 РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РёР· РїСЂРѕС„РёР»СЊРЅРѕР№ Р‘Р”.
    search_history: SearchHistory,
    /// РЎС‡С‘С‚С‡РёРє РґР»СЏ РіРµРЅРµСЂРёСЂРѕРІР°РЅРёСЏ rowid РїСЂРё РёРЅРґРµРєСЃРёСЂРѕРІР°РЅРёРё РІ history_fts.
    /// РРЅРєСЂРµРјРµРЅС‚РёСЂСѓРµС‚СЃСЏ РїСЂРё РєР°Р¶РґРѕР№ РЅР°РІРёРіР°С†РёРё РЅР° РЅРѕРІСѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    next_history_id: i64,
    /// KnuthвЂ“Liang hyphenation provider вЂ” СЂРµР°Р»РёР·СѓРµС‚ CSS `hyphens: auto`.
    /// Lazy-loads per-locale dictionaries on first use; cached for subsequent layouts.
    /// `Arc`, С‡С‚РѕР±С‹ С„РёРЅР°Р»СЊРЅС‹Р№ pipeline (BUG-171 СЌС‚Р°Рї 2) РјРѕРі СЂР°Р·РґРµР»РёС‚СЊ РїСЂРѕРІР°Р№РґРµСЂ СЃ
    /// С„РѕРЅРѕРІС‹Рј СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєРѕРј Р±РµР· РїРѕС‚РµСЂРё РїСЂРѕРіСЂРµС‚РѕРіРѕ РєСЌС€Р° СЃР»РѕРІР°СЂРµР№.
    hyp_provider: Arc<KnuthLiangHyphenation>,
    /// Multi-frame GIF animations keyed by the same src URL used in `DrawImage`.
    /// Populated at image-load time; cleared on page navigation.
    /// Single-frame GIFs are not stored here вЂ” handled as regular static images.
    animated_gifs: HashMap<String, lumen_image::AnimatedGif>,
    /// Last rendered frame index per animated GIF URL. Avoids redundant GPU texture
    /// re-uploads when `frame_index_at(elapsed_ms)` returns the same frame as the
    /// previous tick. Cleared together with `animated_gifs` on navigation.
    gif_last_frame: HashMap<String, usize>,
    /// Last rendered frame index per GIF-backed `<video>` node (keyed by nid).
    /// Cleared together with the VideoGifStore entries on navigation.
    video_gif_last_frame: HashMap<u32, usize>,
    /// Decoded animated GIF frames for `<video>` nodes (keyed by nid).
    /// Stored separately from `VideoGifStore` (which has no `lumen_image` dep).
    video_gif_frames: HashMap<u32, lumen_image::AnimatedGif>,
    /// BUG-480 СЃСЂРµР· 1: Р¶РёРІС‹Рµ sub-РґРѕРєСѓРјРµРЅС‚С‹ `<iframe>` С‚РµРєСѓС‰РµР№ СЃС‚СЂР°РЅРёС†С‹.
    /// Р”РµСЂР¶Р°С‚ DOM+JS РґРµС‚РµР№; Р·Р°РјРµРЅСЏРµС‚СЃСЏ С†РµР»РёРєРѕРј РІ [`Lumen::apply_loaded_page`].
    /// Р’ PageSnapshot РЅРµ РїРѕРїР°РґР°РµС‚ вЂ” РїРѕСЃР»Рµ bfcache-РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёСЏ С„СЂРµР№РјС‹ Р±РµР·
    /// СЃРєСЂРёРїС‚РѕРІ (РёР·РІРµСЃС‚РЅРѕРµ РѕРіСЂР°РЅРёС‡РµРЅРёРµ СЃСЂРµР·Р° 1, СЃРј. bugs/BUG-480-OPEN.md).
    frames: Vec<FrameHandle>,
    /// Shared GIF-video store вЂ” same Arc used by JS native bindings (PH3-12).
    ///
    /// The shell owns the Arc; JS bindings hold clones captured at context
    /// creation time.  The shell's render tick drains `pending_loads`, decodes
    /// GIFs, and re-registers frames under `"video:{nid}"` image keys.
    video_gif_store: std::sync::Arc<lumen_js::VideoGifStore>,
    /// Shared TextTrack store вЂ” same Arc used by JS native bindings (P3-webvtt).
    ///
    /// Mirrors `page_tracks.tracks_by_video` so `video.textTracks` exposes the
    /// shell's parsed `<track>` cues. Re-synced on load, cleared on navigation.
    text_track_store: std::sync::Arc<lumen_js::TextTrackStore>,
    /// CPU-side decoded image cache (ADR-008 В§10E.4 scroll-discard).
    ///
    /// Stores one `ImageHandle` per image URL so far-away images can be evicted
    /// from RAM on scroll without discarding the GPU texture in the renderer.
    /// Cleared and repopulated on every page load; entries are dropped by
    /// `try_discard_offscreen_images` once an image leaves the
    /// `gate_image_requests` zone (viewport В± 2 screens).
    image_cache: lumen_image::ImageDecodeCache,
    /// Receiver side of the automation command channel (SDC-1b/SDC-2).
    ///
    /// Connected to an external sender for BiDi/MCP/graphic_tests control.
    /// Each request carries its own reply sender (see [`AutomationRequest`]),
    /// so replies reach the specific caller that issued the command instead
    /// of a shared, unread channel. Commands are drained in `about_to_wait`.
    automation_rx: std::sync::mpsc::Receiver<AutomationRequest>,
    /// Sender side of the automation command channel - cloned for external callers.
    #[allow(dead_code)]
    automation_cmd_tx: std::sync::mpsc::Sender<AutomationRequest>,
    /// `AutomationCommand::Wait` requests not yet satisfied (SDC-1b).
    ///
    /// The event loop cannot block on `Wait` (that would freeze rendering and
    /// starve the very state вЂ” network completions, JS ticks вЂ” the condition
    /// depends on), so a wait is queued here and re-checked once per frame in
    /// `about_to_wait` until it is satisfied or its deadline passes.
    pending_waits: Vec<PendingWait>,
    /// Receiver side of the input injection channel (ADR-007 В§8C).
    ///
    /// Drained each `about_to_wait`; commands are processed through the same
    /// hit-test / JS-dispatch path as real OS events.
    input_rx: input::InputReceiver,
    /// Sender side of the input injection channel вЂ” cloned for external callers.
    #[allow(dead_code)]
    input_tx: input::InputSender,
    /// The DOM node that received the last click (used as target for TypeText injection).
    ///
    /// `None` until the first click is processed.  Updated by `handle_click_at`.
    focused_node: Option<lumen_dom::NodeId>,
    /// Download manager: background download threads, progress channel, and
    /// panel visibility state. Panel toggled via Ctrl+Shift+J.
    downloads: download::DownloadManager,
    /// Tab strip state: open tabs (title, id) and active index.
    ///
    /// The ACTIVE tab's page state lives directly in the `Lumen` fields.
    /// Background tabs have their page state in `bg_tabs` keyed by `TabEntry::id`.
    tab_strip: tabs::strip::TabStrip,
    /// Per-`(origin, ContainerKind)` cookie/storage store ids (7D.2).
    ///
    /// Allocated lazily on first access; the actual cookie jar / storage
    /// dispatch picks up the store id as a partitioning key. Stored on the
    /// shell so isolation survives tab open/close/restore.
    container_store: tabs::containers::ContainerStore,
    /// Frozen page state for each background tab, keyed by `TabEntry::id`.
    ///
    /// `None` entry means the tab was opened but never loaded (blank new tab).
    bg_tabs: HashMap<usize, PageSnapshot>,
    /// Lightweight identity for hibernated (T3) tabs вЂ” keyed by `TabEntry::id`.
    ///
    /// When a background tab is promoted to Hibernated its full `PageSnapshot`
    /// is evicted from `bg_tabs` and stored in `tab_snapshots`; only this
    /// cheap struct remains in RAM.
    hibernated_tabs: HashMap<usize, tab_lifecycle::TabMetadata>,
    /// SQLite-backed blob store for T3 DOM snapshots (ADR-008 В§10J).
    tab_snapshots: lumen_storage::TabSnapshotStore,
    /// SQLite-backed checkpoint store for T2 (BackgroundOld) tabs (ADR-008 В§10I).
    ///
    /// Written on every T1в†’T2 transition so scroll + form state survive a crash.
    /// Restored on T2в†’T0 when `bg_tabs` is empty (crash-recovery path).
    t2_store: lumen_storage::SleepingTabStore,
    /// Monotonic timestamp (ms since epoch) when a T2 SQLite restore started.
    ///
    /// `None` when no restore is in progress.  The `sleep_hint` overlay is shown
    /// once this exceeds 100 ms.
    t2_restore_start_ms: Option<f64>,
    /// SQLite-backed store for the last session вЂ” all open tabs at window close
    /// (В§10I). Overwritten wholesale on `CloseRequested`, read back on launch to
    /// reopen the previous set of tabs. On-disk at `session_persist::SESSION_DB_PATH`.
    session_store: lumen_storage::SessionStore,
    /// Lifecycle tier manager вЂ” tracks T0в†’T4 transitions and LRU ordering.
    ///
    /// Synced with `tab_strip` on open/switch/close; `tick_idle` is polled
    /// from `about_to_wait` once per second to drive automatic hibernation.
    lifecycle_mgr: tab_lifecycle::TabLifecycleManager,
    /// Monotonic instant of the last `tick_lifecycle` call вЂ” used to throttle
    /// polling to approximately once per second.
    lifecycle_last_tick: std::time::Instant,
    /// Active split-view state. `None` = single-pane mode (normal).
    ///
    /// When `Some`, the window is divided into two side-by-side panes:
    /// left = active tab (live `Lumen` state), right = `SplitView::right`
    /// (frozen snapshot of another tab). `Ctrl+\` toggles; `Ctrl+M` switches focus.
    split_view: Option<panels::split_view::SplitView>,
    /// Vim keybinding mode state.  `None` = vim mode is off (default).
    ///
    /// Activated via `Ctrl+Alt+V`; deactivated via `Ctrl+Alt+V` again.
    /// When `Some`, [`VimMode::feed`] intercepts navigation keys before the
    /// global keybinding table.  [`VimState::Insert`] passes keys through.
    vim_mode: Option<input::vim::VimMode>,
    /// Vertical tab panel state. Toggled via Ctrl+B.
    ///
    /// When visible, the left `PANEL_WIDTH` CSS px of the window are occupied by
    /// the tab list and the page viewport shifts right accordingly.
    vertical_tabs: panels::vertical_tabs::VerticalTabsPanel,
    /// Tree-style tab panel state (7A.2): collapse/expand subtrees.
    ///
    /// Stores which subtrees are collapsed. Rendering delegate: see
    /// `panels::tree_tabs::build_panel`. Currently initialised alongside
    /// `vertical_tabs`; future toggle key will switch between flat/tree views.
    tree_tabs: panels::tree_tabs::TreeTabsPanel,
    /// Workspace switcher panel state (7A.3).
    ///
    /// Bottom-docked 32px bar showing named workspaces as coloured chips.
    /// `Ctrl+Shift+W` toggles.  When visible, `viewport_height_css()` subtracts
    /// `SWITCHER_HEIGHT` so the page layout does not overlap the bar.
    workspace_panel: panels::workspace_panel::WorkspacePanel,
    /// Persistent workspace storage вЂ” SQLite in-memory during testing; wired to
    /// a disk path in production via `Workspaces::open(path)`.
    workspaces: lumen_storage::Workspaces,
    /// Profile switcher dropdown state (DS-14), anchored below the toolbar
    /// avatar button (`toolbar::avatar_x()`).
    profile_menu: panels::profile_menu::ProfileMenuPanel,
    /// Persistent profile registry (В§9.3, DS-14): profile metadata + which
    /// one is active. Opened from the portable data dir
    /// (`<exe_dir>/data/profiles.db`); first run seeds 4 default profiles
    /// (Р›РёС‡РЅС‹Р№/Р Р°Р±РѕС‡РёР№/РђРЅРѕРЅРёРјРЅС‹Р№/Р“РѕСЃС‚СЊ вЂ” `panels::profile_menu::DEFAULT_PROFILES`).
    /// DS-14 scope: only the active pointer and visual signature (avatar,
    /// chrome accent) are wired вЂ” per-profile data isolation is DS-16.
    profiles: lumen_storage::ProfileRegistry,
    /// Shields floating panel state (7C.4).
    ///
    /// Shows blocked-request counts per domain, and lets the user toggle
    /// request filtering on/off for the current site.  `Ctrl+Shift+S` toggles
    /// visibility.  Backed by a shared [`BlockedLog`] updated from the network
    /// thread via [`ShieldCountSink`].
    shields: panels::shields_panel::ShieldsPanel,
    /// Per-site permission popover state (7C.2).
    ///
    /// Shows camera/mic/notifications/clipboard grant state for the current
    /// page origin.  Each row has a toggle button cycling Ask в†’ Allow в†’ Deny.
    /// `Ctrl+Shift+P` toggles visibility.  State is in-memory only (no
    /// persistence across sessions).
    permission: panels::permission_panel::PermissionPanel,
    /// Right-docked sidebar web panel state (7D.3).
    ///
    /// Shows a secondary web viewport in a 300 CSS px slot at the right edge.
    /// `Lumen::open_sidebar_page` supplies the page display list.
    /// When visible, `page_content_width_css()` subtracts
    /// [`panels::sidebar_panel::PANEL_WIDTH`] and `relayout()` fires.
    sidebar: panels::sidebar_panel::SidebarPanel,
    /// Re-layoutable source of the web sidebar page (parsed DOM + stylesheet).
    ///
    /// Kept so a drag-resize of the sidebar can reflow its content to the new
    /// width instead of stretching a frozen display list. `None` until a page
    /// is opened via [`Self::open_sidebar_page`].
    sidebar_source: Option<LayoutSource>,
    /// AI assistant sidebar panel (В§12.8, GG-1).
    ///
    /// Right-docked 200 CSS px panel with a prompt input field and response area.
    /// `Ctrl+Shift+A` toggles visibility. When visible, `page_content_width_css()`
    /// subtracts [`panels::ai_panel::PANEL_WIDTH`] and `relayout()` fires.
    /// Queries are dispatched to [`Self::ai_backend`] synchronously (Phase 0).
    ai_panel: panels::ai_panel::AiPanel,
    /// Persisted, drag-resizable widths of the docked sidebars (F2-6).
    ///
    /// Replaces the panels' compiled `PANEL_WIDTH` constants: `width_for(id,
    /// default)` supplies the active width, dragging a panel's inner edge calls
    /// `set_width` + `relayout` + `save`. Loaded at startup, so the layout
    /// survives a restart.
    panel_layout: panel_layout::PanelLayout,
    /// In-flight docked-panel resize drag: `(dock side, panel id)` of the edge
    /// currently being dragged, or `None` when no resize is active.
    panel_resize: Option<(panel_layout::Dock, &'static str)>,
    /// Floating overlay showing a single user annotation (В§12.2, GG-2).
    ///
    /// Opened when the user selects a `@notes`-search result from the omnibox
    /// dropdown and presses Enter. The committed value (`note-viewer:<id>`)
    /// is intercepted in `handle_omnibox_commit`. `Escape` closes the overlay.
    note_viewer: panels::note_viewer::NoteViewerPanel,
    /// AI inference backend for the AI sidebar (В§12.8).
    ///
    /// Defaults to [`lumen_core::NullAiBackend`] (returns a stub message).
    /// Replace with a real implementation to enable AI functionality.
    ai_backend: Box<dyn lumen_core::AiBackend>,
    /// SQLite-backed bookmark store (in-memory for the session).
    ///
    /// Backs the bookmark manager panel. `@read-later <url>` omnibox commands and
    /// `Ctrl+D` (bookmark current page) write here; the panel reads via
    /// `Bookmarks::list_all` on every refresh.
    bookmarks: lumen_storage::Bookmarks,
    /// Bookmark manager panel state (task #22).
    ///
    /// Floating overlay anchored under the toolbar. `Ctrl+Shift+O` toggles
    /// visibility. Folder tree + bookmark list + search + drag-and-drop re-file
    /// (move bookmark to folder, persisted via `Bookmarks::set_folder`).
    bookmark_panel: panels::bookmark_panel::BookmarkPanel,
    /// SQLite-backed tab-group metadata store (CC-6, in-memory for the session).
    ///
    /// Persists group label/colour/collapsed state created via the tab context
    /// menu ("Р’ РЅРѕРІСѓСЋ РіСЂСѓРїРїСѓ"). Membership is session state on `TabStrip`.
    tab_groups: lumen_storage::TabGroups,
    /// SQLite-backed browsing history store (in-memory for the session, task D-5).
    ///
    /// Records each page visit. The history panel reads via `History::recent`
    /// (50 entries, grouped by date). `History::delete` / `History::clear` are
    /// called from the panel's delete and clear-all buttons.
    history_store: History,
    /// Browser history panel state (task D-5).
    ///
    /// Centred floating overlay. `Ctrl+H` toggles visibility. Shows recent pages
    /// grouped by date with search (via `HistoryFts`), delete per-entry, and a
    /// "РћС‡РёСЃС‚РёС‚СЊ РІСЃС‘" button.
    history_panel: panels::history_panel::HistoryPanel,
    /// Command palette modal state (task #23, В§7E.2).
    ///
    /// `Ctrl+K` toggles a centred modal that fuzzy-searches across commands,
    /// bookmarks and history. While visible it captures all keyboard and pointer
    /// input; `в†‘/в†“` move the selection, `Enter` activates, `Esc` closes.
    command_palette: panels::command_palette::CommandPalette,
    /// Focus mode + Pomodoro timer panel (task #25, V4).
    ///
    /// `Ctrl+Shift+F` enters a distraction-free focus mode: the tab bar is
    /// hidden and a compact Pomodoro countdown widget with an arc progress ring
    /// floats in the top-right corner. `Esc` exits focus mode (instead of
    /// quitting). The embedded `PomodoroTimer` is ticked from `about_to_wait`.
    focus: panels::focus_panel::FocusModePanel,
    /// Picture-in-picture floating video window (task #21).
    ///
    /// `Ctrl+Shift+V` opens a compact 320Г—180 card that keeps a tab's `<video>`
    /// element visible (poster placeholder) while the page scrolls or the user
    /// switches tabs. Implemented as an in-window overlay (the ad-hoc panel
    /// convention) вЂ” a true second OS window awaits multi-window support. The
    /// card can be dragged by its title bar.
    pip: panels::pip_window::PipWindow,
    /// CC-7 enter/exit state machine for the real OS-level PiP window, driven by
    /// the JS `_lumen_pip_enter` / `_lumen_pip_exit` requests. Pure data; the
    /// live window + backend it tracks live in [`Self::pip_os`].
    pip_controller: panels::pip_os_window::PipController,
    /// The live always-on-top OS window backing video Picture-in-Picture
    /// (CC-7), with its own render backend, or `None` when no `<video>` is in
    /// OS PiP. Created on `_lumen_pip_enter`; dropped on exit / close button.
    /// Falls back to the in-window [`Self::pip`] overlay when a second GPU
    /// surface cannot be created.
    pip_os: Option<PipOsWindow>,
    /// Document Picture-in-Picture open/closed state machine, driven by the JS
    /// `_lumen_docpip_request_window` / `_lumen_docpip_close` requests. Pure
    /// data; the live window + backend it tracks live in [`Self::doc_pip_os`].
    doc_pip_controller: panels::doc_pip_os_window::DocPipController,
    /// The live always-on-top OS window backing `documentPictureInPicture`
    /// (Document PiP slice 1), with its own render backend, or `None` when no
    /// Document PiP window is open. Created on `_lumen_docpip_request_window`;
    /// dropped on `.close()` / OS close button. Unlike [`Self::pip_os`] there
    /// is no in-window overlay fallback вЂ” window/backend creation failure just
    /// leaves the request unfulfilled (the JS `PictureInPictureWindow` promise
    /// still resolves; `.document` stays a JS-only mock either way, see
    /// `document_pip.rs`).
    doc_pip_os: Option<DocPipOsWindow>,
    /// Right-button drag gesture recognizer (В§7B.3).
    ///
    /// Tracks right-button drags, classifies the trajectory into L/R/U/D/LD/RD,
    /// and maps each direction to a [`GestureAction`] via a configurable
    /// [`GestureMap`].  Default bindings: Left=Back, Right=Forward,
    /// LeftDown=CloseTab, RightDown=NewTab.
    gesture: input::gesture::GestureRecognizer,
    /// SQLite-backed omnibox bang-alias registry (В§7B.4).
    ///
    /// Seeded with `!g` (Google) and `!gh` (GitHub) on startup.  Custom aliases
    /// are addable via `set(trigger, expansion)`.
    omnibox_aliases: lumen_storage::OmniboxAliases,
    /// SQLite-backed pinned `about:newtab` speed-dial tiles (DS-11).
    ///
    /// Portable-data store (`<exe_dir>/data/newtab_tiles.db`); falls back to
    /// in-memory if the path cannot be opened.
    newtab_tiles: lumen_storage::NewtabTiles,
    /// In-session notes created via `@notes <text>` in the omnibox.
    ///
    /// Persisted in-memory for the session; each entry is a raw text string.
    /// Displayed nowhere yet вЂ” UI is a future task.
    notes: Vec<String>,
    /// В§12.3 Read-later storage: persists HTML snapshots of saved pages.
    ///
    /// Populated by the `@read-later <url>` omnibox command: a background thread
    /// fetches the page HTML and calls `save()`. In-memory only (no SQLite path
    /// for the first ship вЂ” drop-in replacement once a `read_later.db` path is
    /// wired through the profile directory).
    read_later_store: lumen_knowledge::ReadLater,
    /// В§12.3 Read-later panel state (Ctrl+Shift+R).
    read_later_panel: panels::read_later_panel::ReadLaterPanel,
    /// Channel receiver for completed background read-later fetches.
    ///
    /// Background threads send `(url, title, html_bytes)` here when done.
    /// Drained in `about_to_wait` to call `read_later_store.save()`.
    read_later_rx: std::sync::mpsc::Receiver<(String, String, Vec<u8>)>,
    /// Sender half of the read-later fetch channel (cloned into each background thread).
    read_later_tx: std::sync::mpsc::Sender<(String, String, Vec<u8>)>,
    /// Cookie-banner auto-dismiss preference (7C.3).
    ///
    /// When `true` (default) the JS shim in `lumen-js` auto-clicks consent-banner
    /// accept buttons on every page load. When `false` banners are shown normally.
    /// Toggle via `Ctrl+Shift+K` or a future settings UI.
    cookie_banner_dismiss: bool,
    /// Idle GC tick: drains dead DOM node IDs every 30 s and purges JS-side
    /// per-node caches (`_lumen_listeners`, `_input_values`) via `_lumen_gc_collect`.
    gc_tick: gc_tick::GcTick,
    /// Throttled OS memory pressure poller (ADR-008 В§10H).
    ///
    /// Polled every 5 s in `about_to_wait`.  On `Medium` or `High` pressure,
    /// [`CacheRegistry::broadcast_pressure`] is called on `cache_registry`, and
    /// owned caches (`image_cache`, renderer `layer_cache`) are evicted directly.
    memory_poll: memory_poll::MemoryPollTick,
    /// Registry of cross-session shared caches (ADR-008 В§10D.3).
    ///
    /// Caches registered here receive `on_memory_pressure` broadcasts from the
    /// poll loop.  Owned per-page caches (`image_cache`, layer cache) are evicted
    /// directly rather than through the registry to avoid shared-ownership overhead.
    cache_registry: lumen_core::ext::CacheRegistry,
    /// Deterministic render mode (8F).
    ///
    /// When `enabled` (`--deterministic` CLI flag): window opens at 1280Г—800
    /// (unless overridden by `viewport_override`, DEVX-1), `Date.now()` is
    /// frozen at 0, `Math.random` uses a seeded PRNG, and
    /// `requestAnimationFrame` callbacks receive a 0 ms timestamp.
    /// `rng_seed`/`monotonic_clock` (DEVX-16, `--rng-seed`/`--monotonic-clock`)
    /// reach the JS runtime via `V8JsRuntime::set_deterministic_mode`.
    /// Intended for snapshot testing and reproducible output.
    deterministic: deterministic::DetConfig,
    /// `--viewport <W>x<H>` override (DEVX-1): pins the window's CSS content
    /// viewport size, taking priority over both the `deterministic` 1280Г—800
    /// default and the plain 1024Г—720 default (see `resumed()`). Lets
    /// automation combine `--deterministic` with `graphic_tests`'s fixed
    /// 1024Г—720 crop-calibration contract.
    viewport_override: Option<(f32, f32)>,
    /// DevTools JS console panel (В§7E.5).
    ///
    /// Captures `console.log/warn/error` output from the active page's JS runtime.
    /// Visible as a bottom overlay; toggled with `F12`.
    devtools_console: devtools::console_panel::ConsolePanel,
    /// DevTools DOM inspector panel (В§7E.1).
    ///
    /// While active, hovering highlights the box under the cursor with a
    /// box-model overlay and clicking pins a node, showing its computed style
    /// in a right-docked side panel. Toggled with `Ctrl+Shift+I`.
    dom_inspector: devtools::inspector::DomInspectorPanel,
    /// DevTools network log panel (В§7E.4).
    ///
    /// Shows a live log of HTTP requests (method / status / timing / URL),
    /// fed by `NetworkLogSink` from the engine's `EventSink`. Bottom overlay,
    /// toggled with `Ctrl+Shift+E`.
    network_panel: devtools::network_panel::NetworkPanel,
    /// Privacy network panel (V5).
    ///
    /// A privacy-focused, right-docked overlay sharing the same `NetworkLog` as
    /// [`network_panel`]: it presents the request stream as a newest-first log of
    /// tracker domains with blocked/allowed status and the matched filter rule,
    /// plus a blocked/allowed summary. Toggled with `Ctrl+Shift+Y`.
    ///
    /// [`network_panel`]: Lumen::network_panel
    privacy: panels::privacy_panel::PrivacyPanel,
    /// Persistent accessibility preferences store (task E-2).
    ///
    /// Backed by SQLite (in-memory for the session). Stores font-size
    /// multiplier, prefers-reduced-motion, forced-colors, and cursor size.
    /// Read on panel open; written when panel closes.
    a11y_store: lumen_storage::A11yPrefs,
    /// Accessibility settings panel overlay (task E-2, `Ctrl+Shift+Q`).
    ///
    /// A centred 300Г—260 px modal. Holds a working draft; on close the draft
    /// is persisted to `a11y_store` and media changes are re-delivered to JS.
    a11y_panel: panels::a11y_panel::A11yPanel,
    /// Platform accessibility bridge (O-5).
    ///
    /// Receives `AXTree` updates after every page load and focus change.
    /// Routes them to the OS accessibility API (UIA / NSAccessibility / AT-SPI2).
    platform_bridge: Box<dyn lumen_a11y::platform::PlatformBridge>,
    /// Print dialog overlay (task E-1, `Ctrl+P`).
    ///
    /// A centred 560Г—400 px modal with paper size, orientation, margins,
    /// page range, colour mode, and output-file fields. Clicking **Print**
    /// calls `do_print_to_pdf()` with the configured settings.
    print_panel: panels::print_panel::PrintPanel,
    /// Persistent browser settings store (task D-7).
    ///
    /// Backed by SQLite at `<exe_dir>/data/settings.db` (survives restarts;
    /// falls back to an in-memory store if the file cannot be opened). Stores
    /// homepage, search engine ID, shields, fingerprint mode, DoH, font size,
    /// theme, download path, tab layout, and panel layout. Read on panel open;
    /// written when panel closes.
    settings_store: lumen_storage::BrowserSettings,
    /// Settings page overlay state (task D-7, `about:settings`).
    ///
    /// `Ctrl+,`, the settings gear button in the tab strip, or navigating to
    /// `about:settings` toggles a centred overlay with seven tabbed sections:
    /// General, Privacy, Appearance, Downloads, Network, Adblock, Language.
    /// Opened/closed via [`Lumen::open_settings_panel`] /
    /// [`Lumen::close_settings_panel`], which also sync the sections backed by
    /// stores other than `settings_store` (HTTP/3 в†’ `fingerprint.toml`,
    /// ad-block subscriptions в†’ `AdblockStore`, spellcheck locale в†’ `SPELL_DICTS`).
    settings_panel: panels::settings_panel::SettingsPanel,
    /// Persistent ad-block filter-list store (`<exe_dir>/data/adblock/adblock.db`).
    ///
    /// Opened once at startup ([`config::init_adblock`]); shared with the
    /// background refresh thread and with the settings panel's Adblock
    /// section (enable/disable a subscription, trigger a manual refresh).
    adblock_store: std::sync::Arc<lumen_storage::adblock::AdblockStore>,
    /// Keyboard shortcuts panel (Ctrl+Shift+/, В§D-4).
    ///
    /// Shows all `KeyCommand` bindings with rebind-on-click support.
    shortcuts_panel: panels::shortcuts_panel::ShortcutsPanel,
    /// Certificate viewer panel (Ctrl+Shift+C, В§D-1).
    ///
    /// Centred 500Г—440 overlay showing X.509 cert data (subject CN/Org, issuer,
    /// validity dates, SHA-256 fingerprint, SAN list, TLS version).
    cert_panel: panels::cert_panel::CertPanel,
    /// Whether the curated system-font fallback chain has been preloaded into
    /// the renderer (CSS Fonts L4 В§5.3 codepoint cascade).
    ///
    /// The renderer can fall back per-glyph across loaded faces, but those
    /// faces must first be loaded via `Renderer::preload_curated_fallbacks`.
    /// Without it, CJK / emoji / RTL / Indic codepoints on pages with no
    /// explicit `font-family` for that script render as `.notdef`. Preloading
    /// is a one-time, idempotent operation (the curated families are system
    /// fonts, identical across pages), so this guard runs it once after the
    /// first page provides a `FontProvider`.
    fallbacks_preloaded: bool,
    /// Virtual URL shown in the address bar after `history.pushState` /
    /// `history.replaceState`.  `None` в†’ use `source.url_str()`.
    /// Reset to `None` on any full navigation.
    display_url: Option<String>,
    /// Serialised JS state JSON for the current history entry, mirrored from JS
    /// so the shell can populate `NavEntry::same_doc_state_json` on pushState.
    /// `"null"` until a `pushState`/`replaceState` call updates it.
    current_history_state_json: String,
    /// Node ID of the currently fullscreen element, or `None` if not fullscreen.
    ///
    /// Set when `requestFullscreen()` is called in JS and cleared when
    /// `document.exitFullscreen()` or `Escape` exits fullscreen.  Used to deliver
    /// `_lumen_notify_fullscreen_exit()` when the OS exits fullscreen externally.
    fullscreen_nid: Option<u32>,
    /// Pending viewport reconciliation after an OS fullscreen toggle (BUG-167).
    ///
    /// `Some((prev_w, prev_h, attempts_left))` is armed right after
    /// `window.set_fullscreen(..)` is called: `prev_w`/`prev_h` are the window's
    /// **physical** inner size *before* the OS applied the new mode. The OS
    /// resizes the window asynchronously, so `about_to_wait` polls each loop
    /// iteration until `inner_size()` differs from `(prev_w, prev_h)`, then runs
    /// the same resize + relayout path as `WindowEvent::Resized` so the page
    /// viewport (`vw`/`vh`, `innerWidth`/`innerHeight`) follows the fullscreen
    /// area. `attempts_left` bounds the poll so a no-op toggle can't spin the
    /// loop; it is cleared once the size changes or the budget runs out.
    fullscreen_resize_pending: Option<(u32, u32, u8)>,
    /// Active CSS View Transition (CSS View Transitions L1 В§4).
    ///
    /// Set when `document.startViewTransition(callback)` fires `_lumen_vt_end`.
    /// The `old_dl` snapshot fades out over the new display list for `duration_ms`.
    /// `None` when no transition is active.
    view_transition: Option<ViewTransitionState>,
    /// Tab auto-archive state (7A.5).
    ///
    /// Background tabs idle for more than `ARCHIVE_AFTER_MS` are moved here from
    /// the visible tab strip.  Only a title + URL string is retained; restoring
    /// opens a fresh navigation to that URL.  The archive button (rightmost 36 px
    /// of the tab bar) shows a count badge and toggles the archive panel.
    archive: tabs::archive::TabArchive,
    /// Timestamp (wall ms) when restore of a hibernated tab began.
    ///
    /// `Some(ms)` = spinner overlay is active; `None` = no restoration in progress.
    /// Set at the start of `restore_hibernated_tab` and cleared when restore completes.
    restore_spinner_start_ms: Option<f64>,
    /// Active element resize: `Some((node_id, start_x, start_y, allow_width, allow_height))`
    /// when user is dragging the resize grip. `None` when no resize is active.
    /// Set on MouseInput Pressed over a resize grip, cleared on MouseInput Released.
    /// `allow_width`/`allow_height` are the grip node's `Resize` CSS value resolved to
    /// physical axes (CC-CSS-4: `Resize::allowed_axes`, writing-mode aware) at press
    /// time вЂ” they gate which of width/height is updated during CursorMoved via the
    /// JS binding, so `resize: vertical` no longer also changes width on a diagonal drag.
    resize_active: Option<(lumen_dom::NodeId, f32, f32, bool, bool)>,
    /// In-progress tab drag-and-drop (В§O-9).
    ///
    /// `Some` from the moment the user presses on a tab until they release.
    /// Transitions to `active = true` after the cursor crosses
    /// [`tabs::strip::DRAG_THRESHOLD`] px.  On release, calls
    /// `tab_strip.move_tab` if the drag was active.
    tab_drag: Option<tabs::strip::TabDragState>,
    /// In-progress HTML5 drag-and-drop gesture (PH3-9 / HTML LS В§9.3.3).
    ///
    /// `Some` from `mousedown` on a draggable element until `mouseup`.
    /// Transitions to `active = true` after the cursor travels в‰Ґ
    /// `DND_THRESHOLD` px, at which point `dragstart` is fired on `src_nid`.
    /// On `mouseup`: fires `drop` on the current target, `dragend` on `src_nid`,
    /// then clears this field.
    dnd_state: Option<DndState>,
    /// Right-click tab context menu (CC-4): Duplicate / Pin / Move to new
    /// window / Close others / Close to the right. Hidden unless `open`.
    tab_context_menu: tabs::context_menu::TabContextMenu,
    /// Page-level spell-check suggestion menu (P3-spell slice 3): opened by
    /// right-clicking a misspelled word in a focused text `<input>`. Hidden
    /// unless open.
    page_context_menu: page_context_menu::PageContextMenu,
    /// Words the user added to the persistent dictionary
    /// (`data/spell/user_words.txt`), lowercase. Treated as correct spellings.
    spell_user_words: std::collections::HashSet<String>,
    /// Words the user chose to ignore for this session ("РџСЂРѕРїСѓСЃС‚РёС‚СЊ"),
    /// lowercase. Cleared on restart.
    spell_ignored: std::collections::HashSet<String>,
    /// Shell UI theme: base brightness + accent colour (В§O-9).
    ///
    /// Initialised from `BrowserSettings` on startup.  Updated when the user
    /// changes the theme or accent in the settings panel (Appearance section).
    /// The accent drives the active-tab indicator colour passed to
    /// `build_tab_bar`.
    shell_theme: panels::themes::ShellTheme,
    /// Original page source stored when Reader View (В§D-3) is active.
    ///
    /// `Some` when the current page is showing the clean reader HTML (F9 toggle);
    /// `None` in normal browsing mode.  Toggling F9 again restores this source.
    reader_original_source: Option<PageSource>,
    /// TLS certificate information for the current tab (В§D-1).
    ///
    /// Populated when a page loads over HTTPS; cleared on tab switch / navigation.
    /// Phase 0: shell can set this to a stub value via `CertInfo::stub_for`.
    cert_info: Option<panels::cert_panel::PanelCertData>,
}

/// State for an in-progress CSS View Transition cross-fade (CSS View Transitions L1).
///
/// Holds the captured old display list and timing parameters.
struct ViewTransitionState {
    /// Display list captured before the JS callback mutated the DOM.
    old_dl: lumen_paint::DisplayList,
    /// Wall-clock epoch offset (ms) when the cross-fade animation started.
    start_ms: f64,
    /// Total cross-fade duration in milliseconds (currently 300 ms).
    duration_ms: f64,
}

/// CSS View Transitions L1 вЂ” event kind emitted by `document.startViewTransition`.
#[derive(Debug)]
#[allow(dead_code)]
enum ViewTransitionEvent {
    /// Callback is about to run вЂ” shell should snapshot the current frame.
    Begin,
    /// Callback finished вЂ” shell should relayout and start the cross-fade animation.
    End,
    /// Transition was cancelled (nested startViewTransition or explicit abort).
    Cancel,
}

/// Pending intercepted navigation awaiting handler completion.
enum PendingIntercepted {
    Push { url: String, handler_started: bool },
    Replace { url: String, handler_started: bool },
    Back { handler_started: bool },
    Forward { handler_started: bool },
}

impl Lumen {
    /// Finds a layout box with a resize grip at position (x, y) in the layout tree.
    /// Returns `(node_id, allow_width, allow_height)` вЂ” the latter two are the box's
    /// `resize` value resolved to physical axes (CC-CSS-4: `Resize::allowed_axes`,
    /// writing-mode aware), so the caller knows which dimension(s) a drag from this
    /// grip is allowed to change. Returns `None` if no grip is found.
    /// This is used in B-7: CSS Resize property Phase 1 to detect mouse clicks on grips.
    fn find_resize_grip_node(
        &self,
        b: &lumen_layout::LayoutBox,
        x: f32,
        y: f32,
    ) -> Option<(lumen_dom::NodeId, bool, bool)> {
        // Check this box first
        if lumen_paint::point_on_resize_grip(b, x, y) {
            let (allow_w, allow_h) = b.style.resize.allowed_axes(b.style.writing_mode);
            return Some((b.node, allow_w, allow_h));
        }

        // Recursively check children
        for child in &b.children {
            if let Some(hit) = self.find_resize_grip_node(child, x, y) {
                return Some(hit);
            }
        }

        None
    }

    /// Open the OS native file-picker for `<input type="file">` at `id`.
    ///
    /// Reads the `accept` and `multiple` attributes from the DOM, invokes the
    /// platform file dialog (blocking), then delivers the result to JS via
    /// `_lumen_deliver_file_list(nid, json)`.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn open_file_picker(&mut self, id: NodeId) {
        let (accept, multiple) = if let Some(src) = self.layout_source.as_ref() {
            let doc = src.document.lock().unwrap();
            let n = doc.get(id);
            let accept = n.get_attr("accept").unwrap_or("").to_string();
            let multiple = n.get_attr("multiple").is_some();
            (accept, multiple)
        } else {
            (String::new(), false)
        };
        let entries = platform::file_dialog::open_file_dialog(&accept, multiple);
        if entries.is_empty() {
            // User cancelled вЂ” no event fired (HTML LS В§4.10.5.1.16.3 step 3).
            return;
        }
        #[cfg(feature = "v8")]
        if self.js_present {
            // Register each path with an opaque token before delivering to JS.
            // JS never receives raw filesystem paths вЂ” only tokens.
            // BUG-371: the grant is bound to an origin, and only the origin the
            // page's own bindings were installed with can redeem it. Read back
            // from the install path rather than re-derived from `self.source` вЂ”
            // a mismatch would not fail loudly, every read would just come back
            // empty.
            let origin = lumen_js::file_input::active_document_origin();
            let tokens: Vec<String> = entries
                .iter()
                .map(|e| lumen_js::file_input::register_file_token(&e.path, &origin))
                .collect();
            let json = platform::file_dialog::entries_to_json_with_tokens(&entries, &tokens);
            // ADR-016 M2.2c-2d: fire-and-forget file-list delivery С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ”
            // С‚РѕРєРµРЅС‹ СЂРµРіРёСЃС‚СЂРёСЂСѓСЋС‚СЃСЏ РЅР° UI-РїРѕС‚РѕРєРµ (РґРѕ РїРѕСЃС‚Р°РЅРѕРІРєРё РІ РѕС‡РµСЂРµРґСЊ), СЃР°Рј `eval_js`
            // РїРѕРґ С„Р»Р°РіРѕРј СѓС…РѕРґРёС‚ off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `js.eval_js`.
            let script = format!("_lumen_deliver_file_list({}, {})", id.index(), json);
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
        }
        #[cfg(not(feature = "v8"))]
        let _ = entries;
    }

    /// Arm a viewport reconciliation after an OS fullscreen toggle (BUG-167).
    ///
    /// `prev` is the window's **physical** inner size captured right *before*
    /// `set_fullscreen` was called. The OS applies the new size asynchronously,
    /// so `poll_fullscreen_resize` (run from `about_to_wait`) waits until the
    /// real `inner_size()` differs from `prev`, then drives the resize +
    /// relayout path. The `240` attempt budget (~4 s at 60 fps) prevents a
    /// no-op toggle from spinning the event loop forever.
    fn arm_fullscreen_resize(&mut self, prev: winit::dpi::PhysicalSize<u32>) {
        self.fullscreen_resize_pending = Some((prev.width, prev.height, 240));
        // Wake the loop so `about_to_wait` polls even with ControlFlow::Wait.
        self.request_redraw();
    }

    /// Poll for the OS-applied fullscreen size and, once it differs from the
    /// pre-toggle size, run the same resize + relayout path as
    /// `WindowEvent::Resized` so the page viewport (`vw`/`vh`,
    /// `innerWidth`/`innerHeight`) follows the fullscreen area (BUG-167).
    ///
    /// No-op unless a toggle is pending. Called once per `about_to_wait`.
    fn poll_fullscreen_resize(&mut self) {
        let Some((prev_w, prev_h, attempts)) = self.fullscreen_resize_pending else {
            return;
        };
        // Read the current physical size; the immutable borrow of `self.window`
        // ends before the &mut calls below.
        let cur = match self.window.as_ref() {
            Some(w) => w.inner_size(),
            None => {
                self.fullscreen_resize_pending = None;
                return;
            }
        };
        match decide_fullscreen_poll((prev_w, prev_h), (cur.width, cur.height), attempts) {
            FullscreenPoll::Apply(w, h) => {
                // OS applied the new size: drive the normal resize + relayout path.
                self.fullscreen_resize_pending = None;
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(w, h);
                }
                self.relayout();
                self.runtime
                    .deliver_observer_records(runtime::ObserverKind::Resize);
                self.request_redraw();
            }
            FullscreenPoll::Wait(w, h, left) => {
                self.fullscreen_resize_pending = Some((w, h, left));
                self.request_redraw();
            }
            FullscreenPoll::Done => self.fullscreen_resize_pending = None,
        }
    }

    /// Transform-first zoom step (ADR-016 M0.3).
    ///
    /// Called after `zoom_factor` changed via Ctrl+/-/0. Instead of an immediate
    /// (expensive) relayout, scale the retained display list by
    /// `zoom_factor / laid_out_zoom_factor` on the backend for an instant
    /// response, then arm a debounced relayout so a burst of key presses reflows
    /// only once вЂ” `ZOOM_RELAYOUT_DEBOUNCE_MS` after the last press.
    fn begin_zoom_preview(&mut self) {
        let scale = zoom::preview_scale(self.zoom_factor, self.laid_out_zoom_factor);
        if let Some(r) = self.renderer.as_mut() {
            r.set_preview_scale(scale);
        }
        self.pending_zoom_relayout = Some(
            std::time::Instant::now()
                + std::time::Duration::from_millis(zoom::ZOOM_RELAYOUT_DEBOUNCE_MS),
        );
        self.request_redraw();
    }

}

impl Lumen {
    /// Return a cloneable [`InputSender`] for injecting synthetic input events.
    ///
    /// Callers on any thread can use the sender to enqueue [`InputCommand`]s;
    /// they are drained and dispatched in `about_to_wait`.
    #[allow(dead_code)]
    pub fn input_sender(&self) -> input::InputSender {
        self.input_tx.clone()
    }

    /// Return a cloneable handle for driving this window's automation channel (SDC-2).
    ///
    /// Callers on any thread can use this to send [`AutomationCommand`]s and
    /// block for their reply; commands are drained and dispatched in `about_to_wait`.
    #[allow(dead_code)]
    pub fn automation_handle(&self) -> AutomationHandle {
        AutomationHandle::new(self.automation_cmd_tx.clone())
    }

    /// Return the current keyboard modifier flags as a bitmask.
    ///
    /// Bit layout: bit0=ctrl, bit1=shift, bit2=alt, bit3=meta (super).
    #[cfg(feature = "v8")]
    fn mod_flags(&self) -> u8 {
        (self.modifiers.control_key() as u8)
            | ((self.modifiers.shift_key()  as u8) << 1)
            | ((self.modifiers.alt_key()    as u8) << 2)
            | ((self.modifiers.super_key()  as u8) << 3)
    }

    /// Dispatch a `MouseEvent` of the given `event_type` to DOM node `nid`.
    ///
    /// `button` = which button (0=left, 1=middle, 2=right).
    /// `buttons` = bitmask of currently-held buttons.
    /// Coordinates are CSS viewport pixels.
    #[cfg(feature = "v8")]
    fn js_mouse_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32, button: u8, buttons: u8) {
        let script = format!(
            "_lumen_dispatch_mouse_event({}, '{}', {}, {}, {}, {}, {})",
            nid, event_type,
            x_css as i32, y_css as i32,
            button, buttons,
            self.mod_flags(),
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Dispatch a `PointerEvent` of the given `event_type` to DOM node `nid`.
    ///
    /// Always uses pointerId=1, pointerType='mouse', isPrimary=true (mouse input).
    /// Non-bubbling types (`pointerenter`/`pointerleave`) have `bubbles:false` per spec.
    #[cfg(feature = "v8")]
    fn js_pointer_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32, button: u8, buttons: u8) {
        let script = format!(
            "_lumen_dispatch_pointer_event({}, '{}', {}, {}, {}, {}, {})",
            nid, event_type,
            x_css as i32, y_css as i32,
            button, buttons,
            self.mod_flags(),
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Dispatch a `pointermove` whose buffered intermediate samples are exposed
    /// via `PointerEvent.getCoalescedEvents()` (Pointer Events L3 В§4.1).
    /// `coalesced` holds CSS-pixel positions strictly older than
    /// `(x_css, y_css)`, oldest first; the dispatched event is appended last,
    /// per spec. Always dispatches with button=0/buttons=0 вЂ” the only caller
    /// is the plain-move flush path, which (like the rest of this file) does
    /// not track held-button state for hover/move events.
    #[cfg(feature = "v8")]
    fn js_pointer_event_coalesced(&self, nid: u32, x_css: f32, y_css: f32, coalesced: &[(f32, f32)]) {
        let mut points_json = String::from("[");
        for (i, (cx, cy)) in coalesced.iter().enumerate() {
            if i > 0 {
                points_json.push(',');
            }
            points_json.push_str(&format!("[{},{}]", *cx as i32, *cy as i32));
        }
        points_json.push(']');
        let script = format!(
            "_lumen_dispatch_pointer_event({}, 'pointermove', {}, {}, 0, 0, {}, {})",
            nid,
            x_css as i32, y_css as i32,
            self.mod_flags(),
            points_json,
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Dispatch a `DragEvent` of the given `event_type` to DOM node `nid`.
    ///
    /// Calls the JS shim `_lumen_dispatch_drag_event` (defined in `lumen-js::dom`)
    /// with an empty `DataTransfer` (`data_json = "{}"`).  No-op when there is
    /// no JS context.
    #[cfg(feature = "v8")]
    fn js_drag_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32) {
        let script = format!(
            "_lumen_dispatch_drag_event({}, '{}', {}, {}, '{{}}')",
            nid, event_type,
            x_css as i32, y_css as i32,
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Dispatch a `gotpointercapture` or `lostpointercapture` event to DOM node `nid`.
    ///
    /// Calls `_lumen_dispatch_capture_event` (W3C Pointer Events L3 В§4.1).
    /// These events do not bubble per spec.  No-op when there is no JS context.
    #[cfg(feature = "v8")]
    fn js_capture_event(&self, nid: u32, event_type: &str) {
        let script = format!("_lumen_dispatch_capture_event({}, '{}')", nid, event_type);
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Buffer a synthetic pointer-move sample at CSS-pixel viewport
    /// coordinates. Used by [`input::humanlike::HumanLikeSender`] to trace
    /// BГ©zier-curve paths before a click. Real `CursorMoved` samples are
    /// buffered the same way (see the `WindowEvent::CursorMoved` handler); both
    /// sources are flushed together by [`Self::flush_pointer_moves`] as one
    /// coalesced `pointermove` + `mousemove` dispatch (Pointer Events L3 В§4.1).
    fn dispatch_mouse_move(&mut self, x_css: f32, y_css: f32) {
        #[cfg(feature = "v8")]
        self.pending_pointer_moves.push((x_css, y_css));
        #[cfg(not(feature = "v8"))]
        {
            let _ = x_css;
            let _ = y_css;
        }
    }

    /// Flush buffered pointer-move samples (`CursorMoved` + injected automation
    /// moves accumulated since the last flush) as one coalesced `pointermove` +
    /// `mousemove` dispatch (Pointer Events L3 В§4.1). The last buffered sample
    /// hit-tests the target and becomes the "main" dispatched event; earlier
    /// samples are exposed via `PointerEvent.getCoalescedEvents()`. Called once
    /// per `about_to_wait` tick, and before any press/release/enter/leave
    /// dispatch so buffered moves stay ordered ahead of those events. No-op if
    /// nothing is buffered or there is no element at the final position.
    #[cfg(feature = "v8")]
    fn flush_pointer_moves(&mut self) {
        if self.pending_pointer_moves.is_empty() {
            return;
        }
        let samples = std::mem::take(&mut self.pending_pointer_moves);
        let Some(&(x_css, y_css)) = samples.last() else {
            return;
        };
        // BUG-437: same conversion as `handle_click_at` вЂ” `page_point()`, not
        // the legacy `left_dock()`/`CHROME_H` pair, so `mousemove`/`pointermove`
        // target the element the click will target and the one actually painted
        // under the cursor.
        let (page_x, page_y) = self.page_point(x_css, y_css);
        let hit = self.layout_box.as_ref().and_then(|lb| {
            hit_test(Point::new(page_x, page_y), lb)
        });
        if let Some(result) = hit {
            // Pointer Events L3 В§4.1: if a pointer capture is active, redirect
            // pointermove (and all pointer events) to the captured element.
            let hit_nid = result.node.index() as u32;
            // ADR-016 M2.2c-2d: pre-dispatch capture-read С‡РµСЂРµР· `route_query_js`
            // (РїРѕРґ С„Р»Р°РіРѕРј вЂ” Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query`; `None` = В«Р±РµР· JSВ» в†’ `hit_nid`).
            let ptr_nid = route_query_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                |c| c.pointer_capture_nid(),
            )
            .flatten()
            .unwrap_or(hit_nid);
            let coalesced = &samples[..samples.len() - 1];
            self.js_pointer_event_coalesced(ptr_nid, x_css, y_css, coalesced);
            self.js_mouse_event(hit_nid, "mousemove", x_css, y_css, 0, 0);
        }
    }

    /// Handle a left-button click at CSS-pixel viewport coordinates `(x_css, y_css)`.
    ///
    /// Used by both the winit `MouseInput::Pressed` handler and the injected
    /// [`InputCommand::Click`] path so both share identical dispatch logic.
    /// Convert viewport CSS-pixel coordinates `(x_css, y_css)` into page
    /// (document) coordinates, accounting for the current scroll offset and the
    /// left tabs panel width when visible. Mirrors the conversion used by
    /// [`Lumen::handle_click_at`] so hit tests stay consistent across input
    /// paths.
    fn page_point(&self, x_css: f32, y_css: f32) -> (f32, f32) {
        let (offset_x, offset_y) = self.page_offset();
        ((x_css - offset_x) + self.scroll_x, (y_css - offset_y) + self.scroll_y)
    }

    /// HTML LS В§4.10.21.4 step 11 вЂ” fire a cancelable `submit` event at `form`
    /// (with `submitter` exposed as `SubmitEvent.submitter`) and report whether
    /// the submission may proceed.
    ///
    /// Returns `false` only when a page handler called `preventDefault()`. With
    /// no JS runtime installed, or if the shim call itself throws, it returns
    /// `true` вЂ” a script-less page must submit exactly as it did before BUG-437,
    /// and a broken dispatch must never silently swallow a real submission.
    ///
    /// Any navigation the handler queued (`location.href = вЂ¦`, how an SPA
    /// normally takes the form over) is picked up here, mirroring the
    /// click-dispatch path in [`Self::handle_click_at`] вЂ” a *cancelled*
    /// submission still has to honour it.
    /// Run the HTML form-submission algorithm for `form` (HTML LS В§4.10.21.4).
    ///
    /// `submitter` is the activated submit control, or `None` when the page
    /// submitted the form from script with no control (`form.submit()`).
    /// `fire_submit_event` controls step 11 вЂ” the cancelable `submit` event:
    /// a real click passes `true`, while the script paths pass `false` because
    /// `requestSubmit()` already fired the event on the JS side and `submit()`
    /// is defined to skip it entirely (В§4.10.21.3).
    ///
    /// Extracted from the click handler (BUG-383) so `form.submit()` reaching
    /// the shell over `NavigateRequest::SubmitForm` runs the very same encoding,
    /// enctype and navigation code a button press does.
    fn run_form_submission(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        fire_submit_event: bool,
    ) {
        // BUG-437: everything the document lock is needed for is read in
        // one scoped borrow *before* any JS runs. Dispatching the
        // `submit` event below re-enters the JS runtime, which locks the
        // very same `Arc<Mutex<Document>>` вЂ” holding `doc` across that
        // call would deadlock the UI thread.
        let prepared = self.layout_source.as_ref().and_then(|src| {
            let doc = src.document.lock().ok()?;
            let submit_event = lumen_dom::submit_form(&doc, form);
            let enctype = forms::enctype_of_form(&doc, form);
            let dialog_node =
                lumen_dom::find_ancestor_dialog(&doc, submitter.unwrap_or(form));
            Some((submit_event, enctype, dialog_node))
        });
        if let Some((submit_event, enctype, dialog_node)) = prepared {
            match submit_event {
                lumen_dom::FormSubmitEvent::Valid { action, method, fields } => {
                    // HTML LS В§4.10.21.4 step 11: fire a **cancelable**
                    // `submit` event at the form before submitting.
                    // BUG-437: this step was missing entirely вЂ” the shell
                    // went straight to the native submission below, so a
                    // page's own `submit` handler never ran and could not
                    // `preventDefault()` the navigation. That made every
                    // SPA login form (Keycloak, Next.js) unusable, through
                    // the UI and through MCP/BiDi `click` alike.
                    if fire_submit_event
                        && let Some(sub) = submitter
                        && !self.dispatch_submit_event(form, sub)
                    {
                        return;
                    }
                    // Form passed validation вЂ” encode using enctype (HTML LS В§4.10.21.6).
                    let body = if enctype == "multipart/form-data" {
                        // Multipart: deterministic boundary for Phase 0.
                        let boundary = "----LumenFormBoundary0000000000000000";
                        let (_ct, bytes) = forms::encode_form_fields_multipart(&fields, boundary);
                        String::from_utf8_lossy(&bytes).into_owned()
                    } else {
                        forms::encode_form_fields(&fields)
                    };
                    use lumen_core::event::{Event, TabId};
                    self.event_sink.emit(&Event::FormSubmit {
                        tab_id: TabId(0),
                        action: action.clone(),
                        method: method.clone(),
                        body: body.clone(),
                    });
                    match method.as_str() {
                        "dialog" => {
                            // HTML LS В§4.10.18.3: form with method="dialog" closes
                            // the nearest ancestor <dialog>, setting its returnValue
                            // to the submit button's value attribute.
                            let rv = fields.iter()
                                .find(|(n, _)| n.is_empty() || n == "value")
                                .map(|(_, v)| v.as_str())
                                .unwrap_or("");
                            if let Some(dnid) = dialog_node {
                                let dnid_idx = dnid.index() as u32;
                                let rv = rv.to_string();
                                // ADR-016 M2.2c-2d: fire-and-forget dialog-close С‡РµСЂРµР·
                                // РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР°
                                // Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ `js.fire_dialog_close`.
                                route_task_js(
                                    self.engine_thread.as_ref(),
                                    self.js_ctx.as_ref(),
                                    move |j| j.fire_dialog_close(dnid_idx, &rv),
                                );
                            }
                        }
                        "get" => {
                            // HTML LS В§form-submission step 23: navigate
                            // to action + query-string (only urlencoded for GET).
                            let url_body = if enctype == "multipart/form-data" {
                                forms::encode_form_fields(&fields)
                            } else {
                                body.clone()
                            };
                            let get_url = forms::make_get_url(&action, &url_body);
                            let resolved = self.source.resolve_href(&get_url);
                            self.navigate_to(PageSource::from_arg(Some(&resolved)));
                        }
                        _ => {
                            // POST: emit event; real network send is P3 task.
                            eprintln!("[forms] POST {} enctype={} body-len={}", action, enctype, body.len());
                        }
                    }
                }
                lumen_dom::FormSubmitEvent::Invalid { invalid_controls } => {
                    // Form contains invalid controls вЂ” show first error.
                    // HTML LS В§4.10.21.4 step 4 rejects the submission
                    // before step 11, so no `submit` event is fired here.
                    if let Some(&first_invalid) = invalid_controls.first() {
                        let tooltip = self.layout_source.as_ref().and_then(|src| {
                            let doc = src.document.lock().ok()?;
                            let lb = self.layout_box.as_ref()?;
                            forms::find_control_rect_and_error(lb, &doc, first_invalid)
                        });
                        if let Some((rect, msg)) = tooltip {
                            self.validation_tooltip = Some((rect, msg));
                            if let Some(w) = self.window.as_ref() {
                                w.request_redraw();
                            }
                        }
                        eprintln!(
                            "forms: submit blocked вЂ” {} control(s) failed constraint validation",
                            invalid_controls.len()
                        );
                    }
                }
            }
        }
    }

    fn dispatch_submit_event(&mut self, form: NodeId, submitter: NodeId) -> bool {
        let script = format!(
            "_lumen_dispatch_submit_event({}, {})",
            form.index(),
            submitter.index(),
        );
        // `_lumen_dispatch_rich` returns `!event.defaultPrevented`, JSON-encoded
        // by `eval_js_value` вЂ” so only a literal `false` cancels.
        let proceed = match route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            move |j| j.eval_js_value(&script),
        ) {
            Some(Ok(json)) => json.trim() != "false",
            Some(Err(_)) | None => true,
        };
        if let Some(Some(nav)) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_navigate_request(),
        ) {
            self.pending_js_navigate = Some(nav);
        }
        proceed
    }

    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn handle_click_at(&mut self, x_css: f32, y_css: f32) {
        // Dismiss validation tooltip on any non-scrollbar click.
        self.validation_tooltip = None;
        let scroll_y = self.scroll_y;

        // DevTools inspector: a click pins the box under the cursor and shows
        // its computed style, suppressing normal navigation / JS dispatch.
        if self.dom_inspector.visible {
            let win_w_css = self.viewport_width_css();
            // Click inside the right-docked panel в†’ UI interaction (tab switch).
            if self.dom_inspector.is_panel_click(x_css, win_w_css) {
                if self.dom_inspector.click_tab_at(
                    x_css, y_css, win_w_css,
                    toolbar::CHROME_H,
                ) {
                    self.request_redraw();
                }
                return;
            }
            // Click on the page в†’ pin the box under cursor.
            let (page_x, page_y) = self.page_point(x_css, y_css);
            if let Some(hit) = self
                .layout_box
                .as_ref()
                .and_then(|lb| hit_test(Point::new(page_x, page_y), lb))
            {
                let node = hit.node;
                let label = self
                    .layout_source
                    .as_ref()
                    .map(|src| {
                        devtools::inspector::element_label(&src.document.lock().unwrap(), node)
                    })
                    .unwrap_or_else(|| format!("NodeId({})", node.index()));
                let props = self
                    .layout_box
                    .as_ref()
                    .and_then(|lb| devtools::inspector::find_box(lb, node))
                    .map(devtools::inspector::computed_style_map)
                    .unwrap_or_default();
                let computed_props = self
                    .layout_box
                    .as_ref()
                    .and_then(|lb| devtools::inspector::find_box(lb, node))
                    .map(|lb| {
                        let mut entries: Vec<(String, String)> =
                            computed_style_to_map(&lb.style).into_iter().collect();
                        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                        entries
                    })
                    .unwrap_or_default();
                let styles_rules: Vec<(String, Vec<(String, String)>)> = self
                    .layout_source
                    .as_ref()
                    .map(|src| {
                        let doc = src.document.lock().unwrap();
                        lumen_layout::matched_rules_for_node(&doc, node, &src.stylesheet)
                            .into_iter()
                            .map(|r| (r.selector, r.declarations))
                            .collect()
                    })
                    .unwrap_or_default();
                self.dom_inspector.select(node, label, props, styles_rules, computed_props);
                self.request_redraw();
            }
            return;
        }

        // в”Ђв”Ђ Color picker swatch hit в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Check if click lands on an open color picker swatch.
        // Compute swatch result inside a scoped borrow, then act.
        let picker_swatch_result: Option<(NodeId, [u8; 3])> = {
            let picker_node = self.color_picker_node;
            picker_node.and_then(|pn| {
                let anchor = forms::find_box_rect(
                    self.layout_box.as_ref()?,
                    pn,
                )?;
                let color = forms::hit_color_swatch(
                    anchor, scroll_y, x_css, y_css,
                )?;
                Some((pn, color))
            })
        };
        if let Some((pn, color)) = picker_swatch_result {
            self.color_picker_node = None;
            let css_color = forms::swatch_to_css_color(color);
            if let Some(src) = self.layout_source.as_mut() {
                forms::set_value(&mut src.document.lock().unwrap(), pn, &css_color);
            }
            self.form_state.entry(pn).or_default().value = css_color;
            // ADR-016 M2.2c-3: value already in the document; no post-read в†’ off-thread.
            self.relayout_form();
            return;
        }
        // Any click outside the picker closes it.
        self.color_picker_node = None;

        // в”Ђв”Ђ Date picker hit в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        let date_hit: Option<(NodeId, forms::DatePickerHit)> = {
            let dp_node = self.date_picker_node;
            dp_node.and_then(|dn| {
                let anchor = forms::find_box_rect(self.layout_box.as_ref()?, dn)?;
                let vp_w2 = self.viewport_width_css();
                let hit = forms::hit_date_picker(anchor, scroll_y, vp_w2, self.date_picker_year, self.date_picker_month, x_css, y_css);
                Some((dn, hit))
            })
        };
        if let Some((dn, hit)) = date_hit {
            match hit {
                forms::DatePickerHit::Prev => {
                    let (ny, nm) = forms::advance_month(self.date_picker_year, self.date_picker_month, -1);
                    self.date_picker_year = ny;
                    self.date_picker_month = nm;
                    self.request_redraw();
                    return;
                }
                forms::DatePickerHit::Next => {
                    let (ny, nm) = forms::advance_month(self.date_picker_year, self.date_picker_month, 1);
                    self.date_picker_year = ny;
                    self.date_picker_month = nm;
                    self.request_redraw();
                    return;
                }
                forms::DatePickerHit::Day(day) => {
                    self.date_picker_node = None;
                    let date_str = forms::format_date_value(self.date_picker_year, self.date_picker_month, day);
                    if let Some(src) = self.layout_source.as_mut() {
                        forms::set_value(&mut src.document.lock().unwrap(), dn, &date_str);
                    }
                    self.form_state.entry(dn).or_default().value = date_str;
                    // ADR-016 M2.2c-3: async-safe form mutation вЂ” see color picker.
                    self.relayout_form();
                    return;
                }
                forms::DatePickerHit::None => {}
            }
        }
        // Any click outside the date picker closes it.
        self.date_picker_node = None;

        // в”Ђв”Ђ Select dropdown option hit в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Check if click lands on an open <select> dropdown.
        let select_hit: Option<(NodeId, usize)> = {
            let sel_node = self.select_dropdown_node;
            sel_node.and_then(|sn| {
                let anchor = forms::find_box_rect(self.layout_box.as_ref()?, sn)?;
                let opts_count = self.layout_source.as_ref()
                    .map(|src| forms::collect_select_options(&src.document.lock().unwrap(), sn).len())
                    .unwrap_or(0);
                let vp_h = self.viewport_height_css();
                let vp_w2 = self.viewport_width_css();
                let idx = forms::hit_select_option(anchor, opts_count, scroll_y, vp_w2, vp_h, x_css, y_css)?;
                Some((sn, idx))
            })
        };
        if let Some((sn, idx)) = select_hit {
            self.select_dropdown_node = None;
            if let Some(src) = self.layout_source.as_mut() {
                let mut doc = src.document.lock().unwrap();
                let opts = forms::collect_select_options(&doc, sn);
                if !opts.get(idx).is_some_and(|o| o.disabled) {
                    forms::apply_select_choice(&mut doc, &opts, idx);
                    // Update form_state value so form submission includes the chosen value.
                    if let Some(chosen) = opts.get(idx) {
                        self.form_state.entry(sn).or_default().value = chosen.value.clone();
                    }
                    drop(doc);
                    // ADR-016 M2.2c-3: async-safe <select> choice вЂ” see color picker.
                    self.relayout_form();
                }
            }
            return;
        }
        // Any click outside the dropdown closes it.
        self.select_dropdown_node = None;

        // в”Ђв”Ђ Form control + link click в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Single hit test shared by form dispatch and link navigation.
        //
        // BUG-437: the conversion is [`Self::page_point`], the same one the
        // render-time page transform (`page_offset()`) and the DevTools
        // inspector already use. It used to be open-coded here as
        // `left_dock() width` / `toolbar::CHROME_H`, which stopped matching
        // where the page is actually painted once engine chrome became the
        // default (CC-14): `#contentArea` starts at y=68, not at CHROME_H=72,
        // so every click hit-tested 4 px below the pixel the user aimed at and
        // controls within 4 px of an edge resolved to the wrong node.
        let (page_x, page_y) = self.page_point(x_css, y_css);
        let hit_result = self.layout_box.as_ref().and_then(|lb| {
            hit_test(Point::new(page_x, page_y), lb)
        });

        // Debug click log вЂ” Р°РєС‚РёРІРёСЂСѓРµС‚СЃСЏ С„Р»Р°РіРѕРј --click-log РёР»Рё LUMEN_CLICK_LOG=1.
        // For click log: report both the hit box node (<p>) and the inline source_node
        // (<a> text node) so the log shows what find_link_href actually searches from.
        let click_log_hit: Option<(u32, String, String, String)> =
            if click_log::is_enabled() {
                hit_result.as_ref().and_then(|r| {
                    self.layout_source.as_ref().map(|src| {
                        let doc = src.document.lock().unwrap();
                        // Use source_node for tag/class info вЂ” it reveals the inline element.
                        let effective_id = r.source_node;
                        let node = doc.get(effective_id);
                        let (tag, id_attr, class_attr) =
                            if let NodeData::Element { name, attrs } = &node.data {
                                let id = attrs.iter()
                                    .find(|a| a.name.local == "id")
                                    .map(|a| a.value.as_str())
                                    .unwrap_or("");
                                let cls = attrs.iter()
                                    .find(|a| a.name.local == "class")
                                    .map(|a| a.value.as_str())
                                    .unwrap_or("");
                                (name.local.to_string(), id.to_owned(), cls.to_owned())
                            } else if let NodeData::Text(t) = &node.data {
                                // Show which text we clicked and note the parent element.
                                let parent_tag = node.parent
                                    .map(|pid| {
                                        let pn = doc.get(pid);
                                        if let NodeData::Element { name, .. } = &pn.data {
                                            format!("<{}>", name.local)
                                        } else {
                                            "?".to_owned()
                                        }
                                    })
                                    .unwrap_or_default();
                                let preview: String = t.chars().take(30).collect();
                                (format!("#text in {parent_tag}"), String::new(), format!("\"{preview}\""))
                            } else {
                                ("#other".to_owned(), String::new(), String::new())
                            };
                        (effective_id.index() as u32, tag, id_attr, class_attr)
                    })
                })
            } else {
                None
            };

        // Track focused node for TypeText injection and CSS :focus matching.
        let new_focused = hit_result.as_ref().map(|r| r.node);
        let focus_changed = new_focused != self.focused_node;
        self.focused_node = new_focused;
        // Trigger relayout if :focus state changed so :focus / :focus-within rules update.
        if focus_changed {
            // ADR-016 M2.2b-7: `focused_node` is set synchronously above, so
            // `:focus`/`:focus-within` re-evaluates on any later relayout. The
            // subsequent JS click dispatch reads the pre-`:focus` `hit_result`
            // (the geometry the user clicked on вЂ” correct), and any DOM mutation
            // from those handlers takes its own generation-guarded relayout, so
            // this pure restyle has no synchronous geometry read and goes off-thread.
            self.relayout_chrome();
            // Notify platform accessibility bridge so screen readers can track focus.
            self.platform_bridge.focused_node_changed(new_focused);
            // Keep JS _lumen_last_focused_nid in sync so showModal() can save/restore it.
            // ADR-016 M2.2c-2d (16): fire-and-forget void `notify_focus_changed` С‡РµСЂРµР·
            // `route_task_js`. `focus_idx` (owned `Option<u32>`) РІС‹С‡РёСЃР»СЏРµС‚СЃСЏ РґРѕ
            // РјР°СЂС€СЂСѓС‚РёР·Р°С†РёРё, Р·Р°РјС‹РєР°РЅРёРµ `Send + 'static`. РџРѕРґ С„Р»Р°РіРѕРј
            // (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґРёС‚ off-UI-thread РѕРґРЅРёРј `task`; Р±РµР· С„Р»Р°РіР°
            // (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ, **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ**
            // РїСЂРµР¶РЅРµРјСѓ `js.notify_focus_changed`.
            let focus_idx = new_focused.map(|n| n.index() as u32);
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.notify_focus_changed(focus_idx);
            });
        }
        // Dispatch JS click event (bubbles from hit node to document).
        // Passes viewport coordinates and modifier key state so
        // handlers can read event.clientX/clientY/ctrlKey/etc.
        if let Some(result) = hit_result.as_ref() {
            let mod_flags: u8 =
                (self.modifiers.control_key() as u8)
                | ((self.modifiers.shift_key()  as u8) << 1)
                | ((self.modifiers.alt_key()    as u8) << 2)
                | ((self.modifiers.super_key()  as u8) << 3);
            let script = format!(
                "_lumen_dispatch_mouse_event({}, 'click', {}, {}, 0, 1, {})",
                result.node.index(),
                x_css as i32,
                y_css as i32,
                mod_flags,
            );
            // ADR-016 M2.2c-2d (10): read-after-eval click dispatch вЂ” СЃР°Рј
            // `_lumen_dispatch_mouse_event('click', вЂ¦)` СѓС…РѕРґРёС‚ fire-and-forget С‡РµСЂРµР·
            // `route_eval_js`, Р° РїРѕСЃР»РµРґСѓСЋС‰РёР№ `take_navigate_request` (РЅР°РІРёРіР°С†РёСЏ, С‡С‚Рѕ
            // handler РјРѕРі РїРѕСЃС‚Р°РІРёС‚СЊ) вЂ” С‡РµСЂРµР· `route_query_js`. РџРѕРґ С„Р»Р°РіРѕРј
            // (`LUMEN_ENGINE_THREAD=1`) Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ**
            // РѕС‚РїСЂР°РІР»РµРЅРЅРѕРіРѕ `task`, РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°СЏ read-after-eval РїРѕСЂСЏРґРѕРє; Р±РµР· С„Р»Р°РіР°
            // (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ
            // (`js_ctx == None` в†’ `None` в†’ РЅР°РІРёРіР°С†РёСЏ РЅРµ СЃС‚Р°РІРёС‚СЃСЏ, РєР°Рє РїСЂРµР¶РЅСЏСЏ
            // РІРµС‚РєР° `Some(ctx)` РЅРµ СЃРјР°С‚С‡РёР»Р°СЃСЊ).
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
            if let Some(Some(nav)) = route_query_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                |j| j.take_navigate_request(),
            ) {
                self.pending_js_navigate = Some(nav);
            }
        }
        let form_action: forms::FormClickAction =
            if let (Some(result), Some(src)) =
                (hit_result.as_ref(), self.layout_source.as_ref())
            {
                forms::classify_click(&src.document.lock().unwrap(), result.node)
            } else {
                forms::FormClickAction::Nothing
            };

        // Log form actions (non-link outcomes).
        if click_log::is_enabled() {
            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                node_id: *nid, tag, id_attr: id, class_attr: cls,
            });
            match &form_action {
                forms::FormClickAction::Nothing => {} // logged in the Nothing branch below
                forms::FormClickAction::ToggleCheckbox(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleCheckbox"),
                    });
                }
                forms::FormClickAction::ToggleRadio { .. } => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleRadio"),
                    });
                }
                forms::FormClickAction::OpenColorPicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenColorPicker"),
                    });
                }
                forms::FormClickAction::OpenDatePicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenDatePicker"),
                    });
                }
                forms::FormClickAction::OpenSelectDropdown(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenSelectDropdown"),
                    });
                }
                forms::FormClickAction::OpenFilePicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenFilePicker"),
                    });
                }
                forms::FormClickAction::SubmitForm(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("SubmitForm"),
                    });
                }
                forms::FormClickAction::ToggleDetails(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleDetails"),
                    });
                }
                forms::FormClickAction::SlideRange(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("SlideRange"),
                    });
                }
            }
        }

        match form_action {
            forms::FormClickAction::ToggleCheckbox(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), id);
                }
                // ADR-016 M2.2c-3: the `checked` flip is already in the shared
                // document; no geometry is read after в†’ route the reflow off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::ToggleRadio {
                clicked,
                _group_name: _,
            } => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                }
                // ADR-016 M2.2c-3: async-safe form mutation вЂ” see ToggleCheckbox.
                self.relayout_form();
            }
            forms::FormClickAction::OpenColorPicker(id) => {
                self.color_picker_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenDatePicker(id) => {
                let (y, m) = self.layout_source.as_ref()
                    .and_then(|src| {
                        let doc = src.document.lock().ok()?;
                        let val = doc.control_value(id).into_owned();
                        forms::parse_date_value(&val).map(|(y, m, _)| (y, m))
                    })
                    .unwrap_or_else(forms::today_year_month);
                self.date_picker_node = Some(id);
                self.date_picker_year = y;
                self.date_picker_month = m;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenSelectDropdown(id) => {
                self.select_dropdown_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenFilePicker(id) => {
                self.open_file_picker(id);
            }
            forms::FormClickAction::ToggleDetails(id) => {
                // BUG-851: the flip is the shell's alone. The JS click this
                // method already dispatched used to reach a `click` listener on
                // `document` that flipped `open` a second time, so a real mouse
                // click on a `<summary>` left `<details>` exactly as it found it
                // вЂ” and fired two `toggle` events about the change that did not
                // happen. That listener is gone; JS is only *told* what changed.
                let was_open = self.layout_source.as_ref().is_some_and(|src| {
                    src.document
                        .lock()
                        .is_ok_and(|doc| doc.get(id).get_attr("open").is_some())
                });
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_details_open(&mut src.document.lock().unwrap(), id);
                }
                // HTML LS В§4.11.1 attribute change steps for `open` вЂ” the queued
                // `toggle` event and the exclusive-accordion pass. Routing them
                // through the shim instead of dispatching a bare `Event('toggle')`
                // here is what makes the native click and every scripted write to
                // `open` one mechanism.
                // ADR-016 M2.2c-2d: fire-and-forget С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ
                // С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
                #[cfg(feature = "v8")]
                route_eval_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    format!(
                        "_lumen_details_native_toggled({}, {})",
                        id.index(),
                        was_open
                    ),
                );
                // ADR-016 M2.2c-3: <details> open flip already applied to the
                // document (the routed `toggle` event above only notifies JS); no
                // geometry is read after в†’ route the reflow off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::SlideRange(id) => {
                if let (Some(src), Some(lb)) =
                    (self.layout_source.as_mut(), self.layout_box.as_ref())
                    && let Some(rect) = forms::find_box_rect(lb, id)
                {
                    forms::apply_range_value(
                        &mut src.document.lock().unwrap(),
                        id,
                        rect,
                        page_x,
                    );
                }
                // ADR-016 M2.2c-3: range value applied to the document (the
                // pre-relayout `find_box_rect` read is against the old layout to map
                // the click x в†’ value); no post-relayout read в†’ off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::SubmitForm(submit_node) => {
                // Phase 3: HTML5 form submission algorithm integration вЂ”
                // constraint validation, encoding and navigation all live in
                // `run_form_submission`, shared with script-initiated submits.
                let form_node = self.layout_source.as_ref().and_then(|src| {
                    let doc = src.document.lock().ok()?;
                    lumen_dom::find_ancestor_form(&doc, submit_node)
                });
                if let Some(form) = form_node {
                    self.run_form_submission(form, Some(submit_node), true);
                }
            }
            forms::FormClickAction::Nothing => {
                // в”Ђв”Ђ Link click в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
                // No form control was activated вЂ” check if
                // the clicked node is inside an <a href>.
                // Use source_node (text node inside inline element) so find_link_href
                // can walk up and find the <a> parent: text в†’ <a href="вЂ¦"> в†’ found.
                // Falls back to r.node for non-inline boxes.
                let href = hit_result.as_ref().and_then(|r| {
                    self.layout_source
                        .as_ref()
                        .and_then(|src| links::find_link_href(&src.document.lock().unwrap(), r.source_node))
                });
                if let Some(href) = href {
                    if let Some(frag) = links::fragment_only(&href) {
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkFragment(frag),
                            });
                        }
                        // Same-page fragment navigation.
                        self.navigate_fragment(frag.to_owned());
                    } else if links::is_navigable_href(&href) {
                        let resolved = self.source.resolve_href(&href);
                        // `about:newtab?...` special links (pin/unpin, "+",
                        // restore-closed, DS-11) are handled in-place, never
                        // as a real navigation.
                        if let Some(action) = newtab::parse_action(&resolved) {
                            self.apply_newtab_action(action);
                        } else if let Some(frag) =
                            links::same_document_fragment(self.current_display_url(), &resolved)
                        {
                            if click_log::is_enabled() {
                                let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                    node_id: *nid, tag, id_attr: id, class_attr: cls,
                                });
                                click_log::log_click(&click_log::ClickInfo {
                                    win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                    hit: hit_ref,
                                    outcome: click_log::ClickOutcome::LinkFragment(&frag),
                                });
                            }
                            self.navigate_fragment(frag);
                        } else {
                            if click_log::is_enabled() {
                                let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                    node_id: *nid, tag, id_attr: id, class_attr: cls,
                                });
                                click_log::log_click(&click_log::ClickInfo {
                                    win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                    hit: hit_ref,
                                    outcome: click_log::ClickOutcome::LinkNavigate {
                                        href: &href,
                                        resolved: &resolved,
                                    },
                                });
                            }
                            let target = PageSource::from_arg(Some(&resolved));
                            self.navigate_to(target);
                        }
                    } else {
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkBlocked(&href),
                            });
                        }
                    }
                } else if click_log::is_enabled() {
                    let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                        node_id: *nid, tag, id_attr: id, class_attr: cls,
                    });
                    let outcome = if hit_result.is_none() {
                        click_log::ClickOutcome::NoHit
                    } else {
                        click_log::ClickOutcome::NoLink
                    };
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome,
                    });
                }
            }
        }
    }

    /// Inject a typed character into the focused element (TypeText injection path).
    ///
    /// Inject a special (non-printable) key press: `keydown` в†’ `keyup`.
    ///
    /// `code` is a W3C `KeyboardEvent.code` string, e.g. `"Enter"`, `"Backspace"`.
    /// The matching `KeyboardEvent.key` value is resolved via [`input::native::code_to_key`]
    /// (`"Space"` в†’ `" "`, everything else passes through unchanged).
    /// Events have `isTrusted=true`; JS `dispatchEvent()` is never used.
    fn inject_special_key(&mut self, code: &str) {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        let key = input::native::code_to_key(code);
        // ADR-016 M2.2c-2d (10): keyboard injection вЂ” `_lumen_dispatch_key_event`
        // (keydown в†’ keyup) СѓС…РѕРґРёС‚ fire-and-forget С‡РµСЂРµР· `route_eval_js`, Р°
        // РїРѕСЃР»РµРґСѓСЋС‰РёР№ `take_navigate_request` вЂ” С‡РµСЂРµР· `route_query_js`. РџРѕРґ С„Р»Р°РіРѕРј
        // (`LUMEN_ENGINE_THREAD=1`) Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ РїРѕСЃР»Рµ
        // РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… `task`, РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°СЏ read-after-eval РїРѕСЂСЏРґРѕРє; Р±РµР· С„Р»Р°РіР°
        // (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ (`js_ctx == None`
        // в†’ `route_eval_js` no-op + `route_query_js` в†’ `None`, РєР°Рє РїСЂРµР¶РЅРёР№ early-`return`).
        for event_type in &["keydown", "keyup"] {
            let script = format!(
                "_lumen_dispatch_key_event({}, '{}', '{}', '{}', false, false, false, false)",
                node_id, event_type, key, code,
            );
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
        }
        if let Some(Some(nav)) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_navigate_request(),
        ) {
            self.pending_js_navigate = Some(nav);
        }
    }

    /// Classify `nid` as a mutable text-editing form control and read the value
    /// it currently renders (BUG-436).
    ///
    /// Returns `None` for anything that is not a typeable `<input>` (the
    /// text-like types вЂ” the same set [`InProcessSession::type_text`] accepts)
    /// or a `<textarea>`, and for a control that is `disabled` or `readonly`
    /// (HTML LS В§4.10.19.2 вЂ” such a control is not mutable, so the engine
    /// performs no insertion).
    ///
    /// The value read is the *rendered* one вЂ” [`Document::control_value`], the
    /// control's current value, which is what layout paints and what form
    /// submission collects (BUG-441). The `value` attribute / child text behind
    /// it is only the default the field started from.
    fn typeable_field(&self, nid: lumen_dom::NodeId) -> Option<(TypeableField, String)> {
        let doc = self.layout_source.as_ref()?.document.lock().ok()?;
        let node = doc.get(nid);
        if node.get_attr("disabled").is_some() || node.get_attr("readonly").is_some() {
            return None;
        }
        if node.element_name().is_some_and(|n| n.local.eq_ignore_ascii_case("textarea")) {
            return Some((TypeableField::Textarea, doc.control_value(nid).into_owned()));
        }
        let is_typeable_input = matches!(
            node.input_type(),
            Some(lumen_dom::InputType::Text)
                | Some(lumen_dom::InputType::Password)
                | Some(lumen_dom::InputType::Email)
                | Some(lumen_dom::InputType::Tel)
                | Some(lumen_dom::InputType::Url)
                | Some(lumen_dom::InputType::Number)
                | Some(lumen_dom::InputType::Search)
        );
        if !is_typeable_input {
            return None;
        }
        Some((TypeableField::Input, doc.control_value(nid).into_owned()))
    }

    /// Engine-side text-editing default action on the focused form control
    /// (BUG-436): `edit` maps the field's current value to its new value.
    ///
    /// The JS shim only *dispatches* `keydown`/`input`/`keyup`; changing the
    /// control's value is the engine's own default action (HTML LS В§4.10.5.5),
    /// exactly as [`InProcessSession::dispatch_type`] does for the headless
    /// driver. Without it the live window fired `input` events on a field that
    /// never changed вЂ” `type` reported success, `input.value` stayed `""` and
    /// the field rendered empty.
    ///
    /// Returns `true` when a mutable field consumed the edit. The DOM mutation
    /// happens with the document lock held and no JS dispatched under it (the
    /// deadlock trap found in BUG-437); the JS-side value shadow is synced
    /// afterwards so a listener reading `this.value` sees the new value.
    fn edit_focused_field(&mut self, edit: impl FnOnce(&str) -> String) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((kind, current)) = self.typeable_field(nid) else { return false };
        let next = edit(&current);
        if next == current {
            return true;
        }
        if let Some(src) = self.layout_source.as_mut()
            && let Ok(mut doc) = src.document.lock()
        {
            match kind {
                TypeableField::Input => forms::set_value(&mut doc, nid, &next),
                TypeableField::Textarea => forms::set_textarea_text(&mut doc, nid, &next),
            }
        }
        // Runtime value overlay used by form submission and constraint
        // validation (`forms::collect_form_entries`) вЂ” kept in step with the DOM
        // exactly like the spellcheck-replace path does.
        self.form_state.entry(nid).or_default().value = next.clone();
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!("_lumen_set_field_value({}, '{}')", nid.index(), escape_js_string(&next)),
        );
        self.relayout_form();
        true
    }

    /// Fires `keydown` в†’ `input` в†’ `keyup` JS events via `_lumen_dispatch_key_event`
    /// on the last-focused node so events have `isTrusted=true`.
    ///
    /// Between `keydown` and `input` the engine runs its own text-insertion
    /// default action ([`Self::edit_focused_field`], BUG-436), so an `input`
    /// listener reading `this.value` observes the character just typed.
    /// Returns `true` when a form control accepted the character.
    fn inject_char(&mut self, ch: char) -> bool {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        let key = escape_js_string_char(ch);
        // ADR-016 M2.2c-2d (10): same read-after-eval routing as `inject_special_key`
        // вЂ” keydown в†’ input в†’ keyup dispatch off-UI-thread under the flag, then the
        // `take_navigate_request` read ordered after via `route_query_js`; byte-identical
        // off-flag.
        self.dispatch_injected_key(node_id, "keydown", &key);
        let consumed = self.edit_focused_field(|current| {
            let mut next = current.to_owned();
            next.push(ch);
            next
        });
        for event_type in &["input", "keyup"] {
            self.dispatch_injected_key(node_id, event_type, &key);
        }
        if let Some(Some(nav)) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_navigate_request(),
        ) {
            self.pending_js_navigate = Some(nav);
        }
        consumed
    }

    /// Backspace on the focused form control: `keydown` в†’ engine deletes the
    /// last character ([`Self::edit_focused_field`]) в†’ `input` в†’ `keyup`.
    ///
    /// The counterpart of [`Self::inject_char`] вЂ” without it a field could be
    /// filled but never corrected. Returns `true` when a form control consumed
    /// the key.
    fn inject_backspace(&mut self) -> bool {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        self.dispatch_injected_key(node_id, "keydown", "Backspace");
        let consumed = self.edit_focused_field(|current| {
            let mut next = current.to_owned();
            next.pop();
            next
        });
        for event_type in &["input", "keyup"] {
            self.dispatch_injected_key(node_id, event_type, "Backspace");
        }
        consumed
    }

    /// Send one `_lumen_dispatch_key_event` for an injected/typed key.
    ///
    /// `key` must already be escaped for a single-quoted JS literal
    /// ([`escape_js_string_char`]).
    fn dispatch_injected_key(&mut self, node_id: usize, event_type: &str, key: &str) {
        let script = format!(
            "_lumen_dispatch_key_event({}, '{}', '{}', '{}', false, false, false, false)",
            node_id, event_type, key, key,
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn handle_key(&mut self, event_loop: &ActiveEventLoop, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = key_event.physical_key else {
            return;
        };

        // РљРѕРјР°РЅРґРЅР°СЏ РїР°Р»РёС‚СЂР° вЂ” РјРѕРґР°Р»СЊРЅС‹Р№ overlay: РїРѕРєР° РѕС‚РєСЂС‹С‚Р°, РїРµСЂРµС…РІР°С‚С‹РІР°РµС‚ РІСЃРµ
        // РєР»Р°РІРёС€Рё (Esc/Enter/в†‘/в†“/Backspace/РїРµС‡Р°С‚СЊ). Ctrl+K (toggle) РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ
        // РІ РіР»РѕР±Р°Р»СЊРЅС‹Р№ keybinding-РїСѓС‚СЊ РЅРёР¶Рµ, С‡С‚РѕР±С‹ Р·Р°РєСЂС‹С‚СЊ РїР°Р»РёС‚СЂСѓ.
        if self.command_palette.visible
            && !(code == KeyCode::KeyK && self.modifiers == ModifiersState::CONTROL)
            && self.handle_palette_key(code, key_event, event_loop)
        {
            return;
        }

        // РђРґСЂРµСЃРЅР°СЏ СЃС‚СЂРѕРєР° (Ctrl+L) РїРµСЂРµС…РІР°С‚С‹РІР°РµС‚ РІРІРѕРґ РїРµСЂРІРѕР№: Esc=close,
        // Enter=navigate, Backspace=СѓРґР°Р»РёС‚СЊ СЃРёРјРІРѕР», РёРЅР°С‡Рµ вЂ” С‚РµРєСЃС‚ URL.
        if self.address_bar.is_open() {
            self.handle_address_bar_key(code, key_event, event_loop);
            return;
        }

        // РљРѕРіРґР° find bar РѕС‚РєСЂС‹С‚ вЂ” РІСЃРµ РєР»Р°РІРёС€Рё РёРґСѓС‚ РІ РЅРµРіРѕ: РІРІРѕРґ СЃРёРјРІРѕР»РѕРІ,
        // Esc=close, Backspace=СЃС‚РёСЂР°РЅРёРµ, Enter/F3=next (Shift=prev). Р­С‚Рѕ РЅРµ
        // РґР°С‘С‚ СЃР»СѓС‡Р°Р№РЅРѕ СЃСЂР°Р±РѕС‚Р°С‚СЊ Esc=Exit РёР»Рё Ctrl+R=Reload РІ РјРѕРјРµРЅС‚ РїРѕРёСЃРєР°.
        if self.find.is_open() {
            self.handle_find_key(code, key_event);
            return;
        }

        // Hint-СЂРµР¶РёРј: РІСЃРµ РєР»Р°РІРёС€Рё РёРґСѓС‚ РІ РЅРµРіРѕ РїРѕРєР° Р°РєС‚РёРІРµРЅ.
        // Esc=close, Р±СѓРєРІР°=СЃСѓР¶РµРЅРёРµ/Р°РєС‚РёРІР°С†РёСЏ С…РёРЅС‚Р°.
        if self.hint.is_active() {
            self.handle_hint_key(code, key_event);
            return;
        }

        // Bookmark panel search box: when focused, printable input + Backspace +
        // Esc route to the search query. Modified keys (Ctrl/Cmd) fall through so
        // global shortcuts (e.g. Ctrl+Shift+O to close) keep working.
        if self.bookmark_panel.visible
            && self.bookmark_panel.search_active
            && self.handle_bookmark_key(code, key_event)
        {
            return;
        }

        // History panel search box: printable input + Backspace + Esc route here.
        // Arrow keys scroll the list. Modified keys fall through for global shortcuts.
        if self.history_panel.visible && self.handle_history_key(code, key_event) {
            return;
        }

        // Note viewer overlay: Escape closes it.
        if self.note_viewer.visible && code == KeyCode::Escape && !key_event.repeat {
            self.note_viewer.close();
            self.request_redraw();
            return;
        }

        // AI panel input: printable text, Backspace, Enter. Ctrl/Meta fall through.
        if self.ai_panel.visible && self.handle_ai_panel_key(code, key_event) {
            return;
        }

        // Settings panel text inputs + Esc. Modified keys fall through for global shortcuts.
        if self.print_panel.visible && self.handle_print_key(code, key_event) {
            return;
        }
        if self.settings_panel.visible && self.handle_settings_key(code, key_event) {
            return;
        }

        // Keyboard shortcuts panel вЂ” capture any keypress when rebinding (В§D-4).
        if self.shortcuts_panel.visible && self.handle_shortcuts_key(code, key_event) {
            return;
        }

        // Vim keybinding mode: intercept navigation keys in Normal state.
        // In Insert state, PassThrough falls through to the keybinding table.
        if let Some(ref mut vm) = self.vim_mode {
            let action = vm.feed(code, self.modifiers);
            match action {
                input::vim::VimAction::PassThrough => {} // fall through below
                input::vim::VimAction::Consumed => return,
                input::vim::VimAction::Deactivate => {
                    self.vim_mode = None;
                    return;
                }
                input::vim::VimAction::EnterInsert | input::vim::VimAction::ExitInsert => {
                    return;
                }
                input::vim::VimAction::ScrollDown => {
                    self.scroll_active_pane(LINE_STEP_CSS_PX);
                    return;
                }
                input::vim::VimAction::ScrollUp => {
                    self.scroll_active_pane(-LINE_STEP_CSS_PX);
                    return;
                }
                input::vim::VimAction::ScrollHalfPageDown => {
                    let half = self.viewport_height_css() * 0.5;
                    self.scroll_active_pane(half);
                    return;
                }
                input::vim::VimAction::ScrollHalfPageUp => {
                    let half = self.viewport_height_css() * 0.5;
                    self.scroll_active_pane(-half);
                    return;
                }
                input::vim::VimAction::ScrollTop => {
                    self.scroll_active_pane_to(0.0);
                    return;
                }
                input::vim::VimAction::ScrollBottom => {
                    self.scroll_active_pane_to(f32::INFINITY);
                    return;
                }
                input::vim::VimAction::OpenFind => {
                    self.hint.close();
                    self.find.open();
                    self.request_redraw();
                    return;
                }
                input::vim::VimAction::OpenHints | input::vim::VimAction::OpenHintsNewTab => {
                    if let (Some(lb), Some(src)) =
                        (self.layout_box.as_ref(), self.layout_source.as_ref())
                    {
                        let doc = src.document.lock().unwrap();
                        let elements = lumen_layout::collect_clickable_elements(lb, &doc);
                        drop(doc);
                        if !elements.is_empty() {
                            self.hint.open(elements);
                            self.request_redraw();
                        }
                    }
                    return;
                }
                input::vim::VimAction::Copy => {
                    // Copy the current page URL to the OS clipboard (task #26).
                    if let Some(url) = self.source.url_str() {
                        use lumen_core::ext::ClipboardProvider;
                        platform::clipboard::PlatformClipboard.write_text(url);
                        eprintln!("[vim] copy URL: {url}");
                    }
                    return;
                }
                input::vim::VimAction::HistoryBack => {
                    self.navigate_back();
                    return;
                }
                input::vim::VimAction::HistoryForward => {
                    self.navigate_forward();
                    return;
                }
            }
        }

        // Pointer Lock API (W3C Pointer Lock L2 В§6.7): Escape releases pointer lock.
        // Must be processed before fullscreen so a locked pointer in fullscreen exits
        // lock first, letting a second Escape then exit fullscreen.
        #[cfg(feature = "v8")]
        if lumen_js::pointer_lock::is_pointer_locked()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            lumen_js::pointer_lock::exit_pointer_lock();
            // Apply OS cursor release immediately (don't wait for about_to_wait).
            if let Some(window) = self.window.as_ref() {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
                window.set_cursor_visible(true);
            }
            // Dispatch pointerlockchange so document.pointerLockElement clears in
            // JS. ADR-016 M2.2c-2d: fire-and-forget void eval С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ”
            // РїРѕРґ С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                "document.dispatchEvent(new Event('pointerlockchange'))".to_string(),
            );
            return;
        }

        // Fullscreen API (WHATWG Fullscreen В§4.6): Escape always exits fullscreen first.
        // If we are fullscreen and the user presses Escape (no repeat, no mods), exit
        // fullscreen before processing any other shortcut.
        if self.fullscreen_nid.is_some()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.fullscreen_nid = None;
            let prev = self.window.as_ref().map(|w| {
                w.set_fullscreen(None);
                w.inner_size()
            });
            if let Some(prev) = prev {
                self.arm_fullscreen_resize(prev);
            }
            // Notify JS so fullscreenchange fires and document.fullscreenElement clears.
            // ADR-016 M2.2c-2d: fire-and-forget void eval С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ
            // С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ
            // `js.eval_js(вЂ¦)` (РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІСѓСЋС‰РµРј С…СЌРЅРґР»Рµ вЂ” no-op, РєР°Рє РїСЂРµР¶РЅРёР№ `if let`).
            #[cfg(feature = "v8")]
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                "if(typeof _lumen_notify_fullscreen_exit==='function')_lumen_notify_fullscreen_exit()"
                    .to_string(),
            );
            return;
        }

        // CC-4: Escape closes the tab context menu before any other handling.
        if self.tab_context_menu.is_open()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.tab_context_menu.close();
            self.request_redraw();
            return;
        }

        // P3-spell СЃСЂРµР· 3: Escape closes the page spell suggestion menu.
        if self.page_context_menu.is_open()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.page_context_menu.close();
            self.request_redraw();
            return;
        }

        // Focus mode (task #25): while active, Escape exits focus mode instead of
        // quitting the app. Ctrl+Shift+F falls through to the keybinding table so
        // it can toggle focus mode off.
        if self.focus.active
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.focus.exit();
            self.request_redraw();
            return;
        }

        // contenteditable key routing вЂ” before global keybindings so that
        // typing inside an editable region is not swallowed by scroll commands.
        // Only active when the focused node is inside a contenteditable host
        // and no modifier (Ctrl/Alt/Meta) is held (those go to keybindings).
        if (self.modifiers.is_empty() || self.modifiers == ModifiersState::SHIFT)
            && let (Some(nid), Some(src)) = (self.focused_node, self.layout_source.as_ref())
        {
            // ADR-016 M2.2c-2d: contenteditable-key void-eval С‡РµСЂРµР· `route_eval_js` вЂ”
            // СЃРЅРёРјР°РµРј РїСЂСЏРјС‹Рµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёСЏ. DOM-read (`find_editing_host`)
            // РѕСЃС‚Р°С‘С‚СЃСЏ РЅР° UI-РїРѕС‚РѕРєРµ (С‡РёС‚Р°РµС‚ СЂР°Р·РґРµР»СЏРµРјС‹Р№ `src.document`, РЅРµ JS-С…СЌРЅРґР»);
            // СЃР°РјРё `_lumen_handle_contenteditable_key`-РІС‹Р·РѕРІС‹ вЂ” С‡РёСЃС‚С‹Р№ fire-and-forget
            // void Р±РµР· СЃРёРЅС…СЂРѕРЅРЅРѕРіРѕ С‡С‚РµРЅРёСЏ СЂРµР·СѓР»СЊС‚Р°С‚Р° СЃР»РµРґРѕРј, РїРѕСЌС‚РѕРјСѓ РїРѕРґ С„Р»Р°РіРѕРј
            // (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґСЏС‚ off-UI-thread РѕРґРЅРёРј `task`, Р±РµР· С„Р»Р°РіР° (РїРѕ
            // СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ. Р“РµР№С‚ Р·Р°РјРµРЅС‘РЅ
            // СЃ `if let Some(js)` РЅР° `is_some()`, С‡С‚РѕР±С‹ editing-host detection Рё eval
            // РІС‹РїРѕР»РЅСЏР»РёСЃСЊ С‚РѕР»СЊРєРѕ РїСЂРё РЅР°Р»РёС‡РёРё JS-РєРѕРЅС‚РµРєСЃС‚Р° (РєР°Рє РїСЂРµР¶РґРµ).
            #[cfg(feature = "v8")]
            if self.js_present {
                // Check contenteditable by reading the DOM directly (eval_js returns ()).
                let editing_host = src
                    .document
                    .lock()
                    .ok()
                    .and_then(|doc| lumen_dom::find_editing_host(&doc, nid));
                if let Some(host) = editing_host {
                    let host_nid = host.index();
                    let handled = match code {
                        KeyCode::Backspace => {
                            route_eval_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                format!(
                                    "_lumen_handle_contenteditable_key('deleteContentBackward',null,{})",
                                    host_nid
                                ),
                            );
                            true
                        }
                        KeyCode::Delete => {
                            route_eval_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                format!(
                                    "_lumen_handle_contenteditable_key('deleteContentForward',null,{})",
                                    host_nid
                                ),
                            );
                            true
                        }
                        KeyCode::Enter | KeyCode::NumpadEnter => {
                            let input_type = if self.modifiers == ModifiersState::SHIFT {
                                "insertLineBreak"
                            } else {
                                "insertParagraph"
                            };
                            route_eval_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                format!(
                                    "_lumen_handle_contenteditable_key('{}',null,{})",
                                    input_type, host_nid
                                ),
                            );
                            true
                        }
                        _ => {
                            // Printable key вЂ” extract text from logical key.
                            if let Some(text) = key_event.logical_key.to_text()
                                && !text.is_empty()
                                && text.chars().all(|c| !c.is_control())
                            {
                                let escaped =
                                    text.replace('\\', "\\\\").replace('\'', "\\'");
                                route_eval_js(
                                    self.engine_thread.as_ref(),
                                    self.js_ctx.as_ref(),
                                    format!(
                                        "_lumen_handle_contenteditable_key('insertText','{}',{})",
                                        escaped, host_nid
                                    ),
                                );
                                self.request_redraw();
                                return;
                            }
                            false
                        }
                    };
                    if handled {
                        self.request_redraw();
                        return;
                    }
                }
            }
        }

        // Text editing inside a focused `<input>`/`<textarea>` вЂ” same placement
        // rationale as the contenteditable branch above: without it a printable
        // key falls through to the global keybinding table, where a bare `F`
        // opens hint mode and Space scrolls the page instead of reaching the
        // field. The insertion itself is the engine's own default action
        // (`inject_char` в†’ `edit_focused_field`, BUG-436).
        if (self.modifiers.is_empty() || self.modifiers == ModifiersState::SHIFT)
            && self.focused_node.is_some_and(|nid| self.typeable_field(nid).is_some())
        {
            if code == KeyCode::Backspace {
                self.inject_backspace();
                self.request_redraw();
                return;
            }
            if let Some(text) = key_event.logical_key.to_text()
                && !text.is_empty()
                && text.chars().all(|c| !c.is_control())
            {
                for ch in text.chars() {
                    self.inject_char(ch);
                }
                self.request_redraw();
                return;
            }
        }

        let Some(cmd) = keybinding_for(code, self.modifiers) else {
            return;
        };
        // Scroll-РєРѕРјР°РЅРґС‹ СЂР°Р·СЂРµС€Р°РµРј РЅР° repeat (auto-repeat РїСЂРё СѓРґРµСЂР¶Р°РЅРёРё),
        // РѕСЃС‚Р°Р»СЊРЅС‹Рµ вЂ” С‚РѕР»СЊРєРѕ РЅР° РїРµСЂРІРѕРµ РЅР°Р¶Р°С‚РёРµ.
        let is_scroll = matches!(
            cmd,
            KeyCommand::ScrollLineDown
                | KeyCommand::ScrollLineUp
                | KeyCommand::ScrollPageDown
                | KeyCommand::ScrollPageUp
                | KeyCommand::ScrollHome
                | KeyCommand::ScrollEnd
                | KeyCommand::ScrollLineRight
                | KeyCommand::ScrollLineLeft
        );
        if key_event.repeat && !is_scroll {
            return;
        }
        match cmd {
            KeyCommand::Reload => {
                // HTML В§8.1.4 В«Event loopВ»: РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёРµ РґРµР№СЃС‚РІРёСЏ (reload)
                // РїР»Р°РЅРёСЂСѓСЋС‚СЃСЏ С‡РµСЂРµР· UserInteraction task source, Р° РЅРµ РІС‹Р·С‹РІР°СЋС‚СЃСЏ
                // РЅР°РїСЂСЏРјСѓСЋ. `pending_reload` вЂ” С„Р»Р°Рі-РјРѕСЃС‚: closure-Р·Р°РґР°С‡Р° РјРѕР¶РµС‚
                // Р±С‹С‚СЊ `+ 'static`, Lumen вЂ” РЅРµС‚; Cell РїРѕР·РІРѕР»СЏРµС‚ РёР· Р·Р°РјС‹РєР°РЅРёСЏ
                // СѓСЃС‚Р°РЅРѕРІРёС‚СЊ С„Р»Р°Рі, РєРѕС‚РѕСЂС‹Р№ `about_to_wait` РїСЂРѕРІРµСЂСЏРµС‚ Рё РІС‹Р·С‹РІР°РµС‚
                // `reload()` РїРѕСЃР»Рµ РґСЂРµРЅР°Р¶Р° РѕС‡РµСЂРµРґРё.
                let flag = Rc::clone(&self.pending_reload);
                self.runtime.handle().queue_task(
                    runtime::TaskSource::UserInteraction,
                    move || { flag.set(true); },
                );
            }
            KeyCommand::Exit => event_loop.exit(),
            KeyCommand::FindOpen => {
                self.hint.close();
                self.find.open();
                self.request_redraw();
            }
            KeyCommand::OpenAddressBar => {
                self.hint.close();
                let current = self.current_display_url().to_owned();
                self.address_bar.open(&current);
                // CC-7: reflect the now-open state (focus ring, value) in
                // the engine-rendered `#omniInput` вЂ” see the comment on the
                // matching call in `Self::handle_address_bar_key`.
                self.relayout_chrome_host();
                self.request_redraw();
            }
            KeyCommand::HintModeOpen => {
                if let (Some(lb), Some(src)) =
                    (self.layout_box.as_ref(), self.layout_source.as_ref())
                {
                    let doc = src.document.lock().unwrap();
                    let elements = lumen_layout::collect_clickable_elements(lb, &doc);
                    drop(doc);
                    if !elements.is_empty() {
                        self.hint.open(elements);
                        self.request_redraw();
                    }
                }
            }
            KeyCommand::HistoryBack => self.navigate_back(),
            KeyCommand::HistoryForward => self.navigate_forward(),
            KeyCommand::ScrollLineDown => self.scroll_active_pane(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineUp => self.scroll_active_pane(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineRight => self.scroll_x_by(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineLeft => self.scroll_x_by(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollPageDown => {
                let vh = self.viewport_height_css();
                self.scroll_active_pane(page_step(vh));
            }
            KeyCommand::ScrollPageUp => {
                let vh = self.viewport_height_css();
                self.scroll_active_pane(-page_step(vh));
            }
            KeyCommand::ScrollHome => self.scroll_active_pane_to(0.0),
            KeyCommand::ScrollEnd => self.scroll_active_pane_to(f32::INFINITY),
            KeyCommand::NewTab => self.open_new_tab(),
            KeyCommand::CloseTab => {
                let idx = self.tab_strip.active;
                self.close_tab(idx, event_loop);
            }
            KeyCommand::NextTab => {
                let next = (self.tab_strip.active + 1) % self.tab_strip.len();
                self.switch_tab(next);
            }
            KeyCommand::DownloadsPanel => {
                self.downloads.toggle_visible();
                self.request_redraw();
            }
            KeyCommand::SplitView => {
                if self.split_view.is_some() {
                    self.split_view = None;
                } else {
                    self.toggle_split_view();
                }
                self.request_redraw();
            }
            KeyCommand::SplitFocusSwitch => {
                if let Some(ref mut sv) = self.split_view {
                    sv.toggle_focus();
                    self.request_redraw();
                }
            }
            KeyCommand::VimModeToggle => {
                if self.vim_mode.is_some() {
                    self.vim_mode = None;
                } else {
                    self.vim_mode = Some(input::vim::VimMode::new());
                }
            }
            KeyCommand::ToggleVerticalTabs => {
                self.vertical_tabs.toggle();
                self.persist_tab_layout();
                // Viewport width changes вЂ” re-layout the current page (ADR-016
                // M2.2b: chrome-inset change, off-thread when the engine is on).
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::ToggleTreeTabs => {
                self.tree_tabs.toggle();
                // Viewport width changes when switching to/from tree view
                // (ADR-016 M2.2b: async-safe chrome-inset relayout).
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::FlipActiveDock => {
                // Cross-dock the active sidebar (tabs, AI, or web);
                // flip_active_sidebar_dock relayouts internally on success.
                if self.flip_active_sidebar_dock() {
                    self.request_redraw();
                }
            }
            KeyCommand::ToggleWorkspaces => {
                self.workspace_panel.toggle();
                // Viewport height changes вЂ” re-layout so content doesn't hide
                // under bar (ADR-016 M2.2b: async-safe chrome-inset relayout).
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::ToggleShields => {
                self.shields.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePermissions => {
                self.permission.toggle();
                self.request_redraw();
            }
            KeyCommand::ToggleCookieBannerDismiss => {
                self.cookie_banner_dismiss = !self.cookie_banner_dismiss;
                // Preference takes effect on the next page load.
            }
            KeyCommand::ToggleAiPanel => {
                self.ai_panel.toggle();
                // AI panel occupies right PANEL_WIDTH вЂ” relayout so main content
                // width adjusts accordingly. ADR-016 M2.2b-3: async-safe chrome
                // toggle (only the content viewport width shifts, no synchronous
                // geometry read follows), so route off-thread when the engine
                // thread is enabled; the panel itself draws on the redraw below.
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::ToggleBookmarks => {
                self.bookmark_panel.toggle();
                if self.bookmark_panel.visible {
                    self.refresh_bookmarks();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleHistory => {
                self.history_panel.toggle();
                if self.history_panel.visible {
                    self.refresh_history();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleA11y => {
                if self.a11y_panel.visible {
                    let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                    self.a11y_panel.visible = false;
                    self.deliver_a11y_media_changes();
                    // Re-style with the (possibly toggled) forced-colors pref.
                    // ADR-016 M2.2b-3: async-safe вЂ” closing the a11y panel widens
                    // the content viewport and re-styles under the new
                    // forced-colors preference, but nothing reads page geometry
                    // synchronously afterwards, so route off-thread when enabled.
                    self.relayout_chrome();
                } else {
                    self.a11y_panel.load_draft(self.a11y_store.snapshot());
                    self.a11y_panel.visible = true;
                }
                self.request_redraw();
            }
            KeyCommand::ToggleSettings => {
                if self.settings_panel.visible {
                    self.close_settings_panel();
                } else {
                    self.open_settings_panel();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleCommandPalette => {
                self.command_palette.toggle();
                if self.command_palette.visible {
                    self.refresh_palette_items();
                }
                self.request_redraw();
                // CC-10: `#cpOverlay`'s engine-rendered open state/results
                // (`Self::chrome_model_snapshot`) is baked into
                // `self.chrome_layout` at `relayout_chrome_host` time, not
                // recomputed every `RedrawRequested` вЂ” same class of gap
                // CC-7/CC-9 found for the omnibox/find-bar. No-op off the flag.
                self.relayout_chrome_host();
            }
            KeyCommand::ToggleFocusMode => {
                // Enter with a default-length Pomodoro; re-baseline the timer so
                // the elapsed gap before the panel opened is not counted.
                self.focus.toggle(panels::focus_panel::DEFAULT_POMODORO_MIN);
                if self.focus.active {
                    let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                    self.focus.tick(now_ms);
                }
                self.request_redraw();
            }
            KeyCommand::BookmarkCurrentPage => {
                self.bookmark_current_page();
                self.request_redraw();
            }
            KeyCommand::SetTabContainer(container) => {
                let idx = self.tab_strip.active;
                self.set_tab_container(idx, container);
            }
            KeyCommand::DevConsole => {
                self.devtools_console.toggle();
                self.request_redraw();
            }
            KeyCommand::DevInspector => {
                self.dom_inspector.toggle();
                self.request_redraw();
            }
            KeyCommand::DevNetwork => {
                self.network_panel.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePrivacy => {
                self.privacy.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePip => {
                self.toggle_pip();
                self.request_redraw();
            }
            KeyCommand::ToggleReadLater => {
                self.read_later_panel.toggle();
                if self.read_later_panel.visible {
                    self.refresh_read_later();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleReaderView => {
                self.toggle_reader_view();
            }
            KeyCommand::ViewSource => {
                self.show_view_source();
            }
            KeyCommand::ToggleShortcuts => {
                self.shortcuts_panel.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePrint => {
                self.print_panel.toggle();
                self.request_redraw();
                // CC-10: see the matching comment on `ToggleCommandPalette`.
                self.relayout_chrome_host();
            }
            KeyCommand::ToggleCert => {
                let cert = self.cert_info.clone();
                self.cert_panel.toggle(cert);
                self.request_redraw();
                // CC-10: see the matching comment on `ToggleCommandPalette`.
                self.relayout_chrome_host();
            }
            KeyCommand::ZoomIn => {
                self.zoom_factor = zoom::zoom_in(self.zoom_factor);
                self.begin_zoom_preview();
            }
            KeyCommand::ZoomOut => {
                self.zoom_factor = zoom::zoom_out(self.zoom_factor);
                self.begin_zoom_preview();
            }
            KeyCommand::ZoomReset => {
                self.zoom_factor = zoom::zoom_reset();
                self.begin_zoom_preview();
            }
        }
    }

    /// Toggle the picture-in-picture window (task #21).
    ///
    /// When closing, just hides the card.  When opening, scans the current page
    /// layout for the first `<video>` element and embeds its `src` / `poster`;
    /// if the page has no video, the card opens with a placeholder so the user
    /// still gets feedback (and can drag / close it).
    /// Re-deliver media query changes to JS after accessibility prefs change.
    ///
    /// Called when the a11y panel closes so `prefers-reduced-motion` MQLs fire.
    fn deliver_a11y_media_changes(&self) {
        #[cfg(feature = "v8")]
        {
            let w = self.viewport_width_css();
            let h = self.viewport_height_css();
            let dark = if self.dark_mode { "true" } else { "false" };
            let rm = if self.a11y_store.reduced_motion() { "true" } else { "false" };
            // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
            // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!(
                    "if(typeof _lumen_deliver_media_changes==='function')\
                     _lumen_deliver_media_changes({w},{h},{dark},{rm});"
                ),
            );
        }
    }

    fn toggle_pip(&mut self) {
        if self.pip.active {
            self.pip.close();
            return;
        }
        let win_w = self.viewport_width_css();
        let win_h = self.viewport_height_css() + toolbar::CHROME_H;
        let (src, poster) = self
            .layout_box
            .as_ref()
            .and_then(find_video_source)
            .unwrap_or_default();
        let title = self.title.clone().unwrap_or_default();
        self.pip.open(src, poster, title, win_w, win_h);
    }

    /// Open the in-window overlay PiP card (the [`Self::pip`] panel) from current
    /// page state. Used as the fallback when a real OS PiP window cannot be
    /// created (no GPU surface, window-creation failure).
    fn open_pip_overlay(&mut self) {
        let win_w = self.viewport_width_css();
        let win_h = self.viewport_height_css() + toolbar::CHROME_H;
        let (src, poster) = self
            .layout_box
            .as_ref()
            .and_then(find_video_source)
            .unwrap_or_default();
        let title = self.title.clone().unwrap_or_default();
        self.pip.open(src, poster, title, win_w, win_h);
    }

    /// CC-7: open (or re-target) the real OS-level PiP window for `<video>` node
    /// `nid`. Resolves the element's border-box (for aspect ratio) and poster,
    /// then creates a separate always-on-top winit window with its own render
    /// backend. On any window/backend failure, falls back to [`Self::pip`] so the
    /// feature still works without multi-surface support.
    fn open_pip_os(&mut self, event_loop: &ActiveEventLoop, nid: u32) {
        use panels::pip_os_window::{pip_window_attributes, PipOsConfig};

        let (video_rect, poster_url) = self
            .layout_box
            .as_ref()
            .and_then(|root| forms::find_layout_box(root, NodeId::from_index(nid as usize)))
            .map(|lb| {
                let poster = match &lb.kind {
                    lumen_layout::BoxKind::Video { poster, .. } => poster.clone(),
                    _ => String::new(),
                };
                (lb.rect, poster)
            })
            .or_else(|| {
                // Node id has no box yet вЂ” fall back to the first <video>'s poster.
                self.layout_box.as_ref().and_then(|root| {
                    find_video_source(root)
                        .map(|(_, poster)| (Rect::new(0.0, 0.0, 16.0, 9.0), poster))
                })
            })
            .unwrap_or((Rect::new(0.0, 0.0, 16.0, 9.0), String::new()));

        let title = self
            .title
            .clone()
            .unwrap_or_else(|| "Picture-in-Picture".to_owned());
        let attrs = pip_window_attributes(&title, PipOsConfig::DEFAULT);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("PiP: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ OS-РѕРєРЅРѕ ({err}); fallback РЅР° overlay");
                self.open_pip_overlay();
                return;
            }
        };
        let renderer = match backend_factory::create_backend(
            window.clone(),
            INTER_FONT.to_vec(),
            self.target_color_space(),
        ) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("PiP: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ СЂРµРЅРґРµСЂ OS-РѕРєРЅР° ({err}); fallback РЅР° overlay");
                self.open_pip_overlay();
                return;
            }
        };

        let (win_w, win_h) = panels::pip_os_window::physical_to_logical(
            window.inner_size().width,
            window.inner_size().height,
            window.scale_factor() as f32,
        );
        self.pip_os = Some(PipOsWindow {
            window,
            renderer,
            poster_url,
            video_rect,
        });
        self.render_pip_os();
        self.notify_pip_window_resized(win_w, win_h);
    }

    /// P3-pip: open a real OS floating window for Document Picture-in-Picture
    /// (`documentPictureInPicture.requestWindow({width, height})`) вЂ” no
    /// `<video>` is involved, so the window shows a plain sized container
    /// (empty poster в†’ [`panels::pip_os_window::build_pip_content`] draws just
    /// the background fill). Forwarding the requesting document's actual DOM
    /// content into the window is a follow-up вЂ” see
    /// `docs/tasks/ph3-picture-in-picture.md`. Unlike [`Self::open_pip_os`]
    /// there is no video overlay to fall back to on window/backend failure вЂ”
    /// this Phase 0 slice just logs and gives up.
    fn open_pip_os_document(&mut self, event_loop: &ActiveEventLoop, width: f32, height: f32) {
        use panels::pip_os_window::{pip_window_attributes, PipOsConfig};

        let cfg = if width > 0.0 && height > 0.0 {
            PipOsConfig::sized(width, height)
        } else {
            PipOsConfig::DEFAULT
        };
        let title = self
            .title
            .clone()
            .unwrap_or_else(|| "Picture-in-Picture".to_owned());
        let attrs = pip_window_attributes(&title, cfg);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Document PiP: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ OS-РѕРєРЅРѕ ({err})");
                return;
            }
        };
        let renderer = match backend_factory::create_backend(
            window.clone(),
            INTER_FONT.to_vec(),
            self.target_color_space(),
        ) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Document PiP: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ СЂРµРЅРґРµСЂ OS-РѕРєРЅР° ({err})");
                return;
            }
        };

        self.pip_os = Some(PipOsWindow {
            window,
            renderer,
            poster_url: String::new(),
            video_rect: Rect::new(0.0, 0.0, cfg.width, cfg.height),
        });
        self.render_pip_os();
    }

    /// CC-7: tear down the OS PiP window. Releasing the last `Arc<Window>` makes
    /// winit destroy the OS window and free its GPU surface; the overlay fallback
    /// (if it was used instead) is cleared too.
    fn close_pip_os(&mut self) {
        self.pip_os = None;
        self.pip.close();
    }

    /// CC-7: redraw the OS PiP window with the forwarded `<video>` content вЂ”
    /// the poster letterboxed (`object-fit: contain`) into the floating window's
    /// current client area. No-op when no OS PiP window is open.
    fn render_pip_os(&mut self) {
        let Some(pip) = self.pip_os.as_mut() else {
            return;
        };
        let size = pip.window.inner_size();
        let scale = pip.window.scale_factor() as f32;
        let (win_w, win_h) =
            panels::pip_os_window::physical_to_logical(size.width, size.height, scale);
        let content = panels::pip_os_window::build_pip_content(
            pip.video_rect,
            &pip.poster_url,
            win_w,
            win_h,
        );
        if let Err(err) = pip.renderer.render(&[], &content, 0.0, 0.0) {
            eprintln!("PiP OS render error: {err:?}");
        }
    }

    /// P3-pip slice 5: notify JS of the OS PiP window's current CSS-pixel size
    /// via [`Self::notify_pip_window_resized`] вЂ” updates whichever
    /// `PictureInPictureWindow` is active (video or legacy Document PiP,
    /// both backed by [`Self::pip_os`]) and fires its `resize` event. No-op
    /// when no OS PiP window is open. Reads the window's own current size вЂ”
    /// use this from event handlers (e.g. `ScaleFactorChanged`) that don't
    /// already have a fresh logical size on hand; when one is already
    /// computed (e.g. `WindowEvent::Resized`), call
    /// [`Self::notify_pip_window_resized`] directly instead.
    #[cfg(feature = "v8")]
    fn deliver_pip_resize(&mut self) {
        let Some(pip) = self.pip_os.as_ref() else {
            return;
        };
        let size = pip.window.inner_size();
        let scale = pip.window.scale_factor() as f32;
        let (win_w, win_h) =
            panels::pip_os_window::physical_to_logical(size.width, size.height, scale);
        self.notify_pip_window_resized(win_w, win_h);
    }

    /// Push the OS PiP window's current logical size into JS via
    /// `_lumen_pip_deliver_resize` (`video_pip.rs`), so the page's
    /// `PictureInPictureWindow.width`/`.height` reflect the real floating
    /// window instead of the `(0, 0)` stub set at `requestPictureInPicture()`
    /// time, and its `resize` event fires when the user drags the window's
    /// edge. Called once right after the OS window is created and again on
    /// every `WindowEvent::Resized` вЂ” not on `ScaleFactorChanged`/
    /// `RedrawRequested`, which don't change the logical size delivered here.
    /// `route_eval_js` no-ops when no JS runtime is installed, so this is
    /// safe to call unconditionally regardless of the `v8` feature.
    fn notify_pip_window_resized(&mut self, win_w: f32, win_h: f32) {
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!(
                "if(typeof _lumen_pip_deliver_resize==='function')\
                 {{_lumen_pip_deliver_resize({win_w},{win_h});}}"
            ),
        );
    }

    /// Document Picture-in-Picture (slice 1): open the real OS-level floating
    /// window at the requested logical size. Mirrors [`Self::open_pip_os`]
    /// minus the `<video>` forwarding and the in-window overlay fallback вЂ” on
    /// window/backend creation failure the request is simply dropped (the JS
    /// `requestWindow()` promise already resolved with a `PictureInPictureWindow`
    /// whose `.document` stays a JS-only mock either way, see `document_pip.rs`).
    fn open_doc_pip_os(&mut self, event_loop: &ActiveEventLoop, width: u32, height: u32) {
        use panels::doc_pip_os_window::DocPipController;
        use panels::pip_os_window::{pip_window_attributes, PipOsConfig};

        let cfg = PipOsConfig {
            width: width as f32,
            height: height as f32,
            min_width: PipOsConfig::DEFAULT.min_width,
            min_height: PipOsConfig::DEFAULT.min_height,
        };
        let title = self
            .title
            .clone()
            .unwrap_or_else(|| "Picture-in-Picture".to_owned());
        let attrs = pip_window_attributes(&title, cfg);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Document PiP: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ OS-РѕРєРЅРѕ ({err})");
                self.doc_pip_controller = DocPipController::new();
                return;
            }
        };
        let renderer = match backend_factory::create_backend(
            window.clone(),
            INTER_FONT.to_vec(),
            self.target_color_space(),
        ) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Document PiP: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕР·РґР°С‚СЊ СЂРµРЅРґРµСЂ OS-РѕРєРЅР° ({err})");
                self.doc_pip_controller = DocPipController::new();
                return;
            }
        };

        let (win_w, win_h) = panels::pip_os_window::physical_to_logical(
            window.inner_size().width,
            window.inner_size().height,
            window.scale_factor() as f32,
        );
        self.doc_pip_os = Some(DocPipOsWindow { window, renderer, content_html: String::new() });
        self.render_doc_pip_os();
        self.notify_docpip_window_resized(win_w, win_h);
    }

    /// Document Picture-in-Picture (slice 1): tear down the OS floating window.
    /// Releasing the last `Arc<Window>` makes winit destroy the OS window and
    /// free its GPU surface.
    fn close_doc_pip_os(&mut self) {
        self.doc_pip_os = None;
    }

    /// Document Picture-in-Picture (slice 3): redraw the OS floating window.
    /// Background fill (`build_docpip_content`) first, then вЂ” if the page has
    /// appended anything to `pipWindow.document.body` вЂ” the moved subtree's
    /// last-known markup (`pip.content_html`) is re-parsed into a fresh
    /// detached [`lumen_dom::Document`], laid out at the window's own size
    /// against the main page's own author stylesheet (`self.layout_source`),
    /// and painted on top. No-op when no window is open. Known gap: images in
    /// the moved subtree don't render (this window's renderer has its own
    /// image cache, separate from the main page's).
    fn render_doc_pip_os(&mut self) {
        let Some(pip) = self.doc_pip_os.as_mut() else {
            return;
        };
        let size = pip.window.inner_size();
        let scale = pip.window.scale_factor() as f32;
        let (win_w, win_h) =
            panels::pip_os_window::physical_to_logical(size.width, size.height, scale);
        let mut content = panels::doc_pip_os_window::build_docpip_content(win_w, win_h);
        if !pip.content_html.is_empty() {
            let doc = lumen_html_parser::parse(&pip.content_html);
            let empty_sheet;
            let sheet = match self.layout_source.as_ref() {
                Some(src) => src.stylesheet.as_ref(),
                None => {
                    empty_sheet = lumen_css_parser::parse("");
                    &empty_sheet
                }
            };
            let layout = lumen_layout::layout(&doc, sheet, Size::new(win_w, win_h));
            content.extend(paint_ordered(&layout));
        }
        if let Err(err) = pip.renderer.render(&[], &content, 0.0, 0.0) {
            eprintln!("Document PiP OS render error: {err:?}");
        }
    }

    /// Push the OS Document PiP window's current logical size into JS via
    /// `_lumen_docpip_deliver_resize` (`document_pip.rs`), so
    /// `PictureInPictureWindow.width`/`.height` reflect the real floating
    /// window and its `resize` event fires when the user drags the window's
    /// edge. Called once right after the OS window is created and again on
    /// every `WindowEvent::Resized`, mirroring [`Self::notify_pip_window_resized`].
    fn notify_docpip_window_resized(&mut self, win_w: f32, win_h: f32) {
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!(
                "if(typeof _lumen_docpip_deliver_resize==='function')\
                 {{_lumen_docpip_deliver_resize({win_w},{win_h});}}"
            ),
        );
    }

    /// Commit the current navigation state to the JS side so that
    /// `window.navigation.entries()` and `currentEntry` reflect the truth.
    ///
    /// Builds a serialised JSON of `nav_back` + current + `nav_fwd` with the
    /// shell-assigned `nav_key` for each entry and pushes it via
    /// `_lumen_navigation_set_state`.
    ///
    /// BUG-352: also the single point every navigation path (full-document
    /// load, same-document popstate, JS-intercepted push/replace) funnels
    /// through once `self.source`/`self.display_url` has its final value вЂ”
    /// so it doubles as the trigger to refresh the engine-drawn chrome's
    /// `#omniInput` (`relayout_chrome_host`/`chrome_omnibox_value` reads
    /// `current_display_url()`). Without this, the omnibox only ever
    /// refreshed from the omnibox's own key handler (CC-7) вЂ” every other
    /// way the URL can change (a clicked link, `history.back()`/`forward()`,
    /// BiDi/MCP `navigate`, which is exactly `wptrunner`'s navigation model)
    /// left it showing whatever URL was on screen at the last keystroke,
    /// click or resize, indefinitely.
    fn commit_nav_state(&mut self) {
        fn state_value(raw: Option<&str>) -> serde_json::Value {
            match raw {
                Some(s) => serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_owned())),
                None => serde_json::Value::Null,
            }
        }
        let mut entries: Vec<serde_json::Value> = Vec::new();
        for e in &self.nav_back {
            entries.push(serde_json::json!({
                "url": e.source.url_str().unwrap_or(""),
                "key": e.nav_key,
                "id": format!("id-{}", e.nav_key.strip_prefix("nav-").unwrap_or("0")),
                "state": state_value(e.same_doc_state_json.as_deref()),
            }));
        }
        let cur_url = self.source.url_str().unwrap_or("");
        let cur_key = self.current_nav_key.clone();
        entries.push(serde_json::json!({
            "url": cur_url,
            "key": cur_key,
            "id": format!("id-{}", cur_key.strip_prefix("nav-").unwrap_or("0")),
            "state": state_value(Some(&self.current_history_state_json)),
        }));
        let idx = self.nav_back.len();
        for e in &self.nav_fwd {
            entries.push(serde_json::json!({
                "url": e.source.url_str().unwrap_or(""),
                "key": e.nav_key,
                "id": format!("id-{}", e.nav_key.strip_prefix("nav-").unwrap_or("0")),
                "state": state_value(e.same_doc_state_json.as_deref()),
            }));
        }
        let state = serde_json::json!({ "entries": entries, "index": idx });
        // The native binding takes a String argument, so the JSON text must
        // be embedded as a JS string literal (double encoding) вЂ” passing a
        // bare object literal makes the arg conversion fail and the state
        // silently never reaches the runtime.
        let Ok(json) = serde_json::to_string(&state) else { return };
        let Ok(quoted) = serde_json::to_string(&json) else { return };
        // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!("_lumen_navigation_set_state({quoted})"),
        );
        // BUG-352: see doc comment above вЂ” keeps the omnibox in sync with
        // every navigation, not just omnibox-driven ones. No-op off the
        // flag (`relayout_chrome_host` early-returns when `chrome_doc`/the
        // renderer aren't ready yet, e.g. the very first call before the
        // window exists).
        self.relayout_chrome_host();
    }

    fn fire_navigate_success(&self) {
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.fire_navigate_success();
        });
    }

    fn fire_navigate_error(&self) {
        // ADR-016 M2.2d: fire-and-forget void via route_task_js.
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.fire_navigate_error();
        });
    }

    fn fire_current_entry_change(&self) {
        // ADR-016 M2.2d: fire-and-forget void via route_task_js.
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.fire_current_entry_change();
        });
    }

    /// Apply a same-document `history.go(n)` destination whose containing
    /// document had to be (re)loaded first (see `pending_post_reload_traversal`):
    /// fires `popstate` with `state_json`, updates the address bar to
    /// `display_url`, and fires `currententrychange` вЂ” the same tail as the
    /// ordinary same-document branch in `navigate_back`/`navigate_forward`,
    /// just run once the correct document's JS runtime actually exists.
    fn apply_post_reload_traversal(&mut self, state_json: String, display_url: Option<String>) {
        self.current_history_state_json = state_json.clone();
        self.display_url = display_url.clone();
        let url = display_url.unwrap_or_default();
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_popstate(&state_json, &url);
        });
        self.fire_current_entry_change();
        self.request_redraw();
    }

    /// Whether the current page may be stored as a full bfcache freeze.
    ///
    /// `false` when the page has an open WebSocket/EventSource connection, a
    /// registered `unload`/`beforeunload` handler ([`PersistentJs::has_bfcache_freeze_blocker`]),
    /// or the response carried `Cache-Control: no-store` (HTML LS В§8.6).
    /// Ineligible pages fall back to the existing HTML-snapshot bfcache path
    /// (no regression).
    fn bfcache_eligible(&self) -> bool {
        let no_store = self
            .layout_source
            .as_ref()
            .is_some_and(|ls| ls.cache_control_no_store);
        if no_store {
            return false;
        }
        !route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.has_bfcache_freeze_blocker()
        })
        .unwrap_or(false)
    }

    /// Thaw a frozen page вЂ” restore DOM + stylesheet, reinstall a fresh JS runtime
    /// (heap resume gated on 10C.2), re-layout, restore scroll/title, fire
    /// pageshow(persisted=true). Returns false when DOM bytes fail to decode or
    /// the stylesheet was evicted (caller falls back to a normal reload).
    fn bfcache_thaw(&mut self, entry: &BfCacheEntry, frozen: &FrozenPage) -> bool {
        let url = entry.url.as_str();
        let Some(stylesheet) = self.frozen_styles.get(url).cloned() else {
            return false;
        };
        let Ok(doc) = Document::from_bytes(&frozen.dom_bytes) else {
            return false;
        };
        let doc_arc = Arc::new(Mutex::new(doc));
        self.layout_source = Some(LayoutSource {
            document: Arc::clone(&doc_arc),
            stylesheet: Arc::new(stylesheet),
            html_source: None,
            // The page was eligible for a full freeze (bfcache_eligible() was
            // true when it was stored), so it was not no-store at that point.
            cache_control_no_store: false,
            // BUG-743: the frozen entry keeps the parsed sheet, not the CSS
            // parts it was built from вЂ” nothing to rebuild a cascade out of.
            dynamic_css: None,
        });
        // Ph3 V8 migration S4.
        #[cfg(feature = "v8")]
        {
            match lumen_js::v8_runtime::V8JsRuntime::new() {
                Ok(mut rt) => {
                    // BUG-548 (S12b-G6): cookie-banner dismiss now wired for V8.
                    rt.set_cookie_banner_dismiss(self.cookie_banner_dismiss);
                    if self.deterministic.enabled {
                        rt.set_deterministic_mode(true, self.deterministic.rng_seed, self.deterministic.monotonic_clock);
                    }
                    let ls_store = self
                        .source
                        .origin_str()
                        .and_then(|o| self.ls_storage.get(&o).cloned());
                    // BUG-836: a page thawed out of the bfcache is a document of
                    // this tab like any other вЂ” it must see the tab's store.
                    let ss_store = self.source.origin_str().map(|o| {
                        Arc::clone(self.ss_storage.entry(o).or_insert_with(|| {
                            Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                        }))
                    });
                    if let Some(store) = ss_store {
                        rt = rt.with_session_storage(store);
                    }
                    let idb_backend = self.idb_dir.as_deref().and_then(|d| idb_store_for_url(url, Some(d)));
                    let fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> = None;
                    let ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> = None;
                    let sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>> = None;
                    let sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>> = None;
                    let cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>> = None;
                    if let Err(e) = rt.install_dom(
                        Arc::clone(&doc_arc),
                        url,
                        fetch_provider,
                        ws_provider,
                        sse_provider,
                        ls_store,
                        idb_backend,
                        sw_backend,
                        cache_backend,
                        None,
                        false,
                    ) {
                        eprintln!("bfcache thaw: JS DOM init failed: {e}");
                    }
                    self.set_js_ctx(Some(Arc::new(V8PersistentJs { rt }) as Arc<dyn PersistentJs>));
                }
                Err(e) => {
                    eprintln!("bfcache thaw: V8 init failed: {e}");
                    self.set_js_ctx(None);
                }
            }
        }
        #[cfg(not(feature = "v8"))]
        {
            self.set_js_ctx(None);
        }
        // ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёРј РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅС‹Р№ (РёР»Рё СЃР±СЂРѕС€РµРЅРЅС‹Р№) С…СЌРЅРґР» + DOM
        // РІ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє РїРѕСЃР»Рµ bfcache-thaw.
        self.sync_engine_js_state();
        self.relayout();
        self.scroll_x = entry.scroll_x;
        self.scroll_y = entry.scroll_y;
        self.title = entry.title.clone();
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
        }
        // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            "_lumen_fire_page_lifecycle('pageshow', true)".to_string(),
        );
        self.request_redraw();
        self.commit_nav_state();
        true
    }

    /// Park the current page whole вЂ” JS runtime included вЂ” so a later
    /// back/forward navigation can restore a *live* document (BUG-835).
    ///
    /// Only the handles are cloned: `js_ctx` and `layout_source` stay in place
    /// until the incoming navigation replaces them, so nothing about the page
    /// being navigated away from changes here. From the moment the shell swaps
    /// in the next page's handle, the parked runtime stops being pumped вЂ”
    /// `route_task_js`/`route_query_js` reach only the active one вЂ” which is
    /// what pauses its timers and rAF callbacks for the duration of the park.
    ///
    /// Returns `false` (and parks nothing) for a page with no JS runtime or no
    /// layout source; those go down the frozen-DOM path instead, where
    /// reinstalling a fresh runtime loses nothing.
    fn park_current_page(&mut self) -> bool {
        let Some(url) = self.source.url_str().map(str::to_owned) else {
            return false;
        };
        let Some(js) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            Arc::clone,
        ) else {
            return false;
        };
        let Some(ls) = self.layout_source.as_ref() else {
            return false;
        };
        let parked = ParkedPage {
            js,
            document: Arc::clone(&ls.document),
            stylesheet: Arc::clone(&ls.stylesheet),
            html_source: ls.html_source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            title: self.title.clone(),
        };
        // One entry per URL: re-parking the same page replaces the older copy.
        self.parked_pages.retain(|(u, _)| *u != url);
        self.parked_pages.push((url, parked));
        while self.parked_pages.len() > PARKED_PAGES_MAX {
            self.parked_pages.remove(0);
        }
        true
    }

    /// Whether a live page is parked for `url` вЂ” see [`Self::park_current_page`].
    fn has_parked_page(&self, url: &str) -> bool {
        self.parked_pages.iter().any(|(u, _)| u == url)
    }

    /// Restore a page parked by [`Self::park_current_page`]: put its DOM,
    /// stylesheet and JS runtime back into the active slot, re-lay out and fire
    /// `pageshow(persisted=true)`.
    ///
    /// Unlike [`Self::bfcache_thaw`] the runtime is the page's own, so its
    /// timers, listeners and closures resume exactly where the park left them.
    /// The caller must have set `self.source` to the page being restored first вЂ”
    /// `relayout`/`commit_nav_state` read it.
    fn restore_parked_page(&mut self, url: &str) -> bool {
        let Some(pos) = self.parked_pages.iter().position(|(u, _)| u == url) else {
            return false;
        };
        let (_, parked) = self.parked_pages.remove(pos);
        // Same order as `apply_loaded_page`: drop the outgoing handle before the
        // layout source it shares a `Document` with, then install the new pair.
        self.set_js_ctx(None);
        self.layout_source = Some(LayoutSource {
            document: Arc::clone(&parked.document),
            stylesheet: parked.stylesheet,
            html_source: parked.html_source,
            // Restored pages have no live response headers; treated as cacheable,
            // exactly as `bfcache_thaw` does.
            cache_control_no_store: false,
            // BUG-743: the CSS parts the sheet was built from are not kept.
            dynamic_css: None,
        });
        self.set_js_ctx(Some(parked.js));
        self.sync_engine_js_state();
        self.relayout();
        self.scroll_x = parked.scroll_x;
        self.scroll_y = parked.scroll_y;
        self.title = parked.title.clone();
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
        }
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            "_lumen_fire_page_lifecycle('pageshow', true)".to_string(),
        );
        self.request_redraw();
        self.commit_nav_state();
        true
    }

    /// РЎРѕС…СЂР°РЅРёС‚СЊ С‚РµРєСѓС‰СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ РІ bfcache Рё СЃС‚РµРє РЅР°РІРёРіР°С†РёРё,
    /// Р·Р°С‚РµРј Р·Р°РіСЂСѓР·РёС‚СЊ `source` РєР°Рє РЅРѕРІСѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    /// РћС‡РёС‰Р°РµС‚ `nav_fwd` (Р°РЅР°Р»РѕРі Р±СЂР°СѓР·РµСЂР° РїСЂРё РЅР°РІРёРіР°С†РёРё РІРїРµСЂС‘Рґ РёР· СЃРµСЂРµРґРёРЅС‹ РёСЃС‚РѕСЂРёРё).
    /// РЎРѕС…СЂР°РЅРёС‚СЊ С‚РµРєСѓС‰СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ РІ bfcache Рё СЃС‚РµРє РЅР°РІРёРіР°С†РёРё,
    /// Р·Р°С‚РµРј Р·Р°РіСЂСѓР·РёС‚СЊ `source` РєР°Рє РЅРѕРІСѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    /// РћС‡РёС‰Р°РµС‚ `nav_fwd` (Р°РЅР°Р»РѕРі Р±СЂР°СѓР·РµСЂР° РїСЂРё РЅР°РІРёРіР°С†РёРё РІРїРµСЂС‘Рґ РёР· СЃРµСЂРµРґРёРЅС‹ РёСЃС‚РѕСЂРёРё).
    fn navigate_to(&mut self, source: PageSource) {
        // ADR-016 M2.2c-2d: nav dispatch (fire-and-forget) С‡РµСЂРµР· `route_task_js` +
        // read-after-eval intercept-С‡С‚РµРЅРёРµ С‡РµСЂРµР· `route_query_js`. РџРѕРґ С„Р»Р°РіРѕРј
        // (`LUMEN_ENGINE_THREAD=1`) dispatch СѓС…РѕРґРёС‚ off-UI-thread РѕРґРЅРёРј `task`, Р°
        // Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ** РЅРµРіРѕ вЂ” read-after-eval
        // РїРѕСЂСЏРґРѕРє СЃРѕС…СЂР°РЅС‘РЅ; Р±РµР· С„Р»Р°РіР° вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
        {
            let url = source.url_str().unwrap_or("").to_string();
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.eval_js(&format!("_lumen_dispatch_navigate('push', '{url}', true, false)"));
            });
        }
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Push {
                    url: source.url_str().unwrap_or("").to_string(),
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Push { handler_started, .. }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        click_log::log_nav(&source.describe());
        // PERF-6: remember the page under navigation so a panic on any thread can
        // be attributed to it in the health journal.
        health_log::set_current_url(&source.describe());
        self.hint.close();
        // BUG-835: a page that has a JS runtime is parked *whole* вЂ” the runtime
        // goes into `parked_pages` alive, so back/forward restores a document
        // whose timers, listeners and closures still exist. The frozen-DOM path
        // below stays for pages without JS, where reinstalling a fresh runtime
        // over the restored DOM loses nothing.
        let bfcache_eligible = self.bfcache_eligible();
        let mut persisted = bfcache_eligible && self.park_current_page();
        // Phase-3 freeze: serialize live DOM arena + shell-side stylesheet.
        // JS heap suspend is gated on 10C.2, so event handlers are NOT retained.
        // The thaw path reinstalls a fresh runtime over the restored DOM.
        if !persisted
            && bfcache_eligible
            && let Some(ref ls) = self.layout_source
            && let Some(url) = self.source.url_str()
            && let Ok(guard) = ls.document.lock()
            && let Ok(dom_bytes) = guard.to_bytes()
        {
            drop(guard);
            // `frozen_styles` keeps an owned `Stylesheet` (cold freeze path), so
            // deep-clone out of the `Arc` snapshot here.
            self.frozen_styles.insert(url.to_owned(), (*ls.stylesheet).clone());
            // Lazy prune: if we have too many stylesheets, drop those whose
            // corresponding bfcache entries are no longer frozen.
            if self.frozen_styles.len() > 32 {
                let bf = &self.bfcache;
                self.frozen_styles.retain(|k, _| bf.has_frozen(k));
            }
            self.bfcache.store(BfCacheEntry {
                url: url.to_owned(),
                payload: BfCachePayload::Frozen(FrozenPage {
                    dom_bytes,
                    js_heap: Vec::new(),
                    css_source: String::new(),
                }),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                title: self.title.clone(),
            });
            persisted = true;
        }
        // Fallback: store an HTML snapshot if freeze was not possible.
        //
        // BUG-834: this does NOT make the document salvageable. Coming back
        // re-parses the page from its source, so the document object, its
        // listeners, timers and closures are all gone вЂ” HTML LS В§7.4.6 calls
        // that a discarded document, which must hear `unload` and must report
        // `pagehide.persisted === false`. Only the parked and frozen paths above
        // retain anything, so `persisted` is deliberately left untouched here
        // (it used to be raised, which is why an ordinary link navigation
        // reported `persisted=true` and swallowed `unload`).
        if !persisted
            && let Some(ref ls) = self.layout_source
            && let Some(ref html) = ls.html_source
            && let Some(url) = self.source.url_str()
        {
            self.bfcache.store(BfCacheEntry {
                url: url.to_owned(),
                payload: BfCachePayload::HtmlSnapshot(html.clone()),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                title: self.title.clone(),
            });
        }
        // HTML LS В§7.4.5вЂ“В§7.4.6: run the full unload sequence on the outgoing
        // page вЂ” `beforeunload`, then `pagehide` в†’ `visibilityState = 'hidden'`
        // в†’ `unload`. `persisted = true` signals the document was retained
        // (parked/frozen above), which is also its salvageable state: such a
        // page gets `pagehide` but no `unload`, and its listeners can skip
        // teardown they would redo on `pageshow`. BUG-834.
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_beforeunload();
            j.unload_document(persisted);
        });
        // Push current page to back stack (full-doc entry: no same_doc_state_json).
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: None,
            same_doc_state_json: None,
            nav_key: self.current_nav_key.clone(),
        });
        // New navigation invalidates forward history and resets same-doc state.
        self.nav_fwd.clear();
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        // Assign a fresh key to the incoming page before it becomes current.
        self.nav_key_counter += 1;
        self.current_nav_key = format!("nav-{}", self.nav_key_counter);
        // Load new page.
        self.source = source;
        self.commit_nav_state();
        self.reload();
    }

    /// РџРµСЂРµР№С‚Рё РЅР° `source`, Р·Р°РјРµРЅСЏСЏ С‚РµРєСѓС‰СѓСЋ Р·Р°РїРёСЃСЊ РёСЃС‚РѕСЂРёРё (Р±РµР· push РІ back-stack).
    /// РђРЅР°Р»РѕРі `history.replaceState` / `location.replace()` РІ Р±СЂР°СѓР·РµСЂРµ.
    fn navigate_replace(&mut self, source: PageSource) {
        // ADR-016 M2.2c-2d: СЃРј. `navigate_to` вЂ” dispatch С‡РµСЂРµР· `route_task_js`,
        // intercept-С‡С‚РµРЅРёРµ С‡РµСЂРµР· `route_query_js` (read-after-eval РїРѕСЂСЏРґРѕРє РїРѕРґ С„Р»Р°РіРѕРј).
        {
            let url = source.url_str().unwrap_or("").to_string();
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.eval_js(&format!("_lumen_dispatch_navigate('replace', '{url}', true, false)"));
            });
        }
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Replace {
                    url: source.url_str().unwrap_or("").to_string(),
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Replace { handler_started, .. }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        // New navigation invalidates forward history but does NOT push to back stack.
        self.nav_fwd.clear();
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        self.source = source;
        // BUG-352: `navigate_replace` doesn't route through `commit_nav_state`
        // (that call updates JS's `window.navigation`, not needed for a plain
        // `location.replace()`-style navigation), so it needs its own chrome
        // refresh вЂ” see `commit_nav_state`'s doc comment for why this matters.
        self.relayout_chrome_host();
        self.reload();
    }

    /// РџРµСЂРµР№С‚Рё РЅР° РїСЂРµРґС‹РґСѓС‰СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ РІ РёСЃС‚РѕСЂРёРё (Alt+Left).
    fn navigate_back(&mut self) {
        // ADR-016 M2.2c-2d: СЃРј. `navigate_to` вЂ” dispatch С‡РµСЂРµР· `route_task_js`,
        // intercept-С‡С‚РµРЅРёРµ С‡РµСЂРµР· `route_query_js` (read-after-eval РїРѕСЂСЏРґРѕРє РїРѕРґ С„Р»Р°РіРѕРј).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.eval_js("_lumen_dispatch_navigate('traverse', '', true, false)");
        });
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Back {
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Back { handler_started }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        let Some(prev) = self.nav_back.pop() else { return };
        let crossed_document = std::mem::take(&mut self.traversal_crossed_document);

        let mut post_reload_traversal = None;
        if let Some(state_json) = prev.same_doc_state_json {
            if !crossed_document {
                // Same-document navigation: fire popstate, update address bar, don't reload.
                // Push current same-doc state to forward stack so Alt+Right restores it.
                let cur_display = self.display_url.take();
                let cur_state = std::mem::replace(
                    &mut self.current_history_state_json,
                    state_json.clone(),
                );
                self.nav_fwd.push(NavEntry {
                    source: self.source.clone(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    display_url: cur_display,
                    same_doc_state_json: Some(cur_state),
                    nav_key: self.current_nav_key.clone(),
                });
                let url = prev.display_url.unwrap_or_default();
                self.display_url = if url.is_empty() { None } else { Some(url.clone()) };
                // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
                // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.fire_popstate(&state_json, &url);
                });
                self.fire_current_entry_change();
                self.request_redraw();
                self.current_nav_key = prev.nav_key;
                self.source = prev.source;
                self.commit_nav_state();
                return;
            }
            // Cross-document unification: the multi-step shuffle passed through
            // a full-document entry before landing here, so the loaded document
            // is not the one this same-document entry belongs to. Defer the
            // popstate/URL update until the correct document (reloaded below,
            // or thawed from bfcache) actually finishes loading.
            post_reload_traversal = Some((state_json, prev.display_url.clone()));
        }

        // Full-document navigation: restore page and reload.
        // HTML LS В§7.4.5вЂ“В§7.4.6: run the full unload sequence on the current
        // page вЂ” `beforeunload`, then `pagehide` в†’ `visibilityState = 'hidden'`
        // в†’ `unload`. BUG-834: the outgoing document is retained only on the
        // parked-page branch below, so that same condition IS its salvageable
        // state and decides both the `persisted` flag and whether `unload`
        // fires at all. Computed here (before the sequence) rather than read
        // back from `park_current_page`, because the events must reach the
        // page while it is still current.
        let outgoing_parkable = prev
            .source
            .url_str()
            .is_some_and(|u| self.has_parked_page(u))
            && self.bfcache_eligible();
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_beforeunload();
            j.unload_document(outgoing_parkable);
        });
        // Push current page to forward stack.
        let cur_display = self.display_url.take();
        let cur_state = std::mem::replace(
            &mut self.current_history_state_json,
            String::from("null"),
        );
        self.nav_fwd.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: cur_display,

            same_doc_state_json: if cur_state != "null" { Some(cur_state) } else { None },
            nav_key: self.current_nav_key.clone(),
        });
        // BUG-835: a live parked page wins over every frozen/snapshot payload вЂ”
        // it is the only restore path that brings the document's JS state back.
        if let Some(url) = prev.source.url_str().map(str::to_owned)
            && self.has_parked_page(&url)
        {
            // Park the document being left as well, so Forward restores it alive
            // too instead of reloading it from scratch. Eligibility was already
            // resolved above as `outgoing_parkable` вЂ” re-querying it here would
            // let it disagree with the `persisted` flag just reported to the page.
            if outgoing_parkable {
                self.park_current_page();
            }
            self.source = prev.source.clone();
            self.current_nav_key = prev.nav_key.clone();
            if self.restore_parked_page(&url) {
                if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                    self.apply_post_reload_traversal(state_json, display_url);
                }
                return;
            }
        }
        // Try bfcache first: a Frozen payload thaws in place (no reload); an
        // HtmlSnapshot falls back to the existing re-parse path.
        let restored_scroll = if let Some(url) = prev.source.url_str() {
            if let Some(entry) = self.bfcache.retrieve(url).cloned() {
                match entry.payload {
                    BfCachePayload::Frozen(ref frozen) => {
                        self.source = prev.source.clone();
                        self.current_nav_key = prev.nav_key.clone();
                        if self.bfcache_thaw(&entry, frozen) {
                            if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                                self.apply_post_reload_traversal(state_json, display_url);
                            }
                            return;
                        }
                        // Thaw failed (stylesheet evicted / DOM decode error):
                        // fall through to a normal reload of the previous source.
                        None
                    }
                    BfCachePayload::HtmlSnapshot(ref html) => {
                        let base_url = url.to_owned();
                        self.source = PageSource::Snapshot { html: html.clone(), base_url };
                        // Restored from bfcache в†’ the next `pageshow` is `persisted=true`.
                        self.pending_pageshow_persisted = true;
                        Some((entry.scroll_x, entry.scroll_y))
                    }
                }
            } else {
                self.source = prev.source;
                None
            }
        } else {
            self.source = prev.source;
            None
        };
        // Previous entry becomes the new current: preserve its nav key.
        self.current_nav_key = prev.nav_key;
        // Restore scroll position from bfcache (or from nav entry if no bfcache hit).
        // U-1: reload() is now asynchronous (the page resets scroll at LoadDone),
        // so stash the offset for `apply_loaded_page` to apply instead of setting
        // it here вЂ” a direct assignment would be clobbered when LoadDone arrives.
        let (sx, sy) = restored_scroll.unwrap_or((prev.scroll_x, prev.scroll_y));
        self.pending_restore_scroll = Some((sx, sy));
        if let Some(traversal) = post_reload_traversal {
            self.pending_post_reload_traversal = Some(traversal);
        }
        self.reload();
        if let Some(w) = self.window.as_ref() { w.request_redraw(); }
        self.commit_nav_state();
    }

    /// РџРµСЂРµР№С‚Рё РЅР° СЃР»РµРґСѓСЋС‰СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ РІ РёСЃС‚РѕСЂРёРё (Alt+Right).
    fn navigate_forward(&mut self) {
        // ADR-016 M2.2c-2d: СЃРј. `navigate_to` вЂ” dispatch С‡РµСЂРµР· `route_task_js`,
        // intercept-С‡С‚РµРЅРёРµ С‡РµСЂРµР· `route_query_js` (read-after-eval РїРѕСЂСЏРґРѕРє РїРѕРґ С„Р»Р°РіРѕРј).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.eval_js("_lumen_dispatch_navigate('traverse', '', true, false)");
        });
        if let Some(intercept) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_nav_intercept_result(),
        ) {
            if let Some(&(true, false)) = intercept.last() {
                self.pending_intercepted = Some(PendingIntercepted::Forward {
                    handler_started: false,
                });
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                    j.eval_js("_lumen_run_navigate_handler()");
                });
                if let Some(PendingIntercepted::Forward { handler_started }) =
                    self.pending_intercepted.as_mut()
                {
                    *handler_started = true;
                }
                return;
            }
            if let Some(&(false, true)) = intercept.last() {
                self.fire_navigate_error();
                return;
            }
        }
        let Some(next) = self.nav_fwd.pop() else { return };
        let crossed_document = std::mem::take(&mut self.traversal_crossed_document);

        let mut post_reload_traversal = None;
        if let Some(state_json) = next.same_doc_state_json {
            if !crossed_document {
                // Same-document forward navigation: fire popstate, update address bar.
                let cur_display = self.display_url.take();
                let cur_state = std::mem::replace(
                    &mut self.current_history_state_json,
                    state_json.clone(),
                );
                self.nav_back.push(NavEntry {
                    source: self.source.clone(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    display_url: cur_display,
                    same_doc_state_json: Some(cur_state),
                    nav_key: self.current_nav_key.clone(),
                });
                let url = next.display_url.unwrap_or_default();
                self.display_url = if url.is_empty() { None } else { Some(url.clone()) };
                // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
                // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                    j.fire_popstate(&state_json, &url);
                });
                self.fire_current_entry_change();
                self.request_redraw();
                self.current_nav_key = next.nav_key;
                self.source = next.source;
                self.commit_nav_state();
                return;
            }
            // Cross-document unification: see `navigate_back`.
            post_reload_traversal = Some((state_json, next.display_url.clone()));
        }

        // Full-document forward navigation.
        // HTML LS В§7.4.5вЂ“В§7.4.6: mirror of `navigate_back` вЂ” the full unload
        // sequence, with the parked-page branch below as the salvageable state
        // (BUG-834).
        let outgoing_parkable = next
            .source
            .url_str()
            .is_some_and(|u| self.has_parked_page(u))
            && self.bfcache_eligible();
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_beforeunload();
            j.unload_document(outgoing_parkable);
        });
        let cur_display = self.display_url.take();
        let cur_state = std::mem::replace(
            &mut self.current_history_state_json,
            String::from("null"),
        );
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: cur_display,
            same_doc_state_json: if cur_state != "null" { Some(cur_state) } else { None },
            nav_key: self.current_nav_key.clone(),
        });
        // BUG-835: mirror of `navigate_back` вЂ” a live parked page wins.
        if let Some(url) = next.source.url_str().map(str::to_owned)
            && self.has_parked_page(&url)
        {
            // See `navigate_back`: eligibility is `outgoing_parkable`, resolved
            // before the unload sequence so it cannot disagree with `persisted`.
            if outgoing_parkable {
                self.park_current_page();
            }
            self.source = next.source.clone();
            self.current_nav_key = next.nav_key.clone();
            if self.restore_parked_page(&url) {
                if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                    self.apply_post_reload_traversal(state_json, display_url);
                }
                return;
            }
        }
        // Try bfcache first: a Frozen payload thaws in place (no reload); an
        // HtmlSnapshot falls back to the existing re-parse path.
        let restored_scroll = if let Some(url) = next.source.url_str() {
            if let Some(entry) = self.bfcache.retrieve(url).cloned() {
                match entry.payload {
                    BfCachePayload::Frozen(ref frozen) => {
                        self.source = next.source.clone();
                        self.current_nav_key = next.nav_key.clone();
                        if self.bfcache_thaw(&entry, frozen) {
                            if let Some((state_json, display_url)) = post_reload_traversal.clone() {
                                self.apply_post_reload_traversal(state_json, display_url);
                            }
                            return;
                        }
                        // Thaw failed (stylesheet evicted / DOM decode error):
                        // fall through to a normal reload of the next source.
                        None
                    }
                    BfCachePayload::HtmlSnapshot(ref html) => {
                        let base_url = url.to_owned();
                        self.source = PageSource::Snapshot { html: html.clone(), base_url };
                        // Restored from bfcache в†’ the next `pageshow` is `persisted=true`.
                        self.pending_pageshow_persisted = true;
                        Some((entry.scroll_x, entry.scroll_y))
                    }
                }
            } else {
                self.source = next.source;
                None
            }
        } else {
            self.source = next.source;
            None
        };
        // Forward entry becomes the new current: preserve its nav key.
        self.current_nav_key = next.nav_key;
        // U-1: stash scroll offset for `apply_loaded_page` (async reload вЂ” see
        // navigate_back for rationale).
        let (sx, sy) = restored_scroll.unwrap_or((next.scroll_x, next.scroll_y));
        self.pending_restore_scroll = Some((sx, sy));
        if let Some(traversal) = post_reload_traversal {
            self.pending_post_reload_traversal = Some(traversal);
        }
        self.reload();
        self.commit_nav_state();
        if let Some(w) = self.window.as_ref() { w.request_redraw(); }
    }

    /// Traverse the session history by `delta` (negative = back, positive =
    /// forward) as a SINGLE logical step (HTML LS history traversal): the
    /// intermediate entries of a multi-step `history.go(n)` are skipped without
    /// rendering, and only the destination entry fires `popstate` (same-document)
    /// or reloads (full-document) вЂ” exactly one observable event, delivered by the
    /// final `navigate_back` / `navigate_forward`. An out-of-range `delta` is a
    /// no-op (per spec, a step outside the history range does nothing).
    ///
    /// This is the single authority for JS-initiated traversal: `history.go` /
    /// `back` / `forward` queue a delta that the shell drains into this method, so
    /// the real `nav_back` / `nav_fwd` stacks (not the JS read-cache mirror) decide
    /// what actually happens вЂ” eliminating the multi-step `go` drift where the JS
    /// mirror moved its cursor but the shell stacks did not.
    ///
    /// Cross-document unification: if the shuffle passes through a
    /// full-document entry en route to a same-document destination, the
    /// currently loaded document is stale relative to that destination вЂ”
    /// `self.traversal_crossed_document` flags this for `navigate_back`/
    /// `navigate_forward`, which reload the correct document first and defer
    /// the `popstate`/URL update via `pending_post_reload_traversal`.
    #[cfg_attr(not(feature = "v8"), allow(dead_code))]
    fn navigate_by(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let back = delta < 0;
        let steps = delta.unsigned_abs() as usize;

        // Out-of-range traversal is a no-op (the shell stacks are authoritative).
        if back && self.nav_back.len() < steps {
            return;
        }
        if !back && self.nav_fwd.len() < steps {
            return;
        }

        // Skip the intermediate entries without rendering: shuttle the current
        // entry and each crossed entry onto the opposite stack, leaving `self`
        // positioned at the entry just before the destination. The final
        // navigate_back/forward then performs the one real (popstate/reload) hop.
        let mut crossed_document = false;
        if steps > 1 {
            let cur = NavEntry {
                source: self.source.clone(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                display_url: self.display_url.clone(),
                same_doc_state_json: if self.current_history_state_json != "null" {
                    Some(self.current_history_state_json.clone())
                } else {
                    None
                },
                nav_key: self.current_nav_key.clone(),
            };
            let (cur, crossed) = NavEntry::shift_multi_step(
                &mut self.nav_back,
                &mut self.nav_fwd,
                cur,
                steps,
                back,
            );
            crossed_document = crossed;
            self.source = cur.source;
            self.scroll_x = cur.scroll_x;
            self.scroll_y = cur.scroll_y;
            self.display_url = cur.display_url;
            self.current_history_state_json =
                cur.same_doc_state_json.unwrap_or_else(|| "null".to_string());
        }

        self.traversal_crossed_document = crossed_document;
        if back {
            self.navigate_back();
        } else {
            self.navigate_forward();
        }
    }

    /// Compute the delta in history steps needed to reach `key`.
    ///
    /// `nav_back` and `nav_fwd` are stacks where the *last* element is the
    /// nearest entry relative to the current one.  Returns a negative delta
    /// when the key is found in `nav_back` (steps back) and a positive delta
    /// when it is found in `nav_fwd` (steps forward).  `len - pos` counts how
    /// many entries lie between the chosen entry and the top of its stack.
    fn key_traversal_delta(nav_back: &[NavEntry], nav_fwd: &[NavEntry], key: &str) -> Option<i32> {
        if let Some(pos) = nav_back.iter().rposition(|e| e.nav_key == key) {
            Some(-((nav_back.len() - pos) as i32))
        } else {
            nav_fwd
                .iter()
                .rposition(|e| e.nav_key == key)
                .map(|pos| (nav_fwd.len() - pos) as i32)
        }
    }

    /// Perform a history traversal to the entry identified by `key`.
    ///
    /// Backs the JS `navigation.traverseTo(key)` call.  If `key` matches the
    /// current entry no traversal occurs.  Unknown keys are silently ignored
    /// per the Navigation API specification.
    #[cfg_attr(not(feature = "v8"), allow(dead_code))]
    fn navigate_to_key(&mut self, key: &str) {
        if key == self.current_nav_key {
            return;
        }
        if let Some(delta) = Self::key_traversal_delta(&self.nav_back, &self.nav_fwd, key) {
            self.navigate_by(delta);
        }
    }

    /// Execute a gesture action produced by the right-button drag recognizer.
    fn execute_gesture_action(
        &mut self,
        action: input::gesture::GestureAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use input::gesture::GestureAction;
        match action {
            GestureAction::NavigateBack => self.navigate_back(),
            GestureAction::NavigateForward => self.navigate_forward(),
            GestureAction::NewTab => self.open_new_tab(),
            GestureAction::CloseTab => {
                let idx = self.tab_strip.active;
                self.close_tab(idx, event_loop);
            }
        }
    }

    fn handle_ime(&mut self, ime: &Ime) {
        use lumen_core::event::{Event, TabId};
        let tab_id = TabId(0);
        match ime {
            Ime::Enabled => {
                // РќРµ РґРёСЃРїР°С‚С‡РёРј compositionstart СЃСЂР°Р·Сѓ вЂ” Р¶РґС‘Рј РїРµСЂРІС‹Р№ Preedit
                // СЃ С‚РµРєСЃС‚РѕРј (Р±СЂР°СѓР·РµСЂС‹ С‚Р°Рє Р¶Рµ: СЃРѕР±С‹С‚РёРµ С‚РѕР»СЊРєРѕ РєРѕРіРґР° РµСЃС‚СЊ РґР°РЅРЅС‹Рµ).
            }
            Ime::Preedit(text, _cursor) if text.is_empty() => {
                // РџСѓСЃС‚РѕР№ preedit = РєРѕРЅРµС† composition Р±РµР· Commit (РѕС‚РјРµРЅР°).
                if self.ime_composing.take().is_some() {
                    self.event_sink
                        .emit(&Event::ImeCompositionEnded { tab_id, data: String::new() });
                }
            }
            Ime::Preedit(text, _cursor) => {
                if self.ime_composing.is_none() {
                    // РџРµСЂРІС‹Р№ РЅРµРїСѓСЃС‚РѕР№ preedit вЂ” РЅР°С‡Р°Р»Рѕ composition.
                    self.event_sink
                        .emit(&Event::ImeCompositionStarted { tab_id });
                }
                self.ime_composing = Some(text.clone());
                self.event_sink.emit(&Event::ImeCompositionUpdated {
                    tab_id,
                    data: text.clone(),
                });
            }
            Ime::Commit(text) => {
                // Commit РїСЂРёС…РѕРґРёС‚ РїРѕСЃР»Рµ РїСѓСЃС‚РѕРіРѕ Preedit (winit РіР°СЂР°РЅС‚РёСЂСѓРµС‚),
                // РЅРѕ РЅР° СЃР»СѓС‡Р°Р№ РµСЃР»Рё РЅРµС‚ вЂ” СЃР±СЂР°СЃС‹РІР°РµРј composing СЃР°РјРё.
                self.ime_composing = None;
                self.event_sink.emit(&Event::ImeCompositionEnded {
                    tab_id,
                    data: text.clone(),
                });
            }
            Ime::Disabled => {
                // IME РґРµР°РєС‚РёРІРёСЂРѕРІР°РЅ. Р•СЃР»Рё composition Р±С‹Р»Р° РѕС‚РєСЂС‹С‚Р° вЂ” Р·Р°РєСЂС‹РІР°РµРј.
                if self.ime_composing.take().is_some() {
                    self.event_sink
                        .emit(&Event::ImeCompositionEnded { tab_id, data: String::new() });
                }
            }
        }
    }

    fn handle_address_bar_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let _ = event_loop;
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.address_bar.close();
                self.request_redraw();
            }
            KeyCode::Enter if !key_event.repeat => {
                self.address_bar.commit();
                if let Some(value) = self.address_bar.take_commit() {
                    self.handle_omnibox_commit(value);
                }
            }
            KeyCode::ArrowDown if !key_event.repeat => {
                self.address_bar.select_next();
                self.request_redraw();
            }
            KeyCode::ArrowUp if !key_event.repeat => {
                self.address_bar.select_prev();
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.address_bar.backspace();
                let sugg = self.query_omnibox_suggestions();
                self.address_bar.set_suggestions(sugg);
                self.request_redraw();
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                {
                    self.address_bar.append_str(text);
                    let sugg = self.query_omnibox_suggestions();
                    self.address_bar.set_suggestions(sugg);
                    self.request_redraw();
                }
            }
        }
        // CC-7 (docs/tasks/p1-css-chrome.md): `#omniInput`'s engine-rendered
        // value/warning/caret (`Self::chrome_model_snapshot`,
        // `Self::chrome_omni_input_rect`) is baked into `self.chrome_layout`
        // at `relayout_chrome_host` time, not recomputed every
        // `RedrawRequested` вЂ” every branch above mutates `self.address_bar`
        // (text, selection, or open/closed), so without this call the
        // on-screen field would keep showing stale text while the user
        // types. No-op off the flag (`Self::relayout_chrome_host` early-
        // returns when `chrome_doc` is `None`).
        self.relayout_chrome_host();
    }

    /// Process a committed omnibox value: resolve aliases, then navigate or act.
    ///
    /// Order: `sidebar:` prefix в†’ bang aliases (`!g`) в†’ `@notes` / `@read-later`
    /// в†’ record in search_history в†’ plain navigate.
    /// Build a fresh `about:newtab` [`PageSource::Static`] from pinned tiles
    /// (`newtab_tiles`) plus a top-sites filler from `history_store`
    /// (DS-11). Pinned tiles always come first, in their stored order; an
    /// empty/failed read of either store just yields fewer tiles.
    fn build_newtab_source(&self) -> PageSource {
        let pinned: Vec<newtab::TopSite> = self
            .newtab_tiles
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|t| newtab::TopSite { url: t.url, title: t.title, pinned: true })
            .collect();
        let top_sites: Vec<newtab::TopSite> = self
            .history_store
            .most_visited(newtab::MAX_TILES as i64)
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let title = if e.title.trim().is_empty() {
                    e.url.clone()
                } else {
                    e.title
                };
                newtab::TopSite { url: e.url, title, pinned: false }
            })
            .collect();
        let sites = newtab::merge_tiles(&pinned, &top_sites);
        PageSource::Static {
            html: newtab::build_newtab_html(&sites),
            url: newtab::NEWTAB_URL.to_owned(),
        }
    }

    /// Apply a [`newtab::NewtabAction`] parsed from a clicked `about:newtab?...`
    /// link (pin/unpin toggle, the "+" tile, or "Restore closed"), then reload
    /// the newtab page with the updated tile set.
    ///
    /// `RestoreClosed` reuses the cross-restart session-restore mechanism
    /// (`restore_session`, backed by `session_store`) вЂ” Lumen has no separate
    /// per-tab "closed tabs" stack, so this reopens the last persisted session
    /// snapshot wholesale instead of undoing a single tab close.
    fn apply_newtab_action(&mut self, action: newtab::NewtabAction) {
        match action {
            newtab::NewtabAction::Pin { url, title } => {
                let _ = self.newtab_tiles.pin(&url, &title);
            }
            newtab::NewtabAction::Unpin { url } => {
                let _ = self.newtab_tiles.unpin(&url);
            }
            newtab::NewtabAction::PinCurrent => {
                if let Some(prev) = self.nav_back.last()
                    && let Some(url) = prev.source.url_str()
                {
                    let title = self
                        .history_store
                        .get(url)
                        .ok()
                        .flatten()
                        .map(|e| e.title)
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or_else(|| url.to_owned());
                    let _ = self.newtab_tiles.pin(url, &title);
                }
            }
            newtab::NewtabAction::RestoreClosed => {
                self.restore_session();
                self.request_redraw();
                return;
            }
        }
        self.navigate_to(self.build_newtab_source());
    }

    fn handle_omnibox_commit(&mut self, value: String) {
        // `view-source:<url>` вЂ” fetch and display syntax-highlighted source (В§D-2).
        if let Some(target_url) = value.trim().strip_prefix("view-source:") {
            let target_url = target_url.trim().to_owned();
            self.show_view_source_for_url(&target_url);
            return;
        }

        // `note-viewer:<id>` вЂ” open the note viewer overlay (В§12.2, GG-2).
        if let Some(id_str) = value.trim().strip_prefix("note-viewer:") {
            if let Ok(id) = id_str.parse::<i64>()
                && let Ok(Some(note)) = self.notes_store.get(id)
            {
                self.note_viewer.open(id, &note.url, &note.selection, &note.comment);
                self.request_redraw();
            }
            return;
        }

        // `switch-tab:<id>` вЂ” switch to an open tab by its stable id (В§12.4,
        // `@tabs` omnibox prefix). Resolve id в†’ current index (tabs reorder).
        if let Some(id_str) = value.trim().strip_prefix("switch-tab:") {
            if let Ok(id) = id_str.parse::<usize>()
                && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id)
            {
                self.switch_tab(idx);
            }
            return;
        }

        // `ai-answer:noop` вЂ” committing an `@ai` answer row is a no-op (В§12.5):
        // the RAG answer is already fully shown in the dropdown row itself,
        // there is no URL to navigate to.
        if value.trim() == "ai-answer:noop" {
            return;
        }

        // `about:settings` вЂ” open the browser settings overlay (task D-7).
        if value.trim() == "about:settings" {
            self.open_settings_panel();
            self.request_redraw();
            return;
        }

        // `about:newtab?...` вЂ” pin/unpin/"+"/restore-closed special links
        // (DS-11), committed e.g. by pasting a copied tile link.
        if let Some(action) = newtab::parse_action(value.trim()) {
            self.apply_newtab_action(action);
            return;
        }

        // `about:newtab` вЂ” internal start page with a speed dial of pinned +
        // most-visited sites (task CC-5, DS-11).
        if value.trim() == newtab::NEWTAB_URL {
            self.navigate_to(self.build_newtab_source());
            return;
        }

        // `about:chrome-preview` вЂ” CC-1 render-smoke for the engine-drawn
        // chrome asset (docs/tasks/p1-css-chrome.md).
        if value.trim() == chrome_preview::URL {
            self.navigate_to(PageSource::Static {
                html: chrome_preview::HTML.to_owned(),
                url: chrome_preview::URL.to_owned(),
            });
            return;
        }

        // `sidebar:<url>` вЂ” load the URL into the right-docked sidebar panel (7D.3).
        if let Some(sidebar_url) = value.strip_prefix("sidebar:") {
            let sidebar_url = sidebar_url.trim().to_owned();
            if !sidebar_url.is_empty() {
                let sink = Arc::clone(&self.event_sink);
                let src = PageSource::from_arg(Some(&sidebar_url));
                match src.load_bytes(sink, Some(self.active_cookie_jar())) {
                    Ok(raw) => {
                        self.open_sidebar_page(sidebar_url, &raw.bytes, String::new());
                    }
                    Err(err) => {
                        eprintln!("sidebar: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РіСЂСѓР·РёС‚СЊ {sidebar_url}: {err}");
                        // Open panel with placeholder so user sees feedback.
                        self.sidebar.open(sidebar_url);
                        // ADR-016 M2.2b-8: the sidebar becoming visible narrows the
                        // main page's content viewport вЂ” the same async-safe
                        // chrome-inset relayout the success path already routes off
                        // the UI thread (`open_sidebar_page`, M2.2b-3).
                        self.relayout_chrome();
                        self.request_redraw();
                    }
                }
            }
            return;
        }

        let aliases = self.omnibox_aliases.list_all().unwrap_or_default();
        if let Some(action) = omnibox::resolve(&value, &aliases) {
            match action {
                omnibox::AliasAction::Navigate(url) => {
                    self.navigate_to(PageSource::from_arg(Some(&url)));
                }
                omnibox::AliasAction::CreateNote(text) => {
                    self.notes.push(text);
                }
                omnibox::AliasAction::SaveReadLater(url) => {
                    // Spawn a background thread to fetch the page HTML and title.
                    // The result is sent back through `read_later_tx` and processed
                    // in `about_to_wait` via `read_later_rx`.
                    let tx = self.read_later_tx.clone();
                    let url_clone = url.clone();
                    std::thread::spawn(move || {
                        use lumen_core::ext::NetworkTransport;
                        use lumen_core::url::Url;
                        use lumen_network::HttpClient;
                        let Ok(parsed) = Url::parse(&url_clone) else { return };
                        // Р§РµСЂРµР· apply_http, Р° РЅРµ РіРѕР»С‹Рј HttpClient::new(): РёРЅР°С‡Рµ
                        // В«СЃРѕС…СЂР°РЅРёС‚СЊ РЅР° РїРѕС‚РѕРјВ» С…РѕРґРёС‚ РјРёРјРѕ HSTS, РїСЂРѕРєСЃРё, DoH Рё
                        // РєСЌС€Р° вЂ” СЃРІРѕРёРј, РЅРёС‡РµРј РЅРµ РЅР°СЃС‚СЂРѕРµРЅРЅС‹Рј РєР»РёРµРЅС‚РѕРј (BUG-402).
                        let client = crate::config::global().apply_http(HttpClient::new());
                        let Ok(html) = client.fetch(&parsed) else { return };
                        let title = panels::read_later_panel::extract_title_from_html(&html);
                        let title = if title.is_empty() { url_clone.clone() } else { title };
                        let _ = tx.send((url_clone, title, html));
                    });
                    // Also persist into the bookmark store under a dedicated
                    // folder so the bookmark manager panel shows it.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = self.bookmarks.add(
                        &url,
                        &url,
                        "/Read Later",
                        &["read-later".to_owned()],
                        "",
                        now,
                    );
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                }
            }
            return;
        }

        // No alias matched вЂ” plain URL or search query.
        if !value.contains("://") && !value.starts_with('@') {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let _ = self.search_history.record(&value, now);
        }
        self.navigate_to(PageSource::from_arg(Some(&value)));
    }

    /// Persist the current tab-strip layout (horizontal/vertical) into
    /// `browser_settings`.
    ///
    /// CC-15-3: the legacy tab-bar layout-toggle button was the only caller of
    /// `set_tab_layout` outside the settings panel's snapshot apply вЂ” removing
    /// its paint/hit-test would have silently dropped persistence from the two
    /// remaining toggle entry points (`KeyCommand::ToggleVerticalTabs`,
    /// `PaletteAction::ToggleVerticalTabs`), which never persisted on their
    /// own. Both now route through here so the choice survives a restart the
    /// same way the removed button made it.
    fn persist_tab_layout(&self) {
        let layout = if self.vertical_tabs.visible {
            tabs::strip::TabLayout::Vertical
        } else {
            tabs::strip::TabLayout::Horizontal
        };
        let _ = self.settings_store.set_tab_layout(layout.as_str());
    }

    /// Р—Р°РїСЂР°С€РёРІР°РµС‚ РїРѕРґСЃРєР°Р·РєРё РґР»СЏ С‚РµРєСѓС‰РµРіРѕ РІРІРѕРґР° РІ Р°РґСЂРµСЃРЅРѕР№ СЃС‚СЂРѕРєРµ.
    ///
    /// `@history <query>` в†’ FTS5-РїРѕРёСЃРє РїРѕ РёСЃС‚РѕСЂРёРё СЃС‚СЂР°РЅРёС†.
    /// `@notes <query>` в†’ FTS5-РїРѕРёСЃРє РїРѕ Р·Р°РјРµС‚РєР°Рј (В§12.2).
    /// РћР±С‹С‡РЅС‹Р№ РІРІРѕРґ в†’ prefix-match РїРѕ search_history + FTS5.
    fn query_omnibox_suggestions(&self) -> Vec<address_bar::OmniboxSuggestion> {
        use address_bar::{OmniboxPrefix, OmniboxSuggestion, parse_omnibox_prefix};

        let input = self.address_bar.input();
        if input.is_empty() {
            return Vec::new();
        }

        let (prefix, query) = parse_omnibox_prefix(input);
        let mut suggestions = Vec::new();

        match prefix {
            OmniboxPrefix::History => {
                // @history <query> вЂ” С‚РѕР»СЊРєРѕ FTS.
                if !query.is_empty() && let Ok(hits) = self.history_fts.search(query, 7) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: hit.snippet,
                        });
                    }
                }
            }
            OmniboxPrefix::Notes => {
                // @notes <query> вЂ” FTS5-РїРѕРёСЃРє РїРѕ Р·Р°РјРµС‚РєР°Рј В§12.2 (РґРѕ 5 СЂРµР·СѓР»СЊС‚Р°С‚РѕРІ).
                if !query.is_empty() && let Ok(hits) = self.notes_store.search(query, 5) {
                    for hit in hits {
                        let viewer_url = format!("note-viewer:{}", hit.note.id);
                        suggestions.push(OmniboxSuggestion::Note {
                            url: hit.note.url,
                            selection: hit.note.selection,
                            snippet: hit.snippet,
                            viewer_url,
                        });
                    }
                }
            }
            OmniboxPrefix::ReadLater => {
                // @read-later <query> вЂ” FTS5-РїРѕРёСЃРє РїРѕ СЃРѕС…СЂР°РЅС‘РЅРЅС‹Рј СЃС‚СЂР°РЅРёС†Р°Рј В§12.3
                // (РґРѕ 7 СЂРµР·СѓР»СЊС‚Р°С‚РѕРІ). Р’С‹Р±РѕСЂ РїРѕРґСЃРєР°Р·РєРё в†’ РЅР°РІРёРіР°С†РёСЏ РЅР° URL.
                if !query.is_empty() && let Ok(hits) = self.read_later_store.search(query, 7) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::ReadLater {
                            url: hit.entry.url,
                            title: hit.entry.title,
                            snippet: hit.snippet,
                        });
                    }
                }
            }
            OmniboxPrefix::Tabs => {
                // @tabs <query> вЂ” РїРѕРґСЃС‚СЂРѕС‡РЅС‹Р№ РїРѕРёСЃРє РїРѕ РѕС‚РєСЂС‹С‚С‹Рј РІРєР»Р°РґРєР°Рј В§12.4
                // (Р·Р°РіРѕР»РѕРІРѕРє + URL), case-insensitive. РџСѓСЃС‚РѕР№ Р·Р°РїСЂРѕСЃ в†’ РІСЃРµ
                // РІРєР»Р°РґРєРё. Р’С‹Р±РѕСЂ РїРѕРґСЃРєР°Р·РєРё в†’ РїРµСЂРµРєР»СЋС‡РµРЅРёРµ РїРѕ СЃС‚Р°Р±РёР»СЊРЅРѕРјСѓ id.
                let needle = query.to_lowercase();
                let active = self.tab_strip.active;
                for (idx, tab) in self.tab_strip.tabs.iter().enumerate() {
                    let url = if idx == active {
                        self.source.url_str().unwrap_or("").to_owned()
                    } else {
                        self.bg_tabs
                            .get(&tab.id)
                            .and_then(|s| s.source.url_str().map(str::to_owned))
                            .unwrap_or_default()
                    };
                    if needle.is_empty()
                        || tab.title.to_lowercase().contains(&needle)
                        || url.to_lowercase().contains(&needle)
                    {
                        suggestions.push(OmniboxSuggestion::Tab {
                            title: tab.title.clone(),
                            url,
                            switch_value: format!("switch-tab:{}", tab.id),
                        });
                    }
                    if suggestions.len() >= 8 {
                        break;
                    }
                }
            }
            OmniboxPrefix::Bookmarks => {
                // @bookmarks <query> вЂ” РїРѕРґСЃС‚СЂРѕС‡РЅС‹Р№ РїРѕРёСЃРє РїРѕ Р·Р°РєР»Р°РґРєР°Рј В§12.8
                // (title/url/С‚РµРіРё), case-insensitive. РџСЂРё РЅР°Р»РёС‡РёРё AI-СЌРјР±РµРґРґРёРЅРіР°
                // Р·Р°РїСЂРѕСЃР° СЂРµР·СѓР»СЊС‚Р°С‚ РґРѕРїРѕР»РЅСЏРµС‚СЃСЏ cosine-similarity СЂР°РЅР¶РёСЂРѕРІР°РЅРёРµРј
                // РїРѕРІРµСЂС… С‚РµРєСЃС‚РѕРІС‹С… СЃРѕРІРїР°РґРµРЅРёР№ (РЅРµ Р·Р°РјРµРЅСЏРµС‚ РёС… вЂ” closes the loop
                // for bookmarks that don't textually match but are related).
                if let Ok(bookmarks) = self.bookmarks.list_all() {
                    let needle = query.to_lowercase();
                    let query_embedding = if query.is_empty() {
                        Vec::new()
                    } else {
                        self.ai_backend.embed(query)
                    };
                    // Score: text matches always outrank pure-semantic ones (base
                    // 1.0 + similarity as tie-break); semantic-only matches keep
                    // their raw similarity so they still sort by relevance.
                    let mut scored: Vec<(f32, &lumen_storage::bookmarks::Bookmark)> = bookmarks
                        .iter()
                        .filter_map(|b| {
                            let text_match = needle.is_empty()
                                || b.title.to_lowercase().contains(&needle)
                                || b.url.to_lowercase().contains(&needle)
                                || b.tags.iter().any(|t| t.to_lowercase().contains(&needle));
                            let similarity = if !query_embedding.is_empty()
                                && let Some(emb) = &b.embedding
                            {
                                lumen_storage::bookmarks::cosine_similarity(
                                    &query_embedding,
                                    &lumen_storage::bookmarks::embedding_from_bytes(emb),
                                )
                            } else {
                                0.0
                            };
                            if !text_match && similarity <= 0.5 {
                                return None;
                            }
                            let score = if text_match { 1.0 + similarity } else { similarity };
                            Some((score, b))
                        })
                        .collect();
                    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                    for (_, b) in scored.into_iter().take(7) {
                        suggestions.push(OmniboxSuggestion::Bookmark {
                            title: b.title.clone(),
                            url: b.url.clone(),
                            snippet: b.summary.clone().unwrap_or_default(),
                        });
                    }
                }
            }
            OmniboxPrefix::Ai => {
                // @ai <query> вЂ” РµРґРёРЅСЃС‚РІРµРЅРЅР°СЏ СЃС‚СЂРѕРєР°: RAG-РѕС‚РІРµС‚ (В§12.5) РїРѕРґ
                // `--features ai`, Р»РёР±Рѕ СЃС‚Р°С‚РёС‡РЅС‹Р№ hint РїРѕРґ РµС‘ РѕС‚СЃСѓС‚СЃС‚РІРёРµ
                // (СЃРј. `Self::ai_answer_for`, РѕР±Рµ РІРµС‚РєРё cfg-gated). РџСѓСЃС‚РѕР№
                // Р·Р°РїСЂРѕСЃ вЂ” РЅРё РѕРґРЅРѕР№ СЃС‚СЂРѕРєРё, РєР°Рє Сѓ РѕСЃС‚Р°Р»СЊРЅС‹С… РїСЂРµС„РёРєСЃРѕРІ.
                if !query.is_empty() {
                    suggestions.push(OmniboxSuggestion::Ai { answer: self.ai_answer_for(query) });
                }
            }
            OmniboxPrefix::Plain => {
                // prefix-match РїРѕ search_history (РґРѕ 4 СЃС‚СЂРѕРє).
                if let Ok(queries) = self.search_history.prefix_match(query, 4) {
                    for q in queries {
                        suggestions.push(OmniboxSuggestion::SearchQuery {
                            query: q.query,
                            frequency: q.frequency,
                        });
                    }
                }
                // URL/title substring match РїРѕ history_store (РґРѕ 5 СЃС‚СЂРѕРє).
                // Р”Р°С‘С‚ СЂРµР·СѓР»СЊС‚Р°С‚С‹ РїРѕ URL-С„СЂР°РіРјРµРЅС‚Сѓ РґР°Р¶Рµ Р±РµР· FTS5-РёРЅРґРµРєСЃР°.
                if let Ok(hits) = self.history_store.search_prefix(query, 5) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: String::new(),
                        });
                    }
                }
                // FTS5 РїРѕ РёСЃС‚РѕСЂРёРё СЃС‚СЂР°РЅРёС† (РґРѕ 4 СЃС‚СЂРѕРє, РёС‚РѕРіРѕ в‰¤ 8).
                if let Ok(hits) = self.history_fts.search(query, 4) {
                    for hit in hits {
                        // Р”РµРґСѓРїР»РёРєР°С†РёСЏ: FTS5 РјРѕР¶РµС‚ РїРѕРІС‚РѕСЂРёС‚СЊ URL РёР· search_prefix РІС‹С€Рµ.
                        if !suggestions.iter().any(|s| {
                            matches!(s, OmniboxSuggestion::HistoryFts { url, .. } if url == &hit.url)
                        }) {
                            suggestions.push(OmniboxSuggestion::HistoryFts {
                                url: hit.url,
                                title: hit.title,
                                snippet: hit.snippet,
                            });
                        }
                    }
                }
            }
        }

        suggestions
    }

    fn handle_find_key(&mut self, code: KeyCode, key_event: &KeyEvent) {
        let shift = self.modifiers.shift_key();
        let ctrl_or_super = self.modifiers.control_key() || self.modifiers.super_key();

        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.find.close();
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.find.backspace();
                self.scroll_to_active_match();
                self.request_redraw();
            }
            // Enter / F3 вЂ” СЃР»РµРґСѓСЋС‰РёР№ РјР°С‚С‡ (Shift вЂ” РїСЂРµРґС‹РґСѓС‰РёР№).
            // Ctrl+G / Cmd+G вЂ” С‚Рѕ Р¶Рµ (Firefox-СЃС‚РёР»СЊ find-next), Shift вЂ” РїСЂРµРґС‹РґСѓС‰РёР№.
            KeyCode::Enter | KeyCode::F3 => {
                if !key_event.repeat {
                    let total = self.current_matches().len();
                    if shift {
                        self.find.prev(total);
                    } else {
                        self.find.next(total);
                    }
                    self.scroll_to_active_match();
                    self.request_redraw();
                }
            }
            KeyCode::KeyG if ctrl_or_super && !key_event.repeat => {
                let total = self.current_matches().len();
                if shift {
                    self.find.prev(total);
                } else {
                    self.find.next(total);
                }
                self.scroll_to_active_match();
                self.request_redraw();
            }
            // Ctrl+R вЂ” РїРµСЂРµРєР»СЋС‡РёС‚СЊ plain-text в†” regex СЂРµР¶РёРј.
            KeyCode::KeyR if ctrl_or_super && !key_event.repeat => {
                self.find.toggle_regex_mode();
                self.scroll_to_active_match();
                self.request_redraw();
            }
            _ => {
                // РўРµРєСЃС‚РѕРІС‹Р№ РІРІРѕРґ. РџСЂРё РјРѕРґРёС„РёРєР°С‚РѕСЂР°С… Ctrl/Cmd РЅРµ РІСЃС‚Р°РІР»СЏРµРј вЂ”
                // СЌС‚Рѕ shortcut РІ Р°РґСЂРµСЃ find-Р° (РёР»Рё Р±СѓРґСѓС‰РёС… С‡РµРіРѕ-С‚Рѕ РµС‰С‘), РЅРµ
                // СЃРёРјРІРѕР» РґР»СЏ query. Р‘РµР· РЅРёС… text вЂ” СЌС‚Рѕ СѓР¶Рµ layout-aware
                // СЃРёРјРІРѕР» РѕС‚ winit, СЃ СѓС‡С‘С‚РѕРј IME / dead-keys.
                if ctrl_or_super {
                    return;
                }
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                {
                    self.find.append_str(text);
                    self.scroll_to_active_match();
                    self.request_redraw();
                }
            }
        }
        // CC-9 (docs/tasks/p1-css-chrome.md): `#findBar`'s engine-rendered
        // value/count (`Self::chrome_model_snapshot`) is baked into
        // `self.chrome_layout` at `relayout_chrome_host` time, not
        // recomputed every `RedrawRequested` вЂ” every branch above mutates
        // `self.find`, so without this call the on-screen bar would keep
        // showing stale text/count. Mirrors the same call at the end of
        // `Self::handle_address_bar_key` (CC-7). No-op off the flag.
        self.relayout_chrome_host();
    }

    /// Handle a key while the bookmark panel search box is focused.
    ///
    /// Returns `true` when the key was consumed. Modified keys (Ctrl/Cmd) are
    /// *not* consumed so global shortcuts continue to work.
    fn handle_bookmark_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.bookmark_panel.search_active = false;
                self.request_redraw();
                true
            }
            KeyCode::Backspace => {
                self.bookmark_panel.backspace_search();
                self.request_redraw();
                true
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    self.bookmark_panel.append_search(text);
                    self.request_redraw();
                    return true;
                }
                false
            }
        }
    }

    /// Handle keyboard input when the history panel is visible.
    ///
    /// When `search_active`: printable chars в†’ search query, Backspace в†’ delete
    /// char, Escape в†’ blur search (panel stays open). Arrow keys scroll the list.
    /// Returns `true` if the key was consumed.
    fn handle_history_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                if self.history_panel.search_active {
                    self.history_panel.search_active = false;
                } else {
                    self.history_panel.visible = false;
                }
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.history_panel.search_active => {
                self.history_panel.backspace_search();
                self.refresh_history();
                self.request_redraw();
                true
            }
            KeyCode::ArrowDown => {
                self.history_panel.scroll_by(LINE_STEP_CSS_PX);
                self.request_redraw();
                true
            }
            KeyCode::ArrowUp => {
                self.history_panel.scroll_by(-LINE_STEP_CSS_PX);
                self.request_redraw();
                true
            }
            _ => {
                if self.history_panel.search_active
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.history_panel.append_search(ch);
                        }
                        self.refresh_history();
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Handle keyboard input when the print dialog is visible (E-1).
    ///
    /// Printable chars go to the focused text field. Escape closes the dialog.
    /// Returns `true` if the key was consumed.
    /// Handle keyboard input while the AI panel is visible.
    ///
    /// Returns `true` if the event was consumed (swallowed from the global
    /// keybinding table).  Modified keys (Ctrl, Meta) fall through so that
    /// `Ctrl+Shift+A` (toggle AI panel) still works.
    fn handle_ai_panel_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.ai_panel.close();
                // ADR-016 M2.2b-3: closing the AI panel is an async-safe chrome
                // toggle (content viewport widens, no synchronous geometry read),
                // so route off-thread when the engine thread is enabled.
                self.relayout_chrome();
                self.request_redraw();
                true
            }
            KeyCode::Backspace => {
                self.ai_panel.backspace();
                self.request_redraw();
                true
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                // Split borrows: inline the submit logic to let Rust prove
                // ai_panel and ai_backend are disjoint fields.
                let prompt = self.ai_panel.input.clone();
                if !prompt.trim().is_empty() {
                    let response = self.ai_backend.query(&prompt);
                    self.ai_panel.response = response;
                    self.ai_panel.input.clear();
                    self.ai_panel.scroll_y = 0.0;
                }
                self.request_redraw();
                true
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    for ch in text.chars() {
                        self.ai_panel.push_char(ch);
                    }
                    self.request_redraw();
                    return true;
                }
                false
            }
        }
    }

    fn handle_print_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.print_panel.close();
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.print_panel.editing_field.is_some() => {
                self.print_panel.pop_char();
                self.request_redraw();
                true
            }
            _ => {
                if self.print_panel.editing_field.is_some()
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.print_panel.push_char(ch);
                        }
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// Export current document as PDF using parameters from PrintRequest (W-2 Phase 3b).
    fn handle_print_request(&mut self, req: &lumen_js::PrintRequest) {
        // Determine output path: use provided path or generate default.
        let output_path = req
            .output_path
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(default_pdf_output_path);

        // Convert margins from CSS px (96 DPI) to points (1 point = 1/72 inch).
        // 1 CSS px at 96 DPI = 72/96 points = 0.75 points (not used here; we keep px).

        let margin_top = req.margin_top;
        let margin_bottom = req.margin_bottom;
        let margin_left = req.margin_left;
        let margin_right = req.margin_right;

        match do_print_to_pdf_with_opts(
            &self.source,
            &output_path,
            self.event_sink.clone(),
            PrintOptions {
                margin_tb: (margin_top + margin_bottom) / 2.0, // Simplified: average for TB and LR.
                margin_lr: (margin_left + margin_right) / 2.0,
                scale: 100, // Default scale: 100%
                print_backgrounds: true, // print background graphics (JS print request default)
                landscape: false, // BUG-420: JS `window.print()` carries no orientation вЂ” always portrait.
            },
        ) {
            Ok(page_count) => {
                eprintln!(
                    "[shell] PDF exported to {}: {} pages",
                    output_path.display(),
                    page_count
                );
                // Phase 2 future: show user feedback notification.
            }
            Err(e) => {
                eprintln!("[shell] PDF export failed: {}", e);
                // Phase 2 future: show error dialog to user.
            }
        }
    }

    /// The engine chrome's "РџРµС‡Р°С‚СЊ" button (`ChromeAction::PrintConfirm`,
    /// [BUG-420](../../../bugs/BUG-420-FIXED.md)) вЂ” exports the active tab
    /// with `PrintPanel`'s live settings (margin preset, scale, background
    /// graphics, orientation) and closes the dialog, mirroring
    /// `handle_print_request`'s JS `window.print()` path.
    fn handle_print_confirm(&mut self) {
        let output_path = default_pdf_output_path();
        let (margin_tb, margin_lr) = self.print_panel.margin_px();
        let landscape = self.print_panel.orientation == panels::print_panel::Orientation::Landscape;

        match do_print_to_pdf_with_opts(
            &self.source,
            &output_path,
            self.event_sink.clone(),
            PrintOptions {
                margin_tb,
                margin_lr,
                scale: self.print_panel.scale,
                print_backgrounds: self.print_panel.print_backgrounds,
                landscape,
            },
        ) {
            Ok(page_count) => {
                eprintln!(
                    "[shell] PDF exported to {}: {} pages",
                    output_path.display(),
                    page_count
                );
            }
            Err(e) => {
                eprintln!("[shell] PDF export failed: {}", e);
            }
        }
        self.print_panel.close();
    }

    /// Open the settings panel, populating every section вЂ” including the ones
    /// backed by stores other than `settings_store` (HTTP/3 from the process-
    /// global fingerprint profile, Tor status from the same, ad-block
    /// subscriptions from `AdblockStore`, spellcheck locale from `SPELL_DICTS`).
    fn open_settings_panel(&mut self) {
        let snap = self.settings_store.snapshot();
        self.settings_panel.open(snap);
        self.settings_panel.set_http3(config::global().http3);
        self.settings_panel.set_tor_active(
            config::global().http_profile == lumen_network::HttpProfile::TorBrowser,
        );
        self.settings_panel
            .set_adblock_subs(self.adblock_store.list_subscriptions().unwrap_or_default());
        self.settings_panel
            .set_spell_locale(SPELL_DICTS.get().map(|d| d.locale().to_owned()));
    }

    /// Close the settings panel, flushing the draft to every backing store.
    ///
    /// Centralised so all four close paths (Г— button, click outside, `Ctrl+,`
    /// toggle, `Escape`) apply theme/dark-mode sync and the HTTP/3 rewrite
    /// identically вЂ” previously only the Г— button synced `dark_mode`.
    fn close_settings_panel(&mut self) {
        let draft = self.settings_panel.apply_draft();
        // Apply theme & accent from draft when panel closes.
        self.shell_theme = panels::themes::ShellTheme::parse(&draft.theme);
        // Mirror explicit dark/light lock to dark_mode so that
        // @media prefers-color-scheme reflects the user choice. For System
        // theme, is_dark(self.dark_mode) = self.dark_mode (no change); for
        // Dark/Light it overrides.
        let new_dark = self.shell_theme.is_dark(self.dark_mode);
        if new_dark != self.dark_mode {
            self.dark_mode = new_dark;
            // ADR-016 M2.2b-4: an explicit dark/light lock is async-safe like
            // the OS theme flip вЂ” a whole-page restyle with no synchronous
            // geometry read here (only chrome state follows), so route it
            // off-thread.
            self.relayout_chrome();
        }
        // Live-sync the tab-strip layout so the Appearance section's toggle
        // takes effect immediately rather than only after the next restart.
        self.vertical_tabs.visible =
            tabs::strip::TabLayout::from_str(&draft.tab_layout) == tabs::strip::TabLayout::Vertical;
        let _ = self.settings_store.apply_snapshot(&draft);
        // BUG-411: `Escape`/click-outside close paths never went through
        // `ChromeAction::ToggleShields`, so re-apply the draft's shields flag
        // as the fallback here too before pushing it at the live filter.
        self.shields.set_default_enabled(draft.shields_enabled);
        self.sync_adblock_filter();
        // HTTP/3 lives in fingerprint.toml, loaded once into a process-global
        // at startup вЂ” only rewrite the file (and note the restart) if the
        // draft actually changed it.
        if self.settings_panel.http3_draft != config::global().http3 {
            match config::set_http3(self.settings_panel.http3_draft) {
                Ok(()) => eprintln!(
                    "settings: HTTP/3 РёР·РјРµРЅС‘РЅ РЅР° {} вЂ” РІСЃС‚СѓРїРёС‚ РІ СЃРёР»Сѓ РїРѕСЃР»Рµ РїРµСЂРµР·Р°РїСѓСЃРєР° Р±СЂР°СѓР·РµСЂР°",
                    self.settings_panel.http3_draft
                ),
                Err(e) => eprintln!("settings: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїРёСЃР°С‚СЊ fingerprint.toml: {e}"),
            }
        }
        self.settings_panel.visible = false;
        // CC-6: re-sync the CSS chrome's data-theme/data-layout (no-op off the flag).
        self.relayout_chrome_host();
    }

    /// Handle keyboard input when the settings panel is visible.
    ///
    /// Printable chars go to the focused text input. Escape closes panel (flushing
    /// draft). Returns `true` if the key was consumed.
    fn handle_settings_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if self.modifiers.control_key() || self.modifiers.super_key() {
            return false;
        }
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.close_settings_panel();
                self.request_redraw();
                true
            }
            KeyCode::Backspace if self.settings_panel.focused_input.is_some() => {
                self.settings_panel.backspace();
                self.request_redraw();
                true
            }
            _ => {
                if self.settings_panel.focused_input.is_some()
                    && let Some(text) = key_event.text.as_ref()
                        && !text.is_empty()
                        && !text.chars().any(char::is_control)
                    {
                        for ch in text.chars() {
                            self.settings_panel.append_char(ch);
                        }
                        self.request_redraw();
                        return true;
                    }
                false
            }
        }
    }

    /// РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚ РєР»Р°РІРёС€РЅС‹Р№ РІРІРѕРґ РґР»СЏ РїР°РЅРµР»Рё РіРѕСЂСЏС‡РёС… РєР»Р°РІРёС€ (В§D-4).
    ///
    /// РљРѕРіРґР° Р°РєС‚РёРІРµРЅ rebind mode (`rebinding.is_some()`): Р·Р°С…РІР°С‚С‹РІР°РµС‚
    /// СЃР»РµРґСѓСЋС‰СѓСЋ РєР»Р°РІРёС€Сѓ Рё РїРµСЂРµРґР°С‘С‚ РІ `accept_rebind`. Esc РѕС‚РјРµРЅСЏРµС‚ rebind.
    /// Р’РѕР·РІСЂР°С‰Р°РµС‚ `true`, РµСЃР»Рё СЃРѕР±С‹С‚РёРµ РїРѕРіР»РѕС‰РµРЅРѕ.
    fn handle_shortcuts_key(&mut self, code: KeyCode, key_event: &KeyEvent) -> bool {
        if key_event.repeat {
            return false;
        }
        if self.shortcuts_panel.rebinding.is_some() {
            if code == KeyCode::Escape {
                self.shortcuts_panel.cancel_rebind();
                self.request_redraw();
                return true;
            }
            let modifier = {
                let m = self.modifiers;
                let ctrl = m.control_key();
                let shift = m.shift_key();
                let alt = m.alt_key();
                match (ctrl, shift, alt) {
                    (true, true, false) => "ctrl+shift",
                    (true, false, true) => "ctrl+alt",
                    (true, false, false) => "ctrl",
                    (false, true, false) => "shift",
                    (false, false, true) => "alt",
                    _ => "",
                }
            };
            let key = format!("{:?}", code);
            let key = key.trim_start_matches("Key").trim_start_matches("Digit").to_string();
            self.shortcuts_panel.accept_rebind(modifier, &key);
            self.request_redraw();
            return true;
        }
        if code == KeyCode::Escape {
            self.shortcuts_panel.close();
            self.request_redraw();
            return true;
        }
        false
    }

    /// РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚ РєР»Р°РІРёС€РЅС‹Р№ РІРІРѕРґ РїРѕРєР° hint-СЂРµР¶РёРј Р°РєС‚РёРІРµРЅ.
    ///
    /// `Escape` вЂ” Р·Р°РєСЂС‹С‚СЊ overlay. Р›СЋР±РѕР№ РѕРґРёРЅРѕС‡РЅС‹Р№ СЃРёРјРІРѕР» (СЃС‚СЂРѕС‡РЅС‹Р№ ASCII) вЂ”
    /// РїРµСЂРµРґР°С‘С‚СЃСЏ РІ `HintState::push_char`; РїСЂРё СѓРЅРёРєР°Р»СЊРЅРѕРј СЃРѕРІРїР°РґРµРЅРёРё РІС‹Р·С‹РІР°РµС‚СЃСЏ
    /// `activate_node`. РќРµСЂР°СЃРїРѕР·РЅР°РЅРЅС‹Рµ РєР»Р°РІРёС€Рё РёРіРЅРѕСЂРёСЂСѓСЋС‚СЃСЏ.
    fn handle_hint_key(&mut self, code: KeyCode, key_event: &KeyEvent) {
        if matches!(code, KeyCode::Escape) && !key_event.repeat {
            self.hint.close();
            self.request_redraw();
            return;
        }
        if let Some(text) = key_event.text.as_ref() {
            for c in text.chars() {
                if c.is_ascii_lowercase() {
                    match self.hint.push_char(c) {
                        hints::HintResult::Activate(node_id) => {
                            self.activate_node(node_id);
                        }
                        hints::HintResult::Partial | hints::HintResult::NoMatch => {}
                    }
                    self.request_redraw();
                    break;
                }
            }
        }
    }

    /// РђРєС‚РёРІРёСЂРѕРІР°С‚СЊ DOM-СѓР·РµР» `node_id` РєР°Рє Р±СѓРґС‚Рѕ РїРѕ РЅРµРјСѓ РєР»РёРєРЅСѓР»Рё РјС‹С€СЊСЋ.
    ///
    /// Р”РёСЃРїР°С‚С‡РёС‚ JS click-СЃРѕР±С‹С‚РёРµ, РѕР±СЂР°Р±Р°С‚С‹РІР°РµС‚ form-РґРµР№СЃС‚РІРёРµ (checkbox/radio),
    /// Рё РЅР°РІРёРіРёСЂСѓРµС‚ РїРѕ СЃСЃС‹Р»РєРµ РµСЃР»Рё СѓР·РµР» РІРЅСѓС‚СЂРё `<a href>`. РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ
    /// hint-СЂРµР¶РёРјРѕРј РґР»СЏ Р°РєС‚РёРІР°С†РёРё СЌР»РµРјРµРЅС‚Р° Р±РµР· СѓС‡Р°СЃС‚РёСЏ РјС‹С€Рё.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn activate_node(&mut self, node_id: NodeId) {
        // JS click dispatch (bubbling РѕС‚ СѓР·Р»Р° РґРѕ document).
        // Hint-mode activations have no real mouse coordinates, so x/y are 0.
        // ADR-016 M2.2c-2d (10): same read-after-eval routing as the mouse click
        // dispatch вЂ” `_lumen_dispatch_mouse_event('click', вЂ¦)` fire-and-forget via
        // `route_eval_js`, then `take_navigate_request` ordered after via
        // `route_query_js`; byte-identical off-flag.
        #[cfg(feature = "v8")]
        {
            let script = format!(
                "_lumen_dispatch_mouse_event({}, 'click', 0, 0, 0, 1, 0)",
                node_id.index()
            );
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
            if let Some(Some(nav)) = route_query_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                |j| j.take_navigate_request(),
            ) {
                self.pending_js_navigate = Some(nav);
            }
        }
        // Form action classification.
        let form_action = if let Some(src) = self.layout_source.as_ref() {
            forms::classify_click(&src.document.lock().unwrap(), node_id)
        } else {
            forms::FormClickAction::Nothing
        };
        match form_action {
            forms::FormClickAction::ToggleCheckbox(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), id);
                }
                // ADR-016 M2.2c-3 (2): async-safe form-control DOM mutation (Bucket
                // A) вЂ” no synchronous geometry read after, route off-thread when
                // `LUMEN_ENGINE_THREAD=1`, byte-identical otherwise.
                self.relayout_form();
            }
            forms::FormClickAction::ToggleRadio { clicked, .. } => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                }
                // ADR-016 M2.2c-3 (2): async-safe form-control DOM mutation (Bucket A).
                self.relayout_form();
            }
            forms::FormClickAction::OpenColorPicker(id) => {
                self.color_picker_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenDatePicker(id) => {
                let (y, m) = self.layout_source.as_ref()
                    .and_then(|src| {
                        let doc = src.document.lock().ok()?;
                        let val = doc.control_value(id).into_owned();
                        forms::parse_date_value(&val).map(|(y, m, _)| (y, m))
                    })
                    .unwrap_or_else(forms::today_year_month);
                self.date_picker_node = Some(id);
                self.date_picker_year = y;
                self.date_picker_month = m;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenSelectDropdown(id) => {
                self.select_dropdown_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenFilePicker(id) => {
                self.open_file_picker(id);
            }
            forms::FormClickAction::ToggleDetails(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_details_open(&mut src.document.lock().unwrap(), id);
                }
                // ADR-016 M2.2c-2d: fire-and-forget `toggle` event С‡РµСЂРµР·
                // РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
                #[cfg(feature = "v8")]
                route_eval_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    format!(
                        "_lumen_make_element({}).dispatchEvent(new Event('toggle'))",
                        id.index()
                    ),
                );
                // ADR-016 M2.2c-3 (2): async-safe `<details>` open toggle (Bucket A);
                // the `toggle` event above is independent of the layout job.
                self.relayout_form();
            }
            // Range slide via keyboard activation: no-op (no position known).
            forms::FormClickAction::SlideRange(_) => {}
            forms::FormClickAction::SubmitForm(_) | forms::FormClickAction::Nothing => {
                // Link navigation.
                let href = self.layout_source.as_ref().and_then(|src| {
                    links::find_link_href(&src.document.lock().unwrap(), node_id)
                });
                if let Some(href) = href {
                    if let Some(frag) = links::fragment_only(&href) {
                        self.navigate_fragment(frag.to_owned());
                    } else if links::is_navigable_href(&href) {
                        let resolved = self.source.resolve_href(&href);
                        if let Some(action) = newtab::parse_action(&resolved) {
                            self.apply_newtab_action(action);
                        } else if let Some(frag) =
                            links::same_document_fragment(self.current_display_url(), &resolved)
                        {
                            self.navigate_fragment(frag);
                        } else {
                            self.navigate_to(PageSource::from_arg(Some(&resolved)));
                        }
                    }
                }
            }
        }
    }

    /// Р•СЃР»Рё Р°РєС‚РёРІРЅС‹Р№ match РІРЅРµ РІРёРґРёРјРѕР№ С‡Р°СЃС‚Рё viewport-Р° вЂ” СЃРґРІРёРіР°РµС‚ scroll С‚Р°Рє,
    /// С‡С‚РѕР±С‹ РѕРЅ РїРѕРїР°Р» РІ РІРµСЂС…РЅСЋСЋ С‡РµС‚РІРµСЂС‚СЊ РѕРєРЅР°. Р’С‹Р·С‹РІР°РµС‚СЃСЏ РїРѕСЃР»Рµ Р»СЋР±РѕРіРѕ
    /// РґРµР№СЃС‚РІРёСЏ, РјРµРЅСЏСЋС‰РµРіРѕ active match: next/prev, backspace, С‚РµРєСЃС‚РѕРІС‹Р№ РІРІРѕРґ.
    /// РџСЂРё Р·Р°РєСЂС‹С‚РѕРј Р±Р°СЂРµ / РїСѓСЃС‚РѕРј query / РѕС‚СЃСѓС‚СЃС‚РІРёРё РјР°С‚С‡РµР№ вЂ” no-op.
    fn scroll_to_active_match(&mut self) {
        let matches = self.current_matches();
        if matches.is_empty() {
            return;
        }
        let active = self.find.active_index();
        let Some(m) = matches.get(active) else {
            return;
        };
        let vh = self.viewport_height_css();
        if let Some(target) = find::scroll_to_match(m.rect, vh, self.scroll_y) {
            self.start_smooth_scroll(target);
        }
    }

    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// Returns the URL to display in the address bar and use for history / bookmarks.
    ///
    /// When `history.pushState` / `history.replaceState` has updated the virtual
    /// URL without a page load, `display_url` overrides the real `source` URL.
    fn current_display_url(&self) -> &str {
        self.display_url
            .as_deref()
            .or_else(|| self.source.url_str())
            .unwrap_or("")
    }

    /// Returns the detected target `ColorSpace` for the active display.
    ///
    /// Used by the paint layer to decide wide-gamut output (Step 4) and
    /// by ICC transforms (Step 2). Defaults to `ColorSpace::Srgb` when
    /// the OS query fails or the display is sRGB-only.
    #[allow(dead_code)] // consumer: ph3-color-management Steps 2+4
    fn target_color_space(&self) -> ColorSpace {
        self.display_color_profile.active_profile()
    }

    /// РўРµРєСѓС‰Р°СЏ Р»РѕРіРёС‡РµСЃРєР°СЏ (CSS px) РІС‹СЃРѕС‚Р° viewport-Р°. Р•СЃР»Рё РѕРєРЅРѕ РµС‰С‘ РЅРµ СЃРѕР·РґР°РЅРѕ вЂ”
    /// fallback РЅР° layout-viewport 720 px, РєРѕС‚РѕСЂС‹Р№ Сѓ РЅР°СЃ hardcoded РІ pipeline.
    fn viewport_height_css(&self) -> f32 {
        let total = match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().height as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 720.0,
        };
        let ws_bar = if self.workspace_panel.visible {
            panels::workspace_panel::SWITCHER_HEIGHT
        } else {
            0.0
        };
        (total - toolbar::CHROME_H - ws_bar).max(0.0)
    }

    /// Full logical (CSS px) window height including the tab bar. Used to
    /// clamp the tab context menu (CC-4) so it stays on-screen. Fallback 720.
    fn window_height_css(&self) -> f32 {
        match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().height as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 720.0,
        }
    }

    /// CSS px С€РёСЂРёРЅР° viewport-Р° вЂ” РїРѕР»РЅР°СЏ С€РёСЂРёРЅР° РѕРєРЅР°, РЅСѓР¶РЅР° scrollbar-overlay-Сѓ
    /// РґР»СЏ СЂР°Р·РјРµС‰РµРЅРёСЏ Сѓ РїСЂР°РІРѕРіРѕ РєСЂР°СЏ. Fallback РЅР° layout-viewport 1024 px (С‚РѕС‚
    /// Р¶Рµ hardcoded СЂР°Р·РјРµСЂ, С‡С‚Рѕ Рё РІ pipeline РґРѕ СЃРѕР·РґР°РЅРёСЏ РѕРєРЅР°).
    fn viewport_width_css(&self) -> f32 {
        match (self.window.as_ref(), self.renderer.as_ref()) {
            (Some(w), Some(r)) => {
                let phys = w.inner_size().width as f32;
                let dpr = (r.scale_factor() as f32).max(1e-6);
                phys / dpr
            }
            _ => 1024.0,
        }
    }

    /// CSS px С€РёСЂРёРЅР° РѕР±Р»Р°СЃС‚Рё РєРѕРЅС‚РµРЅС‚Р° СЃС‚СЂР°РЅРёС†С‹ вЂ” РїРѕР»РЅР°СЏ С€РёСЂРёРЅР° РѕРєРЅР° РјРёРЅСѓСЃ
    /// С€РёСЂРёРЅР° РІРµСЂС‚РёРєР°Р»СЊРЅС‹С… РїР°РЅРµР»РµР№ РІРєР»Р°РґРѕРє (СЃР»РµРІР°) Рё sidebar (СЃРїСЂР°РІР°), РµСЃР»Рё
    /// РѕРЅРё РІРёРґРёРјС‹. РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РґР»СЏ РєР»Р°РјРїРёРЅРіР° РіРѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅРѕРіРѕ СЃРєСЂРѕР»Р»Р°.
    fn page_content_width_css(&self) -> f32 {
        let (left_offset, right_offset) = self.docked_panel_offsets();
        (self.viewport_width_css() - left_offset - right_offset).max(0.0)
    }

    /// The cross-dockable docked sidebars as `(persist id, visible, default
    /// width)`, in side-resolution priority order (outermost first). Each can be
    /// flipped to either window edge; [`Self::left_dock`] / [`Self::right_dock`]
    /// pick the first visible one whose effective side matches.
    ///
    /// CC-10b/CC-15-6: `ID_AI`/`ID_SIDEBAR` are **not** listed вЂ” they paint as
    /// `#rightSidebar`, a real flex sibling of `#contentArea` in the engine
    /// chrome layout, so `chrome_page_host_rect` ([`Self::page_offset`]) already
    /// reflects their width. Listing them would make
    /// [`Self::page_content_width_css`]'s horizontal scroll-clamp bound subtract
    /// the same width twice. (They were entries reporting `visible: false` while
    /// the `LUMEN_LEGACY_CHROME` rollback flag existed.)
    fn dockable_sidebars(&self) -> [(&'static str, bool, f32); 2] {
        [
            (
                panel_layout::ID_VERTICAL_TABS,
                self.vertical_tabs.visible,
                panels::vertical_tabs::PANEL_WIDTH,
            ),
            (
                panel_layout::ID_TREE_TABS,
                self.tree_tabs.visible,
                panels::tree_tabs::PANEL_WIDTH,
            ),
        ]
    }

    /// Effective dock side of a cross-dockable sidebar: its persisted override,
    /// falling back to [`panel_layout::default_dock`].
    fn sidebar_dock_side(&self, id: &'static str) -> panel_layout::Dock {
        self.panel_layout.dock_for(id, panel_layout::default_dock(id))
    }

    /// Left x-origin (CSS px) of a docked sidebar of `width` on `side`: left
    /// docks hug `x = 0`, right docks hug the window's right edge.
    fn dock_origin_x(&self, side: panel_layout::Dock, width: f32) -> f32 {
        match side {
            panel_layout::Dock::Left => 0.0,
            panel_layout::Dock::Right => (self.viewport_width_css() - width).max(0.0),
        }
    }

    /// Active left-docked sidebar as `(persist id, current width CSS px)`, or
    /// `None` when no left sidebar is visible. Honours per-panel cross-dock side
    /// overrides: a sidebar moved to the right edge no longer counts here.
    fn left_dock(&self) -> Option<(&'static str, f32)> {
        self.dockable_sidebars().into_iter().find_map(|(id, visible, default_w)| {
            (visible && self.sidebar_dock_side(id) == panel_layout::Dock::Left)
                .then(|| (id, self.panel_layout.width_for(id, default_w)))
        })
    }

    /// Active right-docked sidebar as `(persist id, current width CSS px)`, or
    /// `None` when none is visible. Resolved in [`Self::dockable_sidebars`]
    /// priority order: a tab sidebar flipped to the right edge precedes the AI
    /// panel, which precedes the web sidebar вЂ” mirroring
    /// [`Self::page_content_width_css`].
    fn right_dock(&self) -> Option<(&'static str, f32)> {
        self.dockable_sidebars().into_iter().find_map(|(id, visible, default_w)| {
            (visible && self.sidebar_dock_side(id) == panel_layout::Dock::Right)
                .then(|| (id, self.panel_layout.width_for(id, default_w)))
        })
    }

    /// Move the active docked sidebar to the opposite window edge, persist the
    /// choice, and relayout. The "active" sidebar is the first visible one in
    /// [`Self::dockable_sidebars`] order (tab sidebars, then AI, then web).
    /// Refuses the move when the target edge is already occupied by another
    /// docked panel (avoids overlap), and is a no-op when no sidebar is open.
    /// Returns `true` if a panel was moved.
    fn flip_active_sidebar_dock(&mut self) -> bool {
        let Some((id, _, _)) = self
            .dockable_sidebars()
            .into_iter()
            .find(|(_, visible, _)| *visible)
        else {
            return false;
        };
        let target = self.sidebar_dock_side(id).opposite();
        let occupied = match target {
            panel_layout::Dock::Left => self.left_dock().is_some(),
            panel_layout::Dock::Right => self.right_dock().is_some(),
        };
        if occupied {
            return false;
        }
        self.panel_layout.set_dock(id, target);
        self.panel_layout.save();
        // ADR-016 M2.2b: dock side flip shifts the content viewport; async-safe.
        self.relayout_chrome();
        true
    }

    /// Shift every rect-bearing command in `cmds` right by `dx` CSS px.
    ///
    /// Used to re-home a left-relative sidebar display list onto the right edge
    /// when its dock side is flipped. The tab sidebars emit only `FillRect`,
    /// `FillRoundedRect`, and `DrawText`; other variants are left untouched.
    fn offset_overlay_x(cmds: &mut lumen_paint::DisplayList, dx: f32) {
        if dx == 0.0 {
            return;
        }
        for cmd in cmds.iter_mut() {
            match cmd {
                lumen_paint::DisplayCommand::FillRect { rect, .. }
                | lumen_paint::DisplayCommand::FillRoundedRect { rect, .. }
                | lumen_paint::DisplayCommand::DrawText { rect, .. } => rect.x += dx,
                _ => {}
            }
        }
    }

    /// `(left, right)` docked-sidebar widths in CSS px (0 when not visible).
    fn docked_panel_offsets(&self) -> (f32, f32) {
        (
            self.left_dock().map_or(0.0, |(_, w)| w),
            self.right_dock().map_or(0.0, |(_, w)| w),
        )
    }

    /// If the cursor at `(x_css, y_css)` is within [`panel_layout::RESIZE_GRAB`]
    /// of a visible docked sidebar's inner edge (and below the tab bar), return
    /// the `(dock side, panel id)` a press there would start resizing.
    ///
    /// Left docks have their handle at `x = width`; right docks at
    /// `x = viewport_width в€’ width`.
    fn resize_edge_at(&self, x_css: f32, y_css: f32) -> Option<(panel_layout::Dock, &'static str)> {
        if y_css < toolbar::CHROME_H {
            return None;
        }
        let grab = panel_layout::RESIZE_GRAB;
        if let Some((id, w)) = self.left_dock()
            && (x_css - w).abs() <= grab
        {
            return Some((panel_layout::Dock::Left, id));
        }
        if let Some((id, w)) = self.right_dock()
            && (x_css - (self.viewport_width_css() - w)).abs() <= grab
        {
            return Some((panel_layout::Dock::Right, id));
        }
        None
    }

    /// Apply an in-flight docked-panel resize drag: turn the cursor x into a new
    /// width for the dragged dock, store it (clamped) in [`Self::panel_layout`],
    /// and relayout the page. Returns `true` if the width changed.
    fn drag_panel_resize(&mut self, x_css: f32) -> bool {
        let Some((dock, id)) = self.panel_resize else {
            return false;
        };
        let new_w = dock.width_from_cursor(x_css, self.viewport_width_css());
        if self.panel_layout.set_width(id, new_w) {
            // ADR-016 M2.2b: docked-panel resize changes the content viewport
            // width; async-safe (the drag edge itself follows the cursor via the
            // immediate redraw, only the page reflow underneath is deferred).
            self.relayout_chrome();
            true
        } else {
            false
        }
    }

    /// Open the sidebar with `url` and populate it with a freshly-laid-out page.
    ///
    /// Parses `html_bytes` as HTML, lays it out at [`PANEL_WIDTH`]-wide viewport,
    /// and stores the display list in the sidebar panel.  Triggers a relayout of
    /// the main page when the sidebar becomes visible (page width changes).
    fn open_sidebar_page(&mut self, url: String, html_bytes: &[u8], page_title: String) {
        let was_visible = self.sidebar.visible;
        self.sidebar.open(url.clone());

        // Decode bytes and parse HTML.
        let encoding = lumen_encoding::detect(html_bytes, None);
        let source_str = lumen_encoding::decode(encoding, html_bytes);
        let doc = lumen_html_parser::parse(&source_str);
        let doc_title = if page_title.is_empty() {
            extract_title(&doc).unwrap_or_default()
        } else {
            page_title
        };

        // Collect inline <style> blocks (no external CSS fetch for sidebar).
        let css_text = extract_style_blocks(&doc);
        let sheet = lumen_css_parser::parse(&css_text);

        let doc_arc = Arc::new(Mutex::new(doc));
        let src = LayoutSource {
            document: doc_arc,
            stylesheet: Arc::new(sheet),
            html_source: None,
            // Sidebar panel, not the main navigable page вЂ” not bfcache-tracked.
            cache_control_no_store: false,
            // BUG-743: the sidebar page runs no scripts, so no `<style>` can
            // appear in it after this build.
            dynamic_css: None,
        };

        let sidebar_vp = Size::new(
            self.panel_layout
                .width_for(panel_layout::ID_SIDEBAR, panels::sidebar_panel::PANEL_WIDTH),
            self.viewport_height_css().max(100.0),
        );
        let (dl, _lb) = relayout_page(&src, sidebar_vp, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        let content_h = content_height_of(&dl);
        self.sidebar.set_page(dl, doc_title, content_h);
        // Retain the parsed source so a later drag-resize can reflow the page to
        // the new width (F2-6) instead of stretching the frozen display list.
        self.sidebar_source = Some(src);

        if !was_visible {
            // ADR-016 M2.2b: the sidebar becoming visible narrows the main page's
            // content viewport; async-safe chrome-inset relayout.
            self.relayout_chrome();
        }
        self.request_redraw();
    }

    /// Reflow the web sidebar page to the current sidebar width.
    ///
    /// Re-runs layout over the retained [`Self::sidebar_source`] at the panel's
    /// active `panel_layout` width, replacing the frozen display list while
    /// preserving the title and clamping the scroll offset. No-op when the
    /// sidebar has no open page. Called on a sidebar resize drag release (F2-6).
    fn relayout_sidebar(&mut self) {
        let Some(src) = self.sidebar_source.as_ref() else {
            return;
        };
        let width = self
            .panel_layout
            .width_for(panel_layout::ID_SIDEBAR, panels::sidebar_panel::PANEL_WIDTH);
        let vp = Size::new(width, self.viewport_height_css().max(100.0));
        let (dl, _lb) = relayout_page(src, vp, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        let content_h = content_height_of(&dl);
        self.sidebar.update_page(dl, content_h);
        self.request_redraw();
    }

    /// Reload workspace list from SQLite storage into the panel cache.
    ///
    /// Call this after every `Workspaces::create`, `rename`, or `delete` so
    /// the panel renders up-to-date chips on the next redraw.
    fn refresh_workspaces(&mut self) {
        let entries = self
            .workspaces
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|w| {
                let accent = panels::workspace_panel::parse_ws_color(&w.color);
                panels::workspace_panel::WsEntry {
                    id: w.id,
                    name: w.name,
                    accent,
                }
            })
            .collect();
        self.workspace_panel.set_workspaces(entries);
    }

    /// Reload the profile list from `ProfileRegistry` into the dropdown's
    /// cache (DS-14). Cheap вЂ” the registry only ever holds a handful of
    /// rows вЂ” called each time the dropdown opens so it reflects any
    /// external edit to `profiles.db` between sessions.
    fn refresh_profile_menu_entries(&mut self) {
        let entries: Vec<panels::profile_menu::ProfileEntry> = self
            .profiles
            .list_all()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, p)| panels::profile_menu::ProfileEntry {
                id: p.id,
                name: p.name.clone(),
                color: panels::profile_menu::color_for_profile(&p.name, i),
            })
            .collect();
        self.profile_menu.set_entries(entries);
    }

    /// `true` while the active profile is the seeded Anonymous profile
    /// (DS-16 ephemeral slice, ADR-020) вЂ” gates history writes, the
    /// history-panel banner, and which cookie jar navigation uses.
    fn active_profile_is_anonymous(&self) -> bool {
        self.profile_menu
            .active_entry()
            .is_some_and(|e| panels::profile_menu::is_anonymous(&e.name))
    }

    /// Cookie jar used for outgoing HTTP requests on the active tab: the
    /// shared jar for every profile except Anonymous, which gets its own
    /// ephemeral jar (DS-16) so its cookies never leak into вЂ” or persist
    /// past вЂ” any other profile's browsing.
    fn active_cookie_jar(&self) -> Arc<lumen_storage::CookieJar> {
        if self.active_profile_is_anonymous() {
            Arc::clone(&self.anonymous_cookie_jar)
        } else {
            Arc::clone(&self.cookie_jar)
        }
    }

    /// Reload the bookmark list from storage into the panel cache.
    ///
    /// Call this after every bookmark mutation (add / delete / move) so the
    /// panel renders up-to-date rows on the next redraw.
    /// Reload the read-later entry list from the in-memory store into the panel cache.
    ///
    /// Called after every save/delete and when the panel opens.  Shows the 50
    /// most recent items (unread first, then read, then archived).
    /// Toggle Reader View (В§D-3, F9).
    ///
    /// When entering reader mode: extracts the article region from the current
    /// page's HTML source, wraps it in a clean reading template, and re-renders
    /// it as an in-memory `PageSource::Snapshot` without a network round-trip.
    /// The original source is stashed in `reader_original_source`.
    ///
    /// When exiting: restores the stashed source and reloads.
    fn toggle_reader_view(&mut self) {
        if let Some(original) = self.reader_original_source.take() {
            // Exit reader mode вЂ” restore original page.
            self.source = original;
            self.reload();
            return;
        }

        // Enter reader mode вЂ” extract article from current HTML source.
        let html = match self.layout_source.as_ref().and_then(|ls| ls.html_source.as_deref()) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return, // nothing to extract from
        };

        let Some(article) = reader_view::extract_article(&html) else { return };
        let reader_html = reader_view::build_reader_html(&article);

        let base_url = self.source.url_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "about:reader".to_owned());

        self.reader_original_source = Some(self.source.clone());
        self.source = PageSource::Snapshot { html: reader_html, base_url };
        self.reload();
    }

    /// Show syntax-highlighted source of the current page (Ctrl+U, В§D-2).
    ///
    /// Uses the already-parsed HTML stored in `layout_source.html_source`.
    /// No-op when the page has no HTML source (e.g. empty tab).
    fn show_view_source(&mut self) {
        let html = match self.layout_source.as_ref().and_then(|ls| ls.html_source.as_deref()) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return,
        };
        let url = self.source.url_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "about:source".to_owned());
        let source_html = source_view::build_view_source_html(&url, &html);
        self.navigate_to(PageSource::Snapshot {
            html: source_html,
            base_url: format!("view-source:{url}"),
        });
    }

    /// Fetch `url` and display its raw bytes as syntax-highlighted source (В§D-2).
    ///
    /// Used when the user types `view-source:<url>` in the address bar.
    fn show_view_source_for_url(&mut self, url: &str) {
        let source = PageSource::from_arg(Some(url));
        let sink = Arc::clone(&self.event_sink);
        let jar = self.active_cookie_jar();
        match source.load_bytes(sink, Some(jar)) {
            Ok(raw) => {
                let html_str = String::from_utf8_lossy(&raw.bytes).into_owned();
                let source_html = source_view::build_view_source_html(url, &html_str);
                self.navigate_to(PageSource::Snapshot {
                    html: source_html,
                    base_url: format!("view-source:{url}"),
                });
            }
            Err(e) => {
                eprintln!("view-source: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РіСЂСѓР·РёС‚СЊ {url}: {e}");
            }
        }
    }

    fn refresh_read_later(&mut self) {
        let mut entries = self
            .read_later_store
            .list_by_status(lumen_knowledge::ReadStatus::Unread, 50)
            .unwrap_or_default();
        entries.extend(
            self.read_later_store
                .list_by_status(lumen_knowledge::ReadStatus::Read, 50)
                .unwrap_or_default(),
        );
        self.read_later_panel.refresh(entries);
    }

    fn refresh_bookmarks(&mut self) {
        let entries = self
            .bookmarks
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .map(|b| panels::bookmark_panel::BmEntry {
                id: b.id,
                url: b.url,
                title: b.title,
                folder: b.folder,
            })
            .collect();
        self.bookmark_panel.set_data(entries);
    }

    /// Reload the history panel data from `history_store`.
    ///
    /// When `history_panel.query` is non-empty, uses `HistoryFts::search` for
    /// full-text matching. Otherwise falls back to `History::recent(50)`.
    fn refresh_history(&mut self) {
        let query = self.history_panel.query.trim().to_owned();
        let items: Vec<panels::history_panel::HistoryItem> = if query.is_empty() {
            self.history_store
                .recent(50)
                .unwrap_or_default()
                .into_iter()
                .map(|e| panels::history_panel::HistoryItem {
                    id: e.id,
                    url: e.url,
                    title: e.title,
                    visit_date: e.visit_date,
                    visit_count: e.visit_count,
                })
                .collect()
        } else {
            self.history_fts
                .search(&query, 50)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(i, hit)| panels::history_panel::HistoryItem {
                    id: i as i64 + 1,
                    url: hit.url,
                    title: hit.title,
                    visit_date: 0,
                    visit_count: 1,
                })
                .collect()
        };
        self.history_panel.set_items(items);
    }

    /// Rebuild the command-palette item list: curated commands, every bookmark,
    /// and вЂ” when the query is non-empty вЂ” matching history pages (FTS).
    ///
    /// History depends on the query (the FTS index has no "list all"), so this
    /// is called both on open and on every query edit. Commands and bookmarks
    /// are query-independent; the palette's own fuzzy filter ranks the union.
    fn refresh_palette_items(&mut self) {
        use panels::command_palette::{PaletteAction, PaletteItem};

        let mut items: Vec<PaletteItem> =
            PaletteAction::all().iter().copied().map(PaletteItem::command).collect();

        // Bookmarks (query-independent вЂ” fuzzy-filtered in the palette).
        for b in self.bookmarks.list_all().unwrap_or_default() {
            items.push(PaletteItem::bookmark(b.title, b.url));
        }

        // History: FTS needs a query, so only add hits once the user types.
        let query = self.command_palette.query.trim().to_owned();
        if !query.is_empty()
            && let Ok(hits) = self.history_fts.search(&query, 12)
        {
            for hit in hits {
                items.push(PaletteItem::history(hit.title, hit.url));
            }
        }

        self.command_palette.set_items(items);
    }

    /// Handle a key while the command palette modal is open.
    ///
    /// Always returns `true` (the modal swallows every key). `Esc` closes,
    /// `Enter` activates the selected item, `в†‘/в†“` move the selection,
    /// `Backspace` edits the query, and printable characters extend it. Editing
    /// the query refreshes history results.
    fn handle_palette_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.command_palette.close();
                self.request_redraw();
            }
            KeyCode::ArrowDown if !key_event.repeat => {
                self.command_palette.select_next();
                self.request_redraw();
            }
            KeyCode::ArrowUp if !key_event.repeat => {
                self.command_palette.select_prev();
                self.request_redraw();
            }
            KeyCode::Enter if !key_event.repeat => {
                if let Some(item) = self.command_palette.selected_item().cloned() {
                    self.command_palette.close();
                    self.activate_palette(&item, event_loop);
                }
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.command_palette.backspace();
                self.refresh_palette_items();
                self.request_redraw();
            }
            _ => {
                // Ignore modified keys other than the toggle (handled globally).
                if self.modifiers.control_key() || self.modifiers.super_key() {
                    return false;
                }
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                    && !text.chars().any(char::is_control)
                {
                    self.command_palette.append(text);
                    self.refresh_palette_items();
                    self.request_redraw();
                }
            }
        }
        true
    }

    /// Execute the action behind a selected palette item: run the command, or
    /// navigate to the bookmark / history URL.
    fn activate_palette(
        &mut self,
        item: &panels::command_palette::PaletteItem,
        event_loop: &ActiveEventLoop,
    ) {
        use panels::command_palette::{PaletteAction, PaletteKind};
        match &item.kind {
            PaletteKind::Bookmark | PaletteKind::History => {
                if !item.url.is_empty() {
                    self.navigate_to(PageSource::from_arg(Some(&item.url)));
                }
            }
            PaletteKind::Command(action) => match action {
                PaletteAction::NewTab => self.open_new_tab(),
                PaletteAction::CloseTab => {
                    let idx = self.tab_strip.active;
                    self.close_tab(idx, event_loop);
                }
                PaletteAction::Reload => self.reload(),
                PaletteAction::NavigateBack => self.navigate_back(),
                PaletteAction::NavigateForward => self.navigate_forward(),
                PaletteAction::FindOnPage => {
                    self.hint.close();
                    self.find.open();
                }
                PaletteAction::OpenAddressBar => {
                    self.hint.close();
                    let current = self.current_display_url().to_owned();
                    self.address_bar.open(&current);
                    // CC-7: see the comment on the matching call in
                    // `Self::handle_address_bar_key`.
                    self.relayout_chrome_host();
                }
                PaletteAction::ToggleBookmarks => {
                    self.bookmark_panel.toggle();
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                }
                PaletteAction::BookmarkCurrentPage => self.bookmark_current_page(),
                PaletteAction::ToggleVerticalTabs => {
                    self.vertical_tabs.toggle();
                    self.persist_tab_layout();
                    // ADR-016 M2.2b: async-safe chrome-inset relayout.
                    self.relayout_chrome();
                }
                PaletteAction::ToggleDevConsole => self.devtools_console.toggle(),
                PaletteAction::ToggleShields => self.shields.toggle(),
                PaletteAction::ToggleVimMode => {
                    if self.vim_mode.is_some() {
                        self.vim_mode = None;
                    } else {
                        self.vim_mode = Some(input::vim::VimMode::new());
                    }
                }
            },
        }
        self.request_redraw();
    }

    /// Add the current page to bookmarks (Ctrl+D).
    ///
    /// No-op when the current page has no URL (e.g. blank tab). The active tab
    /// title is used when available, otherwise the URL stands in as the title.
    ///
    /// Also populates the AI summary/embedding (В§12.8, Step 6) via
    /// [`Self::ai_backend`]: with the default [`lumen_core::NullAiBackend`]
    /// `summarise`/`embed` return empty, so `set_semantic` is simply skipped вЂ”
    /// no `feature = "ai"` gate needed here.
    fn bookmark_current_page(&mut self) {
        let url = self.current_display_url().to_owned();
        if url.is_empty() {
            return;
        };
        let title = self
            .tab_strip
            .tabs
            .get(self.tab_strip.active)
            .map(|t| t.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| url.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = self.bookmarks.add(&url, &title, "", &[], "", now);
        let summary = self.ai_backend.summarise(&self.current_page_text());
        if !summary.is_empty() {
            let embedding = self.ai_backend.embed(&summary);
            let embedding_bytes = (!embedding.is_empty())
                .then(|| lumen_storage::bookmarks::embedding_to_bytes(&embedding));
            let _ = self
                .bookmarks
                .set_semantic(&url, Some(&summary), embedding_bytes.as_deref());
        }
        if self.bookmark_panel.visible {
            self.refresh_bookmarks();
        }
    }

    /// Concatenated visible text of the current page, for AI summarisation
    /// (В§12.8). Empty string when there's no layout tree yet.
    fn current_page_text(&self) -> String {
        let Some(lb) = &self.layout_box else {
            return String::new();
        };
        lumen_layout::collect_visible_text(lb)
            .into_iter()
            .map(|f| f.text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Answer an `@ai <query>` omnibox prompt (В§12.5, Step 7).
    ///
    /// Grounds the answer in bookmark embeddings (В§12.8) вЂ” the only
    /// `DefaultKnowledgeStore`-populatable data this shell has today; wiring a
    /// real browsing-history population path is deferred (see
    /// `subsystems/ai.md` В§Deferred, needs its own task brief). Rebuilds an
    /// in-memory `DefaultKnowledgeStore` per query rather than caching one on
    /// `Lumen`, mirroring `query_omnibox_suggestions`'s existing synchronous
    /// per-keystroke `@bookmarks` embed call.
    #[cfg(feature = "ai")]
    fn ai_answer_for(&self, query: &str) -> String {
        use lumen_ai::embedding::OllamaEmbeddingBackend;
        use lumen_ai::generation::OllamaGenerationBackend;
        use lumen_ai::rag::RagEngine;
        use lumen_knowledge::DefaultKnowledgeStore;

        let Ok(store) = DefaultKnowledgeStore::open_in_memory() else {
            return self.ai_backend.query(query);
        };
        if let Ok(bookmarks) = self.bookmarks.list_all() {
            for b in bookmarks {
                if let Some(embedding) = &b.embedding {
                    store.index_semantic(
                        b.id,
                        &b.url,
                        &b.title,
                        lumen_storage::bookmarks::embedding_from_bytes(embedding),
                    );
                }
            }
        }
        let embedding_backend = OllamaEmbeddingBackend::new("nomic-embed-text");
        let generation_backend = OllamaGenerationBackend::new("phi3:mini");
        let answer = RagEngine::new(5).answer(query, &store, &embedding_backend, &generation_backend);
        // Ollama unreachable/erroring в†’ fall back to the NullAiBackend stub
        // message, matching ADR-019's documented degrade-not-error contract.
        if answer.is_empty() { self.ai_backend.query(query) } else { answer }
    }

    /// `--features ai` not compiled in: static hint row, no `lumen-ai` calls.
    #[cfg(not(feature = "ai"))]
    fn ai_answer_for(&self, _query: &str) -> String {
        "AI module not enabled вЂ” rebuild with `cargo build --features ai` \
         (requires a local Ollama daemon, see ADR-019)."
            .to_owned()
    }

    /// РњР°РєСЃРёРјР°Р»СЊРЅС‹Р№ РІР°Р»РёРґРЅС‹Р№ scroll_y: РЅРёС‡РµРіРѕ РЅРµ СЃРєСЂРѕР»Р»РёРј, РµСЃР»Рё РєРѕРЅС‚РµРЅС‚
    /// РїРѕРјРµС‰Р°РµС‚СЃСЏ РІ viewport. РРЅР°С‡Рµ вЂ” `content_height в€’ viewport_height`.
    fn max_scroll(&self) -> f32 {
        (self.content_height - self.viewport_height_css()).max(0.0)
    }

    /// РњР°РєСЃРёРјР°Р»СЊРЅС‹Р№ РІР°Р»РёРґРЅС‹Р№ scroll_x: 0 РµСЃР»Рё РєРѕРЅС‚РµРЅС‚ РїРѕРјРµС‰Р°РµС‚СЃСЏ РїРѕ С€РёСЂРёРЅРµ.
    ///
    /// РСЃРїРѕР»СЊР·СѓРµС‚ `page_content_width_css()` вЂ” РїРѕР»РЅР°СЏ С€РёСЂРёРЅР° РјРёРЅСѓСЃ РїР°РЅРµР»СЊ РІРєР»Р°РґРѕРє.
    fn max_scroll_x(&self) -> f32 {
        (self.content_width - self.page_content_width_css()).max(0.0)
    }

    /// Rebuild `snap_containers` from the current `layout_box`.
    ///
    /// Called whenever `layout_box` changes (relayout, page load, tab switch).
    /// Cheap when the page has no `scroll-snap-type` declarations (returns empty).
    fn update_snap_containers(&mut self) {
        match &self.layout_box {
            Some(lb) => self.snap_containers = collect_snap_containers(lb),
            None => self.snap_containers.clear(),
        }
    }

    /// Rebuild `scroll_containers` from the current `layout_box`.
    ///
    /// Called whenever `layout_box` changes (relayout, page load, tab switch).
    /// Used by the wheel handler to route scroll events to overflow containers.
    fn update_scroll_containers(&mut self) {
        match &self.layout_box {
            Some(lb) => self.scroll_containers = collect_scroll_containers(lb),
            None => self.scroll_containers.clear(),
        }
    }

    /// Try to scroll an overflow container under the cursor by `(dx, dy)` CSS px.
    ///
    /// Returns `true` if a container was found and scrolled, `false` if no
    /// overflow container is under the cursor (caller should scroll the page).
    ///
    /// The cursor position is converted from physical pixels to document-space
    /// CSS px (adds page scroll offsets so hit-testing works on scrolled pages).
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn try_scroll_overflow_container(&mut self, dx: f32, dy: f32) -> bool {
        let Some(cursor) = self.cursor_position else { return false };
        if self.layout_box.is_none() { return false; }

        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32);
        let x_css = (cursor.x as f32) / dpr + self.scroll_x;
        let y_css = (cursor.y as f32) / dpr + self.scroll_y;

        let Some(target) = find_scroll_container_at(&self.scroll_containers, x_css, y_css) else {
            return false;
        };
        let target_nid = target.index() as u32;

        // Find current position and compute new target.
        let current = self.scroll_containers.iter()
            .find(|c| c.node == target)
            .map(|c| (c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height,
                      c.clip_rect.width, c.clip_rect.height,
                      c.overscroll_behavior_x, c.overscroll_behavior_y));
        let Some((cur_x, cur_y, sw, sh, clip_w, clip_h, ob_x, ob_y)) = current else { return false };

        let new_x = (cur_x + dx).clamp(0.0, (sw - clip_w).max(0.0));
        let new_y = (cur_y + dy).clamp(0.0, (sh - clip_h).max(0.0));

        // CSS Overscroll Behavior L1 В§3 вЂ” scroll-chain stop. If the container is
        // at its boundary on every axis and `overscroll-behavior` permits it, let
        // the residual delta propagate to the page; otherwise the chain stops
        // here (event consumed even if the container did not move).
        let moved_x = (new_x - cur_x).abs() > f32::EPSILON;
        let moved_y = (new_y - cur_y).abs() > f32::EPSILON;
        if lumen_layout::overscroll_should_propagate(ob_x, ob_y, dx, dy, moved_x, moved_y) {
            return false;
        }
        if !moved_x && !moved_y {
            // Boundary reached but propagation is blocked (contain/none) вЂ” consume
            // the gesture without a relayout/redraw.
            return true;
        }

        // Borrow layout_box mutably after releasing the immutable scroll_containers borrow.
        let scrolled = if let Some(lb) = self.layout_box.as_mut() {
            set_scroll_position(lb, target, new_x, new_y)
        } else {
            false
        };
        if scrolled {
            // Р‘С‹СЃС‚СЂС‹Р№ РїСѓС‚СЊ: С‚РѕС‡РµС‡РЅС‹Р№ РїР°С‚С‡ СЃРєСЂРѕР»Р»-СЃР»РѕСЏ РІ РіРѕС‚РѕРІРѕРј display list вЂ”
            // layout РґРµС‚РµР№ РїСЂРё СЃРєСЂРѕР»Р»Рµ РЅРµ РјРµРЅСЏРµС‚СЃСЏ, РїРѕСЌС‚РѕРјСѓ РїРѕР»РЅР°СЏ РїРµСЂРµСЃР±РѕСЂРєР°
            // paint_ordered РЅР° РєР°Р¶РґС‹Р№ С‚РёРє РєРѕР»РµСЃР° РЅРµ РЅСѓР¶РЅР° (СЃРј.
            // lumen_paint::patch_scroll_layer; СЌРєРІРёРІР°Р»РµРЅС‚РЅРѕСЃС‚СЊ РїРµСЂРµСЃР±РѕСЂРєРµ
            // Р·Р°РєСЂРµРїР»РµРЅР° С‚РµСЃС‚Р°РјРё patch_scroll_layer_* РІ display_list.rs).
            // РЎРїРёСЃРѕРє РїСЂР°РІРёС‚СЃСЏ РќРђ РњР•РЎРўР• вЂ” РІРµСЂСЃРёСЋ Р±Р°РјРїР°РµРј Р·Р°СЂР°РЅРµРµ: Р·Р°РјС‹РєР°РЅРёРµ РЅРёР¶Рµ
            // Р·Р°С…РІР°С‚С‹РІР°РµС‚ С‚РѕР»СЊРєРѕ РїРѕР»Рµ `display_list` (`layout_box` Р·Р°РЅСЏС‚
            // СЃРѕСЃРµРґРЅРёРј Р·Р°РёРјСЃС‚РІРѕРІР°РЅРёРµРј), РїРѕСЌС‚РѕРјСѓ `&mut self` РІРЅСѓС‚СЂРё РЅРµРіРѕ РЅРµС‚.
            self.bump_display_list_epoch();
            let patched = lumen_layout::find_box_by_node(
                self.layout_box.as_ref().unwrap(),
                target,
            )
            .is_some_and(|cb| lumen_paint::patch_scroll_layer(&mut self.display_list, cb));
            if patched {
                // РўРѕС‡РµС‡РЅР°СЏ РїСЂР°РІРєР°: РіСЂСЏР·РЅС‹Рµ С‚РѕР»СЊРєРѕ С‚Р°Р№Р»С‹ РїРѕРґ РєРѕРЅС‚РµР№РЅРµСЂРѕРј.
                if let Some(c) = self.scroll_containers.iter().find(|c| c.node == target) {
                    self.tile_grid.mark_rect_dirty(c.clip_rect);
                }
            } else {
                // Fallback: РїРѕР»РЅР°СЏ РїРµСЂРµСЃР±РѕСЂРєР° РїСЂРё Р»СЋР±РѕР№ РЅРµСЃС‚Р°РЅРґР°СЂС‚РЅРѕР№ СЃС‚СЂСѓРєС‚СѓСЂРµ DL.
                let new_dl = paint_ordered(self.layout_box.as_ref().unwrap());
                self.tile_grid.update_from_diff(&self.display_list, &new_dl);
                self.set_display_list(new_dl);
            }
            self.update_scroll_containers();
            let states: std::collections::HashMap<_, _> = self.scroll_containers.iter()
                .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                .collect();
            // ADR-016 M2.2c-2d (16): overflow-container scroll fire-and-forget void
            // (`update_scroll_states` push в†’ `fire_element_scroll`) С‡РµСЂРµР· `route_task_js`.
            // `states` (owned `HashMap`) Рё `target_nid` (`u32`, Copy) РїРµСЂРµРµР·Р¶Р°СЋС‚ РІ
            // `move`-Р·Р°РјС‹РєР°РЅРёРµ `Send + 'static`; РїРѕСЂСЏРґРѕРє pushв†’dispatch СЃРѕС…СЂР°РЅС‘РЅ РІРЅСѓС‚СЂРё
            // РѕРґРЅРѕРіРѕ `task`. РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґРёС‚ off-UI-thread;
            // Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ**.
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.update_scroll_states(states);
                js.fire_element_scroll(target_nid);
                // BUG-822: one wheel notch over a container is applied
                // instantly, so it is a complete scroll sequence of its own вЂ”
                // unlike the page, which routes the wheel through
                // `scroll_by_smooth` and therefore ends once per animation.
                js.fire_element_scrollend(target_nid);
            });
            self.request_redraw();
            true
        } else {
            false
        }
    }

    /// BUG-338: bring `target_rect` (a target element's absolute border-box
    /// rect) into view within every scrolling overflow ancestor of `node`,
    /// vertical axis only вЂ” the ancestor-walk part of `Element.scrollIntoView()`
    /// that fragment navigation is supposed to invoke but never did (only the
    /// page-level scroll below ran). Walks the DOM parent chain from `node`,
    /// scrolls each `ScrollContainer` match whose current viewport doesn't
    /// already contain `target_rect` just enough to bring it in (align the
    /// nearer edge), and leaves already-visible containers untouched. Content
    /// boxes carry absolute (unscrolled) coordinates вЂ” see `PushScrollLayer`'s
    /// paint-time `translate(-scroll_x, -scroll_y)` вЂ” so each container's
    /// adjustment is independent of its ancestors' own scroll offset.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn scroll_nested_ancestors_into_view(&mut self, node: NodeId, target_rect: lumen_core::geom::Rect) {
        let Some(src) = self.layout_source.as_ref() else { return };
        let mut ancestor = src.document.lock().unwrap().get(node).parent;
        while let Some(n) = ancestor {
            let Some(c) = self.scroll_containers.iter().find(|c| c.node == n) else {
                ancestor = src.document.lock().unwrap().get(n).parent;
                continue;
            };
            let visible_top = target_rect.y - c.scroll_y;
            let visible_bottom = target_rect.y + target_rect.height - c.scroll_y;
            let new_scroll_y = if visible_top < c.clip_rect.y {
                c.scroll_y - (c.clip_rect.y - visible_top)
            } else if visible_bottom > c.clip_rect.y + c.clip_rect.height {
                c.scroll_y + (visible_bottom - (c.clip_rect.y + c.clip_rect.height))
            } else {
                c.scroll_y
            };
            if (new_scroll_y - c.scroll_y).abs() > f32::EPSILON
                && let Some(lb) = self.layout_box.as_mut()
            {
                set_scroll_position(lb, n, c.scroll_x, new_scroll_y);
            }
            ancestor = src.document.lock().unwrap().get(n).parent;
        }
        self.update_scroll_containers();
    }

    /// Apply CSS Scroll Snap L1 to a proposed page-level Y scroll offset.
    ///
    /// Finds the snap container whose node matches the root layout box (html
    /// element), overrides its rect with the viewport dimensions (the snap port
    /// for page scroll is the viewport, not the full document), then calls
    /// `find_snap_target`. Returns `target_y` unchanged if no snap applies.
    fn apply_page_y_snap(&self, target_y: f32) -> f32 {
        let root_node = match &self.layout_box {
            Some(lb) => lb.node,
            None => return target_y,
        };
        let vw = self.viewport_width_css();
        let vh = self.viewport_height_css();
        for sc in &self.snap_containers {
            if sc.node == root_node {
                // Proximity threshold uses viewport size, not full document size.
                let mut sc_viewport = sc.clone();
                sc_viewport.rect = lumen_core::geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some((_, sy)) = find_snap_target(
                    &sc_viewport,
                    (self.scroll_x, self.scroll_y),
                    (self.scroll_x, target_y),
                ) {
                    return clamp_scroll(sy, self.max_scroll());
                }
            }
        }
        target_y
    }

    /// Apply CSS Scroll Snap L1 to a proposed page-level X scroll offset.
    ///
    /// Mirror of `apply_page_y_snap` for horizontal scroll.
    fn apply_page_x_snap(&self, target_x: f32) -> f32 {
        let root_node = match &self.layout_box {
            Some(lb) => lb.node,
            None => return target_x,
        };
        let vw = self.viewport_width_css();
        let vh = self.viewport_height_css();
        for sc in &self.snap_containers {
            if sc.node == root_node {
                let mut sc_viewport = sc.clone();
                sc_viewport.rect = lumen_core::geom::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some((sx, _)) = find_snap_target(
                    &sc_viewport,
                    (self.scroll_x, self.scroll_y),
                    (target_x, self.scroll_y),
                ) {
                    return clamp_scroll(sx, self.max_scroll_x());
                }
            }
        }
        target_x
    }

    /// Р“РѕСЂРёР·РѕРЅС‚Р°Р»СЊРЅС‹Р№ СЃРєСЂРѕР»Р» РЅР° delta CSS px (РёРЅСЃС‚Р°РЅС‚РЅС‹Р№).
    fn scroll_x_by(&mut self, delta: f32) {
        let clamped = clamp_scroll(self.scroll_x + delta, self.max_scroll_x());
        let snapped = self.apply_page_x_snap(clamped);
        if (snapped - self.scroll_x).abs() > f32::EPSILON {
            self.scroll_x = snapped;
            self.request_redraw();
        }
    }

    /// РЈСЃС‚Р°РЅРѕРІРёС‚СЊ scroll_y РІ Р°Р±СЃРѕР»СЋС‚РЅРѕРµ Р·РЅР°С‡РµРЅРёРµ (РїРѕСЃР»Рµ clamping-Р°). `f32::INFINITY`
    /// = В«Рє СЃР°РјРѕРјСѓ РЅРёР·СѓВ», `0.0` = В«РІРІРµСЂС…В». Р—Р°РїСЂР°С€РёРІР°РµС‚ redraw С‚РѕР»СЊРєРѕ РµСЃР»Рё Р·РЅР°С‡РµРЅРёРµ
    /// РґРµР№СЃС‚РІРёС‚РµР»СЊРЅРѕ РёР·РјРµРЅРёР»РѕСЃСЊ вЂ” РёРЅР°С‡Рµ wheel-spam РІ СЃР°РјРѕРј РЅРёР·Сѓ РЅРµ РґС‘СЂРіР°Р» Р±С‹ GPU.
    ///
    /// РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РґР»СЏ РёРЅСЃС‚Р°РЅС‚-РїСѓС‚РµР№: drag thumb scrollbar-Р°. Р”Р»СЏ
    /// РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёС… scroll-РєРѕРјР°РЅРґ (wheel / keys / page-jump / find) вЂ”
    /// `start_smooth_scroll` / `scroll_by_smooth`.
    fn scroll_to(&mut self, target: f32) {
        // РРЅСЃС‚Р°РЅС‚-РїСѓС‚СЊ cancel-РёС‚ Р°РєС‚РёРІРЅСѓСЋ Р°РЅРёРјР°С†РёСЋ вЂ” РјС‹ С‚РѕР»СЊРєРѕ С‡С‚Рѕ
        // *РїСЂРёРєР°Р·Р°Р»Рё* Р±С‹С‚СЊ РІ РєРѕРЅРєСЂРµС‚РЅРѕР№ С‚РѕС‡РєРµ.
        self.scroll_anim = None;
        let clamped = clamp_scroll(target, self.max_scroll());
        if (clamped - self.scroll_y).abs() > f32::EPSILON {
            self.scroll_y = clamped;
            self.request_redraw();
        }
    }

    /// Р—Р°РїСѓСЃС‚РёС‚СЊ smooth-scroll Рє target Y. Cancel-РёС‚ Р°РєС‚РёРІРЅСѓСЋ Р°РЅРёРјР°С†РёСЋ.
    /// Target РєР»Р°РјРїРёС‚СЃСЏ. Р•СЃР»Рё target == С‚РµРєСѓС‰РµРјСѓ scroll_y вЂ” Р°РЅРёРјР°С†РёСЏ РЅРµ
    /// СЃС‚Р°СЂС‚СѓРµС‚ (Рё С‚РµРєСѓС‰Р°СЏ СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ). РџСЂРёРјРµРЅСЏРµС‚ CSS Scroll Snap L1 РµСЃР»Рё
    /// СЃС‚СЂР°РЅРёС†Р° РѕР±СЉСЏРІР»СЏРµС‚ `scroll-snap-type` РЅР° РєРѕСЂРЅРµРІРѕРј СЌР»РµРјРµРЅС‚Рµ.
    fn start_smooth_scroll(&mut self, target: f32) {
        let max = self.max_scroll();
        let target_clamped = clamp_scroll(target, max);
        // Apply page-level CSS Scroll Snap L1: snap to the nearest declared
        // snap point before starting the animation.
        let target_clamped = self.apply_page_y_snap(target_clamped);
        if (target_clamped - self.scroll_y).abs() <= f32::EPSILON {
            self.scroll_anim = None;
            return;
        }
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        self.scroll_anim = Some(scroll_anim::ScrollAnim {
            start_y: self.scroll_y,
            target_y: target_clamped,
            start_time_ms: now_ms,
        });
        self.request_redraw();
    }

    /// Smooth-РІР°СЂРёР°РЅС‚ `scroll_by`. Р•СЃР»Рё СѓР¶Рµ РёРґС‘С‚ Р°РЅРёРјР°С†РёСЏ вЂ” delta
    /// РґРѕР±Р°РІР»СЏРµС‚СЃСЏ Рє РµС‘ target-Сѓ, Р° РЅРµ Рє С‚РµРєСѓС‰РµРјСѓ scroll_y. Р­С‚Рѕ РїСЂР°РІРёР»СЊРЅР°СЏ
    /// СЃРµРјР°РЅС‚РёРєР° РґР»СЏ repeat-input (key-repeat, wheel-spam): РєР°Р¶РґРѕРµ
    /// РЅР°Р¶Р°С‚РёРµ РґРѕРїРёСЃС‹РІР°РµС‚ delta Рє С‚РѕС‡РєРµ РЅР°Р·РЅР°С‡РµРЅРёСЏ, Р° РЅРµ РґС‘СЂРіР°РµС‚ Р°РЅРёРјР°С†РёСЋ
    /// РІ РѕР±СЂР°С‚РЅСѓСЋ СЃС‚РѕСЂРѕРЅСѓ.
    fn scroll_by_smooth(&mut self, delta: f32) {
        let base = self.scroll_anim.as_ref().map_or(self.scroll_y, |a| a.target());
        self.start_smooth_scroll(base + delta);
    }

    /// Scroll the currently focused pane by `delta` CSS px.
    ///
    /// In split mode, routes to the right pane when it has focus; otherwise
    /// falls through to `scroll_by_smooth` for the left (active) pane.
    fn scroll_active_pane(&mut self, delta: f32) {
        // Pre-compute viewport height before mutably borrowing split_view.
        let vh = self.viewport_height_css();
        let right_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);
        if right_focused {
            if let Some(ref mut sv) = self.split_view {
                let max = (sv.right.content_height - vh).max(0.0);
                sv.right.scroll_y = (sv.right.scroll_y + delta).clamp(0.0, max);
            }
            self.request_redraw();
            return;
        }
        self.scroll_by_smooth(delta);
    }

    /// Scroll the currently focused pane to an absolute position.
    ///
    /// `target = f32::INFINITY` scrolls to the bottom of the pane's content.
    fn scroll_active_pane_to(&mut self, target: f32) {
        let vh = self.viewport_height_css();
        let right_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);
        if right_focused {
            if let Some(ref mut sv) = self.split_view {
                let max = (sv.right.content_height - vh).max(0.0);
                sv.right.scroll_y = target.clamp(0.0, max);
            }
            self.request_redraw();
            return;
        }
        self.start_smooth_scroll(target);
    }

    /// CSS Containment L3 В§4.4 (BB-4): РѕР±РЅРѕРІРёС‚СЊ skipped-СЃРѕСЃС‚РѕСЏРЅРёРµ
    /// `content-visibility: auto` РїРѕСЃР»Рµ СЃРјРµРЅС‹ `layout_box` вЂ” РїРµСЂРµСЃРєР°РЅРёСЂРѕРІР°С‚СЊ
    /// РґРµСЂРµРІРѕ, Р·Р°РґРёС„С„Р°С‚СЊ СЃ РїСЂРµРґС‹РґСѓС‰РёРј РїСЂРѕС…РѕРґРѕРј, РґРѕР±Р°РІРёС‚СЊ СЃРѕР±С‹С‚РёСЏ РІ `cv_events`.
    /// Р”СЂРµРЅРёСЂСѓРµС‚ thread-local layout-РєСЂРµР№С‚Р°, С‡С‚РѕР±С‹ Р·Р°РїРёСЃРё РЅРµ РїРµСЂРµР¶РёР»Рё РїСЂРѕС…РѕРґ.
    fn refresh_cv_state(&mut self) {
        let _ = lumen_layout::take_cv_skipped();
        let mut auto_boxes = Vec::new();
        if let Some(lb) = self.layout_box.as_ref() {
            collect_cv_auto(lb, &mut auto_boxes);
        }
        // BUG-852: СЃРѕСЃС‚РѕСЏРЅРёРµ СЃС‡РёС‚Р°РµС‚СЃСЏ С‚РµРј Р¶Рµ РїСЂР°РІРёР»РѕРј СЂРµР»РµРІР°РЅС‚РЅРѕСЃС‚Рё, С‡С‚Рѕ Рё РІ
        // layout (`cv_is_skipped`), Р° РЅРµ РІС‹РІРѕРґРёС‚СЃСЏ РёР· В«РґРµС‚Рё РїСѓСЃС‚С‹В» вЂ” РёРЅР°С‡Рµ
        // РїСѓСЃС‚РѕР№ auto-СЌР»РµРјРµРЅС‚ РЅРµРѕС‚Р»РёС‡РёРј РѕС‚ РїСЂРѕРїСѓС‰РµРЅРЅРѕРіРѕ.
        let scroll_y = self.scroll_y;
        let viewport_h = self.viewport_height_css();
        let next: Vec<(NodeId, bool)> = auto_boxes
            .iter()
            .map(|&(n, top)| {
                let relevant = self.cv_relevant.contains(&n);
                (n, lumen_layout::cv_is_skipped(relevant, top, scroll_y, viewport_h))
            })
            .collect();
        self.cv_events.extend(diff_cv_state(&self.cv_auto_state, &next));
        // РљР°Рї РѕС‡РµСЂРµРґРё: РґРѕСЃС‚Р°РІРєР° РёРґС‘С‚ СЂР°Р· РІ РєР°РґСЂ, РЅРѕ РєР°РґСЂР° РјРѕР¶РµС‚ Рё РЅРµ Р±С‹С‚СЊ
        // (С„РѕРЅРѕРІР°СЏ РІРєР»Р°РґРєР°) вЂ” С…СЂР°РЅРёРј С‚РѕР»СЊРєРѕ С…РІРѕСЃС‚.
        if self.cv_events.len() > 256 {
            let drop_n = self.cv_events.len() - 256;
            self.cv_events.drain(..drop_n);
        }
        self.cv_auto_state = next.iter().copied().collect();
        self.cv_skipped = auto_boxes
            .into_iter()
            .zip(next)
            .filter_map(|((n, top), (_, skipped))| skipped.then_some((n, top)))
            .collect();
    }

    /// Р”РѕСЃС‚Р°РІРёС‚СЊ РЅР°РєРѕРїР»РµРЅРЅС‹Рµ `contentvisibilityautostatechange` РІ JS.
    ///
    /// Р—РѕРІС‘С‚СЃСЏ СЂР°Р· РІ РєР°РґСЂ РёР· `RedrawRequested` вЂ” С€Р°РіР° В«update the renderingВ»,
    /// РІРЅСѓС‚СЂРё РєРѕС‚РѕСЂРѕРіРѕ CSS Contain L2 В§4.1 Рё РѕРїСЂРµРґРµР»СЏРµС‚ СЂРµР»РµРІР°РЅС‚РЅРѕСЃС‚СЊ. РўРѕС‡РєР°
    /// РѕРґРЅР° РЅР° РІСЃРµ РёСЃС‚РѕС‡РЅРёРєРё СЃРѕСЃС‚РѕСЏРЅРёСЏ (Р·Р°РіСЂСѓР·РєР° СЃС‚СЂР°РЅРёС†С‹, СЂРµР»РµР№Р°СѓС‚, ratchet
    /// РїСЂРё СЃРєСЂРѕР»Р»Рµ), РїРѕС‚РѕРјСѓ С‡С‚Рѕ `refresh_cv_state` РІС‹Р·С‹РІР°РµС‚СЃСЏ РёР· С‡РµС‚С‹СЂС‘С… РјРµСЃС‚,
    /// Рё РІ РґРІСѓС… РёР· РЅРёС… JS-РєРѕРЅС‚РµРєСЃС‚ РµС‰С‘ РЅРµ СѓСЃС‚Р°РЅРѕРІР»РµРЅ.
    #[cfg(feature = "v8")]
    fn deliver_cv_state_changes(&mut self) {
        if self.cv_events.is_empty() || !self.js_present {
            // РџРѕРєР° JS-РєРѕРЅС‚РµРєСЃС‚Р° РЅРµС‚, СЃРѕР±С‹С‚РёСЏ РєРѕРїСЏС‚СЃСЏ: СЃС‚СЂР°РЅРёС†Р°, РѕР±СЉСЏРІРёРІС€Р°СЏ
            // `content-visibility: auto` РІ СЂР°Р·РјРµС‚РєРµ, РґРѕР»Р¶РЅР° РїРѕР»СѓС‡РёС‚СЊ РїРµСЂРІРѕРµ
            // РЅР°Р±Р»СЋРґРµРЅРёРµ, РєРѕРіРґР° РµС‘ СЃРєСЂРёРїС‚С‹ СѓР¶Рµ РјРѕРіСѓС‚ СЃР»СѓС€Р°С‚СЊ.
            return;
        }
        let payload: String = {
            let mut s = String::from("[");
            for (i, ev) in self.cv_events.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&format!("[{},{}]", ev.node.index(), ev.skipped));
            }
            s.push(']');
            s
        };
        self.cv_events.clear();
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.deliver_cv_state_changes(&payload);
        });
    }

    /// РЁР°Рі 1.6 В«Update the renderingВ»: РµСЃР»Рё РїСЂРё СЃРєСЂРѕР»Р»Рµ РїСЂРѕРїСѓС‰РµРЅРЅС‹Р№
    /// `content-visibility: auto` СѓР·РµР» РІРѕС€С‘Р» РІ СЂР°СЃС€РёСЂРµРЅРЅС‹Р№ viewport вЂ”
    /// ratchet РІ `cv_relevant` + relayout (РµРіРѕ СЃРѕРґРµСЂР¶РёРјРѕРµ РІС‹РєР»Р°РґС‹РІР°РµС‚СЃСЏ).
    ///
    /// BUG-286: routed through [`Self::relayout_raf_dirty`] (not the direct
    /// synchronous [`Self::relayout`]) so this scroll-time trigger gets the
    /// same off-UI-thread treatment as the other `RedrawRequested` relayout
    /// sites once `LUMEN_ENGINE_THREAD=1` вЂ” this was the one caller still
    /// calling `relayout()` directly. No behavior change on the default
    /// (flag-off) build: `relayout_raf_dirty()` falls back to the same
    /// incremental-then-full sequence.
    fn maybe_expand_cv_relevant(&mut self) {
        if self.cv_skipped.is_empty() {
            return;
        }
        let bound = self.scroll_y
            + self.viewport_height_css() * (1.0 + lumen_layout::CV_SLACK_FACTOR);
        let newly: Vec<NodeId> = self
            .cv_skipped
            .iter()
            .filter(|(n, top)| *top <= bound && !self.cv_relevant.contains(n))
            .map(|&(n, _)| n)
            .collect();
        if newly.is_empty() {
            return;
        }
        self.cv_relevant.extend(newly);
        self.relayout_raf_dirty();
    }

    /// РўРёРє Р°РЅРёРјР°С†РёРё РїРµСЂРµРґ `Renderer::render`. Р•СЃР»Рё Р°РЅРёРјР°С†РёСЏ Р°РєС‚РёРІРЅР° вЂ”
    /// РѕР±РЅРѕРІР»СЏРµС‚ `scroll_y` РїРѕ out-cubic easing Рё РІРѕР·РІСЂР°С‰Р°РµС‚ `true`,
    /// СЃРёРіРЅР°Р»РёР·РёСЂСѓСЏ caller-Сѓ Р·Р°РїСЂРѕСЃРёС‚СЊ РµС‰С‘ РѕРґРёРЅ redraw. РЎР±СЂР°СЃС‹РІР°РµС‚
    /// `scroll_anim` РїРѕ Р·Р°РІРµСЂС€РµРЅРёРё.
    fn advance_scroll_anim(&mut self) -> bool {
        let Some(anim) = self.scroll_anim else {
            return false;
        };
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let (y, done) = anim.sample(now_ms);
        self.scroll_y = clamp_scroll(y, self.max_scroll());
        if done {
            self.scroll_anim = None;
            false
        } else {
            true
        }
    }

    /// ADR-016 M1.3: РїРµСЂРµРґР°С‚СЊ Р°РєС‚РёРІРЅСѓСЋ РёРЅРµСЂС†РёСЋ СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєСѓ, С‡С‚РѕР±С‹ РїСЂРµР·РµРЅС‚Р°С†РёСЏ
    /// РїСЂРѕРґРѕР»Р¶Р°Р»Р°СЃСЊ РЅР° vsync, РґР°Р¶Рµ РµСЃР»Рё UI-РїРѕС‚РѕРє Р·Р°СЃС‚РѕРїРѕСЂРёС‚СЃСЏ (РґРѕР»РіРёР№ JS-С‚РёРє).
    /// No-op РЅР° РѕРґРЅРѕРїРѕС‚РѕС‡РЅРѕРј Р±СЌРєРµРЅРґРµ (РјРµС‚РѕРґ С‚СЂРµР№С‚Р° РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ РїСѓСЃС‚РѕР№), РїРѕСЌС‚РѕРјСѓ
    /// РїСЂРё РІС‹РєР»СЋС‡РµРЅРЅРѕРј `LUMEN_RENDER_THREAD` РїРѕРІРµРґРµРЅРёРµ РЅРµ РјРµРЅСЏРµС‚СЃСЏ.
    fn forward_momentum_start(&mut self, vel_y: f32, vel_x: f32) {
        let max_y = self.max_scroll();
        let max_x = self.max_scroll_x();
        if let Some(r) = self.renderer.as_mut() {
            r.start_render_momentum(vel_y, vel_x, max_y, max_x);
        }
    }

    /// ADR-016 M1.3: РѕС‚РјРµРЅРёС‚СЊ render-side РёРЅРµСЂС†РёСЋ (РЅРѕРІС‹Р№ Р¶РµСЃС‚, РЅР°РІРёРіР°С†РёСЏ, РєРѕРЅРµС†
    /// Р°РЅРёРјР°С†РёРё). No-op РЅР° РѕРґРЅРѕРїРѕС‚РѕС‡РЅРѕРј Р±СЌРєРµРЅРґРµ.
    fn forward_momentum_stop(&mut self) {
        if let Some(r) = self.renderer.as_mut() {
            r.stop_render_momentum();
        }
    }

    /// РўРёРє momentum-Р°РЅРёРјР°С†РёРё. РћР±РЅРѕРІР»СЏРµС‚ `scroll_y` / `scroll_x` РЅР°РїСЂСЏРјСѓСЋ
    /// (Р±РµР· smooth-scroll Р°РЅРёРјР°С†РёРё). Р’РѕР·РІСЂР°С‰Р°РµС‚ `true` РїРѕРєР° Р°РЅРёРјР°С†РёСЏ Р¶РёРІР°.
    fn advance_momentum(&mut self, now_ms: f64) -> bool {
        let Some(ref mut anim) = self.momentum_anim else {
            return false;
        };
        let (dy, dx, done) = anim.advance(now_ms);
        if dy != 0.0 {
            let new_y = clamp_scroll(self.scroll_y + dy, self.max_scroll());
            if (new_y - self.scroll_y).abs() > f32::EPSILON {
                self.scroll_y = new_y;
            }
        }
        if dx != 0.0 {
            let new_x = clamp_scroll(self.scroll_x + dx, self.max_scroll_x());
            if (new_x - self.scroll_x).abs() > f32::EPSILON {
                self.scroll_x = new_x;
            }
        }
        if done {
            self.momentum_anim = None;
            // РРЅРµСЂС†РёСЏ РёСЃСЃСЏРєР»Р° вЂ” СЃРЅСЏС‚СЊ РІР»Р°РґРµРЅРёРµ СЃ СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєР° (РѕРЅ С‚Р°РєР¶Рµ
            // СЃР°РјРѕР·Р°РІРµСЂС€Р°РµС‚СЃСЏ РїРѕ С‚РѕРјСѓ Р¶Рµ РїРѕСЂРѕРіСѓ, РЅРѕ СЏРІРЅР°СЏ РѕС‚РјРµРЅР° РґРµС‚РµСЂРјРёРЅРёСЂСѓРµС‚).
            self.forward_momentum_stop();
            false
        } else {
            true
        }
    }

    /// Drop CPU-decoded images that have scrolled outside the gate zone (ADR-008 В§10E.4).
    ///
    /// Called once per rendered frame (in `RedrawRequested`) after scroll advancement.
    /// No-op when the cache is empty or the layout tree or renderer is unavailable.
    fn try_discard_offscreen_images(&mut self) {
        let (Some(root), Some(renderer)) = (self.layout_box.as_ref(), self.renderer.as_ref()) else {
            return;
        };
        let vp_size = renderer.viewport_size();
        let viewport = Size::new(vp_size.width, vp_size.height);
        scroll::decode_gating::discard_offscreen_images(
            &mut self.image_cache,
            root,
            viewport,
            self.scroll_x,
            self.scroll_y,
        );
    }

    /// РџРµСЂРµСЃС‡РёС‚Р°С‚СЊ Р¶РµР»Р°РµРјС‹Р№ `CursorIcon` РїРѕ С‚РµРєСѓС‰РµР№ РїРѕР·РёС†РёРё РєСѓСЂСЃРѕСЂР° Рё
    /// РїСЂРё РёР·РјРµРЅРµРЅРёРё РІС‹Р·РІР°С‚СЊ `Window::set_cursor`. CursorMoved РјРѕР¶РµС‚
    /// РґС‘СЂРіР°С‚СЊСЃСЏ СЃРѕС‚РЅРё СЂР°Р· РІ СЃРµРєСѓРЅРґСѓ вЂ” `last_cursor_icon` РєСЌС€РёСЂСѓРµС‚
    /// РїСЂРµРґС‹РґСѓС‰РµРµ Р·РЅР°С‡РµРЅРёРµ, С‡С‚РѕР±С‹ РЅРµ РґРµР»Р°С‚СЊ Р»РёС€РЅРёР№ FFI-РІС‹Р·РѕРІ РІ winit.
    fn update_cursor_icon(&mut self) {
        let (Some(window), Some(renderer), Some(pos)) =
            (self.window.as_ref(), self.renderer.as_ref(), self.cursor_position)
        else {
            return;
        };
        let dpr = (renderer.scale_factor() as f32).max(1e-6);
        let x_css = (pos.x as f32) / dpr;
        let y_css = (pos.y as f32) / dpr;

        // Scrollbar takes highest priority.
        let hover = scrollbar::classify_track_click(
            x_css,
            y_css,
            self.scroll_y,
            self.content_height,
            self.viewport_width_css(),
            self.viewport_height_css(),
        );
        let scrollbar_icon = cursor_icon_for_hover(hover, self.scroll_drag.is_some());

        // F2-6: a docked-panel resize drag (or hovering an edge) shows the
        // horizontal-resize cursor, ahead of scrollbar/page/chrome hover.
        let desired = if self.panel_resize.is_some() || self.resize_edge_at(x_css, y_css).is_some() {
            CursorIcon::EwResize
        } else if self.point_over_chrome(x_css, y_css) {
            // CC-5: the engine-drawn chrome owns the cursor over its own
            // opaque area (sidebar, toolbar, tab strip) вЂ” ahead of
            // scrollbar/page hit-test below, which assume page coordinates.
            match self.chrome_hit_test(x_css, y_css) {
                Some(result) => css_cursor_to_winit(result.cursor),
                None => CursorIcon::Default,
            }
        } else if scrollbar_icon != CursorIcon::Default {
            scrollbar_icon
        } else if let Some(lb) = &self.layout_box {
            // Hit-test layout tree in page coordinates (viewport + scroll offset).
            let (offset_x, offset_y) = self.page_offset();
            let page_x = (x_css - offset_x) + self.scroll_x;
            let page_y = (y_css - offset_y) + self.scroll_y;
            match hit_test(Point::new(page_x, page_y), lb) {
                Some(result) => css_cursor_to_winit(result.cursor),
                None => CursorIcon::Default,
            }
        } else {
            CursorIcon::Default
        };

        if self.last_cursor_icon != Some(desired) {
            window.set_cursor(desired);
            self.last_cursor_icon = Some(desired);
        }
    }

    /// РџРµСЂРµСЃС‡РёС‚С‹РІР°РµС‚ С‚РµРєСѓС‰РёР№ СЃРїРёСЃРѕРє СЃРѕРІРїР°РґРµРЅРёР№.
    ///
    /// - Plain-text СЂРµР¶РёРј: substring search РїРѕ DrawText-РєРѕРјР°РЅРґР°Рј display list.
    /// - Regex СЂРµР¶РёРј (Ctrl+R): regex РїРѕ [`TextFragment`][lumen_layout::TextFragment]
    ///   РёР· [`collect_visible_text`][lumen_layout::collect_visible_text]; РїРѕР·РёС†РёРё
    ///   Р±РµСЂСѓС‚СЃСЏ РёР· `TextFragment.rect`, `dl_index` вЂ” lookup РїРѕ (x, y, text) РІ DL.
    fn current_matches(&self) -> Vec<find::FindMatch> {
        if !self.find.is_open() || self.find.query().is_empty() {
            return Vec::new();
        }
        let Ok(font) = lumen_font::Font::parse(INTER_FONT) else {
            return Vec::new();
        };
        let Ok(measurer) = lumen_paint::FontMeasurer::new(&font) else {
            return Vec::new();
        };
        if self.find.is_regex_mode() {
            let frags = self.layout_box.as_ref().map_or_else(Vec::new, |lb| {
                lumen_layout::collect_visible_text(lb)
            });
            find::find_matches_regex(&frags, &self.display_list, self.find.query(), &measurer)
        } else {
            find::find_matches(&self.display_list, self.find.query(), &measurer)
        }
    }

    /// P3-spell СЃСЂРµР· 2+3+4: РґР»СЏ СѓР·Р»Р° `nid` РїРѕРґ С„РѕРєСѓСЃРѕРј РѕРїСЂРµРґРµР»СЏРµС‚ С†РµР»СЊ
    /// СЃРїРµР»Р»-С‡РµРєР°. Р’РѕР·РІСЂР°С‰Р°РµС‚ `(target_node, placeholder, kind)`:
    /// * `<textarea>` РёР»Рё `<input>` С‚РµРєСЃС‚РѕРІРѕРіРѕ С‚РёРїР° (password РёСЃРєР»СЋС‡С‘РЅ) вЂ” СЃР°Рј
    ///   СѓР·РµР», РµРіРѕ `placeholder` (РїСѓСЃС‚Р°СЏ СЃС‚СЂРѕРєР° РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІРёРё) Рё
    ///   СЃРѕРѕС‚РІРµС‚СЃС‚РІСѓСЋС‰РёР№ [`page_context_menu::SpellTargetKind`];
    /// * СѓР·РµР» РІРЅСѓС‚СЂРё `contenteditable` вЂ” СЂРµРґР°РєС‚РёСЂСѓСЋС‰РёР№ С…РѕСЃС‚, РїСѓСЃС‚РѕР№
    ///   placeholder (Сѓ contenteditable РЅРµС‚ placeholder-Р°С‚СЂРёР±СѓС‚Р°) Рё
    ///   `ContentEditable`;
    /// * РёРЅР°С‡Рµ вЂ” `None`.
    fn spell_target(
        &self,
        nid: lumen_dom::NodeId,
    ) -> Option<(lumen_dom::NodeId, String, page_context_menu::SpellTargetKind)> {
        use page_context_menu::SpellTargetKind;
        let ls = self.layout_source.as_ref()?;
        let doc = ls.document.lock().ok()?;
        let node = doc.get(nid);
        if let Some(name) = node.element_name() {
            let is_textarea = name.local.eq_ignore_ascii_case("textarea");
            let is_text_input = name.local.eq_ignore_ascii_case("input")
                && matches!(
                    node.get_attr("type")
                        .unwrap_or("text")
                        .to_ascii_lowercase()
                        .as_str(),
                    "text" | "search" | "email" | "url"
                );
            if is_textarea || is_text_input {
                let placeholder = node.get_attr("placeholder").unwrap_or_default().to_owned();
                let kind = if is_textarea { SpellTargetKind::Textarea } else { SpellTargetKind::Input };
                return Some((nid, placeholder, kind));
            }
        }
        // contenteditable: check the DOM directly for an editing host.
        lumen_dom::find_editing_host(&doc, nid)
            .map(|host| (host, String::new(), SpellTargetKind::ContentEditable))
    }

    /// P3-spell СЃСЂРµР· 3: СЃР»РѕРІР°, РєРѕС‚РѕСЂС‹Рµ РЅРµ СЃС‡РёС‚Р°СЋС‚СЃСЏ РѕС€РёР±РѕС‡РЅС‹РјРё РїРѕРјРёРјРѕ СЃР»РѕРІР°СЂРµР№ вЂ”
    /// РѕР±СЉРµРґРёРЅРµРЅРёРµ РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРѕРіРѕ СЃР»РѕРІР°СЂСЏ Рё В«РџСЂРѕРїСѓС‰РµРЅРЅС‹С…В» РЅР° СЃРµСЃСЃРёСЋ. Р’СЃРµ
    /// СЃР»РѕРІР° СѓР¶Рµ РІ lowercase.
    fn spell_allow_set(&self) -> std::collections::HashSet<String> {
        self.spell_user_words
            .iter()
            .chain(self.spell_ignored.iter())
            .cloned()
            .collect()
    }

    /// P3-spell СЃСЂРµР· 4: РїРѕР»РЅС‹Р№ Р»РѕРіРёС‡РµСЃРєРёР№ С‚РµРєСЃС‚ РїРѕР»СЏ `target_node` вЂ”
    /// `value`-Р°С‚СЂРёР±СѓС‚ РґР»СЏ `<input>`, Р»РёР±Рѕ (РґР»СЏ `<textarea>`/contenteditable)
    /// РєРѕРЅРєР°С‚РµРЅР°С†РёСЏ С‚РµРєСЃС‚РѕРІС‹С… СѓР·Р»РѕРІ-РїРѕС‚РѕРјРєРѕРІ (`lumen_dom::node_text_content`).
    /// РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РєР°Рє Р±Р°Р·Р° РґР»СЏ РіР»РѕР±Р°Р»СЊРЅС‹С… byte-СЃРјРµС‰РµРЅРёР№ СЃР»РѕРІР° РІ
    /// [`page_context_menu::SpellTarget`], РІ РѕС‚Р»РёС‡РёРµ РѕС‚ С‚РµРєСЃС‚Р° РѕРґРЅРѕР№
    /// РІРёР·СѓР°Р»СЊРЅРѕР№ (wrapped) СЃС‚СЂРѕРєРё.
    fn spell_field_full_text(
        &self,
        target_node: lumen_dom::NodeId,
        kind: page_context_menu::SpellTargetKind,
    ) -> String {
        use page_context_menu::SpellTargetKind;
        let Some(ls) = self.layout_source.as_ref() else { return String::new() };
        let Ok(doc) = ls.document.lock() else { return String::new() };
        match kind {
            // BUG-441: Сѓ `<input>`/`<textarea>` РїСЂРѕРІРµСЂСЏРµРј С‚Рѕ, С‡С‚Рѕ РІ РїРѕР»Рµ СЃРµР№С‡Р°СЃ
            // (runtime-Р·РЅР°С‡РµРЅРёРµ), Р° РЅРµ РґРµС„РѕР»С‚ РёР· СЂР°Р·РјРµС‚РєРё. `contenteditable`
            // СЂРµРґР°РєС‚РёСЂСѓРµС‚СЃСЏ РїСЂСЏРјРѕ РІ DOM, РїРѕСЌС‚РѕРјСѓ С‚Р°Рј РїРѕ-РїСЂРµР¶РЅРµРјСѓ С‚РµРєСЃС‚ СѓР·Р»РѕРІ.
            SpellTargetKind::Input | SpellTargetKind::Textarea => {
                doc.control_value(target_node).into_owned()
            }
            SpellTargetKind::ContentEditable => node_text_content(&doc, target_node),
        }
    }

    /// P3-spell СЃСЂРµР· 3+4: РїСЂРё right-click РїРѕ РѕС€РёР±РѕС‡РЅРѕРјСѓ СЃР»РѕРІСѓ РІ С„РѕРєСѓСЃРЅРѕРј
    /// `<input>`/`<textarea>`/contenteditable РѕС‚РєСЂС‹РІР°РµС‚ РјРµРЅСЋ РїРѕРґСЃРєР°Р·РѕРє.
    /// Р’РѕР·РІСЂР°С‰Р°РµС‚ `true`, РµСЃР»Рё РјРµРЅСЋ РѕС‚РєСЂС‹С‚Рѕ (РєР»РёРє РѕР±СЂР°Р±РѕС‚Р°РЅ), РёРЅР°С‡Рµ `false`
    /// (РєР»РёРє РёРґС‘С‚ РґР°Р»СЊС€Рµ вЂ” Р¶РµСЃС‚).
    ///
    /// РњРЅРѕРіРѕСЃС‚СЂРѕС‡РЅС‹Рµ РїРѕР»СЏ СЂРёСЃСѓСЋС‚ РѕРґРЅСѓ `DrawText`-РєРѕРјР°РЅРґСѓ РЅР° РІРёР·СѓР°Р»СЊРЅСѓСЋ
    /// (wrapped) СЃС‚СЂРѕРєСѓ вЂ” Р±Р°Р№С‚РѕРІРѕРµ СЃРјРµС‰РµРЅРёРµ СЃР»РѕРІР° РІРЅСѓС‚СЂРё РєР»РёРєР° РЅР°Р№РґРµРЅРЅРѕР№
    /// СЃС‚СЂРѕРєРё СЃР°РјРѕ РїРѕ СЃРµР±Рµ Р±РµСЃСЃРјС‹СЃР»РµРЅРЅРѕ Р·Р° РїСЂРµРґРµР»Р°РјРё РїРµСЂРІРѕР№ СЃС‚СЂРѕРєРё.
    /// `spellcheck::locate_line_word_in_full_text` РїРµСЂРµСЃС‡РёС‚С‹РІР°РµС‚ РµРіРѕ РІ
    /// РіР»РѕР±Р°Р»СЊРЅРѕРµ СЃРјРµС‰РµРЅРёРµ РІРЅСѓС‚СЂРё РїРѕР»РЅРѕРіРѕ Р·РЅР°С‡РµРЅРёСЏ РїРѕР»СЏ, РёСЃРїРѕР»СЊР·СѓСЏ
    /// РїСЂРµРґС€РµСЃС‚РІСѓСЋС‰РёРµ СЃС‚СЂРѕРєРё С‚РѕРіРѕ Р¶Рµ РїРѕР»СЏ РєР°Рє СЏРєРѕСЂСЏ.
    fn try_open_spell_menu(&mut self, x_css: f32, y_css: f32) -> bool {
        use lumen_core::ext::SpellChecker;
        let Some(dicts) = SPELL_DICTS.get() else { return false };
        if dicts.is_empty() {
            return false;
        }
        let Some(nid) = self.focused_node else { return false };
        let Some((target_node, _placeholder, kind)) = self.spell_target(nid) else { return false };
        let Some(node_lb) = self
            .layout_box
            .as_ref()
            .and_then(|lb| forms::find_layout_box(lb, target_node))
        else {
            return false;
        };
        let node_rect = node_lb.rect;
        let (page_x, page_y) = self.page_point(x_css, y_css);
        if page_x < node_rect.x
            || page_y < node_rect.y
            || page_x >= node_rect.x + node_rect.width
            || page_y >= node_rect.y + node_rect.height
        {
            return false;
        }
        let Ok(font) = lumen_font::Font::parse(INTER_FONT) else { return false };
        let Ok(m) = lumen_paint::FontMeasurer::new(&font) else { return false };
        let allow = self.spell_allow_set();

        // Walk this field's rendered lines in document order, remembering
        // every line before the one under the cursor (needed to resolve the
        // clicked word's global offset for multi-line fields) and stopping at
        // the first hit. Collected up front (immutable borrow of
        // `display_list`) so the mutable `open_for` call below doesn't overlap.
        let hit: Option<(Vec<String>, page_context_menu::SpellTarget)> = {
            let mut prior_lines: Vec<String> = Vec::new();
            let mut found = None;
            for cmd in &self.display_list {
                let lumen_paint::DisplayCommand::DrawText { rect, text, font_size, .. } = cmd
                else {
                    continue;
                };
                if rect.x < node_rect.x
                    || rect.y < node_rect.y
                    || rect.x >= node_rect.x + node_rect.width
                    || rect.y >= node_rect.y + node_rect.height
                {
                    continue;
                }
                let hits_point = page_x >= rect.x
                    && page_x < rect.x + rect.width
                    && page_y >= rect.y
                    && page_y < rect.y + rect.height;
                if !hits_point {
                    prior_lines.push(text.clone());
                    continue;
                }
                let fs = *font_size;
                let measure = |s: &str| -> f32 {
                    use lumen_layout::TextMeasurer;
                    s.chars().map(|c| m.char_width(c, fs)).sum()
                };
                let Some((s, e)) = spellcheck::word_at_x(text, page_x - rect.x, &measure) else {
                    prior_lines.push(text.clone());
                    continue;
                };
                let word = &text[s..e];
                if dicts.check(word) || allow.contains(&word.to_lowercase()) {
                    // Word under cursor is spelled correctly вЂ” no menu.
                    return false;
                }

                let full_text = match kind {
                    page_context_menu::SpellTargetKind::Input => text.clone(),
                    _ => self.spell_field_full_text(target_node, kind),
                };
                let Some((global_start, global_end)) = spellcheck::locate_line_word_in_full_text(
                    &full_text,
                    &prior_lines,
                    text,
                    s,
                    e,
                ) else {
                    return false;
                };
                let word = full_text[global_start..global_end].to_owned();
                let suggestions = dicts.suggest(&word);
                found = Some((
                    suggestions,
                    page_context_menu::SpellTarget {
                        node: target_node,
                        text: full_text,
                        word_start: global_start,
                        word_end: global_end,
                        kind,
                    },
                ));
                break;
            }
            found
        };

        match hit {
            Some((suggestions, target)) => {
                self.page_context_menu.open_for(x_css, y_css, suggestions, target);
                true
            }
            None => false,
        }
    }

    /// P3-spell СЃСЂРµР· 3+4: РїСЂРёРјРµРЅСЏРµС‚ РІС‹Р±СЂР°РЅРЅРѕРµ РґРµР№СЃС‚РІРёРµ РјРµРЅСЋ РїРѕРґСЃРєР°Р·РѕРє.
    /// `Use` Р·Р°РјРµРЅСЏРµС‚ СЃР»РѕРІРѕ Рё РїРµСЂРµРІС‘СЂСЃС‚С‹РІР°РµС‚ вЂ” РґР»СЏ `<input>`/`<textarea>`
    /// РїРµСЂРµСЃС‚СЂР°РёРІР°СЏ РїРѕР»РЅРѕРµ Р·РЅР°С‡РµРЅРёРµ С‡РµСЂРµР· `target.apply()`; РґР»СЏ
    /// contenteditable С‚РѕС‡РµС‡РЅРѕ РїСЂР°РІСЏ С‚РѕР»СЊРєРѕ С‚РµРєСЃС‚РѕРІС‹Р№ СѓР·РµР», СЃРѕРґРµСЂР¶Р°С‰РёР№ СЃР»РѕРІРѕ
    /// (`lumen_dom::locate_text_offset_range` + `delete_range`/`insert_text_at`),
    /// РЅРµ С‚СЂРѕРіР°СЏ РѕСЃС‚Р°Р»СЊРЅСѓСЋ rich-text СЃС‚СЂСѓРєС‚СѓСЂСѓ. `AddToDict` РґРѕР±Р°РІР»СЏРµС‚ СЃР»РѕРІРѕ РІ
    /// РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёР№ СЃР»РѕРІР°СЂСЊ (С„Р°Р№Р» + РїР°РјСЏС‚СЊ); `Ignore` РґРѕР±Р°РІР»СЏРµС‚ СЃР»РѕРІРѕ РІ
    /// РЅР°Р±РѕСЂ РїСЂРѕРїСѓС‰РµРЅРЅС‹С… РЅР° СЃРµСЃСЃРёСЋ.
    fn exec_spell_menu_action(&mut self, action: page_context_menu::SpellMenuAction) {
        use page_context_menu::{SpellMenuAction, SpellTargetKind};
        let Some(target) = self.page_context_menu.target().cloned() else { return };
        match action {
            SpellMenuAction::Use(replacement) => {
                match target.kind {
                    SpellTargetKind::Input => {
                        let new_val = target.apply(&replacement);
                        if let Some(src) = self.layout_source.as_mut()
                            && let Ok(mut doc) = src.document.lock()
                        {
                            forms::set_value(&mut doc, target.node, &new_val);
                        }
                        self.form_state.entry(target.node).or_default().value = new_val;
                    }
                    SpellTargetKind::Textarea => {
                        let new_val = target.apply(&replacement);
                        if let Some(src) = self.layout_source.as_mut()
                            && let Ok(mut doc) = src.document.lock()
                        {
                            forms::set_textarea_text(&mut doc, target.node, &new_val);
                        }
                        self.form_state.entry(target.node).or_default().value = new_val;
                    }
                    SpellTargetKind::ContentEditable => {
                        if let Some(src) = self.layout_source.as_mut()
                            && let Ok(mut doc) = src.document.lock()
                            && let Some((text_node, local_start, local_end)) =
                                locate_text_offset_range(
                                    &doc,
                                    target.node,
                                    target.word_start,
                                    target.word_end,
                                )
                        {
                            let range = Range {
                                start: DomPosition { container: text_node, offset: local_start },
                                end: DomPosition { container: text_node, offset: local_end },
                            };
                            let collapsed = delete_range(&mut doc, &range);
                            insert_text_at(&mut doc, collapsed, &replacement);
                        }
                    }
                }
                // ADR-016 M2.2c-3 (2): spellcheck-replace mutates the shared DOM
                // (input value / textarea text / contenteditable range) with no
                // synchronous geometry read after вЂ” Bucket A, route off-thread when
                // `LUMEN_ENGINE_THREAD=1`, byte-identical otherwise.
                self.relayout_form();
            }
            SpellMenuAction::AddToDict => {
                let word = target.word().to_lowercase();
                let _ = spellcheck::add_user_word(&spellcheck::user_words_path(), &word);
                self.spell_user_words.insert(word);
            }
            SpellMenuAction::Ignore => {
                self.spell_ignored.insert(target.word().to_lowercase());
            }
        }
    }


    /// РЎРѕС…СЂР°РЅРёС‚СЊ С‚РµРєСѓС‰СѓСЋ РІРєР»Р°РґРєСѓ РІ `last_session.lsession` РїСЂРё Р·Р°РєСЂС‹С‚РёРё РѕРєРЅР°.
    ///
    /// Silent вЂ” РѕС€РёР±РєРё Р·Р°РїРёСЃРё РЅРµ Р»РѕРјР°СЋС‚ РІС‹С…РѕРґ. РќРµ СЃРѕС…СЂР°РЅСЏРµС‚ Empty-СЃС‚СЂР°РЅРёС†Сѓ.
    fn save_session_on_close(&self) {
        let url = match &self.source {
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => return,
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url(u) => u.clone(),
            PageSource::Snapshot { base_url, .. } => base_url.clone(),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let file = SessionFile {
            version: 1,
            name: format!("auto-save {now}"),
            created_at: now,
            tabs: vec![ExportedTab {
                url,
                title: self.title.clone().unwrap_or_default(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                is_active: true,
            }],
        };
        let json = session_export::to_json(&file);
        let _ = std::fs::write("last_session.lsession", json.as_bytes());
    }

    /// Persist every open tab (URL + title + scroll + serialised DOM) to the
    /// SQLite session store on window close (В§10I).
    ///
    /// Walks the tab strip in left-to-right order, pulling each tab's state from
    /// whichever slot holds it: the active tab from `self`, background tabs from
    /// `bg_tabs`, hibernated tabs from `tab_snapshots`. Tabs without a real URL
    /// (blank, never-loaded) are skipped. Silent вЂ” write errors do not block exit.
    fn save_full_session(&self) {
        let mut tabs: Vec<lumen_storage::PersistedTab> = Vec::new();
        let active_idx = self.tab_strip.active;
        for (idx, entry) in self.tab_strip.tabs.iter().enumerate() {
            let persisted = if idx == active_idx {
                source_url_string(&self.source).map(|url| lumen_storage::PersistedTab {
                    url,
                    title: self.title.clone().unwrap_or_default(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    is_active: true,
                    dom_blob: dom_blob_of(self.layout_source.as_ref()),
                })
            } else if let Some(snap) = self.bg_tabs.get(&entry.id) {
                source_url_string(&snap.source).map(|url| lumen_storage::PersistedTab {
                    url,
                    title: snap.title.clone().unwrap_or_default(),
                    scroll_x: snap.scroll_x,
                    scroll_y: snap.scroll_y,
                    is_active: false,
                    dom_blob: dom_blob_of(snap.layout_source.as_ref()),
                })
            } else if self.hibernated_tabs.contains_key(&entry.id) {
                // DOM blob already on disk in tab_snapshots вЂ” copy it over.
                match self.tab_snapshots.fetch(entry.id as i64) {
                    Ok(Some(data)) if !data.url.is_empty() => Some(lumen_storage::PersistedTab {
                        url: data.url,
                        title: data.title,
                        scroll_x: data.scroll_x,
                        scroll_y: data.scroll_y,
                        is_active: false,
                        dom_blob: data.dom_blob,
                    }),
                    _ => None,
                }
            } else {
                None // Blank / never-loaded tab.
            };
            if let Some(t) = persisted {
                tabs.push(t);
            }
        }

        if let Err(e) = self.session_store.save(&tabs) {
            eprintln!("session: РЅРµ СѓРґР°Р»РѕСЃСЊ СЃРѕС…СЂР°РЅРёС‚СЊ СЃРµСЃСЃРёСЋ: {e}");
        }
    }

    /// Reopen the tabs saved by [`Self::save_full_session`] (В§10I).
    ///
    /// Called once at launch only when the user started the browser with no
    /// explicit page (so we do not clobber an `argv`-requested page). The
    /// previously-active tab's source + scroll are installed into `self` so the
    /// normal load pipeline renders it; each background tab is parked via the
    /// hibernation machinery (`hibernated_tabs` + `tab_snapshots`) so switching
    /// to it reconstructs it from its DOM blob without a network round-trip.
    fn restore_session(&mut self) {
        let tabs = match self.session_store.load() {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => return,
            Err(e) => {
                eprintln!("session: РЅРµ СѓРґР°Р»РѕСЃСЊ РїСЂРѕС‡РёС‚Р°С‚СЊ СЃРµСЃСЃРёСЋ: {e}");
                return;
            }
        };
        let active_idx = session_persist::active_index(&tabs);

        // Rebuild the tab strip from scratch вЂ” one entry per restored tab, in
        // saved order. The strip starts with a single blank tab (id 0); reuse it.
        self.tab_strip.tabs.clear();
        self.tab_strip.next_id = 0;

        for (idx, tab) in tabs.into_iter().enumerate() {
            let id = self.tab_strip.next_id;
            self.tab_strip.next_id += 1;
            self.tab_strip.tabs.push(tabs::strip::TabEntry {
                id,
                title: if tab.title.is_empty() {
                    "Р’РѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅР°СЏ РІРєР»Р°РґРєР°".to_owned()
                } else {
                    tab.title.clone()
                },
                tab_state: TabState::Active,
                opener_id: None,
                container: tabs::containers::ContainerKind::None,
                last_activated_ms: 0.0,
                pinned: false,
                group_id: None,
                adblock: false,
            });
            self.lifecycle_mgr.open_tab(id as u64);

            if idx == active_idx {
                // Active tab: load fresh through the normal pipeline.
                self.source = PageSource::from_arg(Some(&tab.url));
                self.scroll_x = tab.scroll_x;
                self.scroll_y = tab.scroll_y;
                self.title = Some(tab.title);
            } else {
                // Background tab: park as hibernated so switch_tab restores it
                // from the DOM blob on demand.
                let data = lumen_storage::HibernatedTabData {
                    dom_blob: tab.dom_blob,
                    css_source: String::new(),
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    scroll_x: tab.scroll_x,
                    scroll_y: tab.scroll_y,
                };
                if self.tab_snapshots.store(id as i64, &data).is_ok() {
                    self.hibernated_tabs.insert(
                        id,
                        tab_lifecycle::TabMetadata { url: tab.url, title: tab.title },
                    );
                    let last = self.tab_strip.tabs.len() - 1;
                    self.tab_strip.set_tab_state(last, TabState::Hibernated);
                }
            }
        }

        self.tab_strip.active = active_idx.min(self.tab_strip.tabs.len().saturating_sub(1));
    }

    // в”Ђв”Ђ Tab lifecycle: hibernation and restore в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

    /// Promote a background tab from T2в†’T3 (Hibernated) by serialising its DOM
    /// to SQLite and evicting the in-memory `PageSnapshot`.
    ///
    /// On failure (serialise error, SQLite error) the snapshot is put back into
    /// `bg_tabs` and the tab stays at T2.
    ///
    /// This is also the T2в†’T3 bfcache degradation point (`docs/tasks/ph3-bfcache.md`
    /// step 8): `snap` owns the tab's `bfcache: BfCache`, which may hold `Frozen`
    /// entries (each carrying a full DOM byte blob). `bg_tabs.remove` moves `snap`
    /// into this function; on the success path it is never re-inserted anywhere,
    /// so it вЂ” and every `FrozenPage` inside its `bfcache` вЂ” is freed when this
    /// function returns. No separate `degrade_bfcache_entries` pass is needed: the
    /// whole per-tab state (bfcache included) is already released at T3.
    fn hibernate_bg_tab(&mut self, tab_id: usize) {
        let Some(snap) = self.bg_tabs.remove(&tab_id) else { return };

        // Serialise DOM via Document::to_bytes() (bincode).
        let (dom_blob, css_source) = if let Some(ls) = snap.layout_source.as_ref() {
            match ls.document.lock() {
                Ok(doc) => {
                    let blob = doc.to_bytes().unwrap_or_default();
                    let css = extract_style_blocks(&doc);
                    (blob, css)
                }
                Err(_) => (vec![], String::new()),
            }
        } else {
            (vec![], String::new())
        };

        let url = match &snap.source {
            PageSource::Url(u) => u.clone(),
            PageSource::File(p) => format!("file://{}", p.display()),
            PageSource::Snapshot { base_url, .. } => base_url.clone(),
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => String::new(),
        };
        let title = snap.title.clone().unwrap_or_default();
        let scroll_x = snap.scroll_x;
        let scroll_y = snap.scroll_y;

        let data = lumen_storage::HibernatedTabData {
            dom_blob,
            css_source,
            url: url.clone(),
            title: title.clone(),
            scroll_x,
            scroll_y,
        };

        if let Err(e) = self.tab_snapshots.store(tab_id as i64, &data) {
            eprintln!("РћС€РёР±РєР° hibernate tab {tab_id}: {e}");
            // Rollback вЂ” keep the snapshot in RAM.
            self.bg_tabs.insert(tab_id, snap);
            return;
        }

        // Keep only lightweight metadata in RAM (scroll state stays in SQLite).
        self.hibernated_tabs.insert(
            tab_id,
            tab_lifecycle::TabMetadata { url, title },
        );

        // Update badge in the strip (T3 = grey dot).
        if let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) {
            self.tab_strip.set_tab_state(idx, tab_lifecycle::TabState::Hibernated);
        }
    }

    /// Restore a T2 (BackgroundOld) tab from SQLite crash-recovery checkpoint.
    ///
    /// Used only when `bg_tabs` is empty for this tab (process-restart path).
    /// Reads scroll + form state from `t2_store` and applies them to the current
    /// (blank-reset) active slot.  The page URL is not stored in `t2_store`, so
    /// the tab will appear blank; a future enhancement may store the URL to
    /// trigger a background reload (10I Phase 2).
    ///
    /// Shows `sleep_hint` overlay if restore takes >100 ms.
    fn restore_t2_tab(&mut self, tab_id: usize) {
        self.t2_restore_start_ms = Some(self.epoch.elapsed().as_secs_f64() * 1000.0);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }

        if let Ok(Some(data)) = self.t2_store.fetch(tab_id as i64) {
            self.scroll_x = data.scroll_x;
            self.scroll_y = data.scroll_y;
            self.form_state = tab_lifecycle::deserialize_form_state(&data.form_state_json);
            let _ = self.t2_store.delete(tab_id as i64);
        }

        self.t2_restore_start_ms = None;
    }

    /// Restore a T3-hibernated tab into the active slot.
    ///
    /// Fetches the DOM blob from SQLite, reconstructs the `Document` via
    /// `Document::from_bytes()`, re-parses inline CSS, and re-runs
    /// layout+paint.  Returns `true` on success so `switch_tab` knows
    /// whether to fall back to a blank tab.
    fn restore_hibernated_tab(&mut self, tab_id: usize) -> bool {
        // Start spinner timer for long restore operations (>200ms).
        self.restore_spinner_start_ms = Some(self.epoch.elapsed().as_secs_f64() * 1000.0);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }

        let Some(meta) = self.hibernated_tabs.remove(&tab_id) else {
            self.restore_spinner_start_ms = None;
            return false;
        };

        // Pre-fill title from lightweight metadata for immediate window title update.
        self.title = Some(meta.title.clone());

        let data = match self.tab_snapshots.fetch(tab_id as i64) {
            Ok(Some(d)) => d,
            Ok(None) => {
                eprintln!("tab {tab_id}: snapshot missing (url={})", meta.url);
                // Put metadata back so the strip still shows Hibernated.
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
            Err(e) => {
                eprintln!("tab {tab_id}: snapshot read error (url={}): {e}", meta.url);
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
        };

        // Reconstruct Document from bincode blob.
        let doc = match Document::from_bytes(&data.dom_blob) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("РћС€РёР±РєР° РґРµСЃРµСЂРёР°Р»РёР·Р°С†РёРё DOM РІРєР»Р°РґРєРё {tab_id}: {e}");
                self.hibernated_tabs.insert(tab_id, meta);
                self.restore_spinner_start_ms = None;
                return false;
            }
        };

        // Re-parse CSS from inline <style> blocks preserved in the DOM.
        let css = if data.css_source.is_empty() {
            extract_style_blocks(&doc)
        } else {
            data.css_source.clone()
        };
        let stylesheet = lumen_css_parser::parse(&css);

        // Rebuild a fresh PersistentJs runtime. The JS heap cannot be
        // serialised, so the page's inline <script> blocks are re-run against
        // the restored DOM. The runtime shares the returned Arc<Mutex<Document>>
        // with the layout tree so both observe the same document.
        self.set_js_ctx(None);
        let event_sink = self.event_sink.clone();
        let cookie_banner_dismiss = self.cookie_banner_dismiss;
        let deterministic = self.deterministic;
        // Computed up front: `&mut self.ls_storage` below would otherwise
        // conflict with this `&self` method call as a later call argument.
        let cookie_jar = self.active_cookie_jar();
        let (document_arc, js_ctx) = tab_lifecycle::hibernate::restore_js_context(
            &data.url,
            doc,
            event_sink,
            &mut self.ls_storage,
            &mut self.ss_storage,
            self.idb_dir.as_deref(),
            &self.sw_backend,
            cookie_banner_dismiss,
            deterministic,
            Some(cookie_jar),
        );

        let layout_source = LayoutSource {
            document: Arc::clone(&document_arc),
            stylesheet: Arc::new(stylesheet),
            html_source: None,
            // Tab hibernation (T3в†’T0) restore вЂ” original Cache-Control is not
            // preserved across the hibernate/restore round-trip; treat as
            // cacheable (matches the rest of this struct's restore paths).
            cache_control_no_store: false,
            // BUG-743: only the inline `<style>` text survives hibernation
            // (`extract_style_blocks`), the external-sheet bodies do not вЂ” a
            // rebuild would silently drop them, so the cascade stays frozen.
            dynamic_css: None,
        };

        // Re-run layout+paint with the current viewport (including zoom).
        let phys = self.renderer.as_ref().map_or_else(
            || (1024.0_f32, 720.0_f32),
            |r| {
                let s = r.viewport_size();
                (s.width, s.height)
            },
        );
        let meta_scale = meta_initial_scale(&layout_source);
        let (css_w, css_h) = zoom::effective_viewport(phys.0, phys.1, meta_scale, self.zoom_factor);
        let viewport = lumen_core::geom::Size::new(css_w, css_h);
        // content-visibility: auto (BB-4): relevance РїСЂРѕС‚РёРІ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅРѕРіРѕ
        // scroll-РїРѕР»РѕР¶РµРЅРёСЏ; ratchet РЅРѕРІРѕР№ СЃС‚СЂР°РЅРёС†С‹ СЃС‚Р°СЂС‚СѓРµС‚ СЃ РЅСѓР»СЏ.
        lumen_layout::set_cv_scroll(data.scroll_x, data.scroll_y);
        lumen_layout::set_cv_relevant(std::collections::HashSet::new());
        let (display_list, lb) = relayout_page(&layout_source, viewport, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        lumen_layout::set_cv_scroll(0.0, 0.0);

        // Install into the active slot.
        self.set_display_list(display_list);
        self.title = Some(data.title);
        self.layout_source = Some(layout_source);
        // BUG-341 S7: hibernate restore bypasses the restyle-aware path.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(lb);
        self.cv_relevant.clear();
        self.cv_events.clear();
        self.cv_skipped.clear();
        self.cv_auto_state.clear();
        self.refresh_cv_state();
        self.set_js_ctx(js_ctx);
        // ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёРј РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅС‹Р№ С…СЌРЅРґР» + DOM РІ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє.
        self.sync_engine_js_state();
        self.scroll_x = data.scroll_x;
        self.scroll_y = data.scroll_y;
        self.content_height = content_height_of(&self.display_list);
        self.content_width = content_width_of(&self.display_list);

        // Seed the restored runtime with layout geometry + viewport so JS can
        // query bounding rects immediately (mirrors the fresh-load path).
        // ADR-016 M2.2c-2d: routed off-thread through `route_task_js`, same as the
        // fresh-load seed above (`self.js_present` gate в†’ byte-identical off).
        #[cfg(feature = "v8")]
        if self.js_present
            && let Some(lb_ref) = self.layout_box.as_ref()
        {
            let rects = collect_layout_rects(lb_ref);
            let styles = collect_computed_styles(lb_ref);
            let customs = collect_custom_properties(lb_ref);
            let (vw, vh) = (viewport.width, viewport.height);
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.update_layout_rects(rects);
                js.update_computed_styles(styles);
                js.update_custom_properties(customs);
                js.update_viewport_size(vw, vh);
            });
        }

        // Remove the SQLite entry вЂ” it is no longer needed.
        let _ = self.tab_snapshots.delete(tab_id as i64);

        // Restore complete вЂ” hide the spinner overlay.
        self.restore_spinner_start_ms = None;

        true
    }

    /// Poll the lifecycle manager approximately once per second.
    ///
    /// Processes tier transitions returned by `tick_idle` + `lru_evict`:
    /// - `Hibernated` transitions evict the corresponding `bg_tabs` entry to SQLite.
    /// - Other transitions update the tab strip badge.
    fn tick_lifecycle(&mut self) {
        if self.lifecycle_last_tick.elapsed().as_secs() < 1 {
            return;
        }
        self.lifecycle_last_tick = std::time::Instant::now();

        let transitions = self.lifecycle_mgr.tick_idle(tab_lifecycle::MemoryPressure::Low);
        let evicted = self.lifecycle_mgr.lru_evict();

        for tr in transitions.into_iter().chain(evicted) {
            let tab_id = tr.tab_id as usize;

            if tr.to == tab_lifecycle::TabState::Hibernated {
                if self.bg_tabs.contains_key(&tab_id) {
                    self.hibernate_bg_tab(tab_id);
                }
                continue;
            }

            // T1 в†’ T2: checkpoint scroll + form state to SQLite for crash recovery.
            if tr.to == tab_lifecycle::TabState::BackgroundOld
                && let Some(snap) = self.bg_tabs.get(&tab_id)
            {
                let data = lumen_storage::T2SleepData {
                    js_heap_blob: vec![],
                    dom_blob: vec![],
                    scroll_x: snap.scroll_x,
                    scroll_y: snap.scroll_y,
                    form_state_json: tab_lifecycle::serialize_form_state(&snap.form_state),
                    ts: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs() as i64),
                };
                let _ = self.t2_store.store(tab_id as i64, &data);
            }

            // GC tuning per tier (10L): run progressively aggressive GC as a
            // background tab ages, reclaiming heap without full hibernation cost.
            let gc_level_opt: Option<u8> = match tr.to {
                tab_lifecycle::TabState::BackgroundRecent => Some(1), // moderate
                tab_lifecycle::TabState::BackgroundOld => Some(2),    // aggressive
                _ => None,
            };
            if let (Some(gc_level), Some(js)) = (
                gc_level_opt,
                self.bg_tabs.get(&tab_id).and_then(|s| s.js_ctx.as_ref()),
            ) {
                js.run_gc_pass(gc_level);
            }

            // Update strip badge for BackgroundOld (amber) or other tier changes.
            if let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) {
                self.tab_strip.set_tab_state(idx, tr.to);
            }
        }

        // Auto-archive (7A.5): move background tabs idle for > 12 h out of the
        // strip.  Only runs when there are в‰Ґ 2 tabs (the active tab is never
        // archived) and the tab is not already hibernated (RAM already saved).
        if self.tab_strip.len() >= 2 {
            let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
            let threshold = tabs::archive::ARCHIVE_AFTER_MS;
            // Collect IDs to archive (avoiding borrow conflict on tab_strip).
            let to_archive: Vec<usize> = self
                .tab_strip
                .tabs
                .iter()
                .enumerate()
                .filter(|(i, t)| {
                    *i != self.tab_strip.active
                        && t.tab_state != tab_lifecycle::TabState::Hibernated
                        && (now_ms - t.last_activated_ms) > threshold
                })
                .map(|(_, t)| t.id)
                .collect();

            for tab_id in to_archive {
                // Guard: never archive down to 0 tabs.
                if self.tab_strip.len() <= 1 {
                    break;
                }
                let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == tab_id) else {
                    continue;
                };
                let title = self.tab_strip.tabs[idx].title.clone();
                let container = self.tab_strip.tabs[idx].container;
                let url = self
                    .bg_tabs
                    .get(&tab_id)
                    .and_then(|s| s.source.url_str().map(|u| u.to_owned()))
                    .unwrap_or_default();
                self.archive.push(tabs::archive::ArchivedTab {
                    id: tab_id,
                    title,
                    url,
                    container,
                });
                // Evict in-memory snapshot and remove from strip + lifecycle.
                self.bg_tabs.remove(&tab_id);
                self.lifecycle_mgr.close_tab(tab_id as u64);
                self.tab_strip.remove(idx);
            }
        }
    }

    // в”Ђв”Ђ Tab management в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

    /// Move all per-page fields from `self` into a `PageSnapshot`.
    ///
    /// Called before switching to a different tab so the current page state can
    /// be frozen while the new tab becomes active.
    fn save_page_snapshot(&mut self) -> PageSnapshot {
        // РЎРїРёСЃРѕРє СѓРµР·Р¶Р°РµС‚ РІ СЃРЅР°РїС€РѕС‚, Р°РєС‚РёРІРЅС‹Р№ СЃР»РѕС‚ РѕСЃС‚Р°С‘С‚СЃСЏ РїСѓСЃС‚С‹Рј вЂ” РІРµСЂСЃРёСЏ
        // РѕР±СЏР·Р°РЅР° СЃРјРµРЅРёС‚СЊСЃСЏ С‚Р°Рє Р¶Рµ, РєР°Рє РїСЂРё РѕР±С‹С‡РЅРѕР№ Р·Р°РјРµРЅРµ (BUG-405 СЃСЂРµР· 39).
        self.bump_display_list_epoch();
        let snap = PageSnapshot {
            display_list: std::mem::take(&mut self.display_list),
            title: self.title.take(),
            pending_images: std::mem::take(&mut self.pending_images),
            page_font_registry: std::mem::replace(
                &mut self.page_font_registry,
                Arc::new(lumen_font::FontRegistry::new()),
            ),
            web_fonts: std::mem::take(&mut self.web_fonts),
            source: self.source.clone(),
            runtime: std::mem::take(&mut self.runtime),
            animation_scheduler: std::mem::replace(
                &mut self.animation_scheduler,
                animation_scheduler::AnimationScheduler::new(),
            ),
            transition_scheduler: std::mem::take(&mut self.transition_scheduler),
            starting_style_tracker: std::mem::take(&mut self.starting_style_tracker),
            prev_styles: std::mem::take(&mut self.prev_styles),
            page_prev_cascade_styles: self.page_prev_cascade_styles.take(),
            page_prev_interactive: std::mem::take(&mut self.page_prev_interactive),
            anim_frame: self.anim_frame.take(),
            layout_box: self.layout_box.take(),
            page_tracks: std::mem::take(&mut self.page_tracks),
            find: std::mem::take(&mut self.find),
            address_bar: std::mem::take(&mut self.address_bar),
            hint: std::mem::take(&mut self.hint),
            scroll_y: self.scroll_y,
            scroll_x: self.scroll_x,
            content_height: self.content_height,
            content_width: self.content_width,
            layout_source: self.layout_source.take(),
            pending_reload: std::mem::replace(
                &mut self.pending_reload,
                Rc::new(Cell::new(false)),
            ),
            pending_js_navigate: self.pending_js_navigate.take(),
            stream_builder: self.stream_builder.take(),
            stream_last_paint: self.stream_last_paint,
            stream_sheet: std::mem::take(&mut self.stream_sheet),
            stream_layout_seeded: self.stream_layout_seeded,
            preload_dispatched: std::mem::take(&mut self.preload_dispatched),
            stream_images_requested: std::mem::take(&mut self.stream_images_requested),
            stream_image_sizes: std::mem::take(&mut self.stream_image_sizes),
            stream_image_sizes_dirty: self.stream_image_sizes_dirty,
            ime_composing: self.ime_composing.take(),
            bfcache: std::mem::replace(&mut self.bfcache, BfCache::new(16)),
            frozen_styles: std::mem::take(&mut self.frozen_styles),
            parked_pages: std::mem::take(&mut self.parked_pages),
            nav_back: std::mem::take(&mut self.nav_back),
            nav_fwd: std::mem::take(&mut self.nav_fwd),
            form_state: std::mem::take(&mut self.form_state),
            validation_tooltip: self.validation_tooltip.take(),
            color_picker_node: self.color_picker_node.take(),
            date_picker_node: self.date_picker_node.take(),
            select_dropdown_node: self.select_dropdown_node.take(),
            ls_storage: std::mem::take(&mut self.ls_storage),
            ss_storage: std::mem::take(&mut self.ss_storage),
            idb_dir: self.idb_dir.clone(),
            sw_backend: std::mem::replace(
                &mut self.sw_backend,
                Arc::new(std::sync::Mutex::new(
                    lumen_storage::store::InMemoryStorage::new(),
                )),
            ),
            js_ctx: self.take_js_ctx(),
            first_paint_delivered: self.first_paint_delivered,
            first_contentful_paint_delivered: self.first_contentful_paint_delivered,
            load_failed: self.load_failed,
            load_error_message: self.load_error_message.take(),
            nav_start: self.nav_start.take(),
            animated_gifs: std::mem::take(&mut self.animated_gifs),
            gif_last_frame: std::mem::take(&mut self.gif_last_frame),
            video_gif_last_frame: std::mem::take(&mut self.video_gif_last_frame),
            video_gif_frames: std::mem::take(&mut self.video_gif_frames),
            image_cache: std::mem::replace(
                &mut self.image_cache,
                lumen_image::ImageDecodeCache::new(),
            ),
            zoom_factor: self.zoom_factor,
            display_url: self.display_url.take(),
            current_history_state_json: std::mem::replace(
                &mut self.current_history_state_json,
                String::from("null"),
            ),
            reader_original_source: self.reader_original_source.take(),
            cert_info: self.cert_info.take(),
        };
        // ADR-016 M2.2d: Р°РєС‚РёРІРЅР°СЏ РІРєР»Р°РґРєР° РѕС‚РґР°Р»Р° СЃРІРѕР№ JS-С…СЌРЅРґР» РІ СЃРЅР°РїС€РѕС‚
        // (`js_ctx.take()` РІС‹С€Рµ) в†’ `js_present` СЃР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІРјРµСЃС‚Рµ СЃ РЅРёРј.
        self.js_present = false;
        snap
    }

    /// Restore per-page fields from a `PageSnapshot` into `self`.
    ///
    /// Called after a tab switch to make a previously-frozen tab active again.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn restore_page_snapshot(&mut self, snap: PageSnapshot) {
        self.set_display_list(snap.display_list);
        self.title = snap.title;
        self.pending_images = snap.pending_images;
        self.page_font_registry = snap.page_font_registry;
        self.web_fonts = snap.web_fonts;
        self.source = snap.source;
        self.runtime = snap.runtime;
        self.animation_scheduler = snap.animation_scheduler;
        self.transition_scheduler = snap.transition_scheduler;
        self.starting_style_tracker = snap.starting_style_tracker;
        self.prev_styles = snap.prev_styles;
        self.page_prev_cascade_styles = snap.page_prev_cascade_styles;
        self.page_prev_interactive = snap.page_prev_interactive;
        self.anim_frame = snap.anim_frame;
        self.layout_box = snap.layout_box;
        self.page_tracks = snap.page_tracks;
        self.sync_text_track_store();
        self.find = snap.find;
        self.address_bar = snap.address_bar;
        self.hint = snap.hint;
        self.scroll_y = snap.scroll_y;
        self.scroll_x = snap.scroll_x;
        self.content_height = snap.content_height;
        self.content_width = snap.content_width;
        self.layout_source = snap.layout_source;
        self.pending_reload = snap.pending_reload;
        self.pending_js_navigate = snap.pending_js_navigate;
        self.stream_builder = snap.stream_builder;
        self.stream_last_paint = snap.stream_last_paint;
        self.stream_sheet = snap.stream_sheet;
        self.stream_layout_seeded = snap.stream_layout_seeded;
        self.preload_dispatched = snap.preload_dispatched;
        self.stream_images_requested = snap.stream_images_requested;
        self.stream_image_sizes = snap.stream_image_sizes;
        self.stream_image_sizes_dirty = snap.stream_image_sizes_dirty;
        self.ime_composing = snap.ime_composing;
        self.bfcache = snap.bfcache;
        self.frozen_styles = snap.frozen_styles;
        self.parked_pages = snap.parked_pages;
        self.nav_back = snap.nav_back;
        self.nav_fwd = snap.nav_fwd;
        self.form_state = snap.form_state;
        self.validation_tooltip = snap.validation_tooltip;
        self.color_picker_node = snap.color_picker_node;
        self.date_picker_node = snap.date_picker_node;
        self.select_dropdown_node = snap.select_dropdown_node;
        self.ls_storage = snap.ls_storage;
        self.ss_storage = snap.ss_storage;
        self.idb_dir = snap.idb_dir;
        self.sw_backend = snap.sw_backend;
        self.set_js_ctx(snap.js_ctx);
        self.first_paint_delivered = snap.first_paint_delivered;
        self.first_contentful_paint_delivered = snap.first_contentful_paint_delivered;
        self.load_failed = snap.load_failed;
        self.load_error_message = snap.load_error_message;
        self.nav_start = snap.nav_start;
        self.animated_gifs = snap.animated_gifs;
        self.gif_last_frame = snap.gif_last_frame;
        self.video_gif_last_frame = snap.video_gif_last_frame;
        self.video_gif_frames = snap.video_gif_frames;
        // Rebuild playback state from restored frames; JS re-queues loads on restore.
        self.video_gif_store.pending_loads.lock().unwrap().clear();
        {
            let mut pb = self.video_gif_store.playback.lock().unwrap();
            pb.clear();
            for (nid, gif) in &self.video_gif_frames {
                let cycle_ms: u64 = gif.total_cycle_ms();
                let loop_count = match gif.loop_count {
                    lumen_image::GifLoopCount::Infinite | lumen_image::GifLoopCount::Finite(0) => 0u32,
                    lumen_image::GifLoopCount::Finite(n) => u32::from(n),
                };
                pb.insert(*nid, lumen_js::video_gif_store::VideoPlaybackState {
                    paused: true,
                    position_ms: 0,
                    play_epoch_ms: None,
                    cycle_ms,
                    loop_count,
                    width: gif.width,
                    height: gif.height,
                });
            }
        }
        self.image_cache = snap.image_cache;
        self.zoom_factor = snap.zoom_factor;
        self.display_url = snap.display_url;
        self.current_history_state_json = snap.current_history_state_json;
        self.reader_original_source = snap.reader_original_source;
        self.cert_info = snap.cert_info;
        // ADR-016 M2.2c-2b: Р·РµСЂРєР°Р»РёРј С…СЌРЅРґР» + DOM РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅРѕР№ РІРєР»Р°РґРєРё РІ РїРѕС‚РѕРє.
        self.sync_engine_js_state();
        // Notify platform bridge with the restored tab's accessibility tree.
        self.update_platform_ax_tree();
    }

    /// Reset all per-page fields to blank-tab defaults.
    ///
    /// Called after `save_page_snapshot()` to prepare `self` for a fresh tab
    /// before loading a URL or showing an empty page.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn reset_to_blank_tab(&mut self) {
        self.set_display_list(Vec::new());
        self.title = None;
        self.pending_images = Vec::new();
        self.source = PageSource::Empty;
        self.runtime = runtime::EventLoop::new();
        self.animation_scheduler = animation_scheduler::AnimationScheduler::new();
        self.transition_scheduler = TransitionScheduler::new();
        self.starting_style_tracker = StartingStyleTracker::new();
        self.prev_styles = HashMap::new();
        self.page_prev_cascade_styles = None;
        self.page_prev_interactive = (None, None, None);
        self.anim_frame = None;
        self.layout_box = None;
        self.find = find::FindState::default();
        self.address_bar = address_bar::AddressBarState::default();
        self.hint = hints::HintState::default();
        self.scroll_y = 0.0;
        self.scroll_x = 0.0;
        // ADR-016 M3.2: the retained scroll band belongs to the old page вЂ” drop
        // it so the next frame repaints instead of blitting stale pixels.
        self.scroll_cache.invalidate();
        self.content_height = 0.0;
        self.content_width = 0.0;
        self.layout_source = None;
        self.pending_reload = Rc::new(Cell::new(false));
        self.pending_js_navigate = None;
        self.stream_builder = None;
        self.stream_last_paint = std::time::Instant::now();
        self.stream_sheet = lumen_css_parser::Stylesheet::default();
        self.stream_layout_seeded = false;
        self.preload_dispatched = std::collections::HashSet::new();
        self.stream_images_requested = std::collections::HashSet::new();
        self.stream_image_sizes = HashMap::new();
        self.stream_image_sizes_dirty = false;
        self.ime_composing = None;
        self.bfcache = BfCache::new(16);
        self.frozen_styles = HashMap::new();
        self.parked_pages = Vec::new();
        self.nav_back = Vec::new();
        self.nav_fwd = Vec::new();
        self.form_state = HashMap::new();
        self.validation_tooltip = None;
        self.color_picker_node = None;
        self.date_picker_node = None;
        self.date_picker_year = 0;
        self.date_picker_month = 0;
        self.select_dropdown_node = None;
        self.ls_storage = HashMap::new();
        // BUG-836: a new tab is a new browsing context, so it starts with empty
        // session storage вЂ” this reset is the *only* place it may be cleared.
        self.ss_storage = HashMap::new();
        // idb_dir is session-level вЂ” intentionally not reset here.
        self.sw_backend = Arc::new(std::sync::Mutex::new(
            lumen_storage::store::InMemoryStorage::new(),
        ));
        self.set_js_ctx(None);
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        self.load_failed = false;
        self.load_error_message = None;
        self.nav_start = None;
        self.animated_gifs = HashMap::new();
        self.gif_last_frame = HashMap::new();
        self.video_gif_store.playback.lock().unwrap().clear();
        self.video_gif_store.pending_loads.lock().unwrap().clear();
        self.video_gif_last_frame = HashMap::new();
        self.video_gif_frames = HashMap::new();
        self.image_cache = lumen_image::ImageDecodeCache::new();
        self.zoom_factor = zoom::ZOOM_DEFAULT;
        self.display_url = None;
        self.current_history_state_json = String::from("null");
        self.reader_original_source = None;
        self.cert_info = None;
        // Cancel in-flight scroll animations.
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.forward_momentum_stop();
        self.scroll_drag = None;
        // ADR-016 M2.2c-2b: РѕС‡РёС‰Р°РµРј С…СЌРЅРґР» + DOM РІ РґРІРёР¶РєРѕРІРѕРј РїРѕС‚РѕРєРµ РґР»СЏ С‡РёСЃС‚РѕР№ РІРєР»Р°РґРєРё.
        self.sync_engine_js_state();
    }

    /// Open a new blank tab.
    fn open_new_tab(&mut self) {
        // In tree-style tab mode, new tabs become children of the active tab,
        // building the parent-child tree automatically.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let new_idx = if self.tree_tabs.visible {
            let opener_id = self.tab_strip.tabs[self.tab_strip.active].id;
            self.tab_strip.push_with_opener(opener_id, now_ms)
        } else {
            self.tab_strip.push_blank(now_ms)
        };
        let new_id = self.tab_strip.tabs[new_idx].id;
        // Save current page into bg_tabs under the old active tab's id.
        let old_active = self.tab_strip.active;
        let old_id = self.tab_strip.tabs[old_active].id;
        // Mark old tab as recently backgrounded so it gets a badge if it ages to T2.
        self.tab_strip.set_tab_state(old_active, TabState::BackgroundRecent);
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        self.tab_strip.active = new_idx;
        self.reset_to_blank_tab();
        // Register the new tab with the lifecycle manager.
        self.lifecycle_mgr.open_tab(new_id as u64);
        // CC-6: re-sync the CSS chrome's tab list (no-op off the flag).
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Open or toggle split view (Ctrl+\).
    ///
    /// Picks the next tab after the active one for the right pane. If no other
    /// tab exists, does nothing (split requires at least two tabs).
    fn toggle_split_view(&mut self) {
        let tab_count = self.tab_strip.len();
        if tab_count < 2 {
            return;
        }
        let next_idx = (self.tab_strip.active + 1) % tab_count;
        let next_id = self.tab_strip.tabs[next_idx].id;

        let (dl, scroll_y, scroll_x, content_height, content_width) =
            if let Some(snap) = self.bg_tabs.get(&next_id) {
                (
                    snap.display_list.clone(),
                    snap.scroll_y,
                    snap.scroll_x,
                    snap.content_height,
                    snap.content_width,
                )
            } else if let Some(meta) = self.hibernated_tabs.get(&next_id) {
                // Hibernated tab: show a minimal placeholder with its title/url.
                let placeholder_dl = build_split_placeholder(&meta.url);
                (placeholder_dl, 0.0, 0.0, 0.0, 0.0)
            } else {
                // Blank/new tab вЂ” show empty pane.
                (vec![], 0.0, 0.0, 0.0, 0.0)
            };

        self.split_view = Some(panels::split_view::SplitView::new(
            next_id,
            dl,
            scroll_y,
            scroll_x,
            content_height,
            content_width,
        ));
    }

    /// Close the tab at `idx`. If it was the last tab, exits the app instead.
    fn close_tab(&mut self, idx: usize, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.tab_strip.len() == 1 {
            // Last tab вЂ” exit.
            event_loop.exit();
            return;
        }
        let closing_id = self.tab_strip.tabs[idx].id;
        // Remove from lifecycle manager.
        self.lifecycle_mgr.close_tab(closing_id as u64);
        if idx == self.tab_strip.active {
            // Closing the active tab: save nothing (it will be dropped),
            // restore the tab that will become active after removal.
            let new_active = self.tab_strip.remove(idx);
            let new_id = self.tab_strip.tabs[new_active].id;
            // Mark the newly-activated tab as Active so its badge clears.
            self.tab_strip.set_tab_state(new_active, TabState::Active);
            // Drop the current active page.
            self.reset_to_blank_tab();
            if let Some(snap) = self.bg_tabs.remove(&new_id) {
                self.restore_page_snapshot(snap);
            } else if self.hibernated_tabs.contains_key(&new_id) {
                // Target tab is hibernated вЂ” restore from SQLite.
                self.restore_hibernated_tab(new_id);
            }
        } else {
            // Closing a background tab: drop snapshot and any hibernated/sleeping data.
            self.bg_tabs.remove(&closing_id);
            self.hibernated_tabs.remove(&closing_id);
            let _ = self.tab_snapshots.delete(closing_id as i64);
            let _ = self.t2_store.delete(closing_id as i64);
            self.tab_strip.remove(idx);
        }
        // CC-6: re-sync the CSS chrome's tab list (no-op off the flag).
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Execute a tab context-menu action (CC-4) on `tab_context_menu.target_idx`.
    fn exec_tab_menu_action(
        &mut self,
        action: tabs::context_menu::MenuAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use tabs::context_menu::MenuAction;
        let idx = self.tab_context_menu.target_idx;
        if idx >= self.tab_strip.len() {
            return;
        }
        match action {
            MenuAction::TogglePin => {
                self.tab_strip.toggle_pin(idx);
                self.request_redraw();
            }
            MenuAction::Duplicate => self.duplicate_tab(idx),
            MenuAction::MoveToNewWindow => self.move_tab_to_new_window(idx, event_loop),
            MenuAction::AddToNewGroup => {
                // CC-6: bundle the target tab into a fresh group, cycling the
                // colour by group count so successive groups differ. Persist
                // the group metadata so a future restore can recover it.
                use tabs::groups::GroupColor;
                let color = GroupColor::from_index((self.tab_strip.groups.len() % 8) as u8);
                let gid = self.tab_strip.create_group("Р“СЂСѓРїРїР°", color);
                self.tab_strip.assign_to_group(idx, gid);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let _ = self.tab_groups.create("Р“СЂСѓРїРїР°", color.index(), now);
                self.request_redraw();
            }
            MenuAction::ToggleGroupCollapse => {
                if let Some(gid) = self.tab_strip.group_of(idx) {
                    let now_collapsed = self.tab_strip.toggle_collapse(gid);
                    // If the active tab is hidden by collapsing, move focus to
                    // the group's chip tab so a valid page stays displayed.
                    if now_collapsed
                        && !self.tab_strip.visible_indices().contains(&self.tab_strip.active)
                        && let Some(&chip) = self.tab_strip.group_members(gid).first()
                    {
                        self.switch_tab(chip);
                    }
                    self.request_redraw();
                }
            }
            MenuAction::RemoveFromGroup => {
                if let Some(gid) = self.tab_strip.group_of(idx) {
                    self.tab_strip.ungroup(idx);
                    // Drop the group entirely once its last member leaves.
                    if self.tab_strip.group_members(gid).is_empty() {
                        self.tab_strip.remove_group(gid);
                    }
                    self.request_redraw();
                }
            }
            MenuAction::CloseOthers => {
                // Keep the target visible: switch to it first so a surviving
                // page is shown, then drop everything else (non-pinned).
                if idx != self.tab_strip.active {
                    self.switch_tab(idx);
                }
                let keep = self.tab_strip.active;
                let removed = self.tab_strip.close_others(keep);
                self.discard_tab_resources(&removed);
                self.request_redraw();
            }
            MenuAction::CloseRight => {
                // If the active tab would be removed, switch to the target
                // (which always survives) so the displayed page stays valid.
                let active = self.tab_strip.active;
                if active > idx && !self.tab_strip.is_pinned(active) {
                    self.switch_tab(idx);
                }
                let removed = self.tab_strip.close_right(idx);
                self.discard_tab_resources(&removed);
                self.request_redraw();
            }
}
        }

        /// Resolve an automation click target to `handle_click_at`'s expected
        /// OS-window CSS-pixel coordinates.
        ///
        /// `NodeId`/`Selector` rects come out of the layout tree in *page*
        /// (document) space; `handle_click_at` expects *OS window* space (what
        /// a real OS mouse event reports вЂ” see `page_point`, which converts the
        /// other way: `page = window - tab_bar/panel_offset + scroll`). This
        /// applies the inverse (`window = page - scroll + tab_bar/panel_offset`)
        /// so a click lands on the resolved element instead of wherever
        /// page-space coordinates happen to fall in window space (off by the
        /// tab-bar height and current scroll вЂ” silently "worked" only by
        /// coincidence when scroll was 0 and the target sat within the
        /// tab-bar-height band).
        ///
        /// `Target::Point` gets a *different* correction: BiDi/MCP callers
        /// (`input.performActions` pointer coordinates) supply pixels in the
        /// rendered *content-viewport* space вЂ” the same space `captureScreenshot`
        /// renders (no scroll subtraction needed, since it's relative to the
        /// already-scrolled visible viewport, not absolute document position) вЂ”
        /// so only the tab-bar/toolbar/panel offset is added, not scroll. Confirmed
        /// by hand: without this, `input.performActions` clicks landed above the
        /// target by exactly `toolbar::CHROME_H` (real pixel offset validated with
        /// a manual BiDi clickв†’navigate scenario; DS-9 widened the offset from
        /// the tab-bar-only height to include the new toolbar row).
        ///
        /// CC-14: the offset itself is [`Self::page_offset`], not a hardcoded
        /// `(left_dock width, toolbar::CHROME_H)` pair вЂ” the content area's
        /// real origin is `chrome_page_host_rect`'s, which can differ from the
        /// legacy toolbar/sidebar geometry (e.g. the web/AI sidebar occupies
        /// chrome layout width but is not a `left_dock()` entry at all,
        /// see [`Self::dockable_sidebars`]). Using the wrong offset here would
        /// silently misfire every MCP/BiDi click/type once engine chrome is
        /// the default, since `page_offset()` is otherwise the single source
        /// of truth for this conversion (real mouse input already uses it).
        fn resolve_automation_target(&self, target: &lumen_driver::Target) -> Option<(f32, f32)> {
            use lumen_driver::Target;
            let (offset_x, offset_y) = self.page_offset();
            let page_to_viewport = |px: f32, py: f32| {
                (px - self.scroll_x + offset_x, py - self.scroll_y + offset_y)
            };
            match target {
                Target::Point { x, y } => Some((x + offset_x, y + offset_y)),
                Target::NodeId(id) => {
                    let lb = self.layout_box.as_ref()?;
                    let node = lumen_dom::NodeId::from_index(*id as usize);
                    let rect = forms::find_box_rect(lb, node)?;
                    Some(page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
                }
                Target::Selector(selector) => {
                    let lb = self.layout_box.as_ref()?;
                    let doc = self.layout_source.as_ref()?.document.lock().ok()?;
                    let rect = lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                        .first()?
                        .rect;
                    Some(page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
                }
            }
        }

        /// Find DOM nodes by CSS selector for `AutomationCommand::Query` (SDC-2).
        ///
        /// Returns an empty vector if no page is loaded or nothing matches вЂ”
        /// mirrors `InProcessSession::query`'s behavior for the same case.
        fn query_automation_nodes(&self, selector: &str) -> Vec<lumen_driver::NodeRef> {
            let Some(lb) = self.layout_box.as_ref() else { return Vec::new() };
            let Some(source) = self.layout_source.as_ref() else { return Vec::new() };
            let Ok(doc) = source.document.lock() else { return Vec::new() };
            lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                .into_iter()
                .map(|found| {
                    let tag_name = match &doc.get(found.node).data {
                        NodeData::Element { name, .. } => name.local.to_string(),
                        _ => String::new(),
                    };
                    let mut text_content = String::new();
                    collect_automation_text(&doc, found.node, &mut text_content);
                    lumen_driver::NodeRef {
                        node_id: found.node.index() as u32,
                        tag_name,
                        text_content,
                        bounding_rect: found.rect,
                    }
                })
                .collect()
        }

        /// Build the accessibility tree for `AutomationCommand::A11yTree` (SDC-2).
        ///
        /// Returns `None` if no page is loaded.
        fn automation_a11y_tree(&self) -> Option<lumen_driver::A11yNode> {
            let source = self.layout_source.as_ref()?;
            let doc = source.document.lock().ok()?;
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            let ax_tree = lumen_a11y::build_ax_tree(&doc, doc.root(), &flat_tree);
            let chrome = self.chrome_ax_nodes();
            let ax_tree = lumen_a11y::chrome::attach_chrome(ax_tree, chrome);
            Some(automation_ax_node(&ax_tree.root))
        }

        /// Box-model snapshot of the whole page for `AutomationCommand::LayoutSnapshot`
        /// (DEVX-14, wires `resource://layout` to the live window).
        ///
        /// Empty if no page is loaded вЂ” mirrors `InProcessSession::layout_snapshot`'s
        /// behavior on the equivalent state.
        fn automation_layout_snapshot(&self) -> Vec<BoxModel> {
            let Some(lb) = self.layout_box.as_ref() else { return Vec::new() };
            let Some(source) = self.layout_source.as_ref() else { return Vec::new() };
            let Ok(doc) = source.document.lock() else { return Vec::new() };
            let mut out = Vec::new();
            lumen_driver::scope::collect_boxes(lb, &doc, &mut out);
            out
        }

        /// Network request log for `AutomationCommand::NetworkLog` (DEVX-14,
        /// wires `resource://network` to the live window) вЂ” reads the same
        /// shared `NetworkLog` the DevTools network panel renders from,
        /// regardless of whether that panel is currently open.
        ///
        /// `size_bytes` is always 0: unlike `InProcessSession`'s network log,
        /// the DevTools panel's `NetworkEntry` doesn't track response size.
        fn automation_network_log(&self) -> Vec<DriverNetworkEntry> {
            self.network_panel
                .entries_clone()
                .iter()
                .map(|e| DriverNetworkEntry {
                    url: e.url.clone(),
                    method: e.method.clone(),
                    status: e.status.unwrap_or(0),
                    size_bytes: 0,
                })
                .collect()
        }

        /// Poll an `AutomationCommand::Wait` condition against current shell
        /// state (SDC-1b). Never blocks вЂ” called once per frame from
        /// `about_to_wait` via `self.pending_waits` until it returns `true` or
        /// the wait's deadline passes.
        ///
        /// `NetworkIdle` and `Stable` are conservative approximations (no
        /// in-flight-request counter or cross-frame rect history exists yet in
        /// the shell вЂ” same simplification `InProcessSession::check_wait_condition`
        /// uses headless): `NetworkIdle` falls back to `DocumentReady`, and
        /// `Stable` only checks that the selector currently matches an element.
        ///
        /// `DocumentReady` reads the real `document.readyState` from the JS
        /// runtime (P2-wpt S1) rather than approximating via `self.layout_box`
        /// вЂ” the layout box exists as soon as the *previous* page's box tree
        /// is still around (it is not reset on ordinary navigation, only on
        /// `reset_to_blank_tab`), so it was `true` immediately on repeat
        /// navigations even before the new page finished loading.
        ///
        /// When a real JS context is available, also gated on
        /// `self.nav_start.is_none()`: on the non-blocking streaming
        /// navigation path (`reload`/`navigate_to` with a window already
        /// open), `self.js_ctx` still holds the *previous* page's context
        /// until `apply_loaded_page` installs the new one вЂ” reading
        /// `document.readyState` without this gate would see the old page's
        /// already-`"complete"` state and report ready immediately,
        /// reproducing the exact bug this fixes. `nav_start` is set at the
        /// start of every navigation and only cleared once
        /// `apply_loaded_page` (which also installs the fresh JS context and
        /// fires the real `load` event) has run вЂ” see `RenderDone` handling.
        /// (`nav_start` is only cleared under `#[cfg(feature = "v8")]`,
        /// so the gate is scoped to the branch that actually has a JS
        /// context вЂ” the `layout_box` fallback below stays independent of it
        /// for JS-less builds/tabs, matching the pre-S1 behavior there.)
        fn check_wait_condition(&self, cond: &WaitCondition) -> bool {
            match cond {
                WaitCondition::DocumentReady | WaitCondition::NetworkIdle => {
                    // A settled navigation error (network/HTTP failure) is "done
                    // loading" вЂ” resolve immediately instead of hanging until the
                    // wait's deadline (BUG-308). Without this, a nav that ends in
                    // `LoadError` with no JS context and no prior `layout_box`
                    // (e.g. `about:blank` в†’ an anti-bot 403) never satisfies
                    // either readiness branch below, so `wait{document_ready}`
                    // blocks for minutes. Still gated on `nav_start.is_none()` so
                    // a stale flag from a superseded nav can't win a race.
                    if self.nav_start.is_none() && self.load_failed {
                        return true;
                    }
                    // ADR-016: eval С‡РµСЂРµР· `route_query_js`, С‚РѕС‚ Р¶Рµ РїР°С‚С‚РµСЂРЅ, С‡С‚Рѕ
                    // `WaitCondition::JsIdle` РЅРёР¶Рµ.
                    match route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                        j.eval_js_value("document.readyState")
                    }) {
                        Some(Ok(json)) => self.nav_start.is_none() && json == "\"complete\"",
                        // No JS context at all (v8 disabled, or a
                        // JS-less blank tab) вЂ” fall back to the coarser
                        // layout signal so `Wait` doesn't hang forever on a
                        // readiness signal that will never arrive. Still gated
                        // on `nav_start.is_none()` (found while diagnosing
                        // P2-wpt S4): without this gate, a navigation issued
                        // from a JS-less blank tab (or racing the brief window
                        // before the new page's JS context is installed) could
                        // see `js_ctx` as the *old* tab's `None`, and report
                        // ready from the *previous* page's already-populated
                        // `layout_box` before the new page had even started
                        // loading вЂ” the same "stale state wins the race"
                        // pattern BUG-296 fixed for session restore.
                        _ => self.nav_start.is_none() && self.layout_box.is_some(),
                    }
                }
                WaitCondition::Visible(selector) => {
                    let Some(lb) = self.layout_box.as_ref() else { return false };
                    let Some(source) = self.layout_source.as_ref() else { return false };
                    let Ok(doc) = source.document.lock() else { return false };
                    lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                        .first()
                        .is_some_and(|b| b.rect.width > 0.0 && b.rect.height > 0.0)
                }
                WaitCondition::Stable(selector) => {
                    let Some(lb) = self.layout_box.as_ref() else { return false };
                    let Some(source) = self.layout_source.as_ref() else { return false };
                    let Ok(doc) = source.document.lock() else { return false };
                    !lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector).is_empty()
                }
                // ADR-016 M2.2c-2d: РїРѕСЃР»РµРґРЅРµРµ РїСЂСЏРјРѕРµ `self.js_ctx`-С‡С‚РµРЅРёРµ РІ wait-poll вЂ”
                // `has_raf_pending` С‡РµСЂРµР· `route_query_js` (РїРѕРґ С„Р»Р°РіРѕРј вЂ” Р±Р»РѕРєРёСЂСѓСЋС‰РёР№
                // `query`; РІРЅРµС€РЅРёР№ `None` = В«Р±РµР· JSВ» в†’ idle, РєР°Рє РїСЂРµР¶РЅРёР№ `is_none_or`).
                WaitCondition::JsIdle => {
                    !route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |c| {
                        c.has_raf_pending()
                    })
                    .unwrap_or(false)
                }
            }
        }

        /// Apply scroll delta with bounds clamping.
        fn scroll_by_delta(&mut self, dx: f32, dy: f32) {
            self.scroll_x = (self.scroll_x + dx).max(0.0);
            self.scroll_y = (self.scroll_y + dy).max(0.0);
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }

        /// Render the currently loaded page's content area to PNG bytes
        /// (`AutomationCommand::Screenshot`, SDC-1b).
        ///
        /// Renders `self.display_list` вЂ” the page content only, not the browser
        /// chrome (tab strip/panels) вЂ” through the deterministic CPU rasterizer
        /// (same renderer as `--screenshot`/`--ipc-server`), at the current
        /// window's content viewport size and scroll offset.
        ///
        /// BUG-729: the image set comes from `self.image_cache`, whose keys are
        /// the very strings `register_image` gets вЂ” i.e. exactly what the
        /// display list's `DrawImage`/`LazyImageSlot`/background-image commands
        /// look up. Passing an empty slice here (the SDC-1b behaviour) made
        /// *every* picture on the page rasterize as the grey placeholder, so an
        /// automation screenshot of a perfectly rendering page read as "the
        /// browser draws no images at all". Canvas 2D bitmaps are still absent:
        /// they live in the JS runtime and reach paint only through the per-frame
        /// `flush_canvas_updates` drain into the GPU renderer, never through this
        /// CPU-side cache.
        fn render_current_page_to_png(&self) -> Result<Vec<u8>, String> {
            use lumen_paint::Renderer;
            let width = (self.viewport_width_css().max(1.0)) as u32;
            let height = (self.viewport_height_css().max(1.0)) as u32;
            let images = self.image_cache.snapshot();
            let image = Renderer::render_to_image_cpu(
                width,
                height,
                &self.display_list,
                &images,
                self.scroll_x,
                self.scroll_y,
            )
            .map_err(|e| format!("render_to_image_cpu: {e}"))?;
            lumen_image::encode_png_rgba8(&image).map_err(|e| format!("PNG encoding: {e}"))
        }

    /// Drop the cached page resources of background tabs removed in bulk
    /// (CC-4 "Close others" / "Close to the right"). Mirrors the background
    /// branch of [`close_tab`].
    fn discard_tab_resources(&mut self, ids: &[usize]) {
        for &id in ids {
            self.lifecycle_mgr.close_tab(id as u64);
            self.bg_tabs.remove(&id);
            self.hibernated_tabs.remove(&id);
            let _ = self.tab_snapshots.delete(id as i64);
            let _ = self.t2_store.delete(id as i64);
        }
    }

    /// Duplicate the tab at `idx` (CC-4): insert a copy right after it and
    /// load the same page into it. Phase 0 re-fetches the source URL rather
    /// than deep-cloning live page/JS state.
    fn duplicate_tab(&mut self, idx: usize) {
        // Bring the source tab to the foreground so `self.source` is its page.
        if idx != self.tab_strip.active {
            self.switch_tab(idx);
        }
        let src_idx = self.tab_strip.active;
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        let Some(new_idx) = self.tab_strip.duplicate(src_idx, now_ms) else {
            return;
        };
        let src_source = self.source.clone();
        // Park the source page in bg_tabs under its own id.
        let old_id = self.tab_strip.tabs[src_idx].id;
        self.tab_strip.set_tab_state(src_idx, TabState::BackgroundRecent);
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        // Activate the duplicate and load a fresh copy of the page.
        let new_id = self.tab_strip.tabs[new_idx].id;
        self.lifecycle_mgr.open_tab(new_id as u64);
        self.tab_strip.active = new_idx;
        self.tab_strip.set_tab_state(new_idx, TabState::Active);
        self.tab_strip.update_last_activated(new_idx, now_ms);
        self.reset_to_blank_tab();
        self.source = src_source;
        self.reload();
        self.request_redraw();
    }

    /// Move the tab at `idx` into a new OS window (CC-4). Phase 0 launches a
    /// fresh Lumen process for the tab's URL and removes the tab from this
    /// window. The last remaining tab is duplicated rather than moved (closing
    /// it would quit the app).
    fn move_tab_to_new_window(
        &mut self,
        idx: usize,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        if idx != self.tab_strip.active {
            self.switch_tab(idx);
        }
        let url = self.source.url_str().map(str::to_owned);
        if let Some(url) = url
            && let Ok(exe) = std::env::current_exe()
        {
            let _ = std::process::Command::new(exe).arg(&url).spawn();
        }
        // Remove the tab here unless it is the only one (closing it would exit).
        if self.tab_strip.len() > 1 {
            self.close_tab(self.tab_strip.active, event_loop);
        }
        self.request_redraw();
    }

    /// Assign `kind` to tab at `idx` for task 7D.2.
    ///
    /// Pre-registers a cookie/storage store id for the active page's origin
    /// if one is known, so subsequent requests can be partitioned. UI
    /// border-top strip refreshes on the next redraw via `build_tab_bar`.
    fn set_tab_container(&mut self, idx: usize, kind: tabs::containers::ContainerKind) {
        if idx >= self.tab_strip.len() {
            return;
        }
        self.tab_strip.set_tab_container(idx, kind);
        // Pre-warm a store id for the active tab's origin so cookie/storage
        // dispatch can partition by container id without a later allocation
        // step. Best-effort only вЂ” non-active tabs are wired up the same way
        // the next time their page loads.
        if idx == self.tab_strip.active
            && let Some(url) = self.source.url_str()
            && let Some(origin) = origin_of_url(url)
        {
            self.container_store.get_or_create(&origin, kind);
        }
        self.request_redraw();
    }

    /// Switch to tab at `idx`. No-op if already active.
    ///
    /// Handles all three cases:
    /// - T1/T2 tab: restore full `PageSnapshot` from `bg_tabs` (in-memory, fast).
    /// - T3 Hibernated tab: restore from SQLite via `Document::from_bytes()`.
    /// - Blank new tab: reset to empty state.
    fn switch_tab(&mut self, idx: usize) {
        if idx == self.tab_strip.active || idx >= self.tab_strip.len() {
            return;
        }
        // Save current active tab, marking it BackgroundRecent in the strip.
        let old_active = self.tab_strip.active;
        let old_id = self.tab_strip.tabs[old_active].id;
        self.tab_strip.set_tab_state(old_active, TabState::BackgroundRecent);
        // T0 в†’ T1: fire visibilitychange(hidden=true) before parking.
        // ADR-016 M2.2d (18): СЃРЅРёРјР°РµРј РїСЂСЏРјРѕРµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёРµ park-СЃР°Р№С‚Р° вЂ”
        // fire-and-forget void С‡РµСЂРµР· `route_task_js` (disjoint borrow РїРѕР»РµР№
        // `engine_thread`/`js_ctx`). РџРѕРґ С„Р»Р°РіРѕРј (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґРёС‚
        // `task`-РѕРј РЅР° РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє, РіРґРµ `state.js` РµС‰С‘ Р·РµСЂРєР°Р»РёС‚ СѓС…РѕРґСЏС‰СѓСЋ РІ С„РѕРЅ
        // РІРєР»Р°РґРєСѓ (СЂРµ-Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅРёРµ `sync_engine_js_state` РІСЃС‚Р°РЅРµС‚ РІ РѕС‡РµСЂРµРґСЊ РїРѕР·Р¶Рµ,
        // РїСЂРё Р·Р°РіСЂСѓР·РєРµ/РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёРё РЅРѕРІРѕР№) вЂ” pause РёСЃРїРѕР»РЅСЏРµС‚СЃСЏ РЅР° РІРµСЂРЅРѕРј С…СЌРЅРґР»Рµ.
        // Р‘РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ
        // РїСЂРµР¶РЅРµРјСѓ `js.pause_event_loop()`.
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.pause_event_loop();
        });
        let snap = self.save_page_snapshot();
        self.bg_tabs.insert(old_id, snap);
        // GC tuning (10L): run one moderate collection on the tab that just
        // went to background so it releases unreachable objects quickly.
        if let Some(js) = self.bg_tabs.get(&old_id).and_then(|s| s.js_ctx.as_ref()) {
            js.run_gc_pass(1);
        }

        // Sync lifecycle manager: deactivate old, activate new.
        let new_id = self.tab_strip.tabs[idx].id;
        self.lifecycle_mgr.activate_tab(new_id as u64);

        // Restore new active tab, marking it Active so any badge clears.
        let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
        self.tab_strip.active = idx;
        self.tab_strip.set_tab_state(idx, TabState::Active);
        self.tab_strip.update_last_activated(idx, now_ms);
        // BUG-411: re-point the process-global ad-block toggle at the shields
        // state of the host now in front. This used to read `TabEntry::adblock`
        // вЂ” a field the legacy in-tab checkbox wrote and CC-15 removed, leaving
        // it permanently `false`, so every tab switch silently disabled
        // filtering for the rest of the session. The restored navigation
        // handler below re-syncs on the host once the restored page loads.
        self.sync_adblock_filter();

        self.reset_to_blank_tab();

        if let Some(snap) = self.bg_tabs.remove(&new_id) {
            // T1/T2: fast in-memory restore.
            self.restore_page_snapshot(snap);
            // T1 в†’ T0: fire visibilitychange(hidden=false) after restore.
            // ADR-016 M2.2d (18): СЃРЅРёРјР°РµРј РїСЂСЏРјРѕРµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёРµ unpark-СЃР°Р№С‚Р° вЂ”
            // fire-and-forget void С‡РµСЂРµР· `route_task_js`. `restore_page_snapshot` РІС‹С€Рµ
            // СѓР¶Рµ РІС‹Р·РІР°Р» `sync_engine_js_state()` (Р·РµСЂРєР°Р»РёС‚ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅС‹Р№ С…СЌРЅРґР»
            // `task`-РѕРј), Р° СЌС‚РѕС‚ `task` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ** РЅРµРіРѕ вЂ” РїРѕРґ С„Р»Р°РіРѕРј
            // unpause+GC РёСЃРїРѕР»РЅСЏСЋС‚СЃСЏ РЅР° РІРµСЂРЅРѕРј (РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРЅРѕРј) С…СЌРЅРґР»Рµ. Р‘РµР· С„Р»Р°РіР° вЂ”
            // СЃРёРЅС…СЂРѕРЅРЅРѕ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРёРј `js.<method>()`.
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                j.unpause_event_loop();
                // GC tuning (10L): reset threshold to active level so the heap
                // can grow freely now that this tab is in the foreground.
                j.run_gc_pass(0);
            });
        } else if self.t2_store.exists(new_id as i64).unwrap_or(false) {
            // T2 crash-recovery: bg_tabs was lost (process restart) but SQLite
            // checkpoint exists вЂ” restore scroll + form state from it.
            self.restore_t2_tab(new_id);
        } else if self.hibernated_tabs.contains_key(&new_id) {
            // T3: restore from SQLite вЂ” Document::from_bytes() + relayout.
            self.restore_hibernated_tab(new_id);
        }
        // Otherwise the tab is blank (never loaded) вЂ” leave reset state.

        // DS-17: the synthetic TabList's `selected` state is rebuilt fresh
        // from `self.tab_strip.active` every time вЂ” without this, switching
        // to an already-loaded tab (no navigation, so nothing else rebuilds
        // the AX tree) left the OS bridge reporting the *previous* tab as
        // selected until the next full page load.
        self.update_platform_ax_tree();
        // CC-6: re-sync the CSS chrome's active-tab highlight (no-op off the flag).
        self.relayout_chrome_host();
        self.request_redraw();
    }
}

/// Р‘СЋРґР¶РµС‚ idle-РѕРєРЅР° РґР»СЏ `requestIdleCallback`-РѕРІ, РїРµСЂРµРґР°РІР°РµРјС‹Р№ РІ
/// `EventLoop::run_idle_callbacks` РЅР° РєР°Р¶РґРѕРј `about_to_wait`. Phase 0 РЅРµ Р·РЅР°РµС‚
/// СЂРµР°Р»СЊРЅРѕРіРѕ РІСЂРµРјРµРЅРё РґРѕ СЃР»РµРґСѓСЋС‰РµРіРѕ vsync, РїРѕСЌС‚РѕРјСѓ РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ С„РёРєСЃРёСЂРѕРІР°РЅРЅС‹Р№
/// 10 ms вЂ” С‚РѕС‚ Р¶Рµ РґРµС„РѕР»С‚, С‡С‚Рѕ Сѓ Chromium РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІРёРё СЏРІРЅРѕРіРѕ measurement-Р°
/// idle-РѕРєРЅР°. Idle-callback-Рё С‚СЂР°РєС‚СѓСЋС‚ СЌС‚Рѕ РєР°Рє В«СѓСЃРїРµР№ Р·Р° ~10 msВ».
const IDLE_BUDGET_MS: f64 = 10.0;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod navigate_by;
