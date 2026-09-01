//! Turning a key or an IME composition into text inside the focused control.
//!
//! The shell owns the value of a focused `<input>` / `<textarea>` /
//! `contenteditable`, so a typed character is applied here and only then
//! reported to JS: `dispatch_injected_key` fires the `keydown` / `keyup` pair
//! with `isTrusted=true` (JS `dispatchEvent()` is never used for this), while
//! `edit_focused_field` writes the new value back through whichever storage
//! model `typeable_field` reports for that control. `handle_ime` is the same
//! job driven by the platform's composition events instead of a key press.
//!
//! The key *dispatch* that decides whether a press is text at all lives next
//! door in [`super::keyboard`].
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour and the
//! method bodies are unchanged; only the module path and the visibility of the
//! methods called from outside this module differ.

use crate::*;

/// What a text-editing default action does to a field's value, applied by
/// [`Lumen::edit_focused_field_at_cursor`] (page) and
/// [`super::frame_text_input::Lumen::edit_focused_frame_field_at_cursor`]
/// (frame) — factored out of the old closure-per-call-site shape (FRAME-7
/// remainder 2) so both can splice an active selection out FIRST and only
/// then decide whether the action still applies (a typed char does; a
/// Backspace/Delete does not — see `edit_focused_field_at_cursor`'s doc
/// comment). `pub(crate)`: shared verbatim by both the page and frame edit
/// paths rather than duplicated.
pub(crate) enum EditAction {
    InsertChar(char),
    Backspace,
    DeleteForward,
}

impl EditAction {
    /// Apply the action against `current` at char-index `cursor` — the same
    /// per-action arithmetic `text_cursor` already provides, used when there
    /// is no active selection to splice out first.
    pub(crate) fn apply(&self, current: &str, cursor: usize) -> (String, usize) {
        match self {
            EditAction::InsertChar(ch) => insert_char_at(current, cursor, *ch),
            EditAction::Backspace => delete_char_before(current, cursor),
            EditAction::DeleteForward => (delete_char_after(current, cursor), cursor),
        }
    }
}

