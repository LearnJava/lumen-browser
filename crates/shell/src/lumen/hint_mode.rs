//! Link-hint mode: typing a hint label to activate an element without the
//! mouse.
//!
//! `crate::hints` assigns the labels and filters them as characters arrive;
//! `activate_node` is the other half - it does to a node what a real click
//! would (dispatch the JS `click`, run the form action of a checkbox or radio,
//! follow an enclosing `<a href>`), which is why it takes a `NodeId` rather
//! than a point and is the only caller-visible entry point of the pair.

use crate::*;

impl Lumen {
    /// РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚ РєР»Р°РІРёС€РЅС‹Р№ РІРІРѕРґ РїРѕРєР° hint-СЂРµР¶РёРј Р°РєС‚РёРІРµРЅ.
    ///
    /// `Escape` вЂ” Р·Р°РєСЂС‹С‚СЊ overlay. Р›СЋР±РѕР№ РѕРґРёРЅРѕС‡РЅС‹Р№ СЃРёРјРІРѕР» (СЃС‚СЂРѕС‡РЅС‹Р№ ASCII) вЂ”
    /// РїРµСЂРµРґР°С‘С‚СЃСЏ РІ `HintState::push_char`; РїСЂРё СѓРЅРёРєР°Р»СЊРЅРѕРј СЃРѕРІРїР°РґРµРЅРёРё РІС‹Р·С‹РІР°РµС‚СЃСЏ
    /// `activate_node`. РќРµСЂР°СЃРїРѕР·РЅР°РЅРЅС‹Рµ РєР»Р°РІРёС€Рё РёРіРЅРѕСЂРёСЂСѓСЋС‚СЃСЏ.
    pub(crate) fn handle_hint_key(&mut self, code: KeyCode, key_event: &KeyEvent) {
        if matches!(code, KeyCode::Escape) && !key_event.repeat {
            self.hint.close();
            self.request_redraw();
            return;
        }
        if let Some(text) = key_event.text.as_ref() {
            for c in text.chars() {
                if c.is_ascii_lowercase() {
                    match self.hint.push_char(c) {
                        hints::HintResult::Activate(node_id) => {
                            self.activate_node(node_id);
                        }
                        hints::HintResult::Partial | hints::HintResult::NoMatch => {}
                    }
                    self.request_redraw();
                    break;
                }
            }
        }
    }

    /// РђРєС‚РёРІРёСЂРѕРІР°С‚СЊ DOM-СѓР·РµР» `node_id` РєР°Рє Р±СѓРґС‚Рѕ РїРѕ РЅРµРјСѓ РєР»РёРєРЅСѓР»Рё РјС‹С€СЊСЋ.
    ///
    /// Р”РёСЃРїР°С‚С‡РёС‚ JS click-СЃРѕР±С‹С‚РёРµ, РѕР±СЂР°Р±Р°С‚С‹РІР°РµС‚ form-РґРµР№СЃС‚РІРёРµ (checkbox/radio),
    /// Рё РЅР°РІРёРіРёСЂСѓРµС‚ РїРѕ СЃСЃС‹Р»РєРµ РµСЃР»Рё СѓР·РµР» РІРЅСѓС‚СЂРё `<a href>`. РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ
    /// hint-СЂРµР¶РёРјРѕРј РґР»СЏ Р°РєС‚РёРІР°С†РёРё СЌР»РµРјРµРЅС‚Р° Р±РµР· СѓС‡Р°СЃС‚РёСЏ РјС‹С€Рё.
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    fn activate_node(&mut self, node_id: NodeId) {
        // JS click dispatch (bubbling РѕС‚ СѓР·Р»Р° РґРѕ document).
        // Hint-mode activations have no real mouse coordinates, so x/y are 0.
        // ADR-016 M2.2c-2d (10): same read-after-eval routing as the mouse click
        // dispatch вЂ” `_lumen_dispatch_mouse_event('click', вЂ¦)` fire-and-forget via
        // `route_eval_js`, then `take_navigate_request` ordered after via
        // `route_query_js`; byte-identical off-flag.
        #[cfg(feature = "v8")]
        {
            let script = format!(
                "_lumen_dispatch_mouse_event({}, 'click', 0, 0, 0, 1, 0)",
                node_id.index()
            );
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
            if let Some(Some(nav)) = route_query_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                |j| j.take_navigate_request(),
            ) {
                self.pending_js_navigate = Some(nav);
            }
        }
        // Form action classification.
        let form_action = if let Some(src) = self.layout_source.as_ref() {
            forms::classify_click(&src.document.lock().unwrap(), node_id)
        } else {
            forms::FormClickAction::Nothing
        };
        match form_action {
            forms::FormClickAction::ToggleCheckbox(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), id);
                }
                // ADR-016 M2.2c-3 (2): async-safe form-control DOM mutation (Bucket
                // A) вЂ” no synchronous geometry read after, route off-thread when
                // `LUMEN_ENGINE_THREAD=1`, byte-identical otherwise.
                self.relayout_form();
            }
            forms::FormClickAction::ToggleRadio { clicked, .. } => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                }
                // ADR-016 M2.2c-3 (2): async-safe form-control DOM mutation (Bucket A).
                self.relayout_form();
            }
            forms::FormClickAction::OpenColorPicker(id) => {
                self.color_picker_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenDatePicker(id) => {
                let (y, m) = self.layout_source.as_ref()
                    .and_then(|src| {
                        let doc = src.document.lock().ok()?;
                        let val = doc.control_value(id).into_owned();
                        forms::parse_date_value(&val).map(|(y, m, _)| (y, m))
                    })
                    .unwrap_or_else(forms::today_year_month);
                self.date_picker_node = Some(id);
                self.date_picker_year = y;
                self.date_picker_month = m;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenSelectDropdown(id) => {
                self.select_dropdown_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenFilePicker(id) => {
                self.open_file_picker(id);
            }
            forms::FormClickAction::ToggleDetails(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_details_open(&mut src.document.lock().unwrap(), id);
                }
                // ADR-016 M2.2c-2d: fire-and-forget `toggle` event С‡РµСЂРµР·
                // РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
                #[cfg(feature = "v8")]
                route_eval_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    format!(
                        "_lumen_make_element({}).dispatchEvent(new Event('toggle'))",
                        id.index()
                    ),
                );
                // ADR-016 M2.2c-3 (2): async-safe `<details>` open toggle (Bucket A);
                // the `toggle` event above is independent of the layout job.
                self.relayout_form();
            }
            // Range slide via keyboard activation: no-op (no position known).
            forms::FormClickAction::SlideRange(_) => {}
            forms::FormClickAction::SubmitForm(_) | forms::FormClickAction::Nothing => {
                // Link navigation.
                let href = self.layout_source.as_ref().and_then(|src| {
                    links::find_link_href(&src.document.lock().unwrap(), node_id)
                });
                if let Some(href) = href {
                    if let Some(frag) = links::fragment_only(&href) {
                        self.navigate_fragment(frag.to_owned());
                    } else if links::is_navigable_href(&href) {
                        let resolved = self.source.resolve_href(&href);
                        if let Some(action) = newtab::parse_action(&resolved) {
                            self.apply_newtab_action(action);
                        } else if let Some(frag) =
                            links::same_document_fragment(self.current_display_url(), &resolved)
                        {
                            self.navigate_fragment(frag);
                        } else {
                            self.navigate_to(PageSource::from_arg(Some(&resolved)));
                        }
                    }
                }
            }
        }
    }
}
