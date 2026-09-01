//! Native text input into a typeable field ВНУТРИ содержимого фрейма
//! (BUG-480 срез 22).
//!
//! Срез 16 довёл клик до под-документа как событие, срез 18 — собственное
//! поведение элементов управления формы на нативный клик. Ввод текста
//! оставался вне очереди: `self.focused_node` после клика внутрь фрейма
//! указывает на host-элемент `<iframe>` (срез 16 — с точки зрения СТРАНИЦЫ
//! клик внутрь фрейма фокусирует контейнер), а `Self::typeable_field` в
//! [`super::text_input`] читает исключительно `self.layout_source` — документ
//! страницы. Печатать в поле внутри фрейма было решительно некуда.
//!
//! Здесь та же пара «классифицировать → применить → перерисовать», что у
//! [`super::frame_forms`], только против ДРУГОГО поля состояния —
//! [`crate::lumen::Lumen::focused_frame`] вместо `focused_node` — и с записью
//! значения через штатный `forms::set_value`/`set_textarea_text`, как у
//! страницы в [`super::text_input`]. Видимого `:focus` (каретка/outline)
//! внутри фрейма это НЕ даёт: `frames::layout_frame_document` не вызывает
//! `set_interactive_state` вовсе — фрейм остаётся интерактивно-слепым для CSS
//! так же, как и для `:hover` ([`crate::lumen::Lumen::hovered_frame`]); это
//! отдельный, больший срез очереди.

use crate::*;

