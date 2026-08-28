//! Freezing the live page into a `PageSnapshot` and thawing it back.
//!
//! Exactly one page is live at a time: every per-page field of `Lumen` - the
//! document, the layout tree, the JS context handle, scroll offsets, find
//! state - is moved wholesale into the outgoing tab's snapshot before the
//! incoming tab's snapshot is moved back in. The pair is therefore the
//! foundation of tab switching (`crate::lumen::tabs_cmd`) and of the T2 tier
//! of the lifecycle (`crate::lumen::hibernation`).

use crate::*;

impl Lumen {
    /// Move all per-page fields from `self` into a `PageSnapshot`.
    ///
    /// Called before switching to a different tab so the current page state can
    /// be frozen while the new tab becomes active.
    pub(crate) fn save_page_snapshot(&mut self) -> PageSnapshot {
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
    pub(crate) fn restore_page_snapshot(&mut self, snap: PageSnapshot) {
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
}
