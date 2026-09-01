//! `ApplicationHandler::resumed` — создание окна и первичная настройка
//! рендер-бэкенда (SPLIT-SH2).
//!
//! Тело вынесено из `impl ApplicationHandler<LoadEvent> for Lumen`
//! (`super`) как есть: trait-impl нельзя разложить по файлам, поэтому
//! метод трейта стал переходником на этот `impl Lumen`.

use crate::*;

impl Lumen {
    pub(crate) fn on_resumed(&mut self, event_loop: &ActiveEventLoop) {
        let (win_w, win_h) = if let Some((w, h)) = self.viewport_override {
            // `--viewport` (DEVX-1) wins over both defaults below — lets
            // `--deterministic` be combined with graphic_tests' fixed 1024×720
            // crop-calibration contract.
            (w, h + toolbar::CHROME_H)
        } else if self.deterministic.enabled {
            (1280.0, 800.0)
        } else {
            // Высота окна = CSS viewport (720) + tab bar + toolbar (CHROME_H) = 792,
            // чтобы веб-контент получал ровно 720 CSS px, как ожидают graphic tests.
            (1024.0, 720.0 + toolbar::CHROME_H)
        };
        let attrs = Window::default_attributes()
            .with_title(window_title(self.title.as_deref()))
            .with_inner_size(LogicalSize::new(win_w, win_h))
            .with_position(LogicalPosition::new(0, 0))
            .with_maximized(self.maximized);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Не удалось создать окно: {err}");
                event_loop.exit();
                return;
            }
        };

        // CSS Media Queries L5 §5.2 — read the OS `prefers-color-scheme` once the
        // window exists. winit resolves it per platform (Win32 immersive dark mode,
        // macOS NSAppearance, Linux portal/XSettings); `None` → light fallback.
        // In deterministic/headless runs we keep light to preserve snapshot stability.
        if !self.deterministic.enabled {
            self.dark_mode = platform::dark_mode::theme_prefers_dark(window.theme());
        }

        // PH2-7 Phase 1: pass the native HWND to the a11y bridge so it can fire
        // Win32 WinEvent notifications (NotifyWinEvent) for focus and tree changes.
        // Only attempted on Windows where WinUiaBridge is active; on other OSes
        // init_hwnd() is a no-op (default PlatformBridge impl).
        #[cfg(target_os = "windows")]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = window.window_handle()
                && let RawWindowHandle::Win32(h) = handle.as_raw()
            {
                self.platform_bridge.init_hwnd(h.hwnd.get());
            }
        }

        self.window = Some(window.clone());

        // Сбрасываем состояние предыдущего streaming-цикла — новая страница.
        self.preload_dispatched.clear();
        // BUG-839: same reset as the navigation path above, for the first load.
        resource_timing::clear();
        self.stream_images_requested.clear();
        self.stream_image_sizes.clear();
        self.stream_image_sizes_dirty = false;
        self.stream_sheet = lumen_css_parser::Stylesheet::default();
        self.stream_layout_seeded = false;
        // Record navigation start for the initial streaming load.
        self.nav_start = Some(std::time::Instant::now());
        // A fresh navigation supersedes any prior settled error (BUG-308).
        self.load_failed = false;
        self.load_error_message = None;
        self.load_generation = self.load_generation.wrapping_add(1);

        // BUG-274: `backend_factory::create_backend` below can take multiple
        // seconds on wgpu/DX12 (pipeline compilation, see BUG-406). Starting
        // `start_streaming_load` here (before backend creation) overlaps
        // network fetch/HTML parsing with GPU init instead of serializing
        // them — `HtmlChunk`'s `paint_partial_dom` already no-ops while
        // `self.renderer` is `None`, so this only changes *when* streaming
        // starts, not what it produces (display-list-neutral). On a local
        // file this can lose to CPU contention with the DX12 driver's
        // background pipeline-compile threads (BUG-406's "call returns
        // early, driver finishes later" hazard) — but three interleaved
        // rounds on a real network page (lenta.ru, live window,
        // `LUMEN_FRAME_LOG=1`, 2026-08-05) showed a consistent win by
        // `RenderDone` (true final page, not the mid-stream "first
        // non-empty frame" snapshot earlier measurements used): OLD
        // 3193/3278/3356ms vs NEW 2960/3001/3019ms, groups don't overlap.
        // Default flipped to early-stream; `LUMEN_NO_EARLY_STREAM=1` restores
        // the old (post-backend) ordering as an escape hatch — see the
        // "срез" write-ups in BUG-274-OPEN.md.
        let early_stream = std::env::var_os("LUMEN_NO_EARLY_STREAM").is_none();
        if early_stream {
            self.start_streaming_load(self.load_generation);
            if lumen_paint::frame_log_enabled()
                && let Some(ms) = bench_frames::since_process_start_ms()
            {
                eprintln!("[frame:cold-start] streaming started (pre-backend) at {ms:.0}ms");
            }
        }

        let mut renderer = match backend_factory::create_backend(
            window.clone(),
            INTER_FONT.to_vec(),
            self.target_color_space(),
        ) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Не удалось инициализировать рендер: {err}");
                event_loop.exit();
                return;
            }
        };
        if lumen_paint::frame_log_enabled()
            && let Some(ms) = bench_frames::since_process_start_ms()
        {
            eprintln!("[frame:cold-start] backend ready at {ms:.0}ms");
        }

        // Заливаем декодированные ранее картинки в GPU. Take, чтобы освободить
        // память Vec (изображение копируется в wgpu Texture внутри register_image).
        for (src, image) in self.pending_images.drain(..) {
            // BUG-272 срез 17: `image` — Arc; register shares it, raw_images
            // holds no separate copy.
            if let Err(err) = renderer.register_image(src.clone(), Arc::clone(&image)) {
                eprintln!("Картинка {src} не зарегистрирована: {err}");
            }
            self.image_cache.insert(lumen_image::ImageKey::new(&src), (*image).clone());
        }

        self.renderer = Some(renderer);
        // CC-4: first chrome layout pass, now that the renderer knows the
        // window's initial size.
        self.relayout_chrome_host();

        // GG-4: Restore vertical-tab layout from persisted settings.
        if tabs::strip::TabLayout::from_str(&self.settings_store.tab_layout())
            == tabs::strip::TabLayout::Vertical
        {
            self.vertical_tabs.visible = true;
        }

        if !early_stream {
            self.start_streaming_load(self.load_generation);
            if lumen_paint::frame_log_enabled()
                && let Some(ms) = bench_frames::since_process_start_ms()
            {
                eprintln!("[frame:cold-start] streaming started (post-backend) at {ms:.0}ms");
            }
        }
    }
}