impl Lumen {
    /// Классифицировать `nid` в документе фрейма `idx` как typeable-поле —
    /// зеркало [`Self::typeable_field`], но против ЕГО документа, а не
    /// страницы. `None` для несуществующего фрейма/отравленного лока, как и у
    /// прочих операций среза 18 ([`super::frame_forms`]).
    pub(crate) fn frame_typeable_field(
        &self,
        idx: usize,
        nid: NodeId,
    ) -> Option<(TypeableField, String)> {
        let handle = self.frames.get(idx)?;
        let doc = handle.doc.lock().ok()?;
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

    /// Read (and lazily initialize) the char-index text cursor for the
    /// typeable field `(idx, nid)` — mirror of
    /// [`super::text_input::Lumen::field_cursor`] (page), keyed against
    /// [`crate::lumen::Lumen::frame_text_cursor`] instead of `form_state`
    /// (a frame's sub-document has no per-`NodeId` state map of its own).
    fn frame_field_cursor(&mut self, idx: usize, nid: NodeId, current: &str) -> usize {
        let len = char_len(current);
        let c = *self.frame_text_cursor.entry((idx, nid)).or_insert(len);
        c.min(len)
    }

    /// Move the focused frame field's text cursor by `delta` chars — mirror
    /// of [`super::text_input::Lumen::move_focused_cursor`] (page).
    pub(crate) fn move_focused_frame_cursor(&mut self, delta: i32) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((_, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current) as i32;
        let len = char_len(&current) as i32;
        let next = (cursor + delta).clamp(0, len) as usize;
        self.frame_text_cursor.insert((idx, nid), next);
        true
    }

    /// Home (`to_start = true`) / End (`to_start = false`) for the focused
    /// frame field — mirror of
    /// [`super::text_input::Lumen::jump_focused_cursor`] (page).
    pub(crate) fn jump_focused_frame_cursor(&mut self, to_start: bool) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((_, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let target = if to_start { 0 } else { char_len(&current) };
        self.frame_text_cursor.insert((idx, nid), target);
        true
    }

    /// FRAME-7 remainder (1): the focused frame `<input>`'s char-index
    /// cursor and current value, if a caret bar should be painted this frame
    /// — mirror of [`super::text_input::Lumen::focused_input_caret`], reading
    /// [`Self::focused_frame`]/`frame_text_cursor` instead of the page's
    /// `focused_node`/`form_state`. Read-only, like its page counterpart.
    /// Returns the value too (unlike the page version): the page's
    /// `CompositorOverride` paint site already carries `value_text` from its
    /// own model, but this frame path paints through a shell-side overlay
    /// (`forms::input_caret_rect`) that has no such model to read from.
    pub(crate) fn focused_frame_input_caret(&self) -> Option<(usize, NodeId, usize, String)> {
        let (idx, nid) = self.focused_frame?;
        let (kind, current) = self.frame_typeable_field(idx, nid)?;
        if kind != TypeableField::Input {
            // FRAME-7: a frame `<textarea>` caret goes through
            // `focused_frame_textarea_caret` instead — same split as the
            // page's two caret paths (see `focused_textarea_caret`'s note).
            return None;
        }
        let len = char_len(&current);
        let cursor = self.frame_text_cursor.get(&(idx, nid)).copied().unwrap_or(len);
        Some((idx, nid, cursor.min(len), current))
    }

    /// FRAME-7 remainder (1): the focused frame `<textarea>`'s char-index
    /// cursor and current value — mirror of
    /// [`super::text_input::Lumen::focused_textarea_caret`].
    pub(crate) fn focused_frame_textarea_caret(&self) -> Option<(usize, NodeId, usize, String)> {
        let (idx, nid) = self.focused_frame?;
        let (kind, current) = self.frame_typeable_field(idx, nid)?;
        if kind != TypeableField::Textarea {
            return None;
        }
        let len = char_len(&current);
        let cursor = self.frame_text_cursor.get(&(idx, nid)).copied().unwrap_or(len);
        Some((idx, nid, cursor.min(len), current))
    }

    /// Собственное действие движка по умолчанию на typeable-поле фрейма,
    /// адресуемом `self.focused_frame` — зеркало
    /// [`super::text_input::Lumen::edit_focused_field_at_cursor`]:
    /// insertion/deletion at the tracked cursor, not always at the end of the
    /// value (FRAME-2 п.1).
    ///
    /// Мутация дерева ребёнка идёт через [`super::frame_forms::Lumen::with_frame_doc`]
    /// (тот же короткий лок, что у нативного переключения элемента
    /// управления), значение в JS-тени фрейма синхронизируется отдельным
    /// `eval_js` по ЕГО хэндлу — `route_eval_js` знает только контекст
    /// страницы (та же причина, что у [`super::frame_forms::Lumen::frame_toggle_details`]).
    fn edit_focused_frame_field_at_cursor(
        &mut self,
        edit: impl FnOnce(&str, usize) -> (String, usize),
    ) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((kind, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let (next, next_cursor) = edit(&current, cursor);
        self.frame_text_cursor.insert((idx, nid), next_cursor);
        if next == current {
            return true;
        }
        if !self.with_frame_doc(idx, |doc| match kind {
            TypeableField::Input => forms::set_value(doc, nid, &next),
            TypeableField::Textarea => forms::set_textarea_text(doc, nid, &next),
        }) {
            return false;
        }
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames.get(idx).and_then(|h| h.js.as_ref()) {
            js.eval_js(&format!(
                "_lumen_set_field_value({}, '{}')",
                nid.index(),
                escape_js_string(&next)
            ));
        }
        self.refresh_frames(Some(idx));
        true
    }

    /// Отправить один `_lumen_dispatch_key_event` в JS-контекст фрейма `idx` —
    /// прямым `eval_js` по ЕГО хэндлу, как у [`super::frame_forms`], а не
    /// через `route_eval_js` (страница).
    #[allow(unused_variables)] // js.eval_js читается только под feature = "v8"
    fn dispatch_frame_key(&mut self, idx: usize, node_id: usize, event_type: &str, key: &str) {
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames.get(idx).and_then(|h| h.js.as_ref()) {
            js.eval_js(&format!(
                "_lumen_dispatch_key_event({}, '{}', '{}', '{}', false, false, false, false)",
                node_id, event_type, key, key,
            ));
        }
    }

    /// Ввести символ во typeable-поле фрейма, адресуемом `self.focused_frame`
    /// (зеркало [`Self::inject_char`]). `true` — символ принят полем.
    pub(crate) fn inject_frame_char(&mut self, ch: char) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let node_id = nid.index();
        let key = escape_js_string_char(ch);
        self.dispatch_frame_key(idx, node_id, "keydown", &key);
        let consumed = self
            .edit_focused_frame_field_at_cursor(|current, cursor| insert_char_at(current, cursor, ch));
        for event_type in &["input", "keyup"] {
            self.dispatch_frame_key(idx, node_id, event_type, &key);
        }
        consumed
    }

    /// Backspace во typeable-поле фрейма, адресуемом `self.focused_frame`
    /// (зеркало [`Self::inject_backspace`]) — удаляет символ ПЕРЕД курсором.
    pub(crate) fn inject_frame_backspace(&mut self) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let node_id = nid.index();
        self.dispatch_frame_key(idx, node_id, "keydown", "Backspace");
        let consumed = self.edit_focused_frame_field_at_cursor(delete_char_before);
        for event_type in &["input", "keyup"] {
            self.dispatch_frame_key(idx, node_id, event_type, "Backspace");
        }
        consumed
    }

    /// Delete (forward-delete) во typeable-поле фрейма, адресуемом
    /// `self.focused_frame` (зеркало [`Self::inject_delete_forward`]) —
    /// удаляет символ ПОСЛЕ курсора.
    pub(crate) fn inject_frame_delete_forward(&mut self) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let node_id = nid.index();
        self.dispatch_frame_key(idx, node_id, "keydown", "Delete");
        let consumed = self.edit_focused_frame_field_at_cursor(|current, cursor| {
            (delete_char_after(current, cursor), cursor)
        });
        for event_type in &["input", "keyup"] {
            self.dispatch_frame_key(idx, node_id, event_type, "Delete");
        }
        consumed
    }
}
