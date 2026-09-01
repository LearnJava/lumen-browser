//! Ветка `WindowEvent::CursorMoved` цикла событий (SPLIT-SH2).
//!
//! Тело ветки вынесено из `Lumen::window_event` (`main.rs`) как есть, с
//! дедентом на 8 пробелов и без единой правки логики; строки внутри
//! многострочных строковых литералов дедент не затронул.

use crate::*;
// Тип биндинга ветки: в `main.rs` он не назывался — паттерн
// `WindowEvent::CursorMoved { position, .. }` выводит его сам.
use winit::dpi::PhysicalPosition;

impl Lumen {
    pub(crate) fn on_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor_position = Some(position);
        // F2-6: while resizing a docked panel, the drag owns the cursor —
        // update its width and relayout, skip page/inspector hover work.
        if self.panel_resize.is_some() {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            if self.drag_panel_resize(x_css) {
                self.request_redraw();
            }
            self.update_cursor_icon();
            return;
        }
        self.update_cursor_icon();
        // DevTools inspector: highlight the box under the cursor.
        if self.dom_inspector.visible {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            let y_css = (position.y as f32) / dpr;
            let hovered = if y_css < toolbar::CHROME_H {
                None
            } else {
                let (page_x, page_y) = self.page_point(x_css, y_css);
                self.layout_box
                    .as_ref()
                    .and_then(|lb| hit_test(Point::new(page_x, page_y), lb))
                    .map(|r| r.node)
            };
            if self.dom_inspector.set_hovered(hovered) {
                self.request_redraw();
            }
        }
        // Feed current position to the gesture recognizer (right-drag tracking).
        {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            self.gesture.track(
                (position.x as f32) / dpr,
                (position.y as f32) / dpr,
            );
        }
        // Tab drag-and-drop (§O-9): update ghost position; activate after threshold.
        if let Some(ref mut tab_drag) = self.tab_drag {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            tab_drag.ghost_x = x_css;
            if !tab_drag.active
                && (x_css - tab_drag.press_x).abs() >= tabs::strip::DRAG_THRESHOLD
            {
                tab_drag.active = true;
            }
            if tab_drag.active {
                self.request_redraw();
            }
        }
        // HTML5 DnD (PH3-9): activate drag after threshold; fire drag/dragover.
        #[cfg(feature = "v8")]
        {
            struct DndMoveEvents {
                src: u32,
                dragstart: bool,
                drag: bool,
                dragleave: Option<u32>,
                dragenter: Option<u32>,
                dragover: Option<u32>,
                x_css: f32,
                y_css: f32,
            }
            // Collect events to fire while mutating state, then release the
            // mutable borrow before calling self.js_drag_event() (needs &self).
            // Width of any left-docked sidebar, computed before the
            // `dnd_state` mutable borrow so page-coord conversion can
            // subtract it. Cross-dock aware across all four sidebars.
            let left_dock_w = self.left_dock().map_or(0.0, |(_, w)| w);
            let ev_opt: Option<DndMoveEvents> = if let Some(dnd) = self.dnd_state.as_mut() {
                let dpr = self
                    .renderer
                    .as_ref()
                    .map_or(1.0_f32, |r| r.scale_factor() as f32)
                    .max(1e-6);
                let x_css = (position.x as f32) / dpr;
                let y_css = (position.y as f32) / dpr;
                let (page_x, page_y) = (
                    x_css - left_dock_w + self.scroll_x,
                    y_css - toolbar::CHROME_H + self.scroll_y,
                );
                let target_nid = self.layout_box.as_ref().and_then(|lb| {
                    hit_test(Point::new(page_x, page_y), lb)
                }).map(|r| r.node);

                let mut dragstart = false;
                if !dnd.active {
                    let dx = x_css - dnd.press_x;
                    let dy = y_css - dnd.press_y;
                    if dx * dx + dy * dy >= DND_THRESHOLD * DND_THRESHOLD {
                        dnd.active = true;
                        dragstart = true;
                    }
                }
                let drag = dnd.active;
                let (dragleave, dragenter) = if dnd.active && dnd.over_nid != target_nid {
                    (
                        dnd.over_nid.map(|n| n.index() as u32),
                        target_nid.map(|n| n.index() as u32),
                    )
                } else {
                    (None, None)
                };
                let dragover = if dnd.active {
                    dnd.over_nid = target_nid;
                    target_nid.map(|n| n.index() as u32)
                } else {
                    None
                };
                Some(DndMoveEvents {
                    src: dnd.src_nid.index() as u32,
                    dragstart, drag, dragleave, dragenter, dragover,
                    x_css, y_css,
                })
            } else {
                None
            }; // mut borrow of self.dnd_state ends here
            if let Some(ev) = ev_opt {
                if ev.dragstart {
                    self.js_drag_event(ev.src, "dragstart", ev.x_css, ev.y_css);
                }
                if ev.drag {
                    self.js_drag_event(ev.src, "drag", ev.x_css, ev.y_css);
                    if let Some(old) = ev.dragleave {
                        self.js_drag_event(old, "dragleave", ev.x_css, ev.y_css);
                    }
                    if let Some(nw) = ev.dragenter {
                        self.js_drag_event(nw, "dragenter", ev.x_css, ev.y_css);
                    }
                    if let Some(ov) = ev.dragover {
                        self.js_drag_event(ov, "dragover", ev.x_css, ev.y_css);
                    }
                }
            }
        }

