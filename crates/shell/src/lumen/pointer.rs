//! Mouse, pointer and drag events on their way from the OS into page JS.
//!
//! Every method here turns one winit-level position or button change into the
//! `_lumen_dispatch_*` call the JS shim expects, after converting OS-window
//! CSS pixels into page coordinates (`page_point`) and hit-testing the layout
//! tree. `flush_pointer_moves` is the coalescing buffer drained once per
//! `about_to_wait` tick, so buffered moves stay ordered ahead of any
//! press/release/enter/leave dispatch.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour and
//! method bodies are unchanged; only the module path and the visibility of the
//! methods called from `crate::app` differ.

use crate::*;

impl Lumen {
    /// Return the current keyboard modifier flags as a bitmask.
    ///
    /// Bit layout: bit0=ctrl, bit1=shift, bit2=alt, bit3=meta (super).
    #[cfg(feature = "v8")]
    pub(crate) fn mod_flags(&self) -> u8 {
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
    pub(crate) fn js_mouse_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32, button: u8, buttons: u8) {
        let script = format!(
            "_lumen_dispatch_mouse_event({}, '{}', {}, {}, {}, {}, {})",
            nid, event_type,
            x_css as i32, y_css as i32,
            button, buttons,
            self.mod_flags(),
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Что под точкой ОКНА: узел страницы, и — если точка попала в содержимое
    /// `<iframe>` — узел под-документа (BUG-480 срез 16).
    ///
    /// Один вызов на оба ответа: hit-тест страницы стоит обхода её layout, а
    /// спрашивают на каждом движении мыши. Страница без фреймов идёт прежним
    /// путём и не платит ничего сверх своего hit-теста.
    pub(crate) fn pointer_target(&self, x_css: f32, y_css: f32) -> frames::PointerTarget {
        let (page_x, page_y) = self.page_point(x_css, y_css);
        let Some(lb) = self.layout_box.as_ref() else {
            return frames::PointerTarget { page: None, frame: None };
        };
        let point = Point::new(page_x, page_y);
        if self.frames.is_empty() {
            return frames::PointerTarget { page: hit_test(point, lb), frame: None };
        }
        frames::pointer_target(&self.frames, lb, point)
    }

    /// Отправить `MouseEvent` в JS ПОД-ДОКУМЕНТА фрейма (BUG-480 срез 16).
    ///
    /// Координаты — уже в системе ребёнка: `clientX`/`clientY` его скрипта
    /// отсчитываются от его собственного вьюпорта, а не от окна браузера.
    ///
    /// Прямой вызов вместо `route_eval_js`: хэндлы фреймов живут только на
    /// UI-стороне (в `EngineJsState` их нет), а `V8PersistentJs` сам
    /// тунеллирует на свой JS-поток (ADR-014) — как и памп фреймов в
    /// `about_to_wait`.
    #[cfg(feature = "v8")]
    pub(crate) fn frame_mouse_event(
        &self,
        frame: usize,
        nid: u32,
        event_type: &str,
        at: (f32, f32),
        buttons: (u8, u8),
    ) {
        self.frame_input_event("_lumen_dispatch_mouse_event", frame, nid, event_type, at, buttons);
    }

    /// То же для `PointerEvent` внутри фрейма (BUG-480 срез 16).
    #[cfg(feature = "v8")]
    pub(crate) fn frame_pointer_event(
        &self,
        frame: usize,
        nid: u32,
        event_type: &str,
        at: (f32, f32),
        buttons: (u8, u8),
    ) {
        self.frame_input_event("_lumen_dispatch_pointer_event", frame, nid, event_type, at, buttons);
    }

    /// Общее тело двух предыдущих: `buttons` — пара `(button, buttons)` спеки
    /// UI Events, `at` — точка в системе РЕБЁНКА.
    #[cfg(feature = "v8")]
    fn frame_input_event(
        &self,
        native: &str,
        frame: usize,
        nid: u32,
        event_type: &str,
        at: (f32, f32),
        buttons: (u8, u8),
    ) {
        let Some(js) = self.frames.get(frame).and_then(|h| h.js.as_ref()) else { return };
        js.eval_js(&format!(
            "{}({}, '{}', {}, {}, {}, {}, {})",
            native, nid, event_type, at.0 as i32, at.1 as i32,
            buttons.0, buttons.1, self.mod_flags(),
        ));
    }

    /// Dispatch a `PointerEvent` of the given `event_type` to DOM node `nid`.
    ///
    /// Always uses pointerId=1, pointerType='mouse', isPrimary=true (mouse input).
    /// Non-bubbling types (`pointerenter`/`pointerleave`) have `bubbles:false` per spec.
    #[cfg(feature = "v8")]
    pub(crate) fn js_pointer_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32, button: u8, buttons: u8) {
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
    /// via `PointerEvent.getCoalescedEvents()` (Pointer Events L3 §4.1).
    /// `coalesced` holds CSS-pixel positions strictly older than
    /// `(x_css, y_css)`, oldest first; the dispatched event is appended last,
    /// per spec. Always dispatches with button=0/buttons=0 — the only caller
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
    pub(crate) fn js_drag_event(&self, nid: u32, event_type: &str, x_css: f32, y_css: f32) {
        let script = format!(
            "_lumen_dispatch_drag_event({}, '{}', {}, {}, '{{}}')",
            nid, event_type,
            x_css as i32, y_css as i32,
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Dispatch a `gotpointercapture` or `lostpointercapture` event to DOM node `nid`.
    ///
    /// Calls `_lumen_dispatch_capture_event` (W3C Pointer Events L3 §4.1).
    /// These events do not bubble per spec.  No-op when there is no JS context.
    #[cfg(feature = "v8")]
    pub(crate) fn js_capture_event(&self, nid: u32, event_type: &str) {
        let script = format!("_lumen_dispatch_capture_event({}, '{}')", nid, event_type);
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    /// Buffer a synthetic pointer-move sample at CSS-pixel viewport
    /// coordinates. Used by [`input::humanlike::HumanLikeSender`] to trace
    /// Bézier-curve paths before a click. Real `CursorMoved` samples are
    /// buffered the same way (see the `WindowEvent::CursorMoved` handler); both
    /// sources are flushed together by [`Self::flush_pointer_moves`] as one
    /// coalesced `pointermove` + `mousemove` dispatch (Pointer Events L3 §4.1).
    pub(crate) fn dispatch_mouse_move(&mut self, x_css: f32, y_css: f32) {
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
    /// `mousemove` dispatch (Pointer Events L3 §4.1). The last buffered sample
    /// hit-tests the target and becomes the "main" dispatched event; earlier
    /// samples are exposed via `PointerEvent.getCoalescedEvents()`. Called once
    /// per `about_to_wait` tick, and before any press/release/enter/leave
    /// dispatch so buffered moves stay ordered ahead of those events. No-op if
    /// nothing is buffered or there is no element at the final position.
    #[cfg(feature = "v8")]
    pub(crate) fn flush_pointer_moves(&mut self) {
        if self.pending_pointer_moves.is_empty() {
            return;
        }
        let samples = std::mem::take(&mut self.pending_pointer_moves);
        let Some(&(x_css, y_css)) = samples.last() else {
            return;
        };
        // BUG-437: same conversion as `handle_click_at` — `page_point()`, not
        // the legacy `left_dock()`/`CHROME_H` pair, so `mousemove`/`pointermove`
        // target the element the click will target and the one actually painted
        // under the cursor.
        let target = self.pointer_target(x_css, y_css);
        // BUG-480 срез 16: движение над содержимым фрейма адресует под-документ.
        // Родителю такое движение не принадлежит вовсе, поэтому ветка не
        // дополняет страничную, а заменяет её.
        if let Some(ft) = target.frame.as_ref() {
            if let Some(fh) = ft.hit.as_ref() {
                let nid = fh.node.index() as u32;
                let at = (ft.client.x, ft.client.y);
                self.frame_pointer_event(ft.frame, nid, "pointermove", at, (0, 0));
                self.frame_mouse_event(ft.frame, nid, "mousemove", at, (0, 0));
            }
            return;
        }
        if let Some(result) = target.page {
            // Pointer Events L3 §4.1: if a pointer capture is active, redirect
            // pointermove (and all pointer events) to the captured element.
            let hit_nid = result.node.index() as u32;
            // ADR-016 M2.2c-2d: pre-dispatch capture-read через `route_query_js`
            // (под флагом — блокирующий `query`; `None` = «без JS» → `hit_nid`).
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
    pub(crate) fn page_point(&self, x_css: f32, y_css: f32) -> (f32, f32) {
        let (offset_x, offset_y) = self.page_offset();
        ((x_css - offset_x) + self.scroll_x, (y_css - offset_y) + self.scroll_y)
    }
}
