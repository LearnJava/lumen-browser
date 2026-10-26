//! Mouse-drag text selection over ordinary page text, with CSS `user-select`
//! enforced (CSS UI L4 §6.2).
//!
//! Counterpart of [`super::text_drag_select`], which only drives the caret
//! inside a focused `<input>`/`<textarea>`. Here a left-button press on page
//! text anchors the document [`lumen_dom::Selection`] (the same one JS reads
//! through `window.getSelection()`), `CursorMoved` extends its focus end, and
//! `Released` stops tracking while the selection stays.
//!
//! `user-select` is enforced at four points:
//! * `none` — layout's `caret_at_point`/`selection_rects` skip the text, so a
//!   press on it anchors nothing and the highlight leaves it out; Ctrl+C drops
//!   it from the copied string ([`lumen_layout::user_select_none_text_nodes`]);
//! * `all` — a press selects the whole element at once
//!   ([`lumen_layout::select_scope_at_point`]);
//! * `contain` — a drag that starts inside is clamped to the element;
//! * `auto`/`text` — plain drag selection.

use crate::*;
use lumen_dom::Range;
use lumen_layout::{SelectScope, TextMeasurer};
use lumen_paint::DisplayList;

/// An armed page-text drag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocSelectDrag {
    /// `user-select: contain` scope the focus end is clamped into.
    pub(crate) contain: Option<Range>,
}

/// The `(from, to)` char range a double (`clicks == 2`) or triple press selects
/// around char index `cursor` of a field `value`: the word, or the whole value
/// of an `<input>` / the line between line feeds of a `<textarea>`. `None` for
/// an empty value.
fn field_run(kind: TypeableField, value: &str, cursor: usize, clicks: u8) -> Option<(usize, usize)> {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let cursor = cursor.min(chars.len());
    if clicks == 2 {
        let byte = value.char_indices().nth(cursor).map_or(value.len(), |(b, _)| b);
        let (from, to) = lumen_dom::word_bounds(value, byte)?;
        return Some((value[..from].chars().count(), value[..to].chars().count()));
    }
    Some(match kind {
        TypeableField::Input => (0, chars.len()),
        TypeableField::Textarea => {
            let from = chars[..cursor].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
            let to = chars[cursor..].iter().position(|&c| c == '\n').map_or(chars.len(), |i| cursor + i);
            (from, to)
        }
    })
}

/// Runs `f` with the bundled-font measurer, `None` only if the bundled font
/// fails to parse (a corrupted binary) — a hit test must not `unwrap` there.
fn with_measurer<R>(f: impl FnOnce(&dyn TextMeasurer) -> R) -> Option<R> {
    let font = lumen_font::Font::parse(INTER_FONT).ok()?;
    let m = lumen_paint::FontMeasurer::new(&font).ok()?;
    Some(f(&m))
}

/// Страница с выделением: список, собранный с `::selection` поверх текста.
#[derive(Clone)]
struct SelectionPage {
    /// Идентификатор документа (адрес мьютекса).
    doc_id: usize,
    /// Версия `display_list`, из которого `layout_box` собран.
    epoch: u64,
    range: Range,
    /// Вьюпорт (биты `w`, `h`) и тема — от них зависят медиа-запросы `::selection`.
    viewport: (u32, u32),
    dark_mode: bool,
    /// `None` — выделение ничего не закрашивает (`user-select: none`, пустые узлы).
    list: Option<DisplayList>,
}

thread_local! {
    /// Последняя собранная страница с выделением. Пересборка из `layout_box` стоит
    /// полного `paint_ordered`, поэтому ключ — `(display_list_epoch, диапазон)`; ряд
    /// кадров, где не менялось ни выделение, ни список, берёт готовый.
    static LAST_SELECTION_PAGE: std::cell::RefCell<Option<SelectionPage>> =
        const { std::cell::RefCell::new(None) };
}

impl Lumen {
    /// Anchor a page-text selection at the press point `(x_css, y_css)` — runs
    /// right after `handle_click_at`, like [`Self::begin_text_drag_select`].
    ///
    /// A press that armed a field's own caret drag, landed over chrome or a
    /// frame, or hit text with nothing selectable under it simply clears the
    /// document selection (a plain click elsewhere deselects).
    pub(crate) fn begin_doc_select(&mut self, x_css: f32, y_css: f32) {
        self.doc_select = None;
        let over_page = self.text_drag.is_none()
            && self.hovered_frame.is_none()
            && !self.point_over_chrome(x_css, y_css);
        let (px, py) = self.page_point(x_css, y_css);
        let hit = if over_page { self.doc_select_hit(px, py) } else { None };
        let Some(src) = self.layout_source.as_ref() else { return };
        let mut doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
        match hit {
            Some((_, Some(SelectScope::All(range)))) => {
                doc.set_selection(lumen_dom::Selection {
                    anchor: Some(range.start),
                    focus: Some(range.end),
                });
            }
            Some((pos, scope)) => {
                doc.set_selection(lumen_dom::Selection { anchor: Some(pos), focus: Some(pos) });
                let contain = match scope {
                    Some(SelectScope::Contain(r)) => Some(r),
                    _ => None,
                };
                self.doc_select = Some(DocSelectDrag { contain });
            }
            None => doc.clear_selection(),
        }
        drop(doc);
        self.request_redraw();
    }

