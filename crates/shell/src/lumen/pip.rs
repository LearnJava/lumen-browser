//! Picture-in-picture in its three forms: the in-page overlay card, an OS
//! window carrying the page's `<video>`, and Document PiP.
//!
//! Which one opens depends on what the page offers and on what the platform
//! supports, but all three have the same shape - find the video (or the
//! document the page handed to `documentPictureInPicture`), open the surface,
//! and then keep the page in step with a window the shell no longer sizes,
//! which is why a resize has to be delivered back to JS. The windows
//! themselves are `crate::panels`; their event-loop plumbing is `crate::app`.

use crate::*;

impl Lumen {
    /// Toggle the picture-in-picture window (task #21).
    ///
    /// When closing, just hides the card.  When opening, scans the current page
    /// layout for the first `<video>` element and embeds its `src` / `poster`;
    /// if the page has no video, the card opens with a placeholder so the user
    /// still gets feedback (and can drag / close it).
    pub(crate) fn toggle_pip(&mut self) {
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
    pub(crate) fn open_pip_os(&mut self, event_loop: &ActiveEventLoop, nid: u32) {
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
    pub(crate) fn open_pip_os_document(&mut self, event_loop: &ActiveEventLoop, width: f32, height: f32) {
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
    pub(crate) fn close_pip_os(&mut self) {
        self.pip_os = None;
        self.pip.close();
    }

    /// CC-7: redraw the OS PiP window with the forwarded `<video>` content вЂ”
    /// the poster letterboxed (`object-fit: contain`) into the floating window's
    /// current client area. No-op when no OS PiP window is open.
    pub(crate) fn render_pip_os(&mut self) {
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
    pub(crate) fn deliver_pip_resize(&mut self) {
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
    pub(crate) fn notify_pip_window_resized(&mut self, win_w: f32, win_h: f32) {
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
    pub(crate) fn open_doc_pip_os(&mut self, event_loop: &ActiveEventLoop, width: u32, height: u32) {
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
    pub(crate) fn close_doc_pip_os(&mut self) {
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
    pub(crate) fn render_doc_pip_os(&mut self) {
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
    pub(crate) fn notify_docpip_window_resized(&mut self, win_w: f32, win_h: f32) {
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!(
                "if(typeof _lumen_docpip_deliver_resize==='function')\
                 {{_lumen_docpip_deliver_resize({win_w},{win_h});}}"
            ),
        );
    }
}
