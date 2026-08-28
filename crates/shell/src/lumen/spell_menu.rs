//! The spelling context menu over an editable field: deciding whether a
//! right-click landed on a misspelled word, building the suggestion list and
//! applying whatever the user picked.
//!
//! The dictionary and the suggestion engine are `crate::spellcheck`; the menu
//! widget is `crate::page_context_menu`. What is here is the part that needs
//! the live document - mapping a click point onto a field and a word range,
//! and writing the replacement back through the same edit path a keystroke
//! uses.

use crate::*;

impl Lumen {
    /// P3-spell СЃСЂРµР· 2+3+4: РґР»СЏ СѓР·Р»Р° `nid` РїРѕРґ С„РѕРєСѓСЃРѕРј РѕРїСЂРµРґРµР»СЏРµС‚ С†РµР»СЊ
    /// СЃРїРµР»Р»-С‡РµРєР°. Р’РѕР·РІСЂР°С‰Р°РµС‚ `(target_node, placeholder, kind)`:
    /// * `<textarea>` РёР»Рё `<input>` С‚РµРєСЃС‚РѕРІРѕРіРѕ С‚РёРїР° (password РёСЃРєР»СЋС‡С‘РЅ) вЂ” СЃР°Рј
    ///   СѓР·РµР», РµРіРѕ `placeholder` (РїСѓСЃС‚Р°СЏ СЃС‚СЂРѕРєР° РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІРёРё) Рё
    ///   СЃРѕРѕС‚РІРµС‚СЃС‚РІСѓСЋС‰РёР№ [`page_context_menu::SpellTargetKind`];
    /// * СѓР·РµР» РІРЅСѓС‚СЂРё `contenteditable` вЂ” СЂРµРґР°РєС‚РёСЂСѓСЋС‰РёР№ С…РѕСЃС‚, РїСѓСЃС‚РѕР№
    ///   placeholder (Сѓ contenteditable РЅРµС‚ placeholder-Р°С‚СЂРёР±СѓС‚Р°) Рё
    ///   `ContentEditable`;
    /// * РёРЅР°С‡Рµ вЂ” `None`.
    pub(crate) fn spell_target(
        &self,
        nid: lumen_dom::NodeId,
    ) -> Option<(lumen_dom::NodeId, String, page_context_menu::SpellTargetKind)> {
        use page_context_menu::SpellTargetKind;
        let ls = self.layout_source.as_ref()?;
        let doc = ls.document.lock().ok()?;
        let node = doc.get(nid);
        if let Some(name) = node.element_name() {
            let is_textarea = name.local.eq_ignore_ascii_case("textarea");
            let is_text_input = name.local.eq_ignore_ascii_case("input")
                && matches!(
                    node.get_attr("type")
                        .unwrap_or("text")
                        .to_ascii_lowercase()
                        .as_str(),
                    "text" | "search" | "email" | "url"
                );
            if is_textarea || is_text_input {
                let placeholder = node.get_attr("placeholder").unwrap_or_default().to_owned();
                let kind = if is_textarea { SpellTargetKind::Textarea } else { SpellTargetKind::Input };
                return Some((nid, placeholder, kind));
            }
        }
        // contenteditable: check the DOM directly for an editing host.
        lumen_dom::find_editing_host(&doc, nid)
            .map(|host| (host, String::new(), SpellTargetKind::ContentEditable))
    }

    /// P3-spell СЃСЂРµР· 3: СЃР»РѕРІР°, РєРѕС‚РѕСЂС‹Рµ РЅРµ СЃС‡РёС‚Р°СЋС‚СЃСЏ РѕС€РёР±РѕС‡РЅС‹РјРё РїРѕРјРёРјРѕ СЃР»РѕРІР°СЂРµР№ вЂ”
    /// РѕР±СЉРµРґРёРЅРµРЅРёРµ РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРѕРіРѕ СЃР»РѕРІР°СЂСЏ Рё В«РџСЂРѕРїСѓС‰РµРЅРЅС‹С…В» РЅР° СЃРµСЃСЃРёСЋ. Р’СЃРµ
    /// СЃР»РѕРІР° СѓР¶Рµ РІ lowercase.
    pub(crate) fn spell_allow_set(&self) -> std::collections::HashSet<String> {
        self.spell_user_words
            .iter()
            .chain(self.spell_ignored.iter())
            .cloned()
            .collect()
    }

