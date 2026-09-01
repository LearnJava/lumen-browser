//! Page state the shell keeps aside: the frozen state of a background tab
//! ([`PageSnapshot`]) and a page parked whole for back/forward ([`ParkedPage`]).
//!
//! Both are the same idea at different lifetimes — every per-page field of
//! `Lumen` moved out of it — but they part on what survives: a snapshot keeps
//! the rendered page of a tab that is not on screen, while a parked page keeps
//! its live V8 runtime and thread, which is the only way a restored document
//! still has its timers, closures and listeners (BUG-835).
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// A page kept *alive* for back/forward restoration (HTML LS §7.4.6, "salvageable
/// document" / bfcache).
///
/// The frozen bfcache path ([`Lumen::bfcache_thaw`]) serializes the DOM and
/// installs a **fresh** JS runtime over it, which loses every timer, closure and
/// event listener the page had — the restored document is inert, and that is
/// exactly what [BUG-835](../../../bugs/BUG-835-FIXED.md) measured: after
/// `history.back()` no script of the restored page ever ran again. Since each
/// `V8JsRuntime` owns its own OS thread and isolate,
/// keeping the whole handle alive is both possible and cheap to reason about:
/// nothing pumps a parked runtime (`route_task_js` only ever reaches the active
/// `js_ctx`), so its timers and animation frames are paused for exactly as long
/// as the page is in the back/forward cache — which is the spec's model.
///
/// Parked entries are capped at [`PARKED_PAGES_MAX`]; a page with no JS runtime
/// at all is not parked here but frozen the old way, where a fresh runtime over
/// the restored DOM loses nothing.
pub(crate) struct ParkedPage {
    /// The page's live JS runtime, unpumped while parked.
    pub(crate) js: Arc<dyn PersistentJs>,
    /// DOM shared with `js` — the same `Arc` the runtime holds.
    pub(crate) document: Arc<Mutex<Document>>,
    /// Stylesheet snapshot the page was laid out with.
    pub(crate) stylesheet: Arc<lumen_css_parser::Stylesheet>,
    /// Decoded HTML source, carried over so a later reload of the restored page
    /// does not need the network.
    pub(crate) html_source: Option<String>,
    /// Scroll offset at the moment the page was parked.
    pub(crate) scroll_x: f32,
    /// Scroll offset at the moment the page was parked.
    pub(crate) scroll_y: f32,
    /// Window title at the moment the page was parked.
    pub(crate) title: Option<String>,
}

/// How many live pages may sit in the back/forward cache at once.
///
/// Each entry pins a V8 isolate and its thread, so this is deliberately much
/// smaller than the HTML-snapshot [`BfCache`] capacity.
pub(crate) const PARKED_PAGES_MAX: usize = 2;

