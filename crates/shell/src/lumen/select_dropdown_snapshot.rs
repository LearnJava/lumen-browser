//! BUG-1109: what the redraw path knows about the open `<select>` dropdown,
//! read without ever blocking on the document mutex.
//!
//! The engine thread holds `Document` for a whole off-thread relayout —
//! 11-16 s on github.com (BUG-683 срез 7). The dropdown overlay used to
//! `lock().unwrap()` the document every frame to collect the options (and,
//! for `appearance: base-select`, their author styles), so with a list open
//! the window froze for the whole cascade, and a poisoned lock took the shell
//! down. The frame now refreshes this snapshot with `try_lock` and paints from
//! it; while the lock is contended the previous reading for the same document
//! and `<select>` stands in, and the relayout's own commit repaints once it is
//! released. Geometry (anchor, scroll, viewport) is not cached — it is applied
//! every frame by [`SelectDropdownSnapshot::build`]. Same shape as
//! [`super::FocusedFieldSnapshot`].
//!
//! One snapshot serves the page's dropdown and a frame's (FRAME-6): the two
//! are mutually exclusive, and the document key tells their readings apart.

use std::sync::{Arc, Mutex};

use lumen_core::geom::{Rect, Size};
use lumen_layout::ComputedStyle;
use lumen_paint::DisplayList;

use crate::forms::{self, OptionRowStyle, SelectOption};
use crate::*;

/// Last document reading about the open `<select>`, taken by the redraw path.
#[derive(Debug, Default)]
pub(crate) struct SelectDropdownSnapshot {
    /// `Arc` address of the document it was read from — a navigation, tab
    /// switch or page↔frame change alters it, so a reading never leaks.
    doc_key: usize,
    /// The `<select>` it describes; `None` = empty snapshot.
    sel: Option<NodeId>,
    /// [`forms::collect_select_options`] of `sel`.
    options: Vec<SelectOption>,
    /// [`forms::resolve_option_row_styles`] of `options`; `None` when the
    /// reading was taken for the native (non-base-select) dropdown.
    row_styles: Option<Vec<OptionRowStyle>>,
}

impl SelectDropdownSnapshot {
    /// Re-read `sel` from `document` if its lock is free right now; resolve
    /// the per-row author styles only when `base_style` (the `<select>`'s
    /// computed style under `appearance: base-select`) is given. While the
    /// lock is held elsewhere — or poisoned — the previous reading is kept
    /// when it describes the same document and `<select>`, and dropped
    /// otherwise: a stale list is better than a frozen window, another
    /// select's list is not.
    pub(crate) fn refresh(
        &mut self,
        document: &Arc<Mutex<Document>>,
        sheet: &lumen_css_parser::Stylesheet,
        sel: NodeId,
        base_style: Option<&ComputedStyle>,
        viewport: Size,
        dark_mode: bool,
    ) {
        let key = Arc::as_ptr(document) as usize;
        if let Ok(doc) = document.try_lock() {
            let options = forms::collect_select_options(&doc, sel);
            let row_styles = base_style.map(|style| {
                forms::resolve_option_row_styles(&doc, sheet, style, &options, viewport, dark_mode)
            });
            *self = Self { doc_key: key, sel: Some(sel), options, row_styles };
        } else if self.doc_key != key || self.sel != Some(sel) {
            *self = Self::default();
        }
    }