    /// P3-spell СЃСЂРµР· 4: РїРѕР»РЅС‹Р№ Р»РѕРіРёС‡РµСЃРєРёР№ С‚РµРєСЃС‚ РїРѕР»СЏ `target_node` вЂ”
    /// `value`-Р°С‚СЂРёР±СѓС‚ РґР»СЏ `<input>`, Р»РёР±Рѕ (РґР»СЏ `<textarea>`/contenteditable)
    /// РєРѕРЅРєР°С‚РµРЅР°С†РёСЏ С‚РµРєСЃС‚РѕРІС‹С… СѓР·Р»РѕРІ-РїРѕС‚РѕРјРєРѕРІ (`lumen_dom::node_text_content`).
    /// РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РєР°Рє Р±Р°Р·Р° РґР»СЏ РіР»РѕР±Р°Р»СЊРЅС‹С… byte-СЃРјРµС‰РµРЅРёР№ СЃР»РѕРІР° РІ
    /// [`page_context_menu::SpellTarget`], РІ РѕС‚Р»РёС‡РёРµ РѕС‚ С‚РµРєСЃС‚Р° РѕРґРЅРѕР№
    /// РІРёР·СѓР°Р»СЊРЅРѕР№ (wrapped) СЃС‚СЂРѕРєРё.
    fn spell_field_full_text(
        &self,
        target_node: lumen_dom::NodeId,
        kind: page_context_menu::SpellTargetKind,
    ) -> String {
        use page_context_menu::SpellTargetKind;
        let Some(ls) = self.layout_source.as_ref() else { return String::new() };
        let Ok(doc) = ls.document.lock() else { return String::new() };
        match kind {
            // BUG-441: Сѓ `<input>`/`<textarea>` РїСЂРѕРІРµСЂСЏРµРј С‚Рѕ, С‡С‚Рѕ РІ РїРѕР»Рµ СЃРµР№С‡Р°СЃ
            // (runtime-Р·РЅР°С‡РµРЅРёРµ), Р° РЅРµ РґРµС„РѕР»С‚ РёР· СЂР°Р·РјРµС‚РєРё. `contenteditable`
            // СЂРµРґР°РєС‚РёСЂСѓРµС‚СЃСЏ РїСЂСЏРјРѕ РІ DOM, РїРѕСЌС‚РѕРјСѓ С‚Р°Рј РїРѕ-РїСЂРµР¶РЅРµРјСѓ С‚РµРєСЃС‚ СѓР·Р»РѕРІ.
            SpellTargetKind::Input | SpellTargetKind::Textarea => {
                doc.control_value(target_node).into_owned()
            }
            SpellTargetKind::ContentEditable => node_text_content(&doc, target_node),
        }
    }

