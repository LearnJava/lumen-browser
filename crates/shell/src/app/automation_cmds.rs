//! Automation commands between frames (SDC-1b/SDC-2): draining the external
//! command queue (BiDi/MCP/graphic_tests) once per `about_to_wait`, and the
//! queued `Wait`/`Eval` requests that later frames re-check.
//!
//! Moved out of `about_to_wait.rs` (file-size gate, docs/lint-policy.md §5.1).
//! `drain_automation_commands` is the verbatim `for (cmd, reply_tx)` loop that
//! stood inline in `on_about_to_wait`, called from the same place; the types
//! below moved whole.

use crate::*;

impl Lumen {
    /// SDC-1b/SDC-2: drain the automation commands queued since the last frame
    /// and route each to its shell action. One step of
    /// [`Lumen::on_about_to_wait`], called where the loop used to stand.
    pub(crate) fn drain_automation_commands(&mut self, event_loop: &MainHandle<'_>) {
        let automation_cmds: Vec<AutomationRequest> = self.automation_rx.try_iter().collect();
        for (cmd, reply_tx) in automation_cmds {
            match cmd {
                AutomationCommand::Navigate(url) => {
                    // BUG-1199: a `window.open()` of the previous page left the
                    // automation tab in the background — navigate *it*, not the
                    // popup that took the foreground.
                    if let Some(idx) =
                        self.automation_tab.and_then(|id| self.tab_strip.inactive_index_of(id))
                    {
                        self.switch_tab(idx);
                    }
                    // Real browsers clear the console on navigation (unless "preserve
                    // log" is set); doing the same here keeps `ConsoleLog` (DEVX-1)
                    // scoped to the page just loaded instead of accumulating across
                    // an entire --live run.
                    self.devtools_console.clear();
                    // BUG-1031: `navigate_to_forced`, not `navigate_to` — this is a
                    // browser/automation-initiated navigation (WebDriver BiDi
                    // `browsingContext.navigate`), which the outgoing page's own
                    // `navigate` listeners must not be able to intercept/prevent.
                    // Letting it through `navigate_to` let a leftover listener from
                    // the previous test page wedge the browsing context forever.
                    self.navigate_to_forced(page_source_for_automation_url(&url));
                    self.automation_tab = Some(self.tab_strip.tabs[self.tab_strip.active].id);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::NewTab(url) => {
                    // Same console semantics as `Navigate`: the fresh tab becomes
                    // active, so `ConsoleLog` stays scoped to the page being loaded.
                    self.devtools_console.clear();
                    self.open_new_tab();
                    // BUG-1031: see `AutomationCommand::Navigate` above — the tab
                    // starts blank, but stay consistent and non-interceptable here too.
                    self.navigate_to_forced(page_source_for_automation_url(&url));
                    self.automation_tab = Some(self.tab_strip.tabs[self.tab_strip.active].id);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::ListTabs => {
                    let _ = reply_tx.send(AutomationReply::Tabs(self.automation_tab_list()));
                }
                AutomationCommand::SwitchTab(id) => {
                    // The chosen tab becomes the automation target: `Navigate`
                    // and a background-aware `Eval` follow `automation_tab`,
                    // which would otherwise pull the foreground back to the
                    // tab the popup was opened from (BUG-1199).
                    let reply = match self.tab_strip.tabs.iter().position(|t| t.id == id) {
                        Some(idx) => {
                            self.switch_tab(idx);
                            self.devtools_console.clear();
                            self.automation_tab = Some(id);
                            AutomationReply::Ack
                        }
                        None => AutomationReply::Error(format!("switch_tab: нет вкладки с id {id}")),
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::CloseTab(id) => {
                    let reply = match self.tab_strip.tabs.iter().position(|t| t.id == id) {
                        None => AutomationReply::Error(format!("close_tab: нет вкладки с id {id}")),
                        Some(_) if self.tab_strip.len() == 1 => AutomationReply::Error(
                            "close_tab: последняя вкладка не закрывается (это завершило бы браузер)".to_owned(),
                        ),
                        Some(idx) => {
                            self.close_tab(idx, event_loop);
                            let target_open = self
                                .automation_tab
                                .is_some_and(|t| self.tab_strip.tabs.iter().any(|e| e.id == t));
                            if !target_open {
                                // The closed tab was the target: follow the tab
                                // that took the foreground.
                                self.automation_tab = Some(self.tab_strip.tabs[self.tab_strip.active].id);
                            }
                            self.devtools_console.clear();
                            AutomationReply::Ack
                        }
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::Click(target) => {
                    match self.resolve_automation_target(&target) {
                        // BUG-934: a raw `Target::Point` may land on engine
                        // chrome (toolbar/tab strip/sidebar) rather than page
                        // content — try that dispatch first, exactly like a
                        // real mouse click (`on_mouse_input`) does, before
                        // falling through to the page hit-test/mismatch check
                        // below. `resolved.node` is only `Some` for
                        // `Target::Selector`/`Target::NodeId`, which never
                        // resolve into the chrome document in the first
                        // place, so this can't shadow a legitimate page miss.
                        Some(resolved) => {
                            // BUG-934: a raw `Target::Point` may land on engine
                            // chrome (toolbar/tab strip/sidebar) rather than page
                            // content — dispatch chrome-first, exactly like a
                            // real mouse click (`on_mouse_input`) does.
                            // `resolved.node` is only `Some` for
                            // `Target::Selector`/`Target::NodeId`, which never
                            // resolve into the chrome document in the first
                            // place, so this can't shadow a legitimate page miss.
                            // BUG-1044: a `Dead` chrome hit is reported, not
                            // `Ack`ed — the point was over chrome furniture but
                            // nothing actionable sits under it.
                            let chrome_outcome = self.dispatch_chrome_click(resolved.x, resolved.y, event_loop);
                            match chrome_outcome {
                                crate::chrome_action::ChromeClickOutcome::Actioned => {
                                    let _ = reply_tx.send(AutomationReply::Ack);
                                }
                                crate::chrome_action::ChromeClickOutcome::Dead => {
                                    let _ = reply_tx.send(AutomationReply::Error(
                                        "Click hit chrome but no actionable element".to_string(),
                                    ));
                                }
                                crate::chrome_action::ChromeClickOutcome::NotChrome => {
                                    // BUG-1044: a resolved point is not yet a delivered
                                    // click — check that the hit test at that point really
                                    // reaches the named element (or its subtree) before
                                    // dispatching, and report the miss instead of `Ack`.
                                    // Answering `success` for a click that landed on the
                                    // parent `<form>` is the "succeeds but does nothing"
                                    // signature a caller cannot recover from.
                                    match self.automation_hit_mismatch(&resolved) {
                                        Some(err) => {
                                            let _ = reply_tx.send(AutomationReply::Error(err));
                                        }
                                        None => {
                                            self.handle_click_at_automation(resolved.x, resolved.y);
                                            let _ = reply_tx.send(AutomationReply::Ack);
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            let _ = reply_tx
                                .send(AutomationReply::Error("Element not found".to_string()));
                        }
                    }
                }
                AutomationCommand::Type(target, text) => {
                    // Same target resolution as `Click` — and the same honest
                    // failure when it resolves to nothing. Reporting `Ack` for
                    // an unresolvable target was half of BUG-436's "succeeds
                    // but does nothing" signature.
                    match self.resolve_automation_target(&target) {
                        // BUG-934: same chrome-first dispatch as `Click`
                        // above. Once the click opens e.g. the address bar,
                        // route characters through `address_bar.append_str`
                        // (the same plain-text path `handle_address_bar_key`
                        // uses) instead of `inject_char`, which only ever
                        // targets a focused *page* DOM node and knows nothing
                        // about the chrome document.
                        Some(resolved)
                            if self.try_dispatch_chrome_click(resolved.x, resolved.y, event_loop) =>
                        {
                            if self.address_bar.is_open() {
                                self.address_bar.append_str(&text);
                                let sugg = self.query_omnibox_suggestions();
                                self.address_bar.set_suggestions(sugg);
                                self.relayout_chrome_host();
                                self.request_redraw();
                                let _ = reply_tx.send(AutomationReply::Ack);
                            } else {
                                let _ = reply_tx.send(AutomationReply::Error(
                                    "Chrome target is not a text field".to_string(),
                                ));
                            }
                        }
                        // BUG-1044: same hit check as `Click` — the focus this
                        // click sets is what the characters below go into, so a
                        // point that lands on a foreign element types into the
                        // wrong field (or silently nowhere) while reporting the
                        // caller's field as typed.
                        Some(resolved) if let Some(err) = self.automation_hit_mismatch(&resolved) => {
                            let _ = reply_tx.send(AutomationReply::Error(err));
                        }
                        Some(resolved) => {
                            let (x, y) = (resolved.x, resolved.y);
                            self.handle_click_at_automation(x, y);
                            // BUG-480 срез 22: клик мог адресовать typeable-поле
                            // ВНУТРИ фрейма (`self.focused_frame`) вместо поля
                            // страницы — тот же выбор пути, что в
                            // `keyboard.rs::handle_key`.
                            let in_frame = self.focused_frame.is_some();
                            let mut consumed = true;
                            for ch in text.chars() {
                                consumed &= if in_frame {
                                    self.inject_frame_char(ch)
                                } else {
                                    self.inject_char(ch)
                                };
                            }
                            if consumed {
                                let _ = reply_tx.send(AutomationReply::Ack);
                            } else {
                                let _ = reply_tx.send(AutomationReply::Error(
                                    "Element is not a mutable text field".to_string(),
                                ));
                            }
                        }
                        None => {
                            let _ = reply_tx
                                .send(AutomationReply::Error("Element not found".to_string()));
                        }
                    }
                }
                AutomationCommand::Scroll { delta, target } => {
                    // MCP-TOTAL-S1e (хвост BUG-338): дельта с целью уходит в
                    // её ближайший скролл-контейнер тем же путём, каким его
                    // скроллит настоящее колесо; без цели (или мимо
                    // контейнера) — страница, как раньше.
                    let routed = target
                        .as_ref()
                        .is_some_and(|t| self.scroll_target_by_delta(t, delta.x, delta.y));
                    if !routed {
                        self.scroll_by_delta(delta.x, delta.y);
                    }
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                // BUG-1194: hover without a click — the same handler a real
                // `CursorMoved` runs (hit test, `:hover`, over/out/enter/leave).
                //
                // BUG-1458: `x`/`y` are content-viewport coordinates (the
                // contract of `Target::Point` in `Click`), while
                // `on_cursor_moved` wants OS-window ones — add the content
                // origin (`page_offset`, toolbar/sidebar) exactly as
                // `resolve_automation_target` does for a click. Without it the
                // pointer landed `CHROME_H` above the target, on the toolbar,
                // and the page never saw `mouseover`/`:hover`.
                AutomationCommand::PointerMove { x, y } => {
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f64, |r| r.scale_factor())
                        .max(1e-6);
                    let (offset_x, offset_y) = self.page_offset();
                    self.on_cursor_moved(winit::dpi::PhysicalPosition::new(
                        (f64::from(x) + f64::from(offset_x)) * dpr,
                        (f64::from(y) + f64::from(offset_y)) * dpr,
                    ));
                    #[cfg(feature = "v8")]
                    self.flush_pointer_moves();
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                // WPT press/release gestures (`checkable-active-onblur-with-click`,
                // `selection-pointer.html`): `pointerDown` is the press half —
                // `:active`, `pointerdown`/`mousedown`, drag arming — and the
                // matching `pointerUp` dispatches `pointerup`/`mouseup`, then
                // the click's focus+`click`+activation half. Collapsing the
                // press into a whole click cleared `:active` before the page
                // could observe it.
                AutomationCommand::PointerDown { x, y } => {
                    // Same coordinate contract as `Click`/`PointerMove`:
                    // content-viewport CSS px → window CSS px via page_offset.
                    let dpr = self
                        .renderer
                        .as_ref()
                        .map_or(1.0_f64, |r| r.scale_factor())
                        .max(1e-6);
                    let (ox, oy) = self.page_offset();
                    let (wx, wy) = (x + ox, y + oy);
                    // A down without a preceding move still presses where the
                    // pointer is: refresh hover/`:hover` first, exactly like a
                    // real cursor arriving at the point.
                    self.on_cursor_moved(winit::dpi::PhysicalPosition::new(
                        f64::from(wx) * dpr,
                        f64::from(wy) * dpr,
                    ));
                    #[cfg(feature = "v8")]
                    self.flush_pointer_moves();
                    // CSS `:active` on press — the same write the real
                    // `MouseInput(Pressed)` branch does; `PointerUp` clears it.
                    // A `<label>` press resolves to its labeled control.
                    let press_nid = match (self.hovered_nid, self.status_hover_source) {
                        (Some(_), Some(src)) => Some(src),
                        (h, _) => h,
                    };
                    let active_target = press_nid.map(|n| self.pointer_active_target(n));
                    if self.active_nid != active_target {
                        self.active_nid = active_target;
                        self.active_from_keyboard = false;
                        self.sync_active_mirror();
                        self.relayout_chrome();
                        self.request_redraw();
                    }
                    if self.active_frame != self.hovered_frame {
                        self.active_frame = self.hovered_frame;
                        self.refresh_frames(None);
                    }
                    // `pointerdown`/`mousedown` on the hovered element — press
                    // half only; the release pair and `click` wait for
                    // `PointerUp` (UI Events §5.2 / Pointer Events §10).
                    #[cfg(feature = "v8")]
                    {
                        if let Some(hov) = self.hovered_nid {
                            let nid = hov.index() as u32;
                            self.js_pointer_event(nid, "pointerdown", wx, wy, 0, 1);
                            self.js_click_event(
                                nid,
                                "mousedown",
                                (wx, wy),
                                (0, 1),
                                self.click_streak.count(),
                            );
                        }
                        if let Some((f, n)) = self.hovered_frame {
                            self.flush_pointer_moves();
                            let at = self
                                .pointer_target(wx, wy)
                                .frame
                                .map_or((0.0, 0.0), |t| (t.client.x, t.client.y));
                            let nid = n.index() as u32;
                            self.frame_pointer_event(f, nid, "pointerdown", at, (0, 1));
                            self.frame_mouse_event(f, nid, "mousedown", at, (0, 1));
                        }
                    }
                    self.automation_button_down = Some((wx, wy));
                    self.begin_text_drag_select(wx, wy);
                    self.begin_doc_select(wx, wy);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::PointerUp => {
                    let was_down = self.automation_button_down.take();
                    // CSS `:active` clears on release — same as the real
                    // `MouseInput(Released)` branch.
                    if self.active_frame.is_some() {
                        self.active_frame = None;
                        self.refresh_frames(None);
                    }
                    if self.active_nid.is_some() {
                        self.active_nid = None;
                        self.active_from_keyboard = false;
                        self.sync_active_mirror();
                        self.relayout_chrome();
                        self.request_redraw();
                    }
                    if was_down.is_some() {
                        // Release coordinates: the live cursor position when a
                        // move landed, else the press point.
                        let dpr = self
                            .renderer
                            .as_ref()
                            .map_or(1.0_f32, |r| r.scale_factor() as f32)
                            .max(1e-6);
                        let (xu, yu) = self
                            .cursor_position
                            .map(|p| (p.x as f32 / dpr, p.y as f32 / dpr))
                            .or(was_down)
                            .unwrap_or((0.0, 0.0));
                        #[cfg(feature = "v8")]
                        {
                            self.flush_pointer_moves();
                            if let Some(hov) = self.hovered_nid {
                                self.js_pointer_release(hov.index() as u32, xu, yu);
                            }
                            if let Some((f, n)) = self.hovered_frame {
                                let at = self
                                    .pointer_target(xu, yu)
                                    .frame
                                    .map_or((0.0, 0.0), |t| (t.client.x, t.client.y));
                                let nid = n.index() as u32;
                                self.frame_pointer_event(f, nid, "pointerup", at, (0, 0));
                                self.frame_mouse_event(f, nid, "mouseup", at, (0, 0));
                            }
                        }
                        // The click half — focus move, `click` dispatch,
                        // activation — with the press pair already sent, so
                        // `press_events = false`.
                        self.handle_release_at_automation(xu, yu);
                    }
                    self.doc_select = None;
                    self.text_drag = None;
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                // BUG-1194: a bare key press/release (Escape, Tab, Shift, …).
                AutomationCommand::Key { key, code, down } => {
                    // BiDi sends each key of a chord as its own Key command —
                    // reconstruct the modifier mask so Shift+Tab etc. replay
                    // with the right direction (bits match `page_keys`' mask).
                    let mod_bit = match key.as_str() {
                        "Shift" => 0b0010,
                        "Control" => 0b0001,
                        "Alt" => 0b0100,
                        "Meta" => 0b1000,
                        _ => 0,
                    };
                    if mod_bit != 0 {
                        if down {
                            self.injected_modifiers |= mod_bit;
                        } else {
                            self.injected_modifiers &= !mod_bit;
                        }
                    }
                    if down && mod_bit == 0 {
                        // Any non-modifier key press is keyboard modality for
                        // `:focus-visible` — the same rule a real keydown
                        // applies in `handle_key`. Bare modifier presses are
                        // RawKeyDown and must NOT flip the modality (WPT
                        // `focus-visible-012`).
                        self.set_focus_modality(true);
                    }
                    // Tab moves page focus exactly like a real Tab press
                    // (`keyboard.rs`): the frame's own order first, then the
                    // page order; `focus_page_node` sets the keyboard
                    // modality that `:focus-visible` reads.
                    if down && key == "Tab" && code == "Tab" {
                        let forward = self.injected_modifiers & 0b0010 == 0;
                        let moved = match self.focused_frame {
                            Some((idx, _)) => {
                                self.advance_frame_focus(idx, forward)
                                    || self.advance_page_focus(forward)
                            }
                            None => self.advance_page_focus(forward),
                        };
                        if moved {
                            self.request_redraw();
                        }
                    }
                    let event = if down { "keydown" } else { "keyup" };
                    self.inject_key_events(&key, &code, &[event]);
                    // Alt+key is the page's `accesskey` — the same chord a
                    // real Alt press resolves in `keyboard.rs`. WebDriver
                    // sends Shift+Alt (Windows) or Ctrl+Alt (macOS); Alt is
                    // the bit that matters, matching `activate_accesskey`'s
                    // ALT|ALT+SHIFT rule. The `key` string stands in for the
                    // layout character a winit event would carry.
                    if down && mod_bit == 0 && self.injected_modifiers & 0b0100 != 0 {
                        let chars: Vec<char> = key
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .flat_map(char::to_lowercase)
                            .collect();
                        // The page keydown above already went out, same as
                        // the real-key path which dispatches the key to the
                        // page before resolving the chord.
                        self.activate_accesskey_chars(&chars);
                    }
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                // BUG-1194: characters into the already focused element.
                AutomationCommand::TypeFocused(text) => {
                    let in_frame = self.focused_frame.is_some();
                    let mut consumed = true;
                    for ch in text.chars() {
                        consumed &= if in_frame { self.inject_frame_char(ch) } else { self.inject_char(ch) };
                    }
                    let reply = if consumed {
                        AutomationReply::Ack
                    } else {
                        AutomationReply::Error("Focused element is not a mutable text field".to_string())
                    };
                    let _ = reply_tx.send(reply);
                }
                // BUG-1199: the page under test called `window.open()` and is
                // parked in `bg_tabs` — its runtime keeps ticking there (GAP-NAVCTX
                // срез 15), so evaluate against it directly, as the per-tick pump
                // below does, instead of polling the popup in the foreground.
                AutomationCommand::Eval(js, _) if let Some(id) = self.automation_tab_in_background() => {
                    let outcome = self
                        .bg_tabs
                        .get(&id)
                        .and_then(|snap| snap.js_ctx.as_ref())
                        .map(|j| j.eval_js_value(&js));
                    let _ = reply_tx.send(eval_outcome_reply(outcome, false));
                }
                AutomationCommand::Eval(js, timeout_ms) => {
                    // BUG-1145: с движковым потоком eval уходит `Task`-ом (по
                    // порядку за уже отправленными `task`) и ждёт в
                    // `pending_evals`, не блокируя UI-поток; срок — из запроса.
                    // Без потока — синхронно по UI-хэндлу, как раньше.
                    match self.engine_thread.as_ref() {
                        Some(engine) => {
                            let (result_tx, result_rx) = std::sync::mpsc::sync_channel::<EvalOutcome>(1);
                            let wake = self.load_proxy.clone();
                            engine.task(move |state| {
                                let outcome = state.js.as_ref().map(|j| j.eval_js_value(&js));
                                // Клиент уже получил отказ по сроку — будить некого.
                                if result_tx.send(outcome).is_ok() {
                                    let _ = wake.send_event(LoadEvent::AutomationWake);
                                }
                            });
                            self.pending_evals.push(PendingEval {
                                result_rx,
                                started: std::time::Instant::now(),
                                timeout: PendingEval::timeout_from_ms(timeout_ms),
                                reply_tx,
                            });
                        }
                        None => {
                            let outcome = self.js_ctx.as_ref().map(|j| j.eval_js_value(&js));
                            let _ = reply_tx.send(eval_outcome_reply(outcome, self.nav_start.is_some()));
                        }
                    }
                }
                AutomationCommand::Screenshot => {
                    match self.render_current_page_to_png() {
                        Ok(png) => {
                            let _ = reply_tx.send(AutomationReply::Screenshot(png));
                        }
                        Err(e) => {
                            let _ = reply_tx.send(AutomationReply::Error(e));
                        }
                    }
                }
                AutomationCommand::Wait(cond, timeout_ms) => {
                    self.pending_waits.push(PendingWait {
                        cond,
                        deadline: std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms),
                        reply_tx,
                    });
                }
                AutomationCommand::Query(selector) => {
                    let nodes = self.query_automation_nodes(&selector);
                    let _ = reply_tx.send(AutomationReply::Query(nodes));
                }
                AutomationCommand::A11yTree => match self.automation_a11y_tree() {
                    Some(tree) => {
                        let _ = reply_tx.send(AutomationReply::A11yTree(Box::new(tree)));
                    }
                    None => {
                        let _ = reply_tx.send(AutomationReply::Error("no page loaded".to_string()));
                    }
                },
                AutomationCommand::ComputedA11y { selectors } => {
                    // BUG-1014: `null` (no page / no match) makes the BiDi layer
                    // answer `no such element`.
                    let json = match self.automation_computed_a11y(&selectors) {
                        Some((role, name)) => serde_json::json!({ "role": role, "name": name }).to_string(),
                        None => "null".to_owned(),
                    };
                    let _ = reply_tx.send(AutomationReply::Eval(json));
                }
                AutomationCommand::ConsoleLog => {
                    // Drain any messages the JS runtime queued this tick before the
                    // periodic DevTools drain below would (same pattern, just eager
                    // so a message logged earlier this frame isn't missed).
                    let msgs = self.drain_query_js(|j| j.take_console_messages()).unwrap_or_default();
                    if !msgs.is_empty() {
                        // PERF-6: record page `console.error(...)` calls as health signals.
                        if health_log::is_enabled() {
                            let url = self.source.describe();
                            for (level, text) in &msgs {
                                if *level == 2 {
                                    health_log::log_console_error(&url, text);
                                }
                            }
                        }
                        self.devtools_console.push_batch(msgs);
                    }
                    let entries: Vec<ConsoleEntry> = self
                        .devtools_console
                        .messages()
                        .iter()
                        .map(|m| ConsoleEntry {
                            level: match m.level {
                                devtools::console_panel::ConsoleLevel::Log => DriverConsoleLevel::Log,
                                devtools::console_panel::ConsoleLevel::Warn => DriverConsoleLevel::Warn,
                                devtools::console_panel::ConsoleLevel::Error => DriverConsoleLevel::Error,
                            },
                            message: m.text.clone(),
                        })
                        .collect();
                    let _ = reply_tx.send(AutomationReply::ConsoleLog(entries));
                }
                AutomationCommand::LayoutSnapshot => {
                    let boxes = self.automation_layout_snapshot();
                    let _ = reply_tx.send(AutomationReply::LayoutSnapshot(boxes));
                }
                AutomationCommand::NetworkLog => {
                    let entries = self.automation_network_log();
                    let _ = reply_tx.send(AutomationReply::NetworkLog(entries));
                }
                AutomationCommand::SetOffline(offline) => {
                    // BUG-295 (`network.setOfflineStatus`): process-global flag
                    // consulted at the one `fetch_with_redirect` chokepoint every
                    // fetch path (navigation, JS `fetch()`/XHR, subresources)
                    // already funnels through — no per-request wiring needed here.
                    lumen_network::set_global_offline(offline);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::SetUserAgent(ua) => {
                    // BUG-295 (`emulation.setUserAgentOverride`): empty string
                    // clears the override (see `LiveWindowSession::set_user_agent`
                    // / `BidiState::user_agent_for` — `None` collapses to `""`
                    // before reaching this command).
                    let override_value = if ua.is_empty() { None } else { Some(ua.clone()) };
                    lumen_network::set_global_ua_override(override_value.clone());
                    #[cfg(feature = "v8")]
                    {
                        // Applies to the *next* navigation (install_dom reads the
                        // global at DOM-bootstrap time — see
                        // `v8_runtime::set_global_user_agent_override`'s doc).
                        lumen_js::v8_runtime::set_global_user_agent_override(override_value);
                        // Also re-inject into the *current* page right now: BiDi
                        // clients (and BUG-295's own repro) set the override then
                        // immediately `script.evaluate` `navigator.userAgent` on
                        // the already-loaded default context, with no navigation
                        // in between. Clearing (`ua.is_empty()`) is intentionally
                        // not re-applied here — the current page keeps whatever
                        // it last had; only the *next* navigation reverts to the
                        // real WEB_API_SHIM default.
                        if !ua.is_empty() {
                            let script = lumen_js::v8_runtime::user_agent_override_script(&ua);
                            let _ = route_query_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                move |j| j.eval_js_value(&script),
                            );
                        }
                    }
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::SetTimezone(timezone_id) => {
                    // BUG-295 (`browser.setTimezoneOverride`): unlike UA
                    // override there is no HTTP-layer counterpart — timezone
                    // only affects JS-visible `Intl`/`Date` behavior.
                    #[cfg(feature = "v8")]
                    {
                        // Applies to the *next* navigation (install_dom reads
                        // the global at DOM-bootstrap time — see
                        // `v8_runtime::set_global_timezone_override`'s doc).
                        lumen_js::v8_runtime::set_global_timezone_override(timezone_id.clone());
                        // Also re-inject into the *current* page right now,
                        // mirroring `SetUserAgent` above: BiDi clients set the
                        // override then immediately `script.evaluate` on the
                        // already-loaded default context, with no navigation
                        // in between. Clearing (`None`) is intentionally not
                        // re-applied here — only the *next* navigation clears
                        // the marker (matches `SetUserAgent`'s accepted gap).
                        if let Some(tz) = &timezone_id {
                            let script = lumen_js::v8_runtime::timezone_override_script(tz);
                            let _ = route_query_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                move |j| j.eval_js_value(&script),
                            );
                        }
                    }
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::SetPermission { name, state } => {
                    // BUG-1014 (`permissions.setPermission`): recorded
                    // process-globally so the next navigation's fresh runtime
                    // re-applies it, and pushed into the current page now.
                    // The reply is the shim's own verdict (`false` = unknown
                    // name/state), so the BiDi client is not told «success»
                    // for a permission `query()` could never report.
                    let mut accepted = true;
                    #[cfg(feature = "v8")]
                    {
                        let script = lumen_js::v8_runtime::permission_override_script(&name, &state);
                        // No live page (`None`) cannot validate: record anyway,
                        // the next navigation's shim ignores what it rejects.
                        if let Some(Ok(v)) = route_query_js(
                            self.engine_thread.as_ref(),
                            self.js_ctx.as_ref(),
                            move |j| j.eval_js_value(&script),
                        ) {
                            accepted = v.trim() == "true";
                        }
                        if accepted {
                            lumen_js::v8_runtime::set_global_permission_override(&name, &state);
                        }
                    }
                    let _ = reply_tx.send(AutomationReply::Eval(accepted.to_string()));
                }
                AutomationCommand::AddIntercept { id, phases, url_patterns } => {
                    // BUG-295 remainder (`network.addIntercept`): synced into
                    // `lumen_network`'s process-global registry, consulted at
                    // the same `fetch_with_redirect` chokepoint as the
                    // offline/UA-override toggles above.
                    lumen_network::add_global_intercept(lumen_network::GlobalIntercept {
                        id,
                        phases,
                        url_patterns,
                    });
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::RemoveIntercept(id) => {
                    lumen_network::remove_global_intercept(&id);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::ResolveIntercept { request_id, continue_request } => {
                    // BUG-295 remainder (`network.continueRequest`/`continueResponse`/`failRequest`):
                    // unblocks the fetch worker thread (if any) paused on
                    // `request_id` in `lumen_network`'s registry.
                    let decision = if continue_request {
                        lumen_network::InterceptDecision::Continue
                    } else {
                        lumen_network::InterceptDecision::Fail
                    };
                    let matched = lumen_network::resolve_intercept(&request_id, decision);
                    let _ = reply_tx.send(AutomationReply::InterceptResolved(matched));
                }
                AutomationCommand::PollIntercepts => {
                    let requests = lumen_network::drain_new_intercept_announcements()
                        .into_iter()
                        .map(|a| InterceptedRequest {
                            request_id: a.request_id,
                            url: a.url,
                            phase: a.phase,
                            status: a.status,
                        })
                        .collect();
                    let _ = reply_tx.send(AutomationReply::Intercepts(requests));
                }
                AutomationCommand::GetCookies => {
                    let _ = reply_tx.send(AutomationReply::Cookies(self.automation_get_cookies()));
                }
                AutomationCommand::SetCookie(cookie) => {
                    let reply = match self.automation_set_cookie(cookie) {
                        Ok(()) => AutomationReply::Ack,
                        Err(e) => AutomationReply::Error(format!("storage.setCookie: {e}")),
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::DeleteCookie { domain, path, name } => {
                    let reply = match self.automation_delete_cookie(&domain, &path, &name) {
                        Ok(()) => AutomationReply::Ack,
                        Err(e) => AutomationReply::Error(format!("storage.deleteCookies: {e}")),
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::AddPreloadScript { id, function_declaration } => {
                    // BUG-1811 (`script.addPreloadScript`): recorded
                    // process-globally and applied by every later
                    // `install_dom` — the next navigation's fresh runtime and
                    // every sub-document/iframe realm alike, before that
                    // document's own first script line.
                    #[cfg(feature = "v8")]
                    lumen_js::v8_runtime::set_global_preload_script(&id, &function_declaration);
                    #[cfg(not(feature = "v8"))]
                    let _ = (&id, &function_declaration);
                    let _ = reply_tx.send(AutomationReply::Ack);
                }
                AutomationCommand::RemovePreloadScript(id) => {
                    #[cfg(feature = "v8")]
                    let known = lumen_js::v8_runtime::remove_global_preload_script(&id);
                    #[cfg(not(feature = "v8"))]
                    let known = true;
                    let reply = if known {
                        AutomationReply::Ack
                    } else {
                        AutomationReply::Error(format!("unknown preload script: {id}"))
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::UserPrompt => {
                    // MCP-TOTAL-S7a: открытый диалог блокирует JS-поток, поэтому
                    // ответ читается из состояния UI, не через движок;
                    // close-warning шелла — не диалог страницы, автоматизации
                    // (которая может закрыть окно) не показывается.
                    let dialog = self
                        .page_dialog
                        .as_ref()
                        .filter(|d| !d.close_warning)
                        .map(|d| AutomationDialog {
                            kind: match d.req.kind {
                                lumen_js::dialog::DialogKind::Alert => "alert",
                                lumen_js::dialog::DialogKind::Confirm => "confirm",
                                lumen_js::dialog::DialogKind::Prompt => "prompt",
                                lumen_js::dialog::DialogKind::BeforeUnload => "beforeUnload",
                            }
                            .to_owned(),
                            message: d.req.message.clone(),
                            default_value: d.req.default_value.clone(),
                        });
                    let _ = reply_tx.send(AutomationReply::UserPrompt(dialog));
                }
                AutomationCommand::HandleUserPrompt { accept, user_text } => {
                    // MCP-TOTAL-S7a: ответ не ждёт JS-поток — его и блокирует
                    // сам диалог; `dialog::answer` будит его с UI-потока.
                    let reply = match self.page_dialog.as_mut().filter(|d| !d.close_warning) {
                        None => AutomationReply::Error("handle_user_prompt: диалог не открыт".to_string()),
                        Some(d) => {
                            if let Some(text) = user_text {
                                d.input = text;
                            }
                            self.answer_page_dialog(accept, event_loop);
                            AutomationReply::Ack
                        }
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::SetFiles { target, paths } => {
                    // MCP-TOTAL-S7b: доставка файлов в файловый input без
                    // OS-диалога — валидация путей и регистрация грантов в
                    // `set_files_on_input`, тот же путь, что у пикера.
                    let reply = match self.resolve_automation_target(&target) {
                        Some(resolved) => match resolved.node {
                            Some(node) => match self.set_files_on_input(node, &paths) {
                                Ok(()) => AutomationReply::Ack,
                                Err(msg) => AutomationReply::Error(msg),
                            },
                            // `Target::Point` резолвится без узла — заполнять нечего.
                            None => AutomationReply::Error("set_files: target has no element".to_string()),
                        },
                        None => AutomationReply::Error("Element not found".to_string()),
                    };
                    let _ = reply_tx.send(reply);
                }
                AutomationCommand::GetDownloads => {
                    // MCP-TOTAL-S7c: финальные события фоновых потоков
                    // подбираем до чтения — иначе только что завершившаяся
                    // загрузка ещё кадр отвечала бы in_progress.
                    self.downloads.poll();
                    let entries = self
                        .downloads
                        .entries()
                        .iter()
                        .map(|e| AutomationDownload {
                            id: e.id.raw(),
                            url: e.url.clone(),
                            file_name: e.filename.clone(),
                            state: download::status_name(&e.status),
                            received_bytes: e.received,
                            total_bytes: e.total.unwrap_or(0),
                            path: e.dest.to_string_lossy().into_owned(),
                        })
                        .collect();
                    let _ = reply_tx.send(AutomationReply::Downloads(entries));
                }
                AutomationCommand::StartDownload { url } => {
                    // MCP-TOTAL-S7c: папка, имя и панель — как у загрузки по
                    // ссылке; запись видна в GetDownloads сразу.
                    let id = self.downloads.start_url_download(url, None);
                    let _ = reply_tx.send(AutomationReply::DownloadId(id.raw()));
                }
                AutomationCommand::CancelDownload { id } => {
                    // MCP-TOTAL-S7c: неизвестный id — честная ошибка (сам
                    // `DownloadManager::cancel` на него молчит).
                    let reply = if self.downloads.entries().iter().any(|e| e.id.raw() == id) {
                        self.downloads.cancel(download::DownloadId(id));
                        AutomationReply::Ack
                    } else {
                        AutomationReply::Error(format!("cancel_download: нет загрузки с id {id}"))
                    };
                    let _ = reply_tx.send(reply);
                }
            }
        }
    }
}

/// A queued `AutomationCommand::Wait` request (SDC-1b), re-checked once per
/// frame in `about_to_wait` rather than blocking the event loop.
pub(crate) struct PendingWait {
    /// Condition to poll — see [`check_pending_wait_condition`].
    pub(crate) cond: WaitCondition,
    /// When this wait gives up and replies `AutomationReply::Error`.
    pub(crate) deadline: std::time::Instant,
    /// Where to send the `Ack`/`Error` reply once resolved.
    pub(crate) reply_tx: std::sync::mpsc::Sender<AutomationReply>,
}

/// Верхняя граница `timeout_ms` у `AutomationCommand::Eval` (BUG-1145): дальше
/// ждать движковый поток бессмысленно, а неограниченное значение переполнило бы
/// `Instant + Duration`.
const MAX_EVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Итог `eval_js_value` на движковом потоке; `None` — JS-контекста нет.
pub(crate) type EvalOutcome = Option<Result<String, String>>;

/// `AutomationCommand::Eval`, поставленный движковому потоку и ждущий его
/// (BUG-1145). Раньше UI-поток блокировался на `EngineThread::query` и после
/// `QUERY_TIMEOUT` отвечал «JS context not available» — так же, как при
/// настоящем отсутствии контекста. Теперь eval уходит `Task`-ом, UI-поток не
/// ждёт, а `about_to_wait` раз в итерацию проверяет ответ и срок.
///
/// По истечении срока задание остаётся в очереди и всё равно исполнится —
/// теряется только ответ, как и у `query`.
pub(crate) struct PendingEval {
    /// Сюда движковый поток кладёт итог; дроп без отправки — поток остановлен.
    pub(crate) result_rx: std::sync::mpsc::Receiver<EvalOutcome>,
    /// Когда eval поставлен в очередь.
    pub(crate) started: std::time::Instant,
    /// Сколько ждать до ответа «поток занят».
    pub(crate) timeout: std::time::Duration,
    /// Куда ответить клиенту автоматизации.
    pub(crate) reply_tx: std::sync::mpsc::Sender<AutomationReply>,
}

impl PendingEval {
    /// Срок ожидания `timeout_ms` (или умолчание — `QUERY_TIMEOUT`), обрезанный
    /// по [`MAX_EVAL_TIMEOUT`].
    pub(crate) fn timeout_from_ms(timeout_ms: Option<u64>) -> std::time::Duration {
        timeout_ms
            .map_or(crate::engine_thread::QUERY_TIMEOUT, std::time::Duration::from_millis)
            .min(MAX_EVAL_TIMEOUT)
    }

    /// Момент, когда eval сдаётся.
    pub(crate) fn deadline(&self) -> std::time::Instant {
        self.started + self.timeout
    }

    /// Ответ клиенту, если eval завершён: итог, остановленный поток или срок
    /// (`busy` называет, чем поток занят); `None` — ждать дальше. `loading` —
    /// идёт навигация (см. [`eval_outcome_reply`]).
    pub(crate) fn poll(
        &self,
        now: std::time::Instant,
        loading: bool,
        busy: impl FnOnce() -> Option<(crate::engine_thread::EngineWork, std::time::Duration)>,
    ) -> Option<AutomationReply> {
        match self.result_rx.try_recv() {
            Ok(outcome) => Some(eval_outcome_reply(outcome, loading)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(AutomationReply::Error(
                "engine thread stopped before running eval".to_string(),
            )),
            Err(std::sync::mpsc::TryRecvError::Empty) if now >= self.deadline() => {
                Some(AutomationReply::Error(eval_busy_message(self.timeout, busy())))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        }
    }
}

/// Ответ автоматизации на итог eval: `None` — контекста действительно нет.
/// `loading` — навигация ещё не дошла до установки рантайма страницы: на
/// тяжёлом сайте это десятки секунд, и без пометки такой ответ неотличим от
/// страницы, где рантайма нет вовсе (BUG-1178).
pub(crate) fn eval_outcome_reply(outcome: EvalOutcome, loading: bool) -> AutomationReply {
    match outcome {
        Some(Ok(json)) => AutomationReply::Eval(json),
        Some(Err(e)) => AutomationReply::Error(e),
        None if loading => {
            AutomationReply::Error("JS context not available: page is still loading".to_string())
        }
        None => AutomationReply::Error("JS context not available".to_string()),
    }
}

/// Текст отказа по сроку: сколько ждали и, если известно, чем занят поток.
fn eval_busy_message(
    timeout: std::time::Duration,
    busy: Option<(crate::engine_thread::EngineWork, std::time::Duration)>,
) -> String {
    let mut msg = format!(
        "engine thread busy: eval not run within {:.1} s",
        timeout.as_secs_f32()
    );
    if let Some((work, for_how_long)) = busy {
        use std::fmt::Write as _;
        let _ = write!(msg, " (running {work} for {:.1} s)", for_how_long.as_secs_f32());
    }
    msg.push_str("; retry or pass a larger timeout_ms");
    msg
}