    /// Caret position under page point `(px, py)` plus its `all`/`contain`
    /// scope; `None` over no selectable text.
    fn doc_select_hit(
        &self,
        px: f32,
        py: f32,
    ) -> Option<(lumen_dom::DomPosition, Option<SelectScope>)> {
        let lb = self.layout_box.as_ref()?;
        let scope = lumen_layout::select_scope_at_point(lb, px, py);
        // `user-select: none` text under the point has no caret even when an
        // ancestor says `all` — `caret_at_point` already skips it.
        let pos = with_measurer(|m| lumen_layout::caret_at_point(lb, px, py, m))??;
        Some((pos, scope))
    }

    /// UX-DBLCLICK: the second press of a streak selects the word under the
    /// pointer, the third (and later) the whole paragraph — in a focused
    /// `<input>`/`<textarea>` (armed by `begin_text_drag_select`) or in page
    /// text (armed by [`Self::begin_doc_select`]). Runs right after both, so
    /// the press has already put the caret where the click landed. `user-select:
    /// all` text is already selected whole and is left alone; a frame's field
    /// keeps the plain caret.
    pub(crate) fn select_by_click_count(&mut self, x_css: f32, y_css: f32, clicks: u8) {
        if clicks < 2 {
            return;
        }
        if let Some(super::text_drag_select::TextDragTarget::Page(nid)) = self.text_drag {
            self.select_field_run(nid, clicks);
            return;
        }
        let Some(drag) = self.doc_select.clone() else { return };
        let (px, py) = self.page_point(x_css, y_css);
        let Some(src) = self.layout_source.as_ref() else { return };
        let mut doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
        let Some(pos) = doc.get_selection().anchor else { return };
        let range = if clicks == 2 {
            lumen_dom::word_range_at(&doc, pos)
        } else {
            self.layout_box
                .as_ref()
                .and_then(|lb| lumen_layout::paragraph_range_at_point(lb, px, py))
        };
        let Some(mut range) = range else { return };
        if let Some(scope) = &drag.contain {
            range.start = lumen_layout::clamp_to_range(range.start, scope);
            range.end = lumen_layout::clamp_to_range(range.end, scope);
        }
        doc.set_selection(lumen_dom::Selection { anchor: Some(range.start), focus: Some(range.end) });
        drop(doc);
        self.request_redraw();
    }

    /// Word (2 presses) or whole value / line (3+) of the focused page field
    /// `nid` around its cursor, as the field's selection.
    fn select_field_run(&mut self, nid: lumen_dom::NodeId, clicks: u8) {
        let Some((kind, value)) = self.typeable_field(nid) else { return };
        let Some(cursor) = self.form_state.get(&nid).and_then(|s| s.cursor) else { return };
        let Some((from, to)) = field_run(kind, &value, cursor, clicks) else { return };
        let slot = self.form_state.entry(nid).or_default();
        slot.selection_anchor = Some(from);
        slot.cursor = Some(to);
        self.publish_text_selection_for(nid, &value);
        self.request_redraw();
    }

    /// Extend the focus end of an in-progress page-text drag to the char under
    /// `(x_css, y_css)` — called from `cursor_moved.rs` while
    /// `self.doc_select` is armed. The anchor never moves.
    pub(crate) fn update_doc_select(&mut self, x_css: f32, y_css: f32) {
        let Some(drag) = self.doc_select.clone() else { return };
        let (px, py) = self.page_point(x_css, y_css);
        let Some((mut pos, _)) = self.doc_select_hit(px, py) else { return };
        if let Some(scope) = &drag.contain {
            pos = lumen_layout::clamp_to_range(pos, scope);
        }
        let Some(src) = self.layout_source.as_ref() else { return };
        let mut doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
        if doc.get_selection().focus == Some(pos) {
            return;
        }
        let mut sel = doc.get_selection().clone();
        sel.extend_focus(pos);
        doc.set_selection(sel);
        drop(doc);
        self.request_redraw();
    }