    /// P3-spell СЃСЂРµР· 3+4: РїСЂРё right-click РїРѕ РѕС€РёР±РѕС‡РЅРѕРјСѓ СЃР»РѕРІСѓ РІ С„РѕРєСѓСЃРЅРѕРј
    /// `<input>`/`<textarea>`/contenteditable РѕС‚РєСЂС‹РІР°РµС‚ РјРµРЅСЋ РїРѕРґСЃРєР°Р·РѕРє.
    /// Р’РѕР·РІСЂР°С‰Р°РµС‚ `true`, РµСЃР»Рё РјРµРЅСЋ РѕС‚РєСЂС‹С‚Рѕ (РєР»РёРє РѕР±СЂР°Р±РѕС‚Р°РЅ), РёРЅР°С‡Рµ `false`
    /// (РєР»РёРє РёРґС‘С‚ РґР°Р»СЊС€Рµ вЂ” Р¶РµСЃС‚).
    ///
    /// РњРЅРѕРіРѕСЃС‚СЂРѕС‡РЅС‹Рµ РїРѕР»СЏ СЂРёСЃСѓСЋС‚ РѕРґРЅСѓ `DrawText`-РєРѕРјР°РЅРґСѓ РЅР° РІРёР·СѓР°Р»СЊРЅСѓСЋ
    /// (wrapped) СЃС‚СЂРѕРєСѓ вЂ” Р±Р°Р№С‚РѕРІРѕРµ СЃРјРµС‰РµРЅРёРµ СЃР»РѕРІР° РІРЅСѓС‚СЂРё РєР»РёРєР° РЅР°Р№РґРµРЅРЅРѕР№
    /// СЃС‚СЂРѕРєРё СЃР°РјРѕ РїРѕ СЃРµР±Рµ Р±РµСЃСЃРјС‹СЃР»РµРЅРЅРѕ Р·Р° РїСЂРµРґРµР»Р°РјРё РїРµСЂРІРѕР№ СЃС‚СЂРѕРєРё.
    /// `spellcheck::locate_line_word_in_full_text` РїРµСЂРµСЃС‡РёС‚С‹РІР°РµС‚ РµРіРѕ РІ
    /// РіР»РѕР±Р°Р»СЊРЅРѕРµ СЃРјРµС‰РµРЅРёРµ РІРЅСѓС‚СЂРё РїРѕР»РЅРѕРіРѕ Р·РЅР°С‡РµРЅРёСЏ РїРѕР»СЏ, РёСЃРїРѕР»СЊР·СѓСЏ
    /// РїСЂРµРґС€РµСЃС‚РІСѓСЋС‰РёРµ СЃС‚СЂРѕРєРё С‚РѕРіРѕ Р¶Рµ РїРѕР»СЏ РєР°Рє СЏРєРѕСЂСЏ.
    pub(crate) fn try_open_spell_menu(&mut self, x_css: f32, y_css: f32) -> bool {
        use lumen_core::ext::SpellChecker;
        let Some(dicts) = SPELL_DICTS.get() else { return false };
        if dicts.is_empty() {
            return false;
        }
        let Some(nid) = self.focused_node else { return false };
        let Some((target_node, _placeholder, kind)) = self.spell_target(nid) else { return false };
        let Some(node_lb) = self
            .layout_box
            .as_ref()
            .and_then(|lb| forms::find_layout_box(lb, target_node))
        else {
            return false;
        };
        let node_rect = node_lb.rect;
        let (page_x, page_y) = self.page_point(x_css, y_css);
        if page_x < node_rect.x
            || page_y < node_rect.y
            || page_x >= node_rect.x + node_rect.width
            || page_y >= node_rect.y + node_rect.height
        {
            return false;
        }
        let Ok(font) = lumen_font::Font::parse(INTER_FONT) else { return false };
        let Ok(m) = lumen_paint::FontMeasurer::new(&font) else { return false };
        let allow = self.spell_allow_set();

        // Walk this field's rendered lines in document order, remembering
        // every line before the one under the cursor (needed to resolve the
        // clicked word's global offset for multi-line fields) and stopping at
        // the first hit. Collected up front (immutable borrow of
        // `display_list`) so the mutable `open_for` call below doesn't overlap.
        let hit: Option<(Vec<String>, page_context_menu::SpellTarget)> = {
            let mut prior_lines: Vec<String> = Vec::new();
            let mut found = None;
            for cmd in &self.display_list {
                let lumen_paint::DisplayCommand::DrawText { rect, text, font_size, .. } = cmd
                else {
                    continue;
                };
                if rect.x < node_rect.x
                    || rect.y < node_rect.y
                    || rect.x >= node_rect.x + node_rect.width
                    || rect.y >= node_rect.y + node_rect.height
                {
                    continue;
                }
                let hits_point = page_x >= rect.x
                    && page_x < rect.x + rect.width
                    && page_y >= rect.y
                    && page_y < rect.y + rect.height;
                if !hits_point {
                    prior_lines.push(text.clone());
                    continue;
                }
                let fs = *font_size;
                let measure = |s: &str| -> f32 {
                    use lumen_layout::TextMeasurer;
                    s.chars().map(|c| m.char_width(c, fs)).sum()
                };
                let Some((s, e)) = spellcheck::word_at_x(text, page_x - rect.x, &measure) else {
                    prior_lines.push(text.clone());
                    continue;
                };
                let word = &text[s..e];
                if dicts.check(word) || allow.contains(&word.to_lowercase()) {
                    // Word under cursor is spelled correctly вЂ” no menu.
                    return false;
                }

                let full_text = match kind {
                    page_context_menu::SpellTargetKind::Input => text.clone(),
                    _ => self.spell_field_full_text(target_node, kind),
                };
                let Some((global_start, global_end)) = spellcheck::locate_line_word_in_full_text(
                    &full_text,
                    &prior_lines,
                    text,
                    s,
                    e,
                ) else {
                    return false;
                };
                let word = full_text[global_start..global_end].to_owned();
                let suggestions = dicts.suggest(&word);
                found = Some((
                    suggestions,
                    page_context_menu::SpellTarget {
                        node: target_node,
                        text: full_text,
                        word_start: global_start,
                        word_end: global_end,
                        kind,
                    },
                ));
                break;
            }
            found
        };

        match hit {
            Some((suggestions, target)) => {
                self.page_context_menu.open_for(x_css, y_css, suggestions, target);
                true
            }
            None => false,
        }
    }

