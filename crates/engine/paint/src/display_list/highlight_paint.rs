//! Highlight overlays on text (CSS Pseudo-Elements L4 §3.6): `::selection`,
//! `::target-text`, `::spelling-error`, `::grammar-error`, `::highlight()`.
//!
//! The highlights are resolved before painting (`lumen_layout::resolve_highlights`) and
//! handed to the builder through [`with_highlights`], the same way the fixed-background
//! viewport is. A fragment a highlight touches arrives here cut into pieces; the
//! uncovered pieces paint as ordinary text; the originating element's shadows paint first
//! as one run, then the backgrounds of the lowest highlight of every piece, then the glyphs:
//! plain pieces as ordinary text, covered pieces with the element's decorations and each
//! highlight on top, lowest layer first — its shadows, its glyphs, its decorations (a
//! higher layer fills its own background first).

use std::cell::RefCell;
use std::sync::Arc;

use lumen_layout::{HighlightLook, HighlightPiece, Highlights};

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

/// Fills the background of `look` behind `f`, snapped like an inline element's.
fn fill_background(out: &mut Vec<DisplayCommand>, look: &HighlightLook, f: &InlineFrag, geo: &FragGeometry) {
    let Some(color) = look.background else { return };
    let x = (geo.container_x + f.x).round();
    let w = ((geo.container_x + f.x + f.width).round() - x).max(0.0);
    if w > 0.0 {
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, geo.frag_y.round(), w, geo.line_h.round()),
            color,
        });
    }
}

/// Shadows, glyphs and decorations of `look` over `f`.
fn emit_look_text(out: &mut Vec<DisplayCommand>, look: &HighlightLook, f: &InlineFrag, geo: &FragGeometry) {
    let mut lf = f.clone();
    lf.style = Arc::new(look.apply_to(&f.style));
    emit_frag_text(out, &lf, geo, TextParts::ALL);
}

/// Emits `frag`, cut into `pieces`, with its highlights.
pub(super) fn emit_highlighted_frag(
    out: &mut Vec<DisplayCommand>,
    frag: &InlineFrag,
    pieces: &[HighlightPiece],
    geo: &FragGeometry,
) {
    let parts: Vec<InlineFrag> = pieces.iter().map(|p| piece_frag(frag, p)).collect();
    // The originating element's shadows go under everything, as one run: the glyphs of a
    // shadow are the rasterizer's own advance of the whole text, not the cut the highlight
    // makes (the cut positions come from the layout measurer).
    emit_frag_text(out, frag, geo, TextParts::SHADOWS);
    // Then the backgrounds of the lowest highlight of every piece, all before any glyph,
    // so that a glyph overhanging into the highlighted stretch stays visible over it.
    for (piece, f) in pieces.iter().zip(&parts) {
        if let Some(look) = piece.looks.first() {
            fill_background(out, look, f, geo);
        }
    }
    for (piece, f) in pieces.iter().zip(&parts) {
        let Some((lowest, upper)) = piece.looks.split_first() else {
            emit_frag_text(out, f, geo, TextParts::GLYPHS);
            continue;
        };
        // Underlying text decorations stay; the underlying glyphs are replaced by the
        // highlight's, which paint in the highlight's colour — and so do the decorations
        // whose colour is `currentcolor` (CSS Pseudo-Elements L4 §3.6).
        let mut under = f.clone();
        if let Some(c) = piece.looks.iter().rev().find_map(|l| l.color) {
            let mut style = (*under.style).clone();
            style.color = c;
            under.style = Arc::new(style);
        }
        super::svg_text_decoration::push_text_decoration(out, geo.container_x, geo.frag_y, &under);
        emit_look_text(out, lowest, f, geo);
        for look in upper {
            fill_background(out, look, f, geo);
            emit_look_text(out, look, f, geo);
        }
    }
}