impl Lumen {
    /// Inject a typed character into the focused element (TypeText injection path).
    ///
    /// Inject a special (non-printable) key press: `keydown` в†’ `keyup`.
    ///
    /// `code` is a W3C `KeyboardEvent.code` string, e.g. `"Enter"`, `"Backspace"`.
    /// The matching `KeyboardEvent.key` value is resolved via [`input::native::code_to_key`]
    /// (`"Space"` в†’ `" "`, everything else passes through unchanged).
    /// Events have `isTrusted=true`; JS `dispatchEvent()` is never used.
    pub(crate) fn inject_special_key(&mut self, code: &str) {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        let key = input::native::code_to_key(code);
        // ADR-016 M2.2c-2d (10): keyboard injection вЂ” `_lumen_dispatch_key_event`
        // (keydown в†’ keyup) СѓС…РѕРґРёС‚ fire-and-forget С‡РµСЂРµР· `route_eval_js`, Р°
        // РїРѕСЃР»РµРґСѓСЋС‰РёР№ `take_navigate_request` вЂ” С‡РµСЂРµР· `route_query_js`. РџРѕРґ С„Р»Р°РіРѕРј
        // (`LUMEN_ENGINE_THREAD=1`) Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ РїРѕСЃР»Рµ
        // РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… `task`, РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°СЏ read-after-eval РїРѕСЂСЏРґРѕРє; Р±РµР· С„Р»Р°РіР°
        // (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ (`js_ctx == None`
        // в†’ `route_eval_js` no-op + `route_query_js` в†’ `None`, РєР°Рє РїСЂРµР¶РЅРёР№ early-`return`).
        for event_type in &["keydown", "keyup"] {
            let script = format!(
                "_lumen_dispatch_key_event({}, '{}', '{}', '{}', false, false, false, false)",
                node_id, event_type, key, code,
            );
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
        }
        if let Some(Some(nav)) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_navigate_request(),
        ) {
            self.pending_js_navigate = Some(nav);
        }
    }

    /// Classify `nid` as a mutable text-editing form control and read the value
    /// it currently renders (BUG-436).
    ///
    /// Returns `None` for anything that is not a typeable `<input>` (the
    /// text-like types вЂ” the same set [`InProcessSession::type_text`] accepts)
    /// or a `<textarea>`, and for a control that is `disabled` or `readonly`
    /// (HTML LS В§4.10.19.2 вЂ” such a control is not mutable, so the engine
    /// performs no insertion).
    ///
    /// The value read is the *rendered* one вЂ” [`Document::control_value`], the
    /// control's current value, which is what layout paints and what form
    /// submission collects (BUG-441). The `value` attribute / child text behind
    /// it is only the default the field started from.
    pub(crate) fn typeable_field(&self, nid: lumen_dom::NodeId) -> Option<(TypeableField, String)> {
        let doc = self.layout_source.as_ref()?.document.lock().ok()?;
        let node = doc.get(nid);
        if node.get_attr("disabled").is_some() || node.get_attr("readonly").is_some() {
            return None;
        }
        if node.element_name().is_some_and(|n| n.local.eq_ignore_ascii_case("textarea")) {
            return Some((TypeableField::Textarea, doc.control_value(nid).into_owned()));
        }
        let is_typeable_input = matches!(
            node.input_type(),
            Some(lumen_dom::InputType::Text)
                | Some(lumen_dom::InputType::Password)
                | Some(lumen_dom::InputType::Email)
                | Some(lumen_dom::InputType::Tel)
                | Some(lumen_dom::InputType::Url)
                | Some(lumen_dom::InputType::Number)
                | Some(lumen_dom::InputType::Search)
        );
        if !is_typeable_input {
            return None;
        }
        Some((TypeableField::Input, doc.control_value(nid).into_owned()))
    }

    /// Read (and lazily initialize) the char-index text cursor for `nid`'s
    /// current value (FRAME-2 п.1).
    ///
    /// Defaults to end-of-text on first touch — exactly the append-only
    /// behaviour the field had before cursor tracking existed, so a field
    /// never touched by Left/Right/Home/End keeps typing at the end like it
    /// always did. Clamped to the current value's length on every read: an
    /// external mutation (JS `input.value = …`, spellcheck replace) can shrink
    /// the value out from under a stale cursor.
    fn field_cursor(&mut self, nid: lumen_dom::NodeId, current: &str) -> usize {
        let len = char_len(current);
        let slot = self.form_state.entry(nid).or_default();
        let c = *slot.cursor.get_or_insert(len);
        c.min(len)
    }

    /// FRAME-7: the focused page-level `<input>`'s char-index cursor, if a
    /// caret bar should be painted this frame — `None` when nothing typeable
    /// is focused. Read-only (unlike `field_cursor`): a paint pass must never
    /// write `form_state` as a side effect of building the display list.
    /// Untouched-but-focused reads as end-of-value, mirroring `field_cursor`'s
    /// own default so a field never touched by Left/Right/Home/End still
    /// shows its caret at the end, exactly where typing would land.
    pub(crate) fn focused_input_caret(&self) -> Option<(lumen_dom::NodeId, usize)> {
        let nid = self.focused_node?;
        let (kind, current) = self.typeable_field(nid)?;
        if kind != TypeableField::Input {
            // FRAME-7: a `<textarea>` caret needs multi-line InlineRun
            // line/glyph geometry, not the single-line box math this
            // input-only path assumes — a separate, larger slice.
            return None;
        }
        let len = char_len(&current);
        let cursor = self.form_state.get(&nid).and_then(|s| s.cursor).unwrap_or(len);
        Some((nid, cursor.min(len)))
    }

    /// FRAME-7 (remainder item 1): the focused page-level `<textarea>`'s
    /// char-index cursor and current value, if a caret bar should be painted
    /// this frame — `None` when nothing typeable/textarea is focused.
    /// Read-only, like `focused_input_caret`. Kept as a separate query rather
    /// than folding into `focused_input_caret`: the two paint through
    /// entirely different mechanisms (`CompositorOverride` vs. a shell-side
    /// overlay — see `redraw_requested.rs` and `forms::textarea_caret_rect`),
    /// so a caller must already know which one it wants.
    pub(crate) fn focused_textarea_caret(&self) -> Option<(lumen_dom::NodeId, usize, String)> {
        let nid = self.focused_node?;
        let (kind, current) = self.typeable_field(nid)?;
        if kind != TypeableField::Textarea {
            return None;
        }
        let len = char_len(&current);
        let cursor = self.form_state.get(&nid).and_then(|s| s.cursor).unwrap_or(len);
        Some((nid, cursor.min(len), current))
    }

    /// Char-index selection range for `nid`, normalized (`start <= end`) —
    /// `None` when no selection is active (no anchor, or anchor == cursor).
    /// Shared by cursor movement (collapse-to-edge) and painting queries.
    fn field_selection_range(&self, nid: lumen_dom::NodeId, cursor: usize) -> Option<(usize, usize)> {
        let anchor = self.form_state.get(&nid)?.selection_anchor?;
        if anchor == cursor {
            return None;
        }
        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    /// Move the focused field's text cursor by `delta` chars (Left = `-1`,
    /// Right = `+1`), clamped to `[0, value length]`. `true` iff a typeable
    /// field was focused (regardless of whether the cursor was already at the
    /// clamped edge).
    ///
    /// FRAME-7 remainder 2: with an active selection, an unshifted Left/Right
    /// collapses the cursor to the near edge of the selection (the OS-wide
    /// convention) instead of moving `delta` chars from wherever the cursor
    /// itself sat — matching what every other multi-line/single-line text
    /// editor does, and clears the selection either way.
    pub(crate) fn move_focused_cursor(&mut self, delta: i32) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((_, current)) = self.typeable_field(nid) else { return false };
        let cursor = self.field_cursor(nid, &current);
        let next = match self.field_selection_range(nid, cursor) {
            Some((start, end)) => if delta < 0 { start } else { end },
            None => {
                let len = char_len(&current) as i32;
                (cursor as i32 + delta).clamp(0, len) as usize
            }
        };
        let slot = self.form_state.entry(nid).or_default();
        slot.cursor = Some(next);
        slot.selection_anchor = None;
        true
    }

    /// Home (`to_start = true`) / End (`to_start = false`) вЂ” jump the focused
    /// field's text cursor to the start or end of the value, clearing any
    /// active selection (FRAME-7 remainder 2).
    pub(crate) fn jump_focused_cursor(&mut self, to_start: bool) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((_, current)) = self.typeable_field(nid) else { return false };
        let target = if to_start { 0 } else { char_len(&current) };
        let slot = self.form_state.entry(nid).or_default();
        slot.cursor = Some(target);
        slot.selection_anchor = None;
        true
    }

    /// Shift+Left/Right (FRAME-7 remainder 2): extend the focused field's
    /// selection by `delta` chars. The anchor is pinned at the cursor's
    /// position the FIRST time a selection starts (an already-active
    /// selection keeps its anchor — only the moving end, the cursor,
    /// changes), mirroring how every text editor's Shift+arrow works.
    pub(crate) fn extend_focused_selection(&mut self, delta: i32) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((_, current)) = self.typeable_field(nid) else { return false };
        let cursor = self.field_cursor(nid, &current);
        let len = char_len(&current) as i32;
        let next = (cursor as i32 + delta).clamp(0, len) as usize;
        let slot = self.form_state.entry(nid).or_default();
        if slot.selection_anchor.is_none() {
            slot.selection_anchor = Some(cursor);
        }
        slot.cursor = Some(next);
        true
    }

    /// Shift+Home (`to_start = true`) / Shift+End (`to_start = false`)
    /// (FRAME-7 remainder 2): extend the focused field's selection to the
    /// start or end of the value. Same anchor-pinning rule as
    /// [`Self::extend_focused_selection`].
    pub(crate) fn extend_focused_selection_to_edge(&mut self, to_start: bool) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((_, current)) = self.typeable_field(nid) else { return false };
        let cursor = self.field_cursor(nid, &current);
        let target = if to_start { 0 } else { char_len(&current) };
        let slot = self.form_state.entry(nid).or_default();
        if slot.selection_anchor.is_none() {
            slot.selection_anchor = Some(cursor);
        }
        slot.cursor = Some(target);
        true
    }

    /// FRAME-7 remainder 2: the focused page-level `<input>`'s selection
    /// range, if one should be painted this frame — `None` when nothing
    /// typeable is focused or no selection is active. Read-only, like
    /// [`Self::focused_input_caret`] — a paint pass must never mutate
    /// `form_state`. Returned bounds are normalized (`start <= end`) and
    /// clamped to the current value's length, same defensive clamp
    /// `focused_input_caret` applies (an external mutation can shrink the
    /// value out from under a stale selection).
    pub(crate) fn focused_input_selection(&self) -> Option<(lumen_dom::NodeId, usize, usize)> {
        let nid = self.focused_node?;
        let (kind, current) = self.typeable_field(nid)?;
        if kind != TypeableField::Input {
            return None;
        }
        let len = char_len(&current);
        let slot = self.form_state.get(&nid)?;
        let anchor = slot.selection_anchor?.min(len);
        let cursor = slot.cursor.unwrap_or(len).min(len);
        if anchor == cursor {
            return None;
        }
        Some((nid, anchor.min(cursor), anchor.max(cursor)))
    }

    /// FRAME-7 remainder 2: the focused page-level `<textarea>`'s selection
    /// range and current value, if one should be painted this frame —
    /// mirror of [`Self::focused_input_selection`], kept separate for the
    /// same reason [`Self::focused_textarea_caret`] is separate from
    /// [`Self::focused_input_caret`] (different paint mechanisms).
    pub(crate) fn focused_textarea_selection(&self) -> Option<(lumen_dom::NodeId, usize, usize, String)> {
        let nid = self.focused_node?;
        let (kind, current) = self.typeable_field(nid)?;
        if kind != TypeableField::Textarea {
            return None;
        }
        let len = char_len(&current);
        let slot = self.form_state.get(&nid)?;
        let anchor = slot.selection_anchor?.min(len);
        let cursor = slot.cursor.unwrap_or(len).min(len);
        if anchor == cursor {
            return None;
        }
        Some((nid, anchor.min(cursor), anchor.max(cursor), current))
    }

    /// Engine-side text-editing default action on the focused form control
    /// (BUG-436): `edit` maps the field's current value + cursor position to
    /// its new value + cursor position (FRAME-2 п.1 вЂ” insertion/deletion at
    /// the tracked cursor, not always at the end of the value).
    ///
    /// The JS shim only *dispatches* `keydown`/`input`/`keyup`; changing the
    /// control's value is the engine's own default action (HTML LS В§4.10.5.5),
    /// exactly as [`InProcessSession::dispatch_type`] does for the headless
    /// driver. Without it the live window fired `input` events on a field that
    /// never changed вЂ” `type` reported success, `input.value` stayed `""` and
    /// the field rendered empty.
    ///
    /// Returns `true` when a mutable field consumed the edit. The DOM mutation
    /// happens with the document lock held and no JS dispatched under it (the
    /// deadlock trap found in BUG-437); the JS-side value shadow is synced
    /// afterwards so a listener reading `this.value` sees the new value.
    ///
    /// FRAME-7 remainder 2: an active selection is spliced OUT of the value
    /// first — a typed char replaces it (OS-wide "typing over a selection"
    /// convention), a Backspace/Delete simply removes it (the selection IS
    /// the thing to delete; it does not also delete one more char beyond the
    /// range). Either way the selection is consumed, matching every other
    /// text editor.
    fn edit_focused_field_at_cursor(&mut self, action: EditAction) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((kind, current)) = self.typeable_field(nid) else { return false };
        let cursor = self.field_cursor(nid, &current);
        let (next, next_cursor) = match self.field_selection_range(nid, cursor) {
            Some((start, end)) => {
                let spliced = delete_char_range(&current, start, end);
                match action {
                    EditAction::InsertChar(ch) => insert_char_at(&spliced, start, ch),
                    EditAction::Backspace | EditAction::DeleteForward => (spliced, start),
                }
            }
            None => action.apply(&current, cursor),
        };
        let slot = self.form_state.entry(nid).or_default();
        slot.cursor = Some(next_cursor);
        slot.selection_anchor = None;
        if next == current {
            return true;
        }
        if let Some(src) = self.layout_source.as_mut()
            && let Ok(mut doc) = src.document.lock()
        {
            match kind {
                TypeableField::Input => forms::set_value(&mut doc, nid, &next),
                TypeableField::Textarea => forms::set_textarea_text(&mut doc, nid, &next),
            }
        }
        // Runtime value overlay used by form submission and constraint
        // validation (`forms::collect_form_entries`) вЂ” kept in step with the DOM
        // exactly like the spellcheck-replace path does.
        self.form_state.entry(nid).or_default().value = next.clone();
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!("_lumen_set_field_value({}, '{}')", nid.index(), escape_js_string(&next)),
        );
        self.relayout_form();
        true
    }

    /// Fires `keydown` в†’ `input` в†’ `keyup` JS events via `_lumen_dispatch_key_event`
    /// on the last-focused node so events have `isTrusted=true`.
    ///
    /// Between `keydown` and `input` the engine runs its own text-insertion
    /// default action ([`Self::edit_focused_field`], BUG-436), so an `input`
    /// listener reading `this.value` observes the character just typed.
    /// Returns `true` when a form control accepted the character.
    pub(crate) fn inject_char(&mut self, ch: char) -> bool {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        let key = escape_js_string_char(ch);
        // ADR-016 M2.2c-2d (10): same read-after-eval routing as `inject_special_key`
        // вЂ” keydown в†’ input в†’ keyup dispatch off-UI-thread under the flag, then the
        // `take_navigate_request` read ordered after via `route_query_js`; byte-identical
        // off-flag.
        self.dispatch_injected_key(node_id, "keydown", &key);
        let consumed = self.edit_focused_field_at_cursor(EditAction::InsertChar(ch));
        for event_type in &["input", "keyup"] {
            self.dispatch_injected_key(node_id, event_type, &key);
        }
        if let Some(Some(nav)) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_navigate_request(),
        ) {
            self.pending_js_navigate = Some(nav);
        }
        consumed
    }

    /// Backspace on the focused form control: `keydown` в†’ engine deletes the
    /// char before the cursor ([`Self::edit_focused_field_at_cursor`]) в†’
    /// `input` в†’ `keyup`.
    ///
    /// The counterpart of [`Self::inject_char`] вЂ” without it a field could be
    /// filled but never corrected. Returns `true` when a form control consumed
    /// the key.
    pub(crate) fn inject_backspace(&mut self) -> bool {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        self.dispatch_injected_key(node_id, "keydown", "Backspace");
        let consumed = self.edit_focused_field_at_cursor(EditAction::Backspace);
        for event_type in &["input", "keyup"] {
            self.dispatch_injected_key(node_id, event_type, "Backspace");
        }
        consumed
    }

    /// Delete (forward-delete) on the focused form control: `keydown` в†’
    /// engine deletes the char after the cursor
    /// ([`Self::edit_focused_field_at_cursor`]) в†’ `input` в†’ `keyup`.
    ///
    /// Previously missing entirely вЂ” only Backspace existed вЂ” because there
    /// was no cursor position for "after" to mean anything relative to.
    /// Returns `true` when a form control consumed the key.
    pub(crate) fn inject_delete_forward(&mut self) -> bool {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        self.dispatch_injected_key(node_id, "keydown", "Delete");
        let consumed = self.edit_focused_field_at_cursor(EditAction::DeleteForward);
        for event_type in &["input", "keyup"] {
            self.dispatch_injected_key(node_id, event_type, "Delete");
        }
        consumed
    }

    /// Send one `_lumen_dispatch_key_event` for an injected/typed key.
    ///
    /// `key` must already be escaped for a single-quoted JS literal
    /// ([`escape_js_string_char`]).
    fn dispatch_injected_key(&mut self, node_id: usize, event_type: &str, key: &str) {
        let script = format!(
            "_lumen_dispatch_key_event({}, '{}', '{}', '{}', false, false, false, false)",
            node_id, event_type, key, key,
        );
        route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
    }

    pub(crate) fn handle_ime(&mut self, ime: &Ime) {
        use lumen_core::event::{Event, TabId};
        let tab_id = TabId(0);
        match ime {
            Ime::Enabled => {
                // РќРµ РґРёСЃРїР°С‚С‡РёРј compositionstart СЃСЂР°Р·Сѓ вЂ” Р¶РґС‘Рј РїРµСЂРІС‹Р№ Preedit
                // СЃ С‚РµРєСЃС‚РѕРј (Р±СЂР°СѓР·РµСЂС‹ С‚Р°Рє Р¶Рµ: СЃРѕР±С‹С‚РёРµ С‚РѕР»СЊРєРѕ РєРѕРіРґР° РµСЃС‚СЊ РґР°РЅРЅС‹Рµ).
            }
            Ime::Preedit(text, _cursor) if text.is_empty() => {
                // РџСѓСЃС‚РѕР№ preedit = РєРѕРЅРµС† composition Р±РµР· Commit (РѕС‚РјРµРЅР°).
                if self.ime_composing.take().is_some() {
                    self.event_sink
                        .emit(&Event::ImeCompositionEnded { tab_id, data: String::new() });
                }
            }
            Ime::Preedit(text, _cursor) => {
                if self.ime_composing.is_none() {
                    // РџРµСЂРІС‹Р№ РЅРµРїСѓСЃС‚РѕР№ preedit вЂ” РЅР°С‡Р°Р»Рѕ composition.
                    self.event_sink
                        .emit(&Event::ImeCompositionStarted { tab_id });
                }
                self.ime_composing = Some(text.clone());
                self.event_sink.emit(&Event::ImeCompositionUpdated {
                    tab_id,
                    data: text.clone(),
                });
            }
            Ime::Commit(text) => {
                // Commit РїСЂРёС…РѕРґРёС‚ РїРѕСЃР»Рµ РїСѓСЃС‚РѕРіРѕ Preedit (winit РіР°СЂР°РЅС‚РёСЂСѓРµС‚),
                // РЅРѕ РЅР° СЃР»СѓС‡Р°Р№ РµСЃР»Рё РЅРµС‚ вЂ” СЃР±СЂР°СЃС‹РІР°РµРј composing СЃР°РјРё.
                self.ime_composing = None;
                self.event_sink.emit(&Event::ImeCompositionEnded {
                    tab_id,
                    data: text.clone(),
                });
            }
            Ime::Disabled => {
                // IME РґРµР°РєС‚РёРІРёСЂРѕРІР°РЅ. Р•СЃР»Рё composition Р±С‹Р»Р° РѕС‚РєСЂС‹С‚Р° вЂ” Р·Р°РєСЂС‹РІР°РµРј.
                if self.ime_composing.take().is_some() {
                    self.event_sink
                        .emit(&Event::ImeCompositionEnded { tab_id, data: String::new() });
                }
            }
        }
    }

}
