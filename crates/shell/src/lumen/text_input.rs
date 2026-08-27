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

    /// Engine-side text-editing default action on the focused form control
    /// (BUG-436): `edit` maps the field's current value to its new value.
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
    fn edit_focused_field(&mut self, edit: impl FnOnce(&str) -> String) -> bool {
        let Some(nid) = self.focused_node else { return false };
        let Some((kind, current)) = self.typeable_field(nid) else { return false };
        let next = edit(&current);
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
        let consumed = self.edit_focused_field(|current| {
            let mut next = current.to_owned();
            next.push(ch);
            next
        });
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
    /// last character ([`Self::edit_focused_field`]) в†’ `input` в†’ `keyup`.
    ///
    /// The counterpart of [`Self::inject_char`] вЂ” without it a field could be
    /// filled but never corrected. Returns `true` when a form control consumed
    /// the key.
    pub(crate) fn inject_backspace(&mut self) -> bool {
        let node_id = self.focused_node.map(|n| n.index()).unwrap_or(0);
        self.dispatch_injected_key(node_id, "keydown", "Backspace");
        let consumed = self.edit_focused_field(|current| {
            let mut next = current.to_owned();
            next.pop();
            next
        });
        for event_type in &["input", "keyup"] {
            self.dispatch_injected_key(node_id, event_type, "Backspace");
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
