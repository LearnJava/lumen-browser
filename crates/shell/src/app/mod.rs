//! Тела методов winit-цикла (`ApplicationHandler<LoadEvent> for Lumen`).
//!
//! Дорожка SPLIT (`docs/tasks/p1-monolith-split-queue.md`, батч SH-2). Сам
//! trait-impl остаётся в `main.rs`: реализацию трейта нельзя разложить по
//! нескольким блокам, а её тело — 5 138 строк, то есть втрое больше потолка
//! `scripts/check_file_sizes.py`. Поэтому метод трейта становится переходником
//! в одну строку, а его тело переезжает сюда как `pub(crate) fn on_<метод>`
//! в `impl Lumen` — обычный inherent impl, который дробить как раз можно.
//!
//! Перенос механический: глубина вложенности `fn` в `impl Lumen` та же, что и
//! в `impl ApplicationHandler`, поэтому тела перенесены без дедента и ни одна
//! строка внутри строковых литералов не тронута.

use crate::*;

pub(crate) mod about_to_wait;
pub(crate) mod resumed;
pub(crate) mod user_event;
pub(crate) mod window_event;

impl ApplicationHandler<LoadEvent> for Lumen {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.on_resumed(event_loop);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: LoadEvent) {
        self.on_user_event(event);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // M0.1 (ADR-016): С„РёРЅР°Р»СЊРЅР°СЏ СЃРµСЃСЃРёРѕРЅРЅР°СЏ СЃРІРѕРґРєР° РІСЂРµРјС‘РЅ РєР°РґСЂРѕРІ. РџРµС‡Р°С‚Р°РµС‚СЃСЏ
        // С‚РѕР»СЊРєРѕ РµСЃР»Рё frame-log С‡С‚Рѕ-С‚Рѕ РЅР°РєРѕРїРёР» (`LUMEN_FRAME_LOG>=1`).
        if let Some(summary) = self.frame_stats.summary() {
            eprintln!("{summary} (session exit)");
        }
        // ADR-016 M2.0: session-final UI-thread relayout-cost summary.
        if let Some(summary) = self.engine_stats.summary() {
            eprintln!("{} (session exit)", summary.display_with("ENGINE_SUMMARY"));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.on_about_to_wait(event_loop);
    }

    /// Handle raw device events вЂ” used for Pointer Lock raw mouse delta.
    ///
    /// W3C Pointer Lock L2 В§6.3: when locked, `DeviceEvent::MouseMotion` delivers
    /// relative mouse movement without OS acceleration or clipping.  Shell dispatches
    /// `mousemove`/`pointermove` with `movementX`/`movementY` to the locked element.
    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        #[cfg(feature = "v8")]
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && lumen_js::pointer_lock::is_pointer_locked()
            && let Some(nid) = lumen_js::pointer_lock::get_locked_element_nid()
        {
            let (cx, cy) = self
                .cursor_position
                .map(|p| {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f32, |r| r.scale_factor() as f32)
                        .max(1e-6);
                    ((p.x as f32) / dpr, (p.y as f32) / dpr)
                })
                .unwrap_or((0.0, 0.0));
            let script = format!(
                "_lumen_dispatch_locked_mousemove({},{},{},{},{},{})",
                nid,
                cx as i32,
                cy as i32,
                dx as i32,
                dy as i32,
                self.mod_flags(),
            );
            // ADR-016 M2.2c-2d: fire-and-forget void eval СѓС…РѕРґРёС‚ С‡РµСЂРµР·
            // РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ
            // РїСЂРµР¶РЅРµРјСѓ `ctx.eval_js(&script)` (script РїРѕСЃС‚СЂРѕРµРЅ РґРѕ РјР°СЂС€СЂСѓС‚РёР·Р°С†РёРё).
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // CC-7: events for the separate OS PiP window are handled here and never
        // fall through to the main-window logic below (which assumes the single
        // page window and ignores the id). Close button exits PiP; resize keeps
        // the floating surface in sync; redraw re-letterboxes the poster.
        if self.pip_os.as_ref().is_some_and(|p| p.window.id() == window_id) {
            match event {
                WindowEvent::CloseRequested => {
                    self.close_pip_os();
                    self.pip_controller.on_exit();
                    // Mirror the close into JS so `leavepictureinpicture` fires
                    // and `document.pictureInPictureElement` clears (video PiP),
                    // and so Document PiP's `PictureInPictureWindow.close()`
                    // runs too (P3-pip) вЂ” the same OS window may have been
                    // opened by either side, and each guards itself so only
                    // the truly-active one does anything. ADR-016 M2.2c-2d:
                    // fire-and-forget void eval С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ
                    // С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
                    #[cfg(feature = "v8")]
                    route_eval_js(
                        self.engine_thread.as_ref(),
                        self.js_ctx.as_ref(),
                        "if(typeof document!=='undefined'&&document.pictureInPictureElement)\
                         {try{document.exitPictureInPicture();}catch(e){}}\
                         if(typeof documentPictureInPicture!=='undefined'&&\
                         documentPictureInPicture._activeWindow&&\
                         !documentPictureInPicture._activeWindow._closed)\
                         {try{documentPictureInPicture._activeWindow.close();}catch(e){}}"
                            .to_string(),
                    );
                }
                WindowEvent::Resized(size) => {
                    if size.width == 0 || size.height == 0 {
                        return;
                    }
                    let scale = self
                        .pip_os
                        .as_ref()
                        .map_or(1.0, |p| p.window.scale_factor() as f32);
                    if let Some(p) = self.pip_os.as_mut() {
                        p.renderer.resize(size.width, size.height);
                    }
                    self.render_pip_os();
                    let (win_w, win_h) =
                        panels::pip_os_window::physical_to_logical(size.width, size.height, scale);
                    self.notify_pip_window_resized(win_w, win_h);
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    if let Some(p) = self.pip_os.as_mut() {
                        p.renderer.set_scale_factor(scale_factor);
                    }
                    self.render_pip_os();
                    #[cfg(feature = "v8")]
                    self.deliver_pip_resize();
                }
                WindowEvent::RedrawRequested => {
                    self.render_pip_os();
                }
                _ => {}
            }
            return;
        }

