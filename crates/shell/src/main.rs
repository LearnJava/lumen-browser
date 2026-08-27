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
mod view_transition;
mod window_metrics;
// Private `use` in the crate root is visible to descendants, so these lines
// double as a re-export for the `use crate::*;` of every submodule — no call
// site elsewhere in the crate had to change when SH-3a moved the bodies out.
use automation_server::{run_ipc_server, run_mcp_mode};
use lumen::Lumen;
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod navigate_by;