        // Активный drag — пересчитать scroll по новой позиции.
        if let Some(drag) = self.scroll_drag {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let cursor_y_css = (position.y as f32) / dpr;
            let target = drag.scroll_for(
                cursor_y_css,
                self.content_height,
                self.viewport_height_css(),
            );
            self.scroll_to(target);
        }
        // FRAME-3 remainder: активный drag СОБСТВЕННОГО scrollbar-а фрейма —
        // тот же приём, что выше у страничного `scroll_drag`, но целевой
        // фрейм несёт свой индекс (несколько фреймов держат независимые
        // drag-и).
        if let Some((fidx, drag)) = self.frame_scroll_drag {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let cursor_y_css = (position.y as f32) / dpr;
            self.drag_frame_scrollbar_to(fidx, drag, cursor_y_css);
        }
        // FRAME-7 остаток: active mouse-drag text selection — extend the
        // cursor (never the anchor) to the char under the new position.
        if self.text_drag.is_some() {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            let y_css = (position.y as f32) / dpr;
            self.update_text_drag_select(x_css, y_css);
        }
        // PiP window drag (task #21): follow the cursor while the title
        // bar is held, clamped to the window.
        if self.pip.dragging() {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let win_w = self.viewport_width_css();
            let win_h = self.viewport_height_css() + toolbar::CHROME_H;
            self.pip.drag_to(
                (position.x as f32) / dpr,
                (position.y as f32) / dpr,
                win_w,
                win_h,
            );
            self.request_redraw();
        }
        // Pointer Lock: skip normal hover/mousemove dispatch while locked.
        // Raw movement deltas arrive via device_event → _lumen_dispatch_locked_mousemove.
        #[cfg(feature = "v8")]
        if lumen_js::pointer_lock::is_pointer_locked() {
            return;
        }
        // CSS :hover tracking — find the element under the cursor and
        // trigger relayout when it changes so :hover rules re-evaluate.
        {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            let y_css = (position.y as f32) / dpr;
            // CC-5: independent hover tracking for the engine-drawn
            // chrome document — separate thread-locals/relayout pass
            // from the page's own `:hover` below (`relayout_chrome_host`'s
            // doc comment explains why the two must not share state).
            // `point_over_chrome`/`chrome_hit_test` both answer "no
            // chrome" once the pointer is over the page, so moving out
            // of the sidebar/toolbar correctly clears this again.
            let new_chrome_hovered = if self.point_over_chrome(x_css, y_css) {
                self.chrome_hit_test(x_css, y_css).map(|r| r.node)
            } else {
                None
            };
            if new_chrome_hovered != self.chrome_hovered_nid {
                self.chrome_hovered_nid = new_chrome_hovered;
                self.relayout_chrome_host();
                self.request_redraw();
            }
            // Pointer Events L3 §4.1: buffer this raw sample instead of
            // dispatching immediately. Flushed as one coalesced
            // `pointermove` on the next `about_to_wait` tick, or sooner
            // (below) if hover changes so ordering vs enter/leave holds.
            #[cfg(feature = "v8")]
            self.pending_pointer_moves.push((x_css, y_css));
            // CC-5: `point_over_chrome` replaces the legacy `y_css <
            // toolbar::CHROME_H` gate — that constant no longer
            // describes where the chrome's opaque area ends
            // (variable-width sidebar, differently-sized toolbar row).
            // BUG-480 срез 16: курсор над содержимым фрейма — hover страницы
            // пуст, а его место занимает пара «фрейм + узел ребёнка». Две
            // разные переменные, потому что `NodeId` уникален лишь внутри
            // своего документа: положив узел ребёнка в `hovered_nid`, страница
            // подсветила бы по `:hover` свой бокс с тем же индексом и послала
            // бы ему `mousedown`.
            let target = if self.point_over_chrome(x_css, y_css) {
                frames::PointerTarget { page: None, frame: None }
            } else {
                self.pointer_target(x_css, y_css)
            };
            let frame_target = target.frame;
            let new_hovered_frame =
                frame_target.as_ref().and_then(|t| t.hit.as_ref().map(|h| (t.frame, h.node)));
            let new_hovered = if frame_target.is_some() {
                None
            } else {
                target.page.map(|r| r.node)
            };
            // BUG-480 срез 23: признак считается ДО обеих `cfg`-веток ниже —
            // каждая из них присваивает `hovered_frame` сама, а `:hover`
            // под-документа надо пересчитать в обеих: нарисованный вид не
            // зависит от того, собран ли движок со скриптами.
            let hovered_frame_changed = new_hovered_frame != self.hovered_frame;
            #[cfg(feature = "v8")]
            if new_hovered_frame != self.hovered_frame {
                let old_frame = self.hovered_frame;
                self.hovered_frame = new_hovered_frame;
                // Порядок тот же, что у страницы: сначала уход, потом приход.
                // Координаты — в системе КАЖДОГО из документов; для ухода
                // берётся точка того же фрейма, которую он видел последней,
                // а её больше нет, поэтому уход адресуется без пересчёта — с
                // нулём (`clientX`/`clientY` события leave в этом движке
                // читаются редко, а неверное окно координат было бы хуже).
                if let Some((f, n)) = old_frame {
                    let nid = n.index() as u32;
                    self.frame_pointer_event(f, nid, "pointerout", (0.0, 0.0), (0, 0));
                    self.frame_mouse_event(f, nid, "mouseout", (0.0, 0.0), (0, 0));
                    self.frame_pointer_event(f, nid, "pointerleave", (0.0, 0.0), (0, 0));
                    self.frame_mouse_event(f, nid, "mouseleave", (0.0, 0.0), (0, 0));
                }
                if let (Some((f, n)), Some(t)) = (new_hovered_frame, frame_target.as_ref()) {
                    let nid = n.index() as u32;
                    let at = (t.client.x, t.client.y);
                    self.frame_pointer_event(f, nid, "pointerover", at, (0, 0));
                    self.frame_mouse_event(f, nid, "mouseover", at, (0, 0));
                    self.frame_pointer_event(f, nid, "pointerenter", at, (0, 0));
                    self.frame_mouse_event(f, nid, "mouseenter", at, (0, 0));
                }
            }
            #[cfg(not(feature = "v8"))]
            {
                self.hovered_frame = new_hovered_frame;
            }
            if hovered_frame_changed {
                self.refresh_frames(None);
            }
            if new_hovered != self.hovered_nid {
                #[cfg(feature = "v8")]
                let old_nid = self.hovered_nid;
                self.hovered_nid = new_hovered;
                // ADR-016 M2.2b-5: :hover restyle is async-safe (no
                // geometry read of its own; the JS pointer events below
                // target `old_nid`/`new_hovered`, not this reflow).
                self.relayout_chrome();
                self.request_redraw();
                // Dispatch hover-change events per W3C UI Events §17.5 / Pointer Events L2 §10.
                #[cfg(feature = "v8")]
                {
                    // Ph3 pointer-events-l3: flush pointermove samples
                    // queued before this boundary crossing first, so
                    // they precede pointerout/leave/over/enter in
                    // dispatch order (spec-observable event order).
                    self.flush_pointer_moves();
                    // Leave events on the element losing hover.
                    if let Some(old) = old_nid {
                        let nid = old.index() as u32;
                        self.js_pointer_event(nid, "pointerout",   x_css, y_css, 0, 0);
                        self.js_mouse_event(nid,   "mouseout",     x_css, y_css, 0, 0);
                        self.js_pointer_event(nid, "pointerleave", x_css, y_css, 0, 0);
                        self.js_mouse_event(nid,   "mouseleave",   x_css, y_css, 0, 0);
                    }
                    // Enter events on the element gaining hover.
                    if let Some(nw) = new_hovered {
                        let nid = nw.index() as u32;
                        self.js_pointer_event(nid, "pointerover",  x_css, y_css, 0, 0);
                        self.js_mouse_event(nid,   "mouseover",    x_css, y_css, 0, 0);
                        self.js_pointer_event(nid, "pointerenter", x_css, y_css, 0, 0);
                        self.js_mouse_event(nid,   "mouseenter",   x_css, y_css, 0, 0);
                    }
                }
            }
            // Ph3 pointer-events-l3: queue this raw sample for the next
            // coalesced pointermove flush (about_to_wait tick, next
            // hover-boundary crossing, or press/release) — Pointer
            // Events L3 §4.1.
            #[cfg(feature = "v8")]
            self.pending_pointer_moves.push((x_css, y_css));
        }
        // CC-15-4: the settings-panel hover tracker lived here — it fed
        // only `settings_panel::tooltip_for`/`build_tooltip`, both
        // deleted with the legacy paint, so every `CursorMoved` while
        // the panel was open cost a `request_redraw()` for nothing.
        // CC-4: update tab context-menu hover highlight.
        if self.tab_context_menu.is_open() {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            let y_css = (position.y as f32) / dpr;
            let win_w = self.viewport_width_css();
            let win_h = self.window_height_css();
            let new_hover = tabs::context_menu::item_at(
                &self.tab_context_menu,
                x_css,
                y_css,
                win_w,
                win_h,
            );
            if new_hover != self.tab_context_menu.hovered {
                self.tab_context_menu.hovered = new_hover;
                self.request_redraw();
            }
        }
        // B-7/CC-CSS-4: Active resize — update element width/height as mouse
        // moves, gated to the axes the grip's `resize` value allows (a pure
        // `resize: vertical` grip must not also change width on a diagonal drag).
        #[cfg(feature = "v8")]
        if let Some((node_id, start_x, start_y, allow_w, allow_h)) = self.resize_active {
            let dpr = self
                .renderer
                .as_ref()
                .map_or(1.0_f32, |r| r.scale_factor() as f32)
                .max(1e-6);
            let x_css = (position.x as f32) / dpr;
            let y_css = (position.y as f32) / dpr;
            let delta_x = if allow_w { x_css - start_x } else { 0.0 };
            let delta_y = if allow_h { y_css - start_y } else { 0.0 };
            let nid_u32 = node_id.index() as u32;
            // ADR-016 M2.2c-2d: resize-eval через `route_eval_js` — снимаем прямое
            // `self.js_ctx`-обращение. Чистый fire-and-forget void без чтения
            // результата следом; под флагом (`LUMEN_ENGINE_THREAD=1`) уходит
            // off-UI-thread одним `task`, без флага (по умолчанию) — синхронный
            // вызов по UI-хэндлу, байт-идентично прежнему `js.eval_js`.
            #[cfg(feature = "v8")]
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_apply_resize({}, {}, {});", nid_u32, delta_x, delta_y),
            );
            self.request_redraw();
        }
    }
}