    /// P3-spell СЃСЂРµР· 3+4: РїСЂРёРјРµРЅСЏРµС‚ РІС‹Р±СЂР°РЅРЅРѕРµ РґРµР№СЃС‚РІРёРµ РјРµРЅСЋ РїРѕРґСЃРєР°Р·РѕРє.
    /// `Use` Р·Р°РјРµРЅСЏРµС‚ СЃР»РѕРІРѕ Рё РїРµСЂРµРІС‘СЂСЃС‚С‹РІР°РµС‚ вЂ” РґР»СЏ `<input>`/`<textarea>`
    /// РїРµСЂРµСЃС‚СЂР°РёРІР°СЏ РїРѕР»РЅРѕРµ Р·РЅР°С‡РµРЅРёРµ С‡РµСЂРµР· `target.apply()`; РґР»СЏ
    /// contenteditable С‚РѕС‡РµС‡РЅРѕ РїСЂР°РІСЏ С‚РѕР»СЊРєРѕ С‚РµРєСЃС‚РѕРІС‹Р№ СѓР·РµР», СЃРѕРґРµСЂР¶Р°С‰РёР№ СЃР»РѕРІРѕ
    /// (`lumen_dom::locate_text_offset_range` + `delete_range`/`insert_text_at`),
    /// РЅРµ С‚СЂРѕРіР°СЏ РѕСЃС‚Р°Р»СЊРЅСѓСЋ rich-text СЃС‚СЂСѓРєС‚СѓСЂСѓ. `AddToDict` РґРѕР±Р°РІР»СЏРµС‚ СЃР»РѕРІРѕ РІ
    /// РїРѕР»СЊР·РѕРІР°С‚РµР»СЊСЃРєРёР№ СЃР»РѕРІР°СЂСЊ (С„Р°Р№Р» + РїР°РјСЏС‚СЊ); `Ignore` РґРѕР±Р°РІР»СЏРµС‚ СЃР»РѕРІРѕ РІ
    /// РЅР°Р±РѕСЂ РїСЂРѕРїСѓС‰РµРЅРЅС‹С… РЅР° СЃРµСЃСЃРёСЋ.
    pub(crate) fn exec_spell_menu_action(&mut self, action: page_context_menu::SpellMenuAction) {
        use page_context_menu::{SpellMenuAction, SpellTargetKind};
        let Some(target) = self.page_context_menu.target().cloned() else { return };
        match action {
            SpellMenuAction::Use(replacement) => {
                match target.kind {
                    SpellTargetKind::Input => {
                        let new_val = target.apply(&replacement);
                        if let Some(src) = self.layout_source.as_mut()
                            && let Ok(mut doc) = src.document.lock()
                        {
                            forms::set_value(&mut doc, target.node, &new_val);
                        }
                        self.form_state.entry(target.node).or_default().value = new_val;
                    }
                    SpellTargetKind::Textarea => {
                        let new_val = target.apply(&replacement);
                        if let Some(src) = self.layout_source.as_mut()
                            && let Ok(mut doc) = src.document.lock()
                        {
                            forms::set_textarea_text(&mut doc, target.node, &new_val);
                        }
                        self.form_state.entry(target.node).or_default().value = new_val;
                    }
                    SpellTargetKind::ContentEditable => {
                        if let Some(src) = self.layout_source.as_mut()
                            && let Ok(mut doc) = src.document.lock()
                            && let Some((text_node, local_start, local_end)) =
                                locate_text_offset_range(
                                    &doc,
                                    target.node,
                                    target.word_start,
                                    target.word_end,
                                )
                        {
                            let range = Range {
                                start: DomPosition { container: text_node, offset: local_start },
                                end: DomPosition { container: text_node, offset: local_end },
                            };
                            let collapsed = delete_range(&mut doc, &range);
                            insert_text_at(&mut doc, collapsed, &replacement);
                        }
                    }
                }
                // ADR-016 M2.2c-3 (2): spellcheck-replace mutates the shared DOM
                // (input value / textarea text / contenteditable range) with no
                // synchronous geometry read after вЂ” Bucket A, route off-thread when
                // `LUMEN_ENGINE_THREAD=1`, byte-identical otherwise.
                self.relayout_form();
            }
            SpellMenuAction::AddToDict => {
                let word = target.word().to_lowercase();
                let _ = spellcheck::add_user_word(&spellcheck::user_words_path(), &word);
                self.spell_user_words.insert(word);
            }
            SpellMenuAction::Ignore => {
                self.spell_ignored.insert(target.word().to_lowercase());
            }
        }
    }
}
