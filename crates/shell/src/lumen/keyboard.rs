//! What a key press does before it can become text: the shell's key dispatch.
//!
//! `handle_key` is the single winit `KeyboardInput` entry point. It walks the
//! chrome front to back - overlays and panels first (each of
//! [`super::panel_keys`] answers "did this panel consume the key"), then the
//! address bar, then the page - so a shortcut can never reach the page behind
//! an open panel. Only what nothing above claimed becomes text, which is the
//! job of [`super::text_input`].
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour and the
//! method body are unchanged; only the module path and the visibility of
//! `handle_key` differ.

use crate::*;

impl Lumen {
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn handle_key(&mut self, event_loop: &ActiveEventLoop, key_event: &KeyEvent) {
        if key_event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = key_event.physical_key else {
            return;
        };

        // РљРѕРјР°РЅРґРЅР°СЏ РїР°Р»РёС‚СЂР° вЂ” РјРѕРґР°Р»СЊРЅС‹Р№ overlay: РїРѕРєР° РѕС‚РєСЂС‹С‚Р°, РїРµСЂРµС…РІР°С‚С‹РІР°РµС‚ РІСЃРµ
        // РєР»Р°РІРёС€Рё (Esc/Enter/в†‘/в†“/Backspace/РїРµС‡Р°С‚СЊ). Ctrl+K (toggle) РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ
        // РІ РіР»РѕР±Р°Р»СЊРЅС‹Р№ keybinding-РїСѓС‚СЊ РЅРёР¶Рµ, С‡С‚РѕР±С‹ Р·Р°РєСЂС‹С‚СЊ РїР°Р»РёС‚СЂСѓ.
        if self.command_palette.visible
            && !(code == KeyCode::KeyK && self.modifiers == ModifiersState::CONTROL)
            && self.handle_palette_key(code, key_event, event_loop)
        {
            return;
        }

        // РђРґСЂРµСЃРЅР°СЏ СЃС‚СЂРѕРєР° (Ctrl+L) РїРµСЂРµС…РІР°С‚С‹РІР°РµС‚ РІРІРѕРґ РїРµСЂРІРѕР№: Esc=close,
        // Enter=navigate, Backspace=СѓРґР°Р»РёС‚СЊ СЃРёРјРІРѕР», РёРЅР°С‡Рµ вЂ” С‚РµРєСЃС‚ URL.
        if self.address_bar.is_open() {
            self.handle_address_bar_key(code, key_event, event_loop);
            return;
        }

        // РљРѕРіРґР° find bar РѕС‚РєСЂС‹С‚ вЂ” РІСЃРµ РєР»Р°РІРёС€Рё РёРґСѓС‚ РІ РЅРµРіРѕ: РІРІРѕРґ СЃРёРјРІРѕР»РѕРІ,
        // Esc=close, Backspace=СЃС‚РёСЂР°РЅРёРµ, Enter/F3=next (Shift=prev). Р­С‚Рѕ РЅРµ
        // РґР°С‘С‚ СЃР»СѓС‡Р°Р№РЅРѕ СЃСЂР°Р±РѕС‚Р°С‚СЊ Esc=Exit РёР»Рё Ctrl+R=Reload РІ РјРѕРјРµРЅС‚ РїРѕРёСЃРєР°.
        if self.find.is_open() {
            self.handle_find_key(code, key_event);
            return;
        }

        // Hint-СЂРµР¶РёРј: РІСЃРµ РєР»Р°РІРёС€Рё РёРґСѓС‚ РІ РЅРµРіРѕ РїРѕРєР° Р°РєС‚РёРІРµРЅ.
        // Esc=close, Р±СѓРєРІР°=СЃСѓР¶РµРЅРёРµ/Р°РєС‚РёРІР°С†РёСЏ С…РёРЅС‚Р°.
        if self.hint.is_active() {
            self.handle_hint_key(code, key_event);
            return;
        }

        // Bookmark panel search box: when focused, printable input + Backspace +
        // Esc route to the search query. Modified keys (Ctrl/Cmd) fall through so
        // global shortcuts (e.g. Ctrl+Shift+O to close) keep working.
        if self.bookmark_panel.visible
            && self.bookmark_panel.search_active
            && self.handle_bookmark_key(code, key_event)
        {
            return;
        }

        // History panel search box: printable input + Backspace + Esc route here.
        // Arrow keys scroll the list. Modified keys fall through for global shortcuts.
        if self.history_panel.visible && self.handle_history_key(code, key_event) {
            return;
        }

        // Note viewer overlay: Escape closes it.
        if self.note_viewer.visible && code == KeyCode::Escape && !key_event.repeat {
            self.note_viewer.close();
            self.request_redraw();
            return;
        }

        // AI panel input: printable text, Backspace, Enter. Ctrl/Meta fall through.
        if self.ai_panel.visible && self.handle_ai_panel_key(code, key_event) {
            return;
        }

        // Settings panel text inputs + Esc. Modified keys fall through for global shortcuts.
        if self.print_panel.visible && self.handle_print_key(code, key_event) {
            return;
        }
        if self.settings_panel.visible && self.handle_settings_key(code, key_event) {
            return;
        }

        // Keyboard shortcuts panel вЂ” capture any keypress when rebinding (В§D-4).
        if self.shortcuts_panel.visible && self.handle_shortcuts_key(code, key_event) {
            return;
        }

        // Vim keybinding mode: intercept navigation keys in Normal state.
        // In Insert state, PassThrough falls through to the keybinding table.
        if let Some(ref mut vm) = self.vim_mode {
            let action = vm.feed(code, self.modifiers);
            match action {
                input::vim::VimAction::PassThrough => {} // fall through below
                input::vim::VimAction::Consumed => return,
                input::vim::VimAction::Deactivate => {
                    self.vim_mode = None;
                    return;
                }
                input::vim::VimAction::EnterInsert | input::vim::VimAction::ExitInsert => {
                    return;
                }
                input::vim::VimAction::ScrollDown => {
                    self.scroll_active_pane(LINE_STEP_CSS_PX);
                    return;
                }
                input::vim::VimAction::ScrollUp => {
                    self.scroll_active_pane(-LINE_STEP_CSS_PX);
                    return;
                }
                input::vim::VimAction::ScrollHalfPageDown => {
                    let half = self.viewport_height_css() * 0.5;
                    self.scroll_active_pane(half);
                    return;
                }
                input::vim::VimAction::ScrollHalfPageUp => {
                    let half = self.viewport_height_css() * 0.5;
                    self.scroll_active_pane(-half);
                    return;
                }
                input::vim::VimAction::ScrollTop => {
                    self.scroll_active_pane_to(0.0);
                    return;
                }
                input::vim::VimAction::ScrollBottom => {
                    self.scroll_active_pane_to(f32::INFINITY);
                    return;
                }
                input::vim::VimAction::OpenFind => {
                    self.hint.close();
                    self.find.open();
                    self.request_redraw();
                    return;
                }
                input::vim::VimAction::OpenHints | input::vim::VimAction::OpenHintsNewTab => {
                    if let (Some(lb), Some(src)) =
                        (self.layout_box.as_ref(), self.layout_source.as_ref())
                    {
                        let doc = src.document.lock().unwrap();
                        let elements = lumen_layout::collect_clickable_elements(lb, &doc);
                        drop(doc);
                        if !elements.is_empty() {
                            self.hint.open(elements);
                            self.request_redraw();
                        }
                    }
                    return;
                }
                input::vim::VimAction::Copy => {
                    // Copy the current page URL to the OS clipboard (task #26).
                    if let Some(url) = self.source.url_str() {
                        use lumen_core::ext::ClipboardProvider;
                        platform::clipboard::PlatformClipboard.write_text(url);
                        eprintln!("[vim] copy URL: {url}");
                    }
                    return;
                }
                input::vim::VimAction::HistoryBack => {
                    self.navigate_back();
                    return;
                }
                input::vim::VimAction::HistoryForward => {
                    self.navigate_forward();
                    return;
                }
            }
        }

        // Pointer Lock API (W3C Pointer Lock L2 В§6.7): Escape releases pointer lock.
        // Must be processed before fullscreen so a locked pointer in fullscreen exits
        // lock first, letting a second Escape then exit fullscreen.
        #[cfg(feature = "v8")]
        if lumen_js::pointer_lock::is_pointer_locked()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            lumen_js::pointer_lock::exit_pointer_lock();
            // Apply OS cursor release immediately (don't wait for about_to_wait).
            if let Some(window) = self.window.as_ref() {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
                window.set_cursor_visible(true);
            }
            // Dispatch pointerlockchange so document.pointerLockElement clears in
            // JS. ADR-016 M2.2c-2d: fire-and-forget void eval С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ”
            // РїРѕРґ С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                "document.dispatchEvent(new Event('pointerlockchange'))".to_string(),
            );
            return;
        }

        // Fullscreen API (WHATWG Fullscreen В§4.6): Escape always exits fullscreen first.
        // If we are fullscreen and the user presses Escape (no repeat, no mods), exit
        // fullscreen before processing any other shortcut.
        if self.fullscreen_nid.is_some()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.fullscreen_nid = None;
            let prev = self.window.as_ref().map(|w| {
                w.set_fullscreen(None);
                w.inner_size()
            });
            if let Some(prev) = prev {
                self.arm_fullscreen_resize(prev);
            }
            // Notify JS so fullscreenchange fires and document.fullscreenElement clears.
            // ADR-016 M2.2c-2d: fire-and-forget void eval С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ
            // С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ РїСЂРµР¶РЅРµРјСѓ
            // `js.eval_js(вЂ¦)` (РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІСѓСЋС‰РµРј С…СЌРЅРґР»Рµ вЂ” no-op, РєР°Рє РїСЂРµР¶РЅРёР№ `if let`).
            #[cfg(feature = "v8")]
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                "if(typeof _lumen_notify_fullscreen_exit==='function')_lumen_notify_fullscreen_exit()"
                    .to_string(),
            );
            return;
        }

        // CC-4: Escape closes the tab context menu before any other handling.
        if self.tab_context_menu.is_open()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.tab_context_menu.close();
            self.request_redraw();
            return;
        }

        // P3-spell СЃСЂРµР· 3: Escape closes the page spell suggestion menu.
        if self.page_context_menu.is_open()
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.page_context_menu.close();
            self.request_redraw();
            return;
        }

        // Focus mode (task #25): while active, Escape exits focus mode instead of
        // quitting the app. Ctrl+Shift+F falls through to the keybinding table so
        // it can toggle focus mode off.
        if self.focus.active
            && code == KeyCode::Escape
            && self.modifiers.is_empty()
            && !key_event.repeat
        {
            self.focus.exit();
            self.request_redraw();
            return;
        }

        // contenteditable key routing вЂ” before global keybindings so that
        // typing inside an editable region is not swallowed by scroll commands.
        // Only active when the focused node is inside a contenteditable host
        // and no modifier (Ctrl/Alt/Meta) is held (those go to keybindings).
        if (self.modifiers.is_empty() || self.modifiers == ModifiersState::SHIFT)
            && let (Some(nid), Some(src)) = (self.focused_node, self.layout_source.as_ref())
        {
            // ADR-016 M2.2c-2d: contenteditable-key void-eval С‡РµСЂРµР· `route_eval_js` вЂ”
            // СЃРЅРёРјР°РµРј РїСЂСЏРјС‹Рµ `self.js_ctx`-РѕР±СЂР°С‰РµРЅРёСЏ. DOM-read (`find_editing_host`)
            // РѕСЃС‚Р°С‘С‚СЃСЏ РЅР° UI-РїРѕС‚РѕРєРµ (С‡РёС‚Р°РµС‚ СЂР°Р·РґРµР»СЏРµРјС‹Р№ `src.document`, РЅРµ JS-С…СЌРЅРґР»);
            // СЃР°РјРё `_lumen_handle_contenteditable_key`-РІС‹Р·РѕРІС‹ вЂ” С‡РёСЃС‚С‹Р№ fire-and-forget
            // void Р±РµР· СЃРёРЅС…СЂРѕРЅРЅРѕРіРѕ С‡С‚РµРЅРёСЏ СЂРµР·СѓР»СЊС‚Р°С‚Р° СЃР»РµРґРѕРј, РїРѕСЌС‚РѕРјСѓ РїРѕРґ С„Р»Р°РіРѕРј
            // (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґСЏС‚ off-UI-thread РѕРґРЅРёРј `task`, Р±РµР· С„Р»Р°РіР° (РїРѕ
            // СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ. Р“РµР№С‚ Р·Р°РјРµРЅС‘РЅ
            // СЃ `if let Some(js)` РЅР° `is_some()`, С‡С‚РѕР±С‹ editing-host detection Рё eval
            // РІС‹РїРѕР»РЅСЏР»РёСЃСЊ С‚РѕР»СЊРєРѕ РїСЂРё РЅР°Р»РёС‡РёРё JS-РєРѕРЅС‚РµРєСЃС‚Р° (РєР°Рє РїСЂРµР¶РґРµ).
            #[cfg(feature = "v8")]
            if self.js_present {
                // Check contenteditable by reading the DOM directly (eval_js returns ()).
                let editing_host = src
                    .document
                    .lock()
                    .ok()
                    .and_then(|doc| lumen_dom::find_editing_host(&doc, nid));
                if let Some(host) = editing_host {
                    let host_nid = host.index();
                    let handled = match code {
                        KeyCode::Backspace => {
                            route_eval_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                format!(
                                    "_lumen_handle_contenteditable_key('deleteContentBackward',null,{})",
                                    host_nid
                                ),
                            );
                            true
                        }
                        KeyCode::Delete => {
                            route_eval_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                format!(
                                    "_lumen_handle_contenteditable_key('deleteContentForward',null,{})",
                                    host_nid
                                ),
                            );
                            true
                        }
                        KeyCode::Enter | KeyCode::NumpadEnter => {
                            let input_type = if self.modifiers == ModifiersState::SHIFT {
                                "insertLineBreak"
                            } else {
                                "insertParagraph"
                            };
                            route_eval_js(
                                self.engine_thread.as_ref(),
                                self.js_ctx.as_ref(),
                                format!(
                                    "_lumen_handle_contenteditable_key('{}',null,{})",
                                    input_type, host_nid
                                ),
                            );
                            true
                        }
                        _ => {
                            // Printable key вЂ” extract text from logical key.
                            if let Some(text) = key_event.logical_key.to_text()
                                && !text.is_empty()
                                && text.chars().all(|c| !c.is_control())
                            {
                                let escaped =
                                    text.replace('\\', "\\\\").replace('\'', "\\'");
                                route_eval_js(
                                    self.engine_thread.as_ref(),
                                    self.js_ctx.as_ref(),
                                    format!(
                                        "_lumen_handle_contenteditable_key('insertText','{}',{})",
                                        escaped, host_nid
                                    ),
                                );
                                self.request_redraw();
                                return;
                            }
                            false
                        }
                    };
                    if handled {
                        self.request_redraw();
                        return;
                    }
                }
            }
        }

        // Text editing inside a typeable field ВНУТРИ фрейма (BUG-480 срез 22)
        // — та же логика и то же место, что у страницы ниже, но проверяется
        // ПЕРВОЙ: `self.focused_node` в этот момент указывает на host-элемент
        // `<iframe>` (срез 16), который не typeable, так что страничная ветка
        // всё равно бы не сработала — порядок только для ясности.
        if (self.modifiers.is_empty() || self.modifiers == ModifiersState::SHIFT)
            && let Some((idx, nid)) = self.focused_frame
            && self.frame_typeable_field(idx, nid).is_some()
        {
            if code == KeyCode::Backspace {
                self.inject_frame_backspace();
                self.request_redraw();
                return;
            }
            if let Some(text) = key_event.logical_key.to_text()
                && !text.is_empty()
                && text.chars().all(|c| !c.is_control())
            {
                for ch in text.chars() {
                    self.inject_frame_char(ch);
                }
                self.request_redraw();
                return;
            }
        }

        // Text editing inside a focused `<input>`/`<textarea>` вЂ” same placement
        // rationale as the contenteditable branch above: without it a printable
        // key falls through to the global keybinding table, where a bare `F`
        // opens hint mode and Space scrolls the page instead of reaching the
        // field. The insertion itself is the engine's own default action
        // (`inject_char` в†’ `edit_focused_field`, BUG-436).
        if (self.modifiers.is_empty() || self.modifiers == ModifiersState::SHIFT)
            && self.focused_node.is_some_and(|nid| self.typeable_field(nid).is_some())
        {
            if code == KeyCode::Backspace {
                self.inject_backspace();
                self.request_redraw();
                return;
            }
            if let Some(text) = key_event.logical_key.to_text()
                && !text.is_empty()
                && text.chars().all(|c| !c.is_control())
            {
                for ch in text.chars() {
                    self.inject_char(ch);
                }
                self.request_redraw();
                return;
            }
        }

        let Some(cmd) = keybinding_for(code, self.modifiers) else {
            return;
        };
        // Scroll-РєРѕРјР°РЅРґС‹ СЂР°Р·СЂРµС€Р°РµРј РЅР° repeat (auto-repeat РїСЂРё СѓРґРµСЂР¶Р°РЅРёРё),
        // РѕСЃС‚Р°Р»СЊРЅС‹Рµ вЂ” С‚РѕР»СЊРєРѕ РЅР° РїРµСЂРІРѕРµ РЅР°Р¶Р°С‚РёРµ.
        let is_scroll = matches!(
            cmd,
            KeyCommand::ScrollLineDown
                | KeyCommand::ScrollLineUp
                | KeyCommand::ScrollPageDown
                | KeyCommand::ScrollPageUp
                | KeyCommand::ScrollHome
                | KeyCommand::ScrollEnd
                | KeyCommand::ScrollLineRight
                | KeyCommand::ScrollLineLeft
        );
        if key_event.repeat && !is_scroll {
            return;
        }
        match cmd {
            KeyCommand::Reload => {
                // HTML В§8.1.4 В«Event loopВ»: РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёРµ РґРµР№СЃС‚РІРёСЏ (reload)
                // РїР»Р°РЅРёСЂСѓСЋС‚СЃСЏ С‡РµСЂРµР· UserInteraction task source, Р° РЅРµ РІС‹Р·С‹РІР°СЋС‚СЃСЏ
                // РЅР°РїСЂСЏРјСѓСЋ. `pending_reload` вЂ” С„Р»Р°Рі-РјРѕСЃС‚: closure-Р·Р°РґР°С‡Р° РјРѕР¶РµС‚
                // Р±С‹С‚СЊ `+ 'static`, Lumen вЂ” РЅРµС‚; Cell РїРѕР·РІРѕР»СЏРµС‚ РёР· Р·Р°РјС‹РєР°РЅРёСЏ
                // СѓСЃС‚Р°РЅРѕРІРёС‚СЊ С„Р»Р°Рі, РєРѕС‚РѕСЂС‹Р№ `about_to_wait` РїСЂРѕРІРµСЂСЏРµС‚ Рё РІС‹Р·С‹РІР°РµС‚
                // `reload()` РїРѕСЃР»Рµ РґСЂРµРЅР°Р¶Р° РѕС‡РµСЂРµРґРё.
                let flag = Rc::clone(&self.pending_reload);
                self.runtime.handle().queue_task(
                    runtime::TaskSource::UserInteraction,
                    move || { flag.set(true); },
                );
            }
            KeyCommand::Exit => event_loop.exit(),
            KeyCommand::FindOpen => {
                self.hint.close();
                self.find.open();
                self.request_redraw();
            }
            KeyCommand::OpenAddressBar => {
                self.hint.close();
                let current = self.current_display_url().to_owned();
                self.address_bar.open(&current);
                // CC-7: reflect the now-open state (focus ring, value) in
                // the engine-rendered `#omniInput` вЂ” see the comment on the
                // matching call in `Self::handle_address_bar_key`.
                self.relayout_chrome_host();
                self.request_redraw();
            }
            KeyCommand::HintModeOpen => {
                if let (Some(lb), Some(src)) =
                    (self.layout_box.as_ref(), self.layout_source.as_ref())
                {
                    let doc = src.document.lock().unwrap();
                    let elements = lumen_layout::collect_clickable_elements(lb, &doc);
                    drop(doc);
                    if !elements.is_empty() {
                        self.hint.open(elements);
                        self.request_redraw();
                    }
                }
            }
            KeyCommand::HistoryBack => self.navigate_back(),
            KeyCommand::HistoryForward => self.navigate_forward(),
            KeyCommand::ScrollLineDown => self.scroll_active_pane(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineUp => self.scroll_active_pane(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineRight => self.scroll_x_by(LINE_STEP_CSS_PX),
            KeyCommand::ScrollLineLeft => self.scroll_x_by(-LINE_STEP_CSS_PX),
            KeyCommand::ScrollPageDown => {
                let vh = self.viewport_height_css();
                self.scroll_active_pane(page_step(vh));
            }
            KeyCommand::ScrollPageUp => {
                let vh = self.viewport_height_css();
                self.scroll_active_pane(-page_step(vh));
            }
            KeyCommand::ScrollHome => self.scroll_active_pane_to(0.0),
            KeyCommand::ScrollEnd => self.scroll_active_pane_to(f32::INFINITY),
            KeyCommand::NewTab => self.open_new_tab(),
            KeyCommand::CloseTab => {
                let idx = self.tab_strip.active;
                self.close_tab(idx, event_loop);
            }
            KeyCommand::NextTab => {
                let next = (self.tab_strip.active + 1) % self.tab_strip.len();
                self.switch_tab(next);
            }
            KeyCommand::DownloadsPanel => {
                self.downloads.toggle_visible();
                self.request_redraw();
            }
            KeyCommand::SplitView => {
                if self.split_view.is_some() {
                    self.split_view = None;
                } else {
                    self.toggle_split_view();
                }
                self.request_redraw();
            }
            KeyCommand::SplitFocusSwitch => {
                if let Some(ref mut sv) = self.split_view {
                    sv.toggle_focus();
                    self.request_redraw();
                }
            }
            KeyCommand::VimModeToggle => {
                if self.vim_mode.is_some() {
                    self.vim_mode = None;
                } else {
                    self.vim_mode = Some(input::vim::VimMode::new());
                }
            }
            KeyCommand::ToggleVerticalTabs => {
                self.vertical_tabs.toggle();
                self.persist_tab_layout();
                // Viewport width changes вЂ” re-layout the current page (ADR-016
                // M2.2b: chrome-inset change, off-thread when the engine is on).
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::ToggleTreeTabs => {
                self.tree_tabs.toggle();
                // Viewport width changes when switching to/from tree view
                // (ADR-016 M2.2b: async-safe chrome-inset relayout).
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::FlipActiveDock => {
                // Cross-dock the active sidebar (tabs, AI, or web);
                // flip_active_sidebar_dock relayouts internally on success.
                if self.flip_active_sidebar_dock() {
                    self.request_redraw();
                }
            }
            KeyCommand::ToggleWorkspaces => {
                self.workspace_panel.toggle();
                // Viewport height changes вЂ” re-layout so content doesn't hide
                // under bar (ADR-016 M2.2b: async-safe chrome-inset relayout).
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::ToggleShields => {
                self.shields.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePermissions => {
                self.permission.toggle();
                self.request_redraw();
            }
            KeyCommand::ToggleCookieBannerDismiss => {
                self.cookie_banner_dismiss = !self.cookie_banner_dismiss;
                // Preference takes effect on the next page load.
            }
            KeyCommand::ToggleAiPanel => {
                self.ai_panel.toggle();
                // AI panel occupies right PANEL_WIDTH вЂ” relayout so main content
                // width adjusts accordingly. ADR-016 M2.2b-3: async-safe chrome
                // toggle (only the content viewport width shifts, no synchronous
                // geometry read follows), so route off-thread when the engine
                // thread is enabled; the panel itself draws on the redraw below.
                self.relayout_chrome();
                self.request_redraw();
            }
            KeyCommand::ToggleBookmarks => {
                self.bookmark_panel.toggle();
                if self.bookmark_panel.visible {
                    self.refresh_bookmarks();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleHistory => {
                self.history_panel.toggle();
                if self.history_panel.visible {
                    self.refresh_history();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleA11y => {
                if self.a11y_panel.visible {
                    let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
                    self.a11y_panel.visible = false;
                    self.deliver_a11y_media_changes();
                    // Re-style with the (possibly toggled) forced-colors pref.
                    // ADR-016 M2.2b-3: async-safe вЂ” closing the a11y panel widens
                    // the content viewport and re-styles under the new
                    // forced-colors preference, but nothing reads page geometry
                    // synchronously afterwards, so route off-thread when enabled.
                    self.relayout_chrome();
                } else {
                    self.a11y_panel.load_draft(self.a11y_store.snapshot());
                    self.a11y_panel.visible = true;
                }
                self.request_redraw();
            }
            KeyCommand::ToggleSettings => {
                if self.settings_panel.visible {
                    self.close_settings_panel();
                } else {
                    self.open_settings_panel();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleCommandPalette => {
                self.command_palette.toggle();
                if self.command_palette.visible {
                    self.refresh_palette_items();
                }
                self.request_redraw();
                // CC-10: `#cpOverlay`'s engine-rendered open state/results
                // (`Self::chrome_model_snapshot`) is baked into
                // `self.chrome_layout` at `relayout_chrome_host` time, not
                // recomputed every `RedrawRequested` вЂ” same class of gap
                // CC-7/CC-9 found for the omnibox/find-bar. No-op off the flag.
                self.relayout_chrome_host();
            }
            KeyCommand::ToggleFocusMode => {
                // Enter with a default-length Pomodoro; re-baseline the timer so
                // the elapsed gap before the panel opened is not counted.
                self.focus.toggle(panels::focus_panel::DEFAULT_POMODORO_MIN);
                if self.focus.active {
                    let now_ms = self.epoch.elapsed().as_secs_f64() * 1000.0;
                    self.focus.tick(now_ms);
                }
                self.request_redraw();
            }
            KeyCommand::BookmarkCurrentPage => {
                self.bookmark_current_page();
                self.request_redraw();
            }
            KeyCommand::SetTabContainer(container) => {
                let idx = self.tab_strip.active;
                self.set_tab_container(idx, container);
            }
            KeyCommand::DevConsole => {
                self.devtools_console.toggle();
                self.request_redraw();
            }
            KeyCommand::DevInspector => {
                self.dom_inspector.toggle();
                self.request_redraw();
            }
            KeyCommand::DevNetwork => {
                self.network_panel.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePrivacy => {
                self.privacy.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePip => {
                self.toggle_pip();
                self.request_redraw();
            }
            KeyCommand::ToggleReadLater => {
                self.read_later_panel.toggle();
                if self.read_later_panel.visible {
                    self.refresh_read_later();
                }
                self.request_redraw();
            }
            KeyCommand::ToggleReaderView => {
                self.toggle_reader_view();
            }
            KeyCommand::ViewSource => {
                self.show_view_source();
            }
            KeyCommand::ToggleShortcuts => {
                self.shortcuts_panel.toggle();
                self.request_redraw();
            }
            KeyCommand::TogglePrint => {
                self.print_panel.toggle();
                self.request_redraw();
                // CC-10: see the matching comment on `ToggleCommandPalette`.
                self.relayout_chrome_host();
            }
            KeyCommand::ToggleCert => {
                let cert = self.cert_info.clone();
                self.cert_panel.toggle(cert);
                self.request_redraw();
                // CC-10: see the matching comment on `ToggleCommandPalette`.
                self.relayout_chrome_host();
            }
            KeyCommand::ZoomIn => {
                self.zoom_factor = zoom::zoom_in(self.zoom_factor);
                self.begin_zoom_preview();
            }
            KeyCommand::ZoomOut => {
                self.zoom_factor = zoom::zoom_out(self.zoom_factor);
                self.begin_zoom_preview();
            }
            KeyCommand::ZoomReset => {
                self.zoom_factor = zoom::zoom_reset();
                self.begin_zoom_preview();
            }
        }
    }

}