        // Document Picture-in-Picture (slice 1): same routing as video PiP
        // above вЂ” events for this second window are fully handled here and
        // never fall through to the main-window logic.
        if self.doc_pip_os.as_ref().is_some_and(|p| p.window.id() == window_id) {
            match event {
                WindowEvent::CloseRequested => {
                    self.close_doc_pip_os();
                    self.doc_pip_controller.on_close();
                    // Mirror the close into JS so `_closed` / `pictureInPictureElement`
                    // reflect reality when the user closes via the OS window chrome
                    // rather than calling `.close()`.
                    #[cfg(feature = "v8")]
                    route_eval_js(
                        self.engine_thread.as_ref(),
                        self.js_ctx.as_ref(),
                        "if(typeof _lumen_docpip_deliver_close==='function')\
                         {_lumen_docpip_deliver_close();}"
                            .to_string(),
                    );
                }
                WindowEvent::Resized(size) => {
                    if size.width == 0 || size.height == 0 {
                        return;
                    }
                    let scale = self
                        .doc_pip_os
                        .as_ref()
                        .map_or(1.0, |p| p.window.scale_factor() as f32);
                    if let Some(p) = self.doc_pip_os.as_mut() {
                        p.renderer.resize(size.width, size.height);
                    }
                    self.render_doc_pip_os();
                    let (win_w, win_h) =
                        panels::pip_os_window::physical_to_logical(size.width, size.height, scale);
                    self.notify_docpip_window_resized(win_w, win_h);
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    if let Some(p) = self.doc_pip_os.as_mut() {
                        p.renderer.set_scale_factor(scale_factor);
                    }
                    self.render_doc_pip_os();
                }
                WindowEvent::RedrawRequested => {
                    self.render_doc_pip_os();
                }
                _ => {}
            }
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.save_session_on_close();
                self.save_full_session();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                // Windows fires Resized(0, 0) when the window is minimized.
                // Skip resize + relayout entirely вЂ” the layout stays valid at
                // the last non-zero size and will be refreshed on restore.
                if size.width == 0 || size.height == 0 {
                    return;
                }
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
                self.relayout();
                // CC-4: re-lay-out the engine-drawn chrome at the new window
                // size.
                self.relayout_chrome_host();
                // HTML В§8.1.5.1, С€Р°Рі 13: ResizeObserver delivery.
                // JS-observers are delivered inside relayout() via deliver_layout_observers().
                // The shell runtime.deliver_observer_records delivers Rust-level observers.
                self.runtime
                    .deliver_observer_records(runtime::ObserverKind::Resize);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // РћРєРЅРѕ РїРµСЂРµС‚Р°С‰РёР»Рё РЅР° РјРѕРЅРёС‚РѕСЂ СЃ РґСЂСѓРіРёРј DPI. Surface РЅРµ РїРµСЂРµСЃРѕР·РґР°С‘Рј вЂ”
                // winit РѕС‚РґР°СЃС‚ РЅРѕРІС‹Р№ physical inner_size С‡РµСЂРµР· РїРѕСЃР»РµРґСѓСЋС‰РёР№
                // `WindowEvent::Resized`; Р·РґРµСЃСЊ С‚РѕР»СЊРєРѕ РѕР±РЅРѕРІР»СЏРµРј РєРѕСЌС„С„РёС†РёРµРЅС‚,
                // РїРѕ РєРѕС‚РѕСЂРѕРјСѓ shader РґРµР»РёС‚ РєРѕРѕСЂРґРёРЅР°С‚С‹, С‡С‚РѕР±С‹ 1 CSS px РѕСЃС‚Р°Р»СЃСЏ
                // СЂР°РІРµРЅ scale_factor device px.
                if let Some(r) = self.renderer.as_mut() {
                    r.set_scale_factor(scale_factor);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = winit_modifiers_state(&new_mods);
            }
            WindowEvent::ThemeChanged(theme) => {
                // OS switched lightв†”dark. Update the stored preference and re-run
                // layout: it re-evaluates `@media (prefers-color-scheme)` and pushes
                // the new value to JS matchMedia listeners via
                // deliver_media_query_changes(.., self.dark_mode). ADR-016 M2.2b-4:
                // an OS theme flip is async-safe (a whole-page restyle with no
                // synchronous read of page geometry afterwards вЂ” matchMedia delivery
                // rides `apply_relayout_result`, and `dark_mode` is captured by the
                // off-thread job), so route it through `relayout_chrome()`.
                let dark = platform::dark_mode::theme_prefers_dark(Some(theme));
                if dark != self.dark_mode {
                    self.dark_mode = dark;
                    self.relayout_chrome();
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event: ref key_event, .. } => {
                self.handle_key(event_loop, key_event);
            }
            WindowEvent::Ime(ref ime_event) => {
                self.handle_ime(ime_event);
            }
            WindowEvent::CursorMoved { position, .. } => self.on_cursor_moved(position),
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                self.resize_active = None; // Clear resize when cursor leaves window
                // Clear hover state when cursor leaves the window.
                if self.hovered_nid.is_some() {
                    // Dispatch leave events before clearing hovered state.
                    #[cfg(feature = "v8")]
                    if let Some(old) = self.hovered_nid {
                        // Ph3 pointer-events-l3: flush queued pointermove
                        // samples first so they precede pointerout/leave.
                        self.flush_pointer_moves();
                        let nid = old.index() as u32;
                        self.js_pointer_event(nid, "pointerout",   0.0, 0.0, 0, 0);
                        self.js_mouse_event(nid,   "mouseout",     0.0, 0.0, 0, 0);
                        self.js_pointer_event(nid, "pointerleave", 0.0, 0.0, 0, 0);
                        self.js_mouse_event(nid,   "mouseleave",   0.0, 0.0, 0, 0);
                    }
                    self.hovered_nid = None;
                    // ADR-016 M2.2b-8: clearing `:hover` on cursor-leave is the
                    // same async-safe restyle as the in-window hover flip
                    // (M2.2b-5) вЂ” no synchronous geometry read; the leave events
                    // above target the old node, not this reflow. Route off-thread.
                    self.relayout_chrome();
                    self.request_redraw();
                }
                self.gesture.cancel();
                // Р”СЂР°Рі РїСЂРѕРґРѕР»Р¶Р°РµС‚СЃСЏ РґР°Р¶Рµ РєРѕРіРґР° РєСѓСЂСЃРѕСЂ РІС‹С€РµР» РёР· РѕРєРЅР° вЂ” winit
                // РїСЂРѕРґРѕР»Р¶РёС‚ СЃР»Р°С‚СЊ CursorMoved-СЃРѕР±С‹С‚РёСЏ Р·Р° РїСЂРµРґРµР»Р°РјРё client area,
                // РїРѕРєР° Р·Р°Р¶Р°С‚Р° РєРЅРѕРїРєР°. РЎР±СЂРѕСЃРёРј drag С‚РѕР»СЊРєРѕ РЅР° MouseInput Release
                // РёР»Рё РµСЃР»Рё СЃРѕР±С‹С‚РёСЏ РїСЂРµРєСЂР°С‚СЏС‚СЃСЏ (РјС‹ РЅРµ РїРѕР»СѓС‡РёРј MouseInput, РЅРѕ
                // РїРѕРІС‚РѕСЂРЅС‹Р№ CursorEntered/CursorMoved РѕР¶РёРІСЏС‚ drag вЂ” РґРѕРїСѓСЃС‚РёРјРѕ
                // РґР»СЏ Phase 0).
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_input(event_loop, state, button)
            }
            WindowEvent::MouseWheel { delta, phase, .. } => self.on_mouse_wheel(delta, phase),
            WindowEvent::RedrawRequested => self.on_redraw_requested(),
            _ => {}
        }
    }
}
