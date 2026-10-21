//! Highlight overlays on text (CSS Pseudo-Elements L4 §3.6): `::selection`,
//! `::target-text`, `::spelling-error`, `::grammar-error`, `::highlight()`.
//!
//! The highlights are resolved before painting (`lumen_layout::resolve_highlights`) and
//! handed to the builder through [`with_highlights`], the same way the fixed-background
//! viewport is. A fragment a highlight touches arrives here cut into pieces; the
//! uncovered pieces paint as ordinary text, the covered ones paint the originating
//! element's shadows and decorations, then each highlight on top, lowest layer first:
//! its background, its shadows, its glyphs, its decorations.

use std::cell::RefCell;
use std::sync::Arc;

use lumen_layout::{HighlightPiece, Highlights};

use super::text_run::{emit_frag_text, FragGeometry, TextParts};
use super::*;

thread_local! {
    static ACTIVE: RefCell<Option<Arc<Highlights>>> = const { RefCell::new(None) };
}

/// Runs `f` with `highlights` in effect for every display list built on this thread
/// inside it — `None` paints no highlight. Restores the previous value on exit.
pub fn with_highlights<R>(highlights: Option<Arc<Highlights>>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<Highlights>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.with(|a| *a.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(ACTIVE.with(|a| a.replace(highlights)));
    f()
}

/// The highlights in effect, if any.
pub(super) fn active_highlights() -> Option<Arc<Highlights>> {
    ACTIVE.with(|a| a.borrow().clone())
}

/// A piece of `frag`: its text, position and width.
fn piece_frag(frag: &InlineFrag, piece: &HighlightPiece) -> InlineFrag {
    let mut f = frag.clone();
    f.text = frag.text[piece.bytes.clone()].to_string();
    f.x = frag.x + piece.x;
    f.width = piece.width;
    f.merged_sources = Vec::new();
    f.padding_left = 0.0;
    f.padding_right = 0.0;
    f
}

/// Emits `frag`, cut into `pieces`, with its highlights.
pub(super) fn emit_highlighted_frag(
    out: &mut Vec<DisplayCommand>,
    frag: &InlineFrag,
    pieces: &[HighlightPiece],
    geo: &FragGeometry,
) {
    let parts: Vec<InlineFrag> = pieces.iter().map(|p| piece_frag(frag, p)).collect();
    // The originating element's shadows go under everything.
    for f in &parts {
        emit_frag_text(out, f, geo, TextParts::SHADOWS);
    }
    for (piece, f) in pieces.iter().zip(&parts) {
        if piece.looks.is_empty() {
            emit_frag_text(out, f, geo, TextParts::GLYPHS);
            continue;
        }
        // Underlying text decorations stay; the underlying glyphs are replaced by the
        // highlight's, which paint in the highlight's colour.
        super::svg_text_decoration::push_text_decoration(out, geo.container_x, geo.frag_y, f);
        for look in &piece.looks {
            if let Some(bg) = look.background {
                // Same pixel snapping as an inline element's background.
                let x = (geo.container_x + f.x).round();
                let w = ((geo.container_x + f.x + f.width).round() - x).max(0.0);
                if w > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: Rect::new(x, geo.line_y.round(), w, geo.line_h.round()), color: bg });
                }
            }
            let mut lf = f.clone();
            lf.style = Arc::new(look.apply_to(&f.style));
            emit_frag_text(out, &lf, geo, TextParts::ALL);
        }
    }
}