/// Frozen state of a background tab — moved in/out of `Lumen` on tab switch.
///
/// All per-page fields from `Lumen` live here while the tab is not active.
/// The active tab's state always lives directly in the `Lumen` struct fields.
pub(crate) struct PageSnapshot {
    pub(crate) display_list: DisplayList,
    pub(crate) title: Option<String>,
    pub(crate) pending_images: Vec<(String, Arc<lumen_image::Image>)>,
    /// PH3-19: saved across tab switch so web-fonts persist in background tabs.
    pub(crate) page_font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: web fonts decoded from @font-face url() sources, needed to
    /// rebuild MultiFontMeasurer on relayout when the tab is restored.
    pub(crate) web_fonts: Vec<LoadedWebFont>,
    pub(crate) source: PageSource,
    pub(crate) runtime: runtime::EventLoop,
    pub(crate) animation_scheduler: animation_scheduler::AnimationScheduler,
    pub(crate) transition_scheduler: TransitionScheduler,
    pub(crate) starting_style_tracker: StartingStyleTracker,
    pub(crate) prev_styles: HashMap<NodeId, ComputedStyle>,
    /// BUG-341 S7: mirrors `Lumen::page_prev_cascade_styles` — must travel
    /// with `layout_box` (same producer, same invalidation rule) so a tab
    /// switch back to this snapshot cannot resurrect a cache that no longer
    /// matches the restored tree.
    pub(crate) page_prev_cascade_styles: Option<lumen_layout::CascadeStyles>,
    pub(crate) page_prev_interactive: (Option<NodeId>, Option<NodeId>, Option<NodeId>),
    pub(crate) anim_frame: Option<lumen_layout::AnimationFrame>,
    pub(crate) layout_box: Option<lumen_layout::LayoutBox>,
    /// P3-webvtt срез 3: cues страницы — переезжают вместе с вкладкой.
    pub(crate) page_tracks: tracks::PageTracks,
    pub(crate) find: find::FindState,
    pub(crate) address_bar: address_bar::AddressBarState,
    pub(crate) hint: hints::HintState,
    pub(crate) scroll_y: f32,
    pub(crate) scroll_x: f32,
    pub(crate) content_height: f32,
    pub(crate) content_width: f32,
    pub(crate) layout_source: Option<LayoutSource>,
    pub(crate) pending_reload: Rc<Cell<bool>>,
    pub(crate) pending_js_navigate: Option<JsNavigateRequest>,
    pub(crate) stream_builder: Option<lumen_html_parser::IncrementalTreeBuilder>,
    pub(crate) stream_last_paint: std::time::Instant,
    /// CSS accumulated from parallel CSS-loader threads during streaming; applied to intermediate frames.
    pub(crate) stream_sheet: lumen_css_parser::Stylesheet,
    /// PH1-2b: whether `layout_box` is a valid graft source for the current stream.
    pub(crate) stream_layout_seeded: bool,
    pub(crate) preload_dispatched: std::collections::HashSet<String>,
    /// PH1-2c: image `src` keys already dispatched to background decode threads
    /// during the current streaming load. Dedup across intermediate frames so
    /// each `<img>` is fetched once. Cleared at the start of every navigation.
    pub(crate) stream_images_requested: std::collections::HashSet<String>,
    /// BUG-735: mirrors [`Lumen::stream_image_sizes`].
    pub(crate) stream_image_sizes: HashMap<String, (u32, u32)>,
    /// BUG-735: mirrors [`Lumen::stream_image_sizes_dirty`].
    pub(crate) stream_image_sizes_dirty: bool,
    pub(crate) ime_composing: Option<String>,
    pub(crate) bfcache: BfCache,
    /// Parsed stylesheets of frozen bfcache pages, keyed by URL.
    /// Kept shell-side because `Stylesheet` is not serializable.
    pub(crate) frozen_styles: HashMap<String, lumen_css_parser::Stylesheet>,
    /// Mirrors [`Lumen::parked_pages`] — travels with the tab so a parked page
    /// can never be restored into a different tab.
    pub(crate) parked_pages: Vec<(String, ParkedPage)>,
    pub(crate) nav_back: Vec<NavEntry>,
    pub(crate) nav_fwd: Vec<NavEntry>,
    pub(crate) form_state: forms::FormState,
    pub(crate) validation_tooltip: Option<(Rect, String)>,
    pub(crate) color_picker_node: Option<NodeId>,
    /// NodeId of the `<input type="date/…">` whose calendar picker is open in this tab snapshot.
    pub(crate) date_picker_node: Option<NodeId>,
    /// NodeId of the `<select>` whose dropdown is open in this tab snapshot.
    pub(crate) select_dropdown_node: Option<NodeId>,
    pub(crate) ls_storage: HashMap<String, Arc<Mutex<lumen_core::WebStorage>>>,
    /// Mirrors [`Lumen::ss_storage`] — travels with the tab so `sessionStorage`
    /// written by one of its documents is there for the next one, and for no
    /// other tab (BUG-836).
    pub(crate) ss_storage: HashMap<String, Arc<Mutex<lumen_core::WebStorage>>>,
    /// Directory for per-origin IndexedDB SQLite files. Cloned from the active
    /// tab's `idb_dir` when saving a snapshot; restored on tab switch-back.
    pub(crate) idb_dir: Option<std::path::PathBuf>,
    pub(crate) sw_backend: Arc<Mutex<dyn lumen_core::ext::StorageBackend>>,
    /// ADR-016 M2.2c-2b: `Arc` (не `Box`) — общий тип хэндла с активной вкладкой.
    pub(crate) js_ctx: Option<Arc<dyn PersistentJs>>,
    pub(crate) first_paint_delivered: bool,
    pub(crate) first_contentful_paint_delivered: bool,
    /// Per-tab settled-navigation-error flag (BUG-308); see the `Lumen` field.
    pub(crate) load_failed: bool,
    /// Per-tab settled-navigation-error message (BUG-438); see the `Lumen` field.
    pub(crate) load_error_message: Option<String>,
    /// Instant at which the current navigation began (set in `reload()`).
    /// Used to compute `duration` for the W3C Navigation Timing entry.
    pub(crate) nav_start: Option<std::time::Instant>,
    pub(crate) animated_gifs: HashMap<String, lumen_image::AnimatedGif>,
    pub(crate) gif_last_frame: HashMap<String, usize>,
    /// GIF-backed `<video>` frame keys: `"video:{nid}"` → current frame index.
    /// Parallel to `animated_gifs` but keyed by node ID, not URL.
    pub(crate) video_gif_last_frame: HashMap<u32, usize>,
    /// Decoded animated GIF frames for `<video>` nodes (keyed by nid).
    pub(crate) video_gif_frames: HashMap<u32, lumen_image::AnimatedGif>,
    pub(crate) image_cache: lumen_image::ImageDecodeCache,
    /// Per-tab user zoom factor. Preserved when the tab goes to background.
    pub(crate) zoom_factor: f32,
    /// Virtual URL shown in the address bar when `history.pushState` /
    /// `history.replaceState` changed the displayed URL without a page load.
    /// `None` → use `source.url_str()`.  Reset to `None` on any full navigation.
    pub(crate) display_url: Option<String>,
    /// Serialised JS state object for the current history entry, mirrored from
    /// the JS side so the shell can store it in `NavEntry` when pushState fires.
    /// Initialised to `"null"` (the default initial `history.state`).
    pub(crate) current_history_state_json: String,
    /// Original page source preserved while Reader View (§D-3) is active.
    /// `None` = this tab is not in reader mode.
    pub(crate) reader_original_source: Option<PageSource>,
    /// TLS certificate data for the current page (§D-1).
    ///
    /// Populated when a successful HTTPS connection is made; `None` for HTTP pages
    /// or when cert extraction is not yet wired (Phase 0 uses stubs).
    pub(crate) cert_info: Option<panels::cert_panel::PanelCertData>,
}
