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

use super::text_input::{
    ClipboardOp, ClipboardReply, EditAction, HistoryOp, field_meta_in, input_event_script, write_clipboard_nonempty,
};

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
        // BUG-995: `nid` can outlive the frame document it was focused in —
        // `typeable_field_in` reads it with `try_get`.
        super::text_input::typeable_field_in(&doc, nid)
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

    /// Char-index selection range for the frame field `(idx, nid)`, normalized
    /// (`start <= end`) — mirror of
    /// [`super::text_input::Lumen::field_selection_range`] (page). `None`
    /// when no selection is active.
    fn frame_field_selection_range(&self, idx: usize, nid: NodeId, cursor: usize) -> Option<(usize, usize)> {
        let anchor = *self.frame_text_selection_anchor.get(&(idx, nid))?;
        if anchor == cursor {
            return None;
        }
        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    /// Move the focused frame field's text cursor by `delta` chars — mirror
    /// of [`super::text_input::Lumen::move_focused_cursor`] (page), including
    /// the FRAME-7 remainder 2 selection-collapse behaviour.
    pub(crate) fn move_focused_frame_cursor(&mut self, delta: i32) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((_, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let next = match self.frame_field_selection_range(idx, nid, cursor) {
            Some((start, end)) => if delta < 0 { start } else { end },
            None => {
                let len = char_len(&current) as i32;
                (cursor as i32 + delta).clamp(0, len) as usize
            }
        };
        self.frame_text_cursor.insert((idx, nid), next);
        self.frame_text_selection_anchor.remove(&(idx, nid));
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
        self.frame_text_selection_anchor.remove(&(idx, nid));
        true
    }

    /// Shift+Left/Right for the focused frame field — mirror of
    /// [`super::text_input::Lumen::extend_focused_selection`] (page).
    pub(crate) fn extend_focused_frame_selection(&mut self, delta: i32) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((_, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let len = char_len(&current) as i32;
        let next = (cursor as i32 + delta).clamp(0, len) as usize;
        self.frame_text_selection_anchor.entry((idx, nid)).or_insert(cursor);
        self.frame_text_cursor.insert((idx, nid), next);
        true
    }

    /// Shift+Home (`to_start = true`) / Shift+End (`to_start = false`) for
    /// the focused frame field — mirror of
    /// [`super::text_input::Lumen::extend_focused_selection_to_edge`] (page).
    pub(crate) fn extend_focused_frame_selection_to_edge(&mut self, to_start: bool) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((_, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let target = if to_start { 0 } else { char_len(&current) };
        self.frame_text_selection_anchor.entry((idx, nid)).or_insert(cursor);
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

    /// FRAME-7 remainder 2: the focused frame `<input>`'s selection range and
    /// current value, if one should be painted this frame — mirror of
    /// [`super::text_input::Lumen::focused_input_selection`].
    pub(crate) fn focused_frame_input_selection(&self) -> Option<(usize, NodeId, usize, usize, String)> {
        let (idx, nid) = self.focused_frame?;
        let (kind, current) = self.frame_typeable_field(idx, nid)?;
        if kind != TypeableField::Input {
            return None;
        }
        let len = char_len(&current);
        let anchor = self.frame_text_selection_anchor.get(&(idx, nid)).copied()?.min(len);
        let cursor = self.frame_text_cursor.get(&(idx, nid)).copied().unwrap_or(len).min(len);
        if anchor == cursor {
            return None;
        }
        Some((idx, nid, anchor.min(cursor), anchor.max(cursor), current))
    }

    /// FRAME-7 remainder 2: the focused frame `<textarea>`'s selection range
    /// and current value — mirror of
    /// [`super::text_input::Lumen::focused_textarea_selection`].
    pub(crate) fn focused_frame_textarea_selection(&self) -> Option<(usize, NodeId, usize, usize, String)> {
        let (idx, nid) = self.focused_frame?;
        let (kind, current) = self.frame_typeable_field(idx, nid)?;
        if kind != TypeableField::Textarea {
            return None;
        }
        let len = char_len(&current);
        let anchor = self.frame_text_selection_anchor.get(&(idx, nid)).copied()?.min(len);
        let cursor = self.frame_text_cursor.get(&(idx, nid)).copied().unwrap_or(len).min(len);
        if anchor == cursor {
            return None;
        }
        Some((idx, nid, anchor.min(cursor), anchor.max(cursor), current))
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
    fn edit_focused_frame_field_at_cursor(&mut self, action: EditAction) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((kind, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let selection = self.frame_field_selection_range(idx, nid, cursor);
        let history_kind = action.history_kind(&current, cursor, selection.is_some());
        let (next, next_cursor) = match selection {
            Some((start, end)) => {
                let spliced = delete_char_range(&current, start, end);
                match action {
                    EditAction::InsertChar(ch) => insert_char_at(&spliced, start, ch),
                    EditAction::InsertStr(text) => insert_str_at(&spliced, start, &text),
                    EditAction::Backspace | EditAction::DeleteForward => (spliced, start),
                }
            }
            None => action.apply(&current, cursor),
        };
        self.frame_text_cursor.insert((idx, nid), next_cursor);
        self.frame_text_selection_anchor.remove(&(idx, nid));
        if next == current {
            return true;
        }
        self.field_history.entry((Some(idx), nid)).or_default().record(
            FieldSnapshot { value: current, cursor },
            history_kind,
            &next,
            next_cursor,
            std::time::Instant::now(),
        );
        self.write_frame_field_value(idx, nid, kind, &next)
    }

    /// Store `next` as the frame field's value: its document, the JS shadow of
    /// the frame, then a frame refresh. `false` when the frame is gone.
    fn write_frame_field_value(&mut self, idx: usize, nid: NodeId, kind: TypeableField, next: &str) -> bool {
        if !self.with_frame_doc(idx, |doc| match kind {
            TypeableField::Input => forms::set_value(doc, nid, next),
            TypeableField::Textarea => forms::set_textarea_text(doc, nid, next),
        }) {
            return false;
        }
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames.get(idx).and_then(|h| h.js.as_ref()) {
            js.eval_js(&format!(
                "_lumen_set_field_value({}, '{}')",
                nid.index(),
                escape_js_string(next)
            ));
        }
        self.refresh_frames(Some(idx));
        true
    }

    /// Ctrl+Z / Ctrl+Y in a frame field — mirror of
    /// [`super::text_input::Lumen::field_history_op`] (UX-UNDO).
    pub(crate) fn frame_field_history_op(&mut self, op: HistoryOp) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((kind, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let here = FieldSnapshot { value: current, cursor };
        let Some(hist) = self.field_history.get_mut(&(Some(idx), nid)) else { return true };
        let Some(target) = (match op {
            HistoryOp::Undo => hist.undo(&here),
            HistoryOp::Redo => hist.redo(&here),
        }) else {
            return true;
        };
        let ty = op.input_type();
        if !self.frame_clip_input_event(idx, nid, "beforeinput", ty, None) {
            if let Some(hist) = self.field_history.get_mut(&(Some(idx), nid)) {
                match op {
                    HistoryOp::Undo => hist.redo(&target),
                    HistoryOp::Redo => hist.undo(&target),
                };
            }
            return true;
        }
        let len = char_len(&target.value);
        self.frame_text_cursor.insert((idx, nid), target.cursor.min(len));
        self.frame_text_selection_anchor.remove(&(idx, nid));
        self.write_frame_field_value(idx, nid, kind, &target.value);
        self.frame_clip_input_event(idx, nid, "input", ty, None);
        true
    }

    /// Ctrl+A / Ctrl+C / Ctrl+X / Ctrl+V во typeable-поле фрейма — зеркало
    /// [`super::text_input::Lumen::field_clipboard_op`] (UX-CLIPBOARD); события
    /// уходят в JS-контекст ФРЕЙМА.
    pub(crate) fn frame_field_clipboard_op(&mut self, op: ClipboardOp) -> bool {
        let Some((idx, nid)) = self.focused_frame else { return false };
        let Some((kind, current)) = self.frame_typeable_field(idx, nid) else { return false };
        let cursor = self.frame_field_cursor(idx, nid, &current);
        let sel = self.frame_field_selection_range(idx, nid, cursor);
        let (maxlength, password) = self
            .frames
            .get(idx)
            .and_then(|h| h.doc.lock().ok())
            .map(|doc| field_meta_in(&doc, nid))
            .unwrap_or((None, false));
        match op {
            ClipboardOp::SelectAll => {
                self.frame_text_selection_anchor.insert((idx, nid), 0);
                self.frame_text_cursor.insert((idx, nid), char_len(&current));
            }
            ClipboardOp::Copy | ClipboardOp::Cut => {
                let Some((start, end)) = sel else { return true };
                if password {
                    return true;
                }
                let cut = op == ClipboardOp::Cut;
                let name = if cut { "cut" } else { "copy" };
                if let ClipboardReply::Cancelled(data) = self.frame_clipboard_event(idx, nid, name, "") {
                    write_clipboard_nonempty(&data);
                    return true;
                }
                write_clipboard_nonempty(&char_range(&current, start, end));
                if cut && self.frame_clip_input_event(idx, nid, "beforeinput", "deleteByCut", None) {
                    self.edit_focused_frame_field_at_cursor(EditAction::Backspace);
                    self.frame_clip_input_event(idx, nid, "input", "deleteByCut", None);
                }
            }
            ClipboardOp::Paste => {
                use lumen_core::ext::ClipboardProvider;
                let raw = platform::clipboard::PlatformClipboard.read_text();
                if raw.is_empty() {
                    return true;
                }
                if let ClipboardReply::Cancelled(_) = self.frame_clipboard_event(idx, nid, "paste", &raw) {
                    return true;
                }
                let replaced = sel.map_or(0, |(s, e)| e - s);
                let text = sanitize_paste(
                    &raw,
                    kind == TypeableField::Textarea,
                    maxlength,
                    char_len(&current),
                    replaced,
                );
                if text.is_empty() {
                    return true;
                }
                if self.frame_clip_input_event(idx, nid, "beforeinput", "insertFromPaste", Some(&text)) {
                    self.edit_focused_frame_field_at_cursor(EditAction::InsertStr(text.clone()));
                    self.frame_clip_input_event(idx, nid, "input", "insertFromPaste", Some(&text));
                }
            }
        }
        true
    }

    /// Eval `script` в JS-контексте фрейма `idx`, вернуть строковый результат.
    #[allow(unused_variables)] // js читается только под feature = "v8"
    fn frame_eval_completion(&self, idx: usize, script: &str) -> Option<String> {
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames.get(idx).and_then(|h| h.js.as_ref()) {
            return js.eval_js_completion(script).ok().flatten();
        }
        None
    }

    fn frame_clipboard_event(&self, idx: usize, nid: NodeId, kind: &str, text: &str) -> ClipboardReply {
        let script = format!(
            "_lumen_dispatch_clipboard_event({}, '{}', '{}')",
            nid.index(),
            kind,
            escape_js_string(text)
        );
        ClipboardReply::parse(self.frame_eval_completion(idx, &script).as_deref())
    }

    fn frame_clip_input_event(&self, idx: usize, nid: NodeId, kind: &str, input_type: &str, data: Option<&str>) -> bool {
        let script = input_event_script(nid.index(), kind, input_type, data);
        self.frame_eval_completion(idx, &script).is_none_or(|r| r != "0")
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

    /// UX-IME-4: composition-событие на поле фрейма — прямым `eval_js` по
    /// хэндлу фрейма, как [`Self::dispatch_frame_key`].
    #[allow(unused_variables)] // js.eval_js читается только под feature = "v8"
    pub(crate) fn frame_composition_event(&self, idx: usize, nid: lumen_dom::NodeId, kind: &str, data: &str) {
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames.get(idx).and_then(|h| h.js.as_ref()) {
            js.eval_js(&format!(
                "_lumen_dispatch_composition_at({}, '{}', '{}')",
                nid.index(),
                kind,
                escape_js_string(data)
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
        let consumed = self.edit_focused_frame_field_at_cursor(EditAction::InsertChar(ch));
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
        let consumed = self.edit_focused_frame_field_at_cursor(EditAction::Backspace);
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
        let consumed = self.edit_focused_frame_field_at_cursor(EditAction::DeleteForward);
        for event_type in &["input", "keyup"] {
            self.dispatch_frame_key(idx, node_id, event_type, "Delete");
        }
        consumed
    }
}
