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
use lumen_core::geom::Rect;
use lumen_dom::Range;
use lumen_layout::{Color, SelectScope, TextMeasurer};
use lumen_paint::{DisplayCommand, DisplayList};

/// Fallback highlight when no `::selection { background-color }` applies —
/// same colour the form-control selection and `::target-text` default use.
const SELECTION_FILL_DEFAULT: Color = Color { r: 0x30, g: 0x8a, b: 0xff, a: 110 };

/// An armed page-text drag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocSelectDrag {
    /// `user-select: contain` scope the focus end is clamped into.
    pub(crate) contain: Option<Range>,
}

/// Inserts one selection `FillRect` per `rects` entry directly before the
/// `DrawText` the rect belongs to (the last text command on the same line that
/// starts at or left of the rect), so the highlight sits above the box
/// backgrounds and below the glyphs — the `::selection` paint order. A rect
/// with no matching text goes to the front of the list.
pub(crate) fn build_page_with_selection_highlight(
    base: &DisplayList,
    rects: &[Rect],
    color: Color,
) -> DisplayList {
    if rects.is_empty() {
        return base.clone();
    }
    // (insert-before index, rect) for every selection rect.
    let mut inserts: Vec<(usize, Rect)> = Vec::with_capacity(rects.len());
    for r in rects {
        let mut best: Option<(usize, f32)> = None;
        for (i, cmd) in base.iter().enumerate() {
            let DisplayCommand::DrawText { rect, .. } = cmd else { continue };
            if (rect.y - r.y).abs() > 0.5 || rect.x > r.x + 0.5 {
                continue;
            }
            if best.is_none_or(|(_, bx)| rect.x > bx) {
                best = Some((i, rect.x));
            }
        }
        inserts.push((best.map_or(0, |(i, _)| i), *r));
    }
    inserts.sort_by_key(|(i, _)| *i);
    let mut out: DisplayList = Vec::with_capacity(base.len() + rects.len());
    let mut next = 0;
    for (idx, cmd) in base.iter().enumerate() {
        while next < inserts.len() && inserts[next].0 == idx {
            out.push(DisplayCommand::FillRect { rect: inserts[next].1, color });
            next += 1;
        }
        out.push(cmd.clone());
    }
    out
}

/// Runs `f` with the bundled-font measurer, `None` only if the bundled font
/// fails to parse (a corrupted binary) — a hit test must not `unwrap` there.
fn with_measurer<R>(f: impl FnOnce(&dyn TextMeasurer) -> R) -> Option<R> {
    let font = lumen_font::Font::parse(INTER_FONT).ok()?;
    let m = lumen_paint::FontMeasurer::new(&font).ok()?;
    Some(f(&m))
}

/// (идентификатор документа, прямоугольники, цвет).
type SelectionOverlayCache = Option<(usize, (Vec<Rect>, Color))>;

thread_local! {
    /// Последний результат `doc_selection_overlay` с идентификатором документа.
    static LAST_SELECTION_OVERLAY: std::cell::RefCell<SelectionOverlayCache> =
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

    /// Page-space rectangles of the current document selection and the fill
    /// colour, `None` when nothing is selected. The colour honours a page
    /// `::selection { background-color }` rule on the range's start element.
    pub(crate) fn doc_selection_overlay(&self) -> Option<(Vec<Rect>, Color)> {
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
                return LAST_SELECTION_OVERLAY.with(|c| {
                    c.borrow().as_ref().filter(|(id, _)| *id == doc_id).map(|(_, v)| v.clone())
                });
            }
            Err(_) => return None,
        };
        let result = Self::selection_overlay_locked(self, &doc, src, lb);
        LAST_SELECTION_OVERLAY.with(|c| *c.borrow_mut() = result.clone().map(|v| (doc_id, v)));
        result
    }

    fn selection_overlay_locked(
        &self,
        doc: &lumen_dom::Document,
        src: &LayoutSource,
        lb: &lumen_layout::LayoutBox,
    ) -> Option<(Vec<Rect>, Color)> {
        let range = doc.get_selection().get_range().filter(|r| !r.is_collapsed())?;
        let rects = with_measurer(|m| lumen_layout::selection_rects(lb, &range, m))?;
        if rects.is_empty() {
            return None;
        }
        let color = (|| {
            let viewport = self.relayout_viewport()?;
            let elem = doc.get(range.start.container).parent.unwrap_or(range.start.container);
            let style = lumen_layout::compute_selection_style(
                doc,
                elem,
                &src.stylesheet,
                &lumen_layout::ComputedStyle::root(),
                viewport,
                self.dark_mode,
            )?;
            style.background_color.map(|c| c.resolve(style.color))
        })()
        .unwrap_or(SELECTION_FILL_DEFAULT);
        Some((rects, color))
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

    fn text(x: f32, y: f32) -> DisplayCommand {
        DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: Rect::new(x, y, 400.0, 20.0),
            text: "t".to_string(),
            font_size: 16.0,
            color: Color::BLACK,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::NORMAL,
            font_style: lumen_layout::FontStyle::Normal,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        }
    }

    fn fill(x: f32) -> DisplayCommand {
        DisplayCommand::FillRect { rect: Rect::new(x, 0.0, 1.0, 1.0), color: Color::BLACK }
    }

    #[test]
    fn highlight_sits_before_its_own_text_and_after_backgrounds() {
        let base = vec![fill(0.0), text(10.0, 0.0), text(60.0, 0.0), text(10.0, 20.0)];
        let sel = Rect::new(70.0, 0.0, 30.0, 20.0);
        let out = build_page_with_selection_highlight(&base, &[sel], SELECTION_FILL_DEFAULT);
        assert_eq!(out.len(), 5);
        // background first, then the first word, then the highlight right
        // before the word it starts in (x = 60), then the rest.
        assert!(matches!(out[0], DisplayCommand::FillRect { color, .. } if color == Color::BLACK));
        assert!(matches!(out[1], DisplayCommand::DrawText { .. }));
        assert!(matches!(out[2], DisplayCommand::FillRect { rect, .. } if rect == sel));
        assert!(matches!(out[3], DisplayCommand::DrawText { rect, .. } if rect.x == 60.0));
    }

    #[test]
    fn highlight_without_matching_text_goes_to_the_front() {
        let base = vec![text(10.0, 100.0)];
        let sel = Rect::new(10.0, 0.0, 30.0, 20.0);
        let out = build_page_with_selection_highlight(&base, &[sel], SELECTION_FILL_DEFAULT);
        assert!(matches!(out[0], DisplayCommand::FillRect { rect, .. } if rect == sel));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn no_rects_leaves_the_list_untouched() {
        let base = vec![text(10.0, 0.0)];
        assert_eq!(build_page_with_selection_highlight(&base, &[], SELECTION_FILL_DEFAULT), base);
    }
}