    /// The dropdown overlay for the current reading, anchored at `anchor`.
    /// A base-select `<select>` whose reading has no row styles yet (it was
    /// taken before the appearance changed) falls back to the native chrome;
    /// an empty snapshot yields an empty list.
    pub(crate) fn build(
        &self,
        anchor: Rect,
        base_style: Option<&ComputedStyle>,
        scroll_y: f32,
        viewport: Size,
    ) -> DisplayList {
        let (vp_w, vp_h) = (viewport.width, viewport.height);
        match (base_style, &self.row_styles) {
            (Some(style), Some(rows)) => forms::build_base_select_dropdown(
                anchor, style, &self.options, rows, scroll_y, vp_w, vp_h,
            ),
            _ => forms::build_select_dropdown(anchor, &self.options, scroll_y, vp_w, vp_h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: Size = Size { width: 1024.0, height: 720.0 };

    fn doc_with_select() -> (Arc<Mutex<Document>>, NodeId) {
        let doc = lumen_html_parser::parse(
            r#"<body><select id="s"><option>Apple</option><option>Banana</option></select></body>"#,
        );
        let sel = doc.find_by_id("s").unwrap_or_else(|| unreachable!());
        (Arc::new(Mutex::new(doc)), sel)
    }

    fn anchor() -> Rect {
        Rect::new(10.0, 10.0, 100.0, 22.0)
    }

    #[test]
    fn free_lock_reads_the_options() {
        let (doc, sel) = doc_with_select();
        let sheet = lumen_css_parser::parse("");
        let mut snap = SelectDropdownSnapshot::default();
        snap.refresh(&doc, &sheet, sel, None, VP, false);
        let labels: Vec<_> = snap.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Apple", "Banana"]);
        assert!(snap.row_styles.is_none());
        assert!(!snap.build(anchor(), None, 0.0, VP).is_empty());
    }

    #[test]
    fn base_select_resolves_row_styles() {
        let (doc, sel) = doc_with_select();
        let sheet = lumen_css_parser::parse("option { background-color: rgb(10, 20, 30); }");
        let style = ComputedStyle::root();
        let mut snap = SelectDropdownSnapshot::default();
        snap.refresh(&doc, &sheet, sel, Some(&style), VP, false);
        assert_eq!(snap.row_styles.as_ref().map(Vec::len), Some(2));
    }

    /// The defect itself: with the document held by another thread (the
    /// engine's off-thread relayout), `refresh` must return at once and keep
    /// painting the last reading — before the fix the frame blocked on
    /// `lock()` until the holder let go.
    #[test]
    fn contended_lock_returns_immediately_with_previous_reading() {
        let (doc, sel) = doc_with_select();
        let sheet = lumen_css_parser::parse("");
        let mut snap = SelectDropdownSnapshot::default();
        snap.refresh(&doc, &sheet, sel, None, VP, false);

        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let holder_doc = Arc::clone(&doc);
        let holder = std::thread::spawn(move || {
            let _guard = holder_doc.lock();
            let _ = locked_tx.send(());
            let _ = release_rx.recv();
        });
        let _ = locked_rx.recv();

        snap.refresh(&doc, &sheet, sel, None, VP, false);
        assert_eq!(snap.options.len(), 2);

        // Another `<select>` has no reading to fall back on.
        let other = NodeId::from_index(sel.index() + 1000);
        snap.refresh(&doc, &sheet, other, None, VP, false);
        assert!(snap.options.is_empty());
        assert!(snap.build(anchor(), None, 0.0, VP).is_empty());

        let _ = release_tx.send(());
        let _ = holder.join();
    }

    /// A poisoned lock (a panic in a thread that held the document) must not
    /// take the frame down — the pre-fix `unwrap()` did.
    #[test]
    fn poisoned_lock_does_not_panic() {
        let (doc, sel) = doc_with_select();
        let poisoner = Arc::clone(&doc);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock();
            panic!("poison the document lock");
        })
        .join();
        assert!(doc.is_poisoned());

        let sheet = lumen_css_parser::parse("");
        let mut snap = SelectDropdownSnapshot::default();
        snap.refresh(&doc, &sheet, sel, None, VP, false);
        assert!(snap.build(anchor(), None, 0.0, VP).is_empty());
    }

    #[test]
    fn another_document_does_not_inherit_the_reading() {
        let (doc, sel) = doc_with_select();
        let sheet = lumen_css_parser::parse("");
        let mut snap = SelectDropdownSnapshot::default();
        snap.refresh(&doc, &sheet, sel, None, VP, false);

        let (other_doc, _) = doc_with_select();
        let _guard = other_doc.lock();
        snap.refresh(&other_doc, &sheet, sel, None, VP, false);
        assert!(snap.options.is_empty());
    }
}
