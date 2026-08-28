//! Starting the window mode: build the event loop, the renderer backend and
//! the [`Lumen`] state, then hand control to winit.
//!
//! One function, and it is long because a window has many parts to assemble
//! before the first frame — the automation servers a `--bidi-port`/`--mcp-live-port`
//! run needs, the persistent stores, the session restore. What happens *after*
//! `run_app` takes over lives in `crate::app`.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`; only visibility
//! changed.

use crate::*;

#[allow(clippy::too_many_arguments)]
#[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn run_window_mode(
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
        hovered_frame: None,
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