    /// Страница, на которой выделение документа закрашено правилами `::selection`
    /// (CSS Pseudo-Elements L4 §3.6): `background-color`, `color`, `text-shadow`,
    /// декорации. `None`, если ничего не выделено — тогда рисуется `display_list`.
    ///
    /// Список собирается из `layout_box` через `resolve_highlights` +
    /// `with_highlights` — тем же путём, что `--screenshot`, — и кэшируется по
    /// `(display_list_epoch, диапазон)`. Вклейка фреймов повторяется, как в
    /// [`Self::set_display_list`].
    pub(crate) fn doc_selection_page(&self) -> Option<DisplayList> {
        let src = self.layout_source.as_ref()?;
        let lb = self.layout_box.as_ref()?;
        // THREAD-12: не ждать мьютекс документа на UI-потоке — пока поток
        // движка держит его в JS-задаче, кадр стоил 0,5–1 с (`build: chrome`
        // на ria.ru). При занятом мьютексе отдаём результат прошлого кадра
        // для того же документа.
        let doc_id = std::sync::Arc::as_ptr(&src.document) as usize;
        let doc = match src.document.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::WouldBlock) => {
                return LAST_SELECTION_PAGE.with(|c| {
                    c.borrow().as_ref().filter(|p| p.doc_id == doc_id).and_then(|p| p.list.clone())
                });
            }
            Err(_) => return None,
        };
        let Some(range) = doc.get_selection().get_range().filter(|r| !r.is_collapsed()) else {
            LAST_SELECTION_PAGE.with(|c| *c.borrow_mut() = None);
            return None;
        };
        let viewport = self.relayout_viewport()?;
        let key = (viewport.width.to_bits(), viewport.height.to_bits());
        let hit = LAST_SELECTION_PAGE.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|p| {
                    p.doc_id == doc_id
                        && p.epoch == self.display_list_epoch
                        && p.range == range
                        && p.viewport == key
                        && p.dark_mode == self.dark_mode
                })
                .map(|p| p.list.clone())
        });
        if let Some(list) = hit {
            return list;
        }
        let font = lumen_font::Font::parse(INTER_FONT).ok()?;
        let measurer = crate::relayout::page_measurer(&font, &self.web_fonts);
        let highlights = crate::display_list_metrics::page_highlights(
            lb,
            &doc,
            &src.stylesheet,
            viewport,
            self.dark_mode,
            &measurer,
            &[],
        );
        drop(doc);
        let list = highlights.map(|h| {
            let mut list = crate::display_list_metrics::paint_ordered_in_highlighted(lb, viewport, Some(h));
            crate::frames::splice_frame_content(&mut list, &self.frames);
            list
        });
        let page = SelectionPage {
            doc_id,
            epoch: self.display_list_epoch,
            range,
            viewport: key,
            dark_mode: self.dark_mode,
            list: list.clone(),
        };
        LAST_SELECTION_PAGE.with(|c| *c.borrow_mut() = Some(page));
        list
    }

    /// The selected text with `user-select: none` runs left out, `None` when
    /// the selection is empty.
    pub(crate) fn selected_page_text(&self) -> Option<String> {
        let src = self.layout_source.as_ref()?;
        let doc = src.document.lock().ok()?;
        let range = doc.get_selection().get_range().filter(|r| !r.is_collapsed())?;
        let hidden = self
            .layout_box
            .as_ref()
            .map(lumen_layout::user_select_none_text_nodes)
            .unwrap_or_default();
        let text = lumen_dom::range_text_filtered(&doc, &range, &|n| hidden.contains(&n));
        (!text.is_empty()).then_some(text)
    }

    /// Ctrl+C over a page-text selection: put it on the OS clipboard. Returns
    /// `true` when something was copied (the key is consumed).
    pub(crate) fn copy_page_selection(&mut self) -> bool {
        let Some(text) = self.selected_page_text() else { return false };
        use lumen_core::ext::ClipboardProvider;
        platform::clipboard::PlatformClipboard.write_text(&text);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_run_double_press_picks_the_word_in_chars() {
        // Char indices, not bytes: "привет мир" — cursor inside the second word.
        assert_eq!(field_run(TypeableField::Input, "привет мир", 8, 2), Some((7, 10)));
        assert_eq!(field_run(TypeableField::Input, "привет мир", 0, 2), Some((0, 6)));
        // End of value: the word before the cursor.
        assert_eq!(field_run(TypeableField::Input, "привет мир", 10, 2), Some((7, 10)));
        assert_eq!(field_run(TypeableField::Input, "", 0, 2), None);
    }

    #[test]
    fn field_run_triple_press_is_the_value_or_the_line() {
        assert_eq!(field_run(TypeableField::Input, "one two", 3, 3), Some((0, 7)));
        assert_eq!(field_run(TypeableField::Textarea, "ab\ncd ef\ngh", 5, 3), Some((3, 8)));
        assert_eq!(field_run(TypeableField::Textarea, "ab\ncd", 1, 3), Some((0, 2)));
        assert_eq!(field_run(TypeableField::Textarea, "ab\ncd", 5, 3), Some((3, 5)));
    }
}
