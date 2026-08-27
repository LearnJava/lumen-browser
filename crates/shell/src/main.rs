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
    window_title,
};
use crate::input::dnd::{DND_THRESHOLD, DndState};
use crate::page_pipeline::{
    LayoutSource, LoadedPage, dispatch_preload_hints, parse_and_layout, render_bytes,
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
