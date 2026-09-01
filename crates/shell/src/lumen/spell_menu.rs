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
    /// P3-spell срез 2+3+4: для узла `nid` под фокусом определяет цель
    /// спелл-чека. Возвращает `(target_node, placeholder, kind)`:
    /// * `<textarea>` или `<input>` текстового типа (password исключён) — сам
    ///   узел, его `placeholder` (пустая строка при отсутствии) и
    ///   соответствующий [`page_context_menu::SpellTargetKind`];
    /// * узел внутри `contenteditable` — редактирующий хост, пустой
    ///   placeholder (у contenteditable нет placeholder-атрибута) и
    ///   `ContentEditable`;
    /// * иначе — `None`.
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

    /// P3-spell срез 3: слова, которые не считаются ошибочными помимо словарей —
    /// объединение пользовательского словаря и «Пропущенных» на сессию. Все
    /// слова уже в lowercase.
    pub(crate) fn spell_allow_set(&self) -> std::collections::HashSet<String> {
        self.spell_user_words
            .iter()
            .chain(self.spell_ignored.iter())
            .cloned()
            .collect()
    }

    /// P3-spell срез 4: полный логический текст поля `target_node` —
    /// `value`-атрибут для `<input>`, либо (для `<textarea>`/contenteditable)
    /// конкатенация текстовых узлов-потомков (`lumen_dom::node_text_content`).
    /// Используется как база для глобальных byte-смещений слова в
    /// [`page_context_menu::SpellTarget`], в отличие от текста одной
    /// визуальной (wrapped) строки.
    fn spell_field_full_text(
        &self,
        target_node: lumen_dom::NodeId,
        kind: page_context_menu::SpellTargetKind,
    ) -> String {
        use page_context_menu::SpellTargetKind;
        let Some(ls) = self.layout_source.as_ref() else { return String::new() };
        let Ok(doc) = ls.document.lock() else { return String::new() };
        match kind {
            // BUG-441: у `<input>`/`<textarea>` проверяем то, что в поле сейчас
            // (runtime-значение), а не дефолт из разметки. `contenteditable`
            // редактируется прямо в DOM, поэтому там по-прежнему текст узлов.
            SpellTargetKind::Input | SpellTargetKind::Textarea => {
                doc.control_value(target_node).into_owned()
            }
            SpellTargetKind::ContentEditable => node_text_content(&doc, target_node),
        }
    }

    /// P3-spell срез 3+4: при right-click по ошибочному слову в фокусном
    /// `<input>`/`<textarea>`/contenteditable открывает меню подсказок.
    /// Возвращает `true`, если меню открыто (клик обработан), иначе `false`
    /// (клик идёт дальше — жест).
    ///
    /// Многострочные поля рисуют одну `DrawText`-команду на визуальную
    /// (wrapped) строку — байтовое смещение слова внутри клика найденной
    /// строки само по себе бессмысленно за пределами первой строки.
    /// `spellcheck::locate_line_word_in_full_text` пересчитывает его в
    /// глобальное смещение внутри полного значения поля, используя
    /// предшествующие строки того же поля как якоря.
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
                    // Word under cursor is spelled correctly — no menu.
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

    /// P3-spell срез 3+4: применяет выбранное действие меню подсказок.
    /// `Use` заменяет слово и перевёрстывает — для `<input>`/`<textarea>`
    /// перестраивая полное значение через `target.apply()`; для
    /// contenteditable точечно правя только текстовый узел, содержащий слово
    /// (`lumen_dom::locate_text_offset_range` + `delete_range`/`insert_text_at`),
    /// не трогая остальную rich-text структуру. `AddToDict` добавляет слово в
    /// пользовательский словарь (файл + память); `Ignore` добавляет слово в
    /// набор пропущенных на сессию.
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
                // synchronous geometry read after — Bucket A, route off-thread when
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
