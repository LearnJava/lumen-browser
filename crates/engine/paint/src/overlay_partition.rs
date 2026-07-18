//! Overlay/band partition for compositor scroll-blit (ADR-016 M3.2.1c).
//!
//! The scroll-blit fast path (M3.2.1b) retains a rastered *band* of page content
//! and, on a [`Blit`] frame, re-presents it shifted by the scroll delta **without
//! re-executing the display list**. That is correct for ordinary in-flow content —
//! it scrolls 1:1 with the page — but it is *wrong* for content that is meant to
//! stay pinned to the viewport as the page scrolls: `position:sticky` and
//! `position:fixed`. Such content is rastered into the band at its seat-time
//! on-screen position and then dragged along by the wholesale shift, instead of
//! staying put. This is what M3.2.1c (this module) solves: it was the last blocker
//! before `LUMEN_SCROLL_BLIT` could default on (M3.2.1c-7).
//!
//! M3.2.1c splits this **overlay** content out of the band: the band is rastered
//! *without* it, and it is redrawn per frame on top of the presented band so its
//! per-frame scroll-clamped offset (computed at draw time — see
//! `BeginStickyLayer`) keeps it pinned. This module is the pure *decision* layer:
//! given a scroll-independent display list it reports which command index ranges
//! are overlay content. It holds no GPU state and executes nothing — the render
//! backend consumes the ranges to skip overlay commands while filling the band and
//! to replay them afterwards.
//!
//! # Scope of this slice (M3.2.1c-2)
//!
//! Both overlay kinds are now reported. `position:sticky` carries the
//! [`BeginStickyLayer`]/[`EndStickyLayer`] pair (with scroll-clamp insets), and
//! `position:fixed` carries the [`BeginFixedLayer`]/[`EndFixedLayer`] pair added
//! this slice — a payload-free bracket, since fixed content needs no draw-time
//! offset (it is already at viewport-fixed coordinates). A shared bracket family
//! folds both into [`overlay_ranges`], so a fixed layer nested inside a sticky one
//! (or vice versa) collapses into the enclosing outermost span. What remains for a
//! later slice is the *consuming* backend work: overlay **replay context**
//! (an overlay span may sit inside ancestor clip/transform state) and **z-order**
//! (overlay redrawn on top of the whole band).
//!
//! # Why index ranges, not extracted commands
//!
//! A returned [`Range`] is a **half-open span into the caller's own slice**
//! (`content[range]`), inclusive of the opening `BeginStickyLayer` and the closing
//! `EndStickyLayer`. Handing back borrowed indices keeps this layer allocation-
//! light and lets the backend iterate the band commands (everything *outside* the
//! ranges) and the overlay commands (everything *inside* them) over the same
//! `&[DisplayCommand]` without cloning. Nested sticky layers collapse into their
//! outermost span: the whole subtree is one overlay unit, replayed together so its
//! inner clip/transform context is preserved.
//!
//! [`Blit`]: crate::scroll_cache::ScrollFramePlan::Blit
//! [`BeginStickyLayer`]: crate::display_list::DisplayCommand::BeginStickyLayer
//! [`EndStickyLayer`]: crate::display_list::DisplayCommand::EndStickyLayer
//! [`BeginFixedLayer`]: crate::display_list::DisplayCommand::BeginFixedLayer
//! [`EndFixedLayer`]: crate::display_list::DisplayCommand::EndFixedLayer

use crate::display_list::DisplayCommand;
use std::ops::Range;

/// Report the command index ranges of viewport-pinned **overlay** content in a
/// scroll-independent display list (ADR-016 M3.2.1c).
///
/// Each returned [`Range`] spans one *outermost* overlay layer — a `position:sticky`
/// (`BeginStickyLayer`..=`EndStickyLayer`) or `position:fixed`
/// (`BeginFixedLayer`..=`EndFixedLayer`) bracket — inclusive of both markers
/// (`content[range]`, half-open, so `range.end` is one past the closing marker).
/// Nested overlay layers, of either kind, are absorbed into the enclosing span
/// rather than reported separately, so the whole overlay subtree is one replay
/// unit. Ranges are returned in ascending order and never overlap.
///
/// The scroll-blit backend uses these to raster the band *without* overlay content
/// (skip the commands inside any range) and to redraw that content per frame on
/// top of the presented band, where its draw-time scroll compensation re-pins it.
///
/// An unbalanced list (a close marker with no open bracket, or a bracket left open
/// at the end of the slice) is tolerated defensively: a stray close is ignored, and
/// an unclosed open extends to the end of the slice. A well-formed display list —
/// the only kind the emitters produce — is always balanced, so these branches are
/// belt-and-suspenders, not expected inputs.
#[must_use]
pub fn overlay_ranges(content: &[DisplayCommand]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut depth: u32 = 0;
    let mut start = 0usize;
    for (i, cmd) in content.iter().enumerate() {
        match cmd {
            // Both overlay kinds — `position:sticky` and `position:fixed` — open a
            // span. A shared depth counter treats them as one bracket family, so a
            // fixed layer nested inside a sticky one (or vice versa) collapses into
            // the enclosing outermost span, replayed together as a single unit.
            DisplayCommand::BeginStickyLayer { .. } | DisplayCommand::BeginFixedLayer => {
                // Only the outermost open records the span start; nested opens
                // just deepen the balance so the whole subtree stays as one unit.
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            DisplayCommand::EndStickyLayer | DisplayCommand::EndFixedLayer => {
                match depth {
                    // Stray close with nothing open — ignore (defensive).
                    0 => {}
                    // Closing the outermost layer completes one overlay span.
                    1 => {
                        depth = 0;
                        ranges.push(start..i + 1);
                    }
                    _ => depth -= 1,
                }
            }
            _ => {}
        }
    }
    // Defensive: an unclosed overlay layer extends to the end of the slice so the
    // backend still skips it from the band rather than half-including it.
    if depth > 0 {
        ranges.push(start..content.len());
    }
    ranges
}

/// `true` when `content` holds any viewport-pinned overlay content — i.e.
/// [`overlay_ranges`] would return a non-empty vector.
///
/// A cheap pre-check the backend can run before allocating: when this is `false`
/// the scroll-blit `Blit` fast path is unconditionally correct (no content needs
/// per-frame re-pinning), so the overlay replay can be skipped entirely.
#[must_use]
pub fn has_overlay(content: &[DisplayCommand]) -> bool {
    content.iter().any(|c| {
        matches!(
            c,
            DisplayCommand::BeginStickyLayer { .. } | DisplayCommand::BeginFixedLayer
        )
    })
}

/// The scroll-blit backend's per-frame overlay decision (ADR-016 M3.2.1c-3).
///
/// Produced by [`plan_overlays`] and consumed by the femtovg backend to route the
/// frame: raster the band without overlay content and replay it on top, or fall
/// back to the plain path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayPlan {
    /// No viewport-pinned overlay content — the blit fast path applies unchanged
    /// (the band is the whole display list, nothing is replayed).
    None,
    /// Overlay content present and **every** span sits at display-list nesting
    /// depth 0 — no ancestor clip / transform / opacity / filter / mask / scroll
    /// layer wraps it — so each span can be replayed in isolation on top of the
    /// band. The band raster skips these ranges; the backend replays them per
    /// frame after presenting the band, where their draw-time scroll compensation
    /// re-pins them. Ranges are the outermost spans [`overlay_ranges`] reports.
    Replay(Vec<Range<usize>>),
    /// Overlay content present but at least one span is nested under ancestor layer
    /// context this slice cannot reconstruct in an isolated replay — e.g. a
    /// `position:sticky` box inside an `overflow:scroll` container's
    /// `PushScrollLayer`, or under a clip/transform/opacity. The backend must fall
    /// back to the direct, un-blitted render for this frame, which draws the
    /// overlay inline with its full ancestor state (byte-identical to the pre-blit
    /// path). A later slice can lift this by capturing the ancestor stack.
    NestedFallback,
}

/// Classify the viewport-pinned overlay content of a scroll-independent display
/// list for the scroll-blit backend (ADR-016 M3.2.1c-3).
///
/// Returns [`OverlayPlan::None`] when there is no overlay content, [`OverlayPlan::Replay`]
/// when every overlay span sits at nesting depth 0 (replayable in isolation), and
/// [`OverlayPlan::NestedFallback`] when any span is wrapped by ancestor layer
/// context (clip / transform / opacity / blend / mask / filter / scroll) that an
/// isolated replay would drop.
///
/// The depth is the running balance of layer-opening vs layer-closing commands
/// ([`layer_delta`]). Because [`overlay_ranges`] reports only the *outermost*
/// overlay spans, no overlay bracket is ever open at a span's start, so the balance
/// there equals the ancestor (non-overlay) layer depth: `0` means top-level.
#[must_use]
pub fn plan_overlays(content: &[DisplayCommand]) -> OverlayPlan {
    let ranges = overlay_ranges(content);
    if ranges.is_empty() {
        return OverlayPlan::None;
    }
    let mut depth: i32 = 0;
    let mut next = 0usize; // index of the next span whose start we are watching for
    for (i, cmd) in content.iter().enumerate() {
        if next < ranges.len() && i == ranges[next].start {
            // At the opening marker of an outermost span: `depth` is the ancestor
            // layer depth here (all earlier overlay brackets are balanced). A
            // non-zero depth means this overlay is wrapped by state we cannot
            // reconstruct in an isolated replay.
            if depth != 0 {
                return OverlayPlan::NestedFallback;
            }
            next += 1;
        }
        depth += layer_delta(cmd);
    }
    OverlayPlan::Replay(ranges)
}

/// One replayable overlay span together with the ancestor **spatial** layer
/// commands that enclose it (ADR-016 M3.2.1c-4).
///
/// The M3.2.1c-3 backend can only replay overlay spans that sit at nesting depth 0;
/// anything wrapped by ancestor layer state takes the direct fallback. This carries
/// the missing context so a later slice can replay a nested overlay in isolation:
/// the enclosing ancestors are all *spatial* layers (clip / transform / scroll —
/// see [`is_spatial_layer_open`]), which composite per-primitive, so re-establishing
/// them around the overlay alone yields the same pixels as the direct render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySpan {
    /// Half-open command range of the overlay bracket itself (inclusive of the
    /// `Begin`/`End` markers) — the same span [`overlay_ranges`] reports.
    pub span: Range<usize>,
    /// Indices of the enclosing ancestor layer-opening commands, **outer→inner**.
    /// Empty for a top-level overlay. Every entry is a spatial layer open
    /// ([`is_spatial_layer_open`]); a replay executes these before `span` and emits
    /// their matching pops (in reverse) after, reconstructing the clip / transform /
    /// scroll state the direct path would have applied.
    pub ancestors: Vec<usize>,
}

/// Richer overlay classification that can also replay spans nested under **spatial**
/// ancestor layers, not only top-level ones (ADR-016 M3.2.1c-4).
///
/// Produced by [`plan_overlays_nested`]. Where [`plan_overlays`] (M3.2.1c-3) reports
/// [`OverlayPlan::NestedFallback`] for *any* nested overlay, this narrows the
/// fallback to overlays wrapped by a *compositing* group (opacity / blend / mask /
/// filter / backdrop-filter) whose group effect an isolated replay cannot reproduce;
/// overlays wrapped only by spatial layers become [`NestedOverlayPlan::Replay`] with
/// their ancestor context captured. Consumed by nobody yet — the backend wiring is a
/// later slice (mirrors the pure-first cadence of M3.2.1c-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NestedOverlayPlan {
    /// No viewport-pinned overlay content — the blit fast path applies unchanged.
    None,
    /// Every overlay span is replayable: it is either top-level (`ancestors` empty)
    /// or nested only under spatial layers whose indices are captured in the
    /// [`OverlaySpan`]. Spans are returned in ascending order.
    Replay(Vec<OverlaySpan>),
    /// At least one overlay span is nested under a *compositing* ancestor
    /// (opacity / blend / mask / filter / backdrop-filter) whose group effect an
    /// isolated replay cannot reproduce — the backend must render this frame
    /// directly (byte-identical to the pre-blit path). A stricter analogue of
    /// [`OverlayPlan::NestedFallback`] that fires only when a split would actually
    /// change pixels.
    Fallback,
}

/// Classify the viewport-pinned overlay content of a scroll-independent display list,
/// capturing the spatial ancestor context of any nested overlay (ADR-016 M3.2.1c-4).
///
/// Walks the list maintaining a stack of the currently-open *ancestor* (non-overlay)
/// layer indices. At each outermost overlay span start the ancestor stack is exactly
/// the layer context wrapping that overlay:
/// - all-spatial (clip / transform / scroll) → the span is replayable, its ancestor
///   indices captured in an [`OverlaySpan`];
/// - contains a compositing layer (opacity / blend / mask / filter / backdrop) →
///   [`NestedOverlayPlan::Fallback`], since compositing a group as one unit cannot be
///   reproduced by drawing the overlay separately.
///
/// Returns [`NestedOverlayPlan::None`] when there is no overlay content. Overlay
/// brackets never enter the ancestor stack (they *are* the spans, and nested overlays
/// collapse into the outermost via [`overlay_ranges`]), so the stack reflects only
/// enclosing clip/transform/scroll/compositing state.
#[must_use]
pub fn plan_overlays_nested(content: &[DisplayCommand]) -> NestedOverlayPlan {
    let ranges = overlay_ranges(content);
    if ranges.is_empty() {
        return NestedOverlayPlan::None;
    }
    // Indices of the ancestor (non-overlay) layers currently open. An overlay
    // bracket is skipped here: it delimits a span, it is not ancestor context.
    let mut stack: Vec<usize> = Vec::new();
    let mut spans: Vec<OverlaySpan> = Vec::with_capacity(ranges.len());
    let mut next = 0usize; // index of the next span whose start we watch for
    for (i, cmd) in content.iter().enumerate() {
        if next < ranges.len() && i == ranges[next].start {
            // A compositing ancestor cannot be split off around an isolated replay.
            if stack.iter().any(|&a| is_compositing_layer_open(&content[a])) {
                return NestedOverlayPlan::Fallback;
            }
            spans.push(OverlaySpan { span: ranges[next].clone(), ancestors: stack.clone() });
            next += 1;
        }
        // Maintain the ancestor stack, ignoring overlay brackets. Spatial and
        // compositing opens push their index; every layer close pops. Interior
        // layers of a span balance out before the span ends, so a later disjoint
        // span sees only its true enclosing ancestors.
        if is_overlay_bracket(cmd) {
            continue;
        }
        match layer_delta(cmd) {
            d if d > 0 => stack.push(i),
            d if d < 0 => {
                stack.pop();
            }
            _ => {}
        }
    }
    NestedOverlayPlan::Replay(spans)
}

/// `true` for an overlay bracket marker of either kind (`Begin`/`End` of
/// `position:sticky` or `position:fixed`). Such markers delimit overlay spans and
/// are excluded from the ancestor-layer stack in [`plan_overlays_nested`].
#[must_use]
fn is_overlay_bracket(cmd: &DisplayCommand) -> bool {
    matches!(
        cmd,
        DisplayCommand::BeginStickyLayer { .. }
            | DisplayCommand::BeginFixedLayer
            | DisplayCommand::EndStickyLayer
            | DisplayCommand::EndFixedLayer
    )
}

/// `true` for a command that opens a **spatial** ancestor layer — clip, transform,
/// or scroll layer. Spatial layers affect *where/how* each primitive draws and
/// composite per-primitive, so re-establishing them around an isolated overlay
/// replay reproduces the direct render exactly.
#[must_use]
pub fn is_spatial_layer_open(cmd: &DisplayCommand) -> bool {
    matches!(
        cmd,
        DisplayCommand::PushClipRect { .. }
            | DisplayCommand::PushClipRoundedRect { .. }
            | DisplayCommand::PushClipPath { .. }
            | DisplayCommand::PushTransform { .. }
            | DisplayCommand::PushScrollLayer { .. }
    )
}

/// `true` for a command that opens a **compositing** ancestor layer — opacity, blend
/// mode, mask, filter, or backdrop filter. Such a layer composites its whole subtree
/// as one group, so splitting an overlay out of it would change pixels; overlays
/// under one force [`NestedOverlayPlan::Fallback`].
#[must_use]
pub fn is_compositing_layer_open(cmd: &DisplayCommand) -> bool {
    matches!(
        cmd,
        DisplayCommand::PushOpacity { .. }
            | DisplayCommand::PushBlendMode { .. }
            | DisplayCommand::PushMaskImage { .. }
            | DisplayCommand::PushMaskLinearGradient { .. }
            | DisplayCommand::PushMaskRadialGradient { .. }
            | DisplayCommand::PushMaskConicGradient { .. }
            | DisplayCommand::PushMaskLayer { .. }
            | DisplayCommand::PushFilter { .. }
            | DisplayCommand::PushBackdropFilter { .. }
    )
}

/// The matching close command for a **spatial** ancestor layer open (ADR-016
/// M3.2.1c-5). Given a spatial-layer-opening command ([`is_spatial_layer_open`]),
/// returns the `Pop*` command that balances it in the display list:
/// clip opens (rect / rounded-rect / path) close with [`PopClip`], a
/// [`PushTransform`] closes with [`PopTransform`], and a [`PushScrollLayer`] closes
/// with [`PopScrollLayer`]. Returns `None` for any non-spatial-open command.
///
/// The scroll-blit replay uses this to reconstruct a nested overlay's ancestor
/// context: it executes the captured ancestor opens, then the overlay span, then the
/// matching closes in reverse — so the backend's `Push`/`Pop` bookkeeping (canvas
/// save/restore, clip stack, layer depth) stays balanced exactly as the direct path.
///
/// [`PopClip`]: DisplayCommand::PopClip
/// [`PushTransform`]: DisplayCommand::PushTransform
/// [`PopTransform`]: DisplayCommand::PopTransform
/// [`PushScrollLayer`]: DisplayCommand::PushScrollLayer
/// [`PopScrollLayer`]: DisplayCommand::PopScrollLayer
#[must_use]
pub fn spatial_layer_close(open: &DisplayCommand) -> Option<DisplayCommand> {
    match open {
        DisplayCommand::PushClipRect { .. }
        | DisplayCommand::PushClipRoundedRect { .. }
        | DisplayCommand::PushClipPath { .. } => Some(DisplayCommand::PopClip),
        DisplayCommand::PushTransform { .. } => Some(DisplayCommand::PopTransform),
        DisplayCommand::PushScrollLayer { .. } => Some(DisplayCommand::PopScrollLayer),
        _ => None,
    }
}

/// The nesting-balance contribution of a display command: `+1` for a command that
/// opens a rendering layer (clip, transform, opacity, blend, mask, filter,
/// backdrop filter, scroll layer, or an overlay bracket), `-1` for the matching
/// close, `0` for leaf/paint commands.
///
/// Used by [`plan_overlays`] to compute the ancestor layer depth at each overlay
/// span. Overlay brackets count too, but since [`overlay_ranges`] yields only
/// outermost spans they are always balanced before a span's start, so they do not
/// perturb the ancestor-depth reading. Keep this in sync with the `Push*`/`Pop*`
/// and `Begin*`/`End*` pairs in [`DisplayCommand`].
///
/// `pub(crate)`: also used by the femtovg backend (BUG-273 срез 1) to find the
/// matching close of an offscreen-composite bracket being culled off-viewport.
#[must_use]
pub(crate) fn layer_delta(cmd: &DisplayCommand) -> i32 {
    match cmd {
        DisplayCommand::PushClipRect { .. }
        | DisplayCommand::PushClipRoundedRect { .. }
        | DisplayCommand::PushClipPath { .. }
        | DisplayCommand::PushOpacity { .. }
        | DisplayCommand::PushBlendMode { .. }
        | DisplayCommand::PushMaskImage { .. }
        | DisplayCommand::PushMaskLinearGradient { .. }
        | DisplayCommand::PushMaskRadialGradient { .. }
        | DisplayCommand::PushMaskConicGradient { .. }
        | DisplayCommand::PushMaskLayer { .. }
        | DisplayCommand::PushTransform { .. }
        | DisplayCommand::PushFilter { .. }
        | DisplayCommand::PushBackdropFilter { .. }
        | DisplayCommand::PushScrollLayer { .. }
        | DisplayCommand::BeginStickyLayer { .. }
        | DisplayCommand::BeginFixedLayer => 1,
        DisplayCommand::PopClip
        | DisplayCommand::PopOpacity
        | DisplayCommand::PopBlendMode
        | DisplayCommand::PopMask
        | DisplayCommand::PopMaskLayer
        | DisplayCommand::PopTransform
        | DisplayCommand::PopFilter
        | DisplayCommand::PopBackdropFilter
        | DisplayCommand::PopScrollLayer
        | DisplayCommand::EndStickyLayer
        | DisplayCommand::EndFixedLayer => -1,
        _ => 0,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;

    /// A `BeginStickyLayer` marker with all-auto insets (the fields are irrelevant
    /// to partitioning — only the marker's presence and nesting matter).
    fn begin_sticky() -> DisplayCommand {
        DisplayCommand::BeginStickyLayer {
            flow_rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            top: None,
            bottom: None,
            left: None,
            right: None,
        }
    }

    /// A `BeginFixedLayer` marker — a payload-free bracket for `position:fixed`.
    fn begin_fixed() -> DisplayCommand {
        DisplayCommand::BeginFixedLayer
    }

    /// A trivial non-marker leaf command to stand in for band content.
    fn leaf() -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            color: lumen_layout::Color { r: 0, g: 0, b: 0, a: 255 },
        }
    }

    #[test]
    fn empty_list_has_no_overlay() {
        assert!(overlay_ranges(&[]).is_empty());
        assert!(!has_overlay(&[]));
    }

    #[test]
    fn list_without_sticky_has_no_overlay() {
        let dl = vec![leaf(), leaf(), leaf()];
        assert!(overlay_ranges(&dl).is_empty());
        assert!(!has_overlay(&dl));
    }

    #[test]
    fn single_sticky_span_is_reported_inclusive_of_markers() {
        // [ leaf, Begin, leaf, End, leaf ] → the span is indices 1..=3 (1..4).
        let dl = vec![
            leaf(),
            begin_sticky(),
            leaf(),
            DisplayCommand::EndStickyLayer,
            leaf(),
        ];
        assert_eq!(overlay_ranges(&dl), vec![1..4]);
        assert!(has_overlay(&dl));
        // The span brackets the markers themselves.
        assert!(matches!(dl[1], DisplayCommand::BeginStickyLayer { .. }));
        assert!(matches!(dl[3], DisplayCommand::EndStickyLayer));
    }

    #[test]
    fn two_disjoint_sticky_spans_are_reported_in_order() {
        let dl = vec![
            begin_sticky(),
            DisplayCommand::EndStickyLayer, // span 0..2
            leaf(),
            begin_sticky(),
            leaf(),
            DisplayCommand::EndStickyLayer, // span 3..6
        ];
        assert_eq!(overlay_ranges(&dl), vec![0..2, 3..6]);
    }

    #[test]
    fn nested_sticky_collapses_into_outer_span() {
        // Outer [1..8) contains a nested sticky; only the outer span is reported.
        let dl = vec![
            leaf(),
            begin_sticky(), // outer open  @1
            leaf(),
            begin_sticky(), // inner open  @3
            leaf(),
            DisplayCommand::EndStickyLayer, // inner close @5
            leaf(),
            DisplayCommand::EndStickyLayer, // outer close @7
        ];
        assert_eq!(overlay_ranges(&dl), vec![1..8]);
    }

    #[test]
    fn band_commands_are_everything_outside_the_ranges() {
        // Demonstrates the intended consumer usage: band = indices not covered by
        // any overlay range.
        let dl = vec![
            leaf(),                         // 0 band
            begin_sticky(),                 // 1 overlay
            leaf(),                         // 2 overlay
            DisplayCommand::EndStickyLayer, // 3 overlay
            leaf(),                         // 4 band
        ];
        let ranges = overlay_ranges(&dl);
        let band: Vec<usize> = (0..dl.len())
            .filter(|i| !ranges.iter().any(|r| r.contains(i)))
            .collect();
        assert_eq!(band, vec![0, 4]);
    }

    #[test]
    fn stray_end_marker_is_ignored() {
        let dl = vec![leaf(), DisplayCommand::EndStickyLayer, leaf()];
        assert!(overlay_ranges(&dl).is_empty());
    }

    #[test]
    fn unclosed_sticky_extends_to_end_of_slice() {
        let dl = vec![leaf(), begin_sticky(), leaf(), leaf()];
        assert_eq!(overlay_ranges(&dl), vec![1..4]);
    }

    #[test]
    fn unbalanced_nested_close_extends_outer_to_end() {
        // Outer never closes; the single inner close only decrements depth back to
        // the outer level, which stays open → the whole tail is one span.
        let dl = vec![
            begin_sticky(),                 // outer @0
            begin_sticky(),                 // inner @1
            DisplayCommand::EndStickyLayer, // inner close @2 (depth 2→1)
            leaf(),                         // @3
        ];
        assert_eq!(overlay_ranges(&dl), vec![0..4]);
    }

    // ── position:fixed (M3.2.1c-2) ───────────────────────────────────────────

    #[test]
    fn single_fixed_span_is_reported_inclusive_of_markers() {
        let dl = vec![
            leaf(),
            begin_fixed(),
            leaf(),
            DisplayCommand::EndFixedLayer,
            leaf(),
        ];
        assert_eq!(overlay_ranges(&dl), vec![1..4]);
        assert!(has_overlay(&dl));
        assert!(matches!(dl[1], DisplayCommand::BeginFixedLayer));
        assert!(matches!(dl[3], DisplayCommand::EndFixedLayer));
    }

    #[test]
    fn list_with_only_leaves_still_reports_no_fixed_overlay() {
        let dl = vec![leaf(), leaf()];
        assert!(!has_overlay(&dl));
        assert!(overlay_ranges(&dl).is_empty());
    }

    #[test]
    fn stray_end_fixed_marker_is_ignored() {
        let dl = vec![leaf(), DisplayCommand::EndFixedLayer, leaf()];
        assert!(overlay_ranges(&dl).is_empty());
    }

    #[test]
    fn unclosed_fixed_extends_to_end_of_slice() {
        let dl = vec![leaf(), begin_fixed(), leaf(), leaf()];
        assert_eq!(overlay_ranges(&dl), vec![1..4]);
    }

    #[test]
    fn disjoint_sticky_and_fixed_spans_reported_in_order() {
        let dl = vec![
            begin_sticky(),
            DisplayCommand::EndStickyLayer, // span 0..2
            leaf(),
            begin_fixed(),
            leaf(),
            DisplayCommand::EndFixedLayer, // span 3..6
        ];
        assert_eq!(overlay_ranges(&dl), vec![0..2, 3..6]);
    }

    #[test]
    fn fixed_nested_in_sticky_collapses_into_outer_span() {
        // A fixed layer inside a sticky one is one overlay unit: the shared bracket
        // family counts both kinds on the same depth, so only the outer span shows.
        let dl = vec![
            leaf(),
            begin_sticky(), // outer open  @1
            leaf(),
            begin_fixed(), // inner open  @3
            leaf(),
            DisplayCommand::EndFixedLayer, // inner close @5
            leaf(),
            DisplayCommand::EndStickyLayer, // outer close @7
        ];
        assert_eq!(overlay_ranges(&dl), vec![1..8]);
    }

    #[test]
    fn sticky_nested_in_fixed_collapses_into_outer_span() {
        let dl = vec![
            begin_fixed(),  // outer open  @0
            begin_sticky(), // inner open  @1
            DisplayCommand::EndStickyLayer, // inner close @2
            DisplayCommand::EndFixedLayer,  // outer close @3
        ];
        assert_eq!(overlay_ranges(&dl), vec![0..4]);
    }

    // ── plan_overlays (M3.2.1c-3) ────────────────────────────────────────────

    /// A `PushClipRect`/`PopClip` pair to stand in for ancestor layer context.
    fn push_clip() -> DisplayCommand {
        DisplayCommand::PushClipRect { rect: Rect::new(0.0, 0.0, 100.0, 100.0) }
    }

    #[test]
    fn plan_none_when_no_overlay() {
        assert_eq!(plan_overlays(&[]), OverlayPlan::None);
        assert_eq!(plan_overlays(&[leaf(), push_clip(), leaf(), DisplayCommand::PopClip]), OverlayPlan::None);
    }

    #[test]
    fn plan_replay_for_top_level_sticky() {
        let dl = vec![
            leaf(),
            begin_sticky(),
            leaf(),
            DisplayCommand::EndStickyLayer,
            leaf(),
        ];
        match plan_overlays(&dl) {
            OverlayPlan::Replay(r) => assert_eq!(r, vec![1..4]),
            other => panic!("expected Replay, got {other:?}"),
        }
    }

    #[test]
    fn plan_replay_for_top_level_fixed() {
        let dl = vec![begin_fixed(), leaf(), DisplayCommand::EndFixedLayer];
        match plan_overlays(&dl) {
            OverlayPlan::Replay(r) => assert_eq!(r, vec![0..3]),
            other => panic!("expected Replay, got {other:?}"),
        }
    }

    #[test]
    fn plan_replay_for_two_disjoint_top_level_spans() {
        // A clip that fully opens and closes *between* the spans keeps both at
        // depth 0, so the plan is still a clean replay.
        let dl = vec![
            begin_sticky(),
            DisplayCommand::EndStickyLayer, // span 0..2 at depth 0
            push_clip(),
            leaf(),
            DisplayCommand::PopClip, // balanced, back to depth 0
            begin_fixed(),
            DisplayCommand::EndFixedLayer, // span 5..7 at depth 0
        ];
        assert_eq!(plan_overlays(&dl), OverlayPlan::Replay(vec![0..2, 5..7]));
    }

    #[test]
    fn plan_nested_fallback_when_overlay_inside_clip() {
        // Sticky wrapped by an ancestor clip → depth 1 at the span start → the
        // isolated replay would drop the clip, so fall back.
        let dl = vec![
            push_clip(), // opens ancestor layer @0
            begin_sticky(), // @1, ancestor depth 1 here
            leaf(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopClip,
        ];
        assert_eq!(plan_overlays(&dl), OverlayPlan::NestedFallback);
    }

    #[test]
    fn plan_nested_fallback_when_overlay_inside_scroll_layer() {
        // The exact real-world case: sticky inside an overflow:scroll container.
        let dl = vec![
            DisplayCommand::PushScrollLayer {
                clip_rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                scroll_x: 0.0,
                scroll_y: 0.0,
            },
            begin_sticky(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopScrollLayer,
        ];
        assert_eq!(plan_overlays(&dl), OverlayPlan::NestedFallback);
    }

    #[test]
    fn plan_fallback_if_any_span_is_nested_even_when_another_is_top_level() {
        // First span top-level, second nested → the whole frame falls back (one
        // decision per frame).
        let dl = vec![
            begin_fixed(),
            DisplayCommand::EndFixedLayer, // span 0..2 depth 0
            push_clip(),
            begin_sticky(), // depth 1
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopClip,
        ];
        assert_eq!(plan_overlays(&dl), OverlayPlan::NestedFallback);
    }

    // ── plan_overlays_nested (M3.2.1c-4) ─────────────────────────────────────

    /// A `PushTransform`/`PopTransform` pair to stand in for a spatial ancestor.
    fn push_transform() -> DisplayCommand {
        DisplayCommand::PushTransform {
            matrix: lumen_layout::Mat4::translation_2d(5.0, 5.0),
        }
    }

    /// A `PushScrollLayer` to stand in for an `overflow:scroll` ancestor.
    fn push_scroll_layer() -> DisplayCommand {
        DisplayCommand::PushScrollLayer {
            clip_rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }

    /// A `PushOpacity`/`PopOpacity` pair to stand in for a compositing ancestor.
    fn push_opacity() -> DisplayCommand {
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None }
    }

    #[test]
    fn nested_plan_none_when_no_overlay() {
        assert_eq!(plan_overlays_nested(&[]), NestedOverlayPlan::None);
        assert_eq!(
            plan_overlays_nested(&[leaf(), push_clip(), leaf(), DisplayCommand::PopClip]),
            NestedOverlayPlan::None
        );
    }

    #[test]
    fn nested_plan_top_level_span_has_empty_ancestors() {
        let dl = vec![leaf(), begin_sticky(), leaf(), DisplayCommand::EndStickyLayer];
        assert_eq!(
            plan_overlays_nested(&dl),
            NestedOverlayPlan::Replay(vec![OverlaySpan { span: 1..4, ancestors: vec![] }])
        );
    }

    #[test]
    fn nested_plan_captures_single_clip_ancestor() {
        // Sticky wrapped by a clip → replayable, ancestor index captured (was a
        // blanket NestedFallback under `plan_overlays`).
        let dl = vec![
            push_clip(), // @0 spatial ancestor
            begin_sticky(),
            leaf(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopClip,
        ];
        assert_eq!(
            plan_overlays_nested(&dl),
            NestedOverlayPlan::Replay(vec![OverlaySpan { span: 1..4, ancestors: vec![0] }])
        );
        // Contrast with the M3.2.1c-3 classifier, which bails on any nesting.
        assert_eq!(plan_overlays(&dl), OverlayPlan::NestedFallback);
    }

    #[test]
    fn nested_plan_captures_scroll_layer_ancestor() {
        // The real-world case: sticky inside overflow:scroll is now replayable.
        let dl = vec![
            push_scroll_layer(), // @0
            begin_sticky(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopScrollLayer,
        ];
        assert_eq!(
            plan_overlays_nested(&dl),
            NestedOverlayPlan::Replay(vec![OverlaySpan { span: 1..3, ancestors: vec![0] }])
        );
    }

    #[test]
    fn nested_plan_captures_multiple_spatial_ancestors_outer_to_inner() {
        // clip → transform → sticky: both ancestors captured, outer first.
        let dl = vec![
            push_clip(),      // @0
            push_transform(), // @1
            begin_fixed(),
            DisplayCommand::EndFixedLayer,
            DisplayCommand::PopTransform,
            DisplayCommand::PopClip,
        ];
        assert_eq!(
            plan_overlays_nested(&dl),
            NestedOverlayPlan::Replay(vec![OverlaySpan { span: 2..4, ancestors: vec![0, 1] }])
        );
    }

    #[test]
    fn nested_plan_fallback_under_compositing_ancestor() {
        // Opacity group cannot be split → fallback, not a captured replay.
        let dl = vec![
            push_opacity(), // @0 compositing
            begin_sticky(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopOpacity,
        ];
        assert_eq!(plan_overlays_nested(&dl), NestedOverlayPlan::Fallback);
    }

    #[test]
    fn nested_plan_fallback_when_compositing_wraps_spatial() {
        // opacity → clip → sticky: the compositing ancestor still forces fallback
        // even though the innermost ancestor is spatial.
        let dl = vec![
            push_opacity(), // @0 compositing
            push_clip(),    // @1 spatial
            begin_sticky(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopClip,
            DisplayCommand::PopOpacity,
        ];
        assert_eq!(plan_overlays_nested(&dl), NestedOverlayPlan::Fallback);
    }

    #[test]
    fn nested_plan_two_disjoint_spans_each_carry_their_own_ancestors() {
        // First span under a clip, second top-level (the clip closed between them).
        let dl = vec![
            push_clip(),    // @0
            begin_sticky(), // span 1..3 under clip
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopClip, // @3 clip closes → back to depth 0
            begin_fixed(),           // span 4..6 top-level
            DisplayCommand::EndFixedLayer,
        ];
        assert_eq!(
            plan_overlays_nested(&dl),
            NestedOverlayPlan::Replay(vec![
                OverlaySpan { span: 1..3, ancestors: vec![0] },
                OverlaySpan { span: 4..6, ancestors: vec![] },
            ])
        );
    }

    #[test]
    fn nested_plan_interior_spatial_layers_do_not_leak_into_later_ancestors() {
        // A span with its own interior clip must not pollute the ancestor stack seen
        // by a later disjoint span.
        let dl = vec![
            begin_sticky(), // span 0..4
            push_clip(),    // interior @1
            leaf(),
            DisplayCommand::PopClip, // interior close @3
            DisplayCommand::EndStickyLayer,
            begin_fixed(), // span 5..7, top-level
            DisplayCommand::EndFixedLayer,
        ];
        assert_eq!(
            plan_overlays_nested(&dl),
            NestedOverlayPlan::Replay(vec![
                OverlaySpan { span: 0..5, ancestors: vec![] },
                OverlaySpan { span: 5..7, ancestors: vec![] },
            ])
        );
    }

    #[test]
    fn nested_plan_fallback_if_any_span_is_compositing_nested() {
        // One clean span, one under opacity → whole frame falls back.
        let dl = vec![
            begin_fixed(),
            DisplayCommand::EndFixedLayer, // span 0..2 top-level
            push_opacity(),                // @2 compositing
            begin_sticky(),
            DisplayCommand::EndStickyLayer,
            DisplayCommand::PopOpacity,
        ];
        assert_eq!(plan_overlays_nested(&dl), NestedOverlayPlan::Fallback);
    }

    // ── spatial_layer_close (M3.2.1c-5) ──────────────────────────────────────

    #[test]
    fn spatial_close_maps_every_spatial_open_to_its_pop() {
        // Each clip kind closes with PopClip; transform and scroll layer close with
        // their own pops. This is the mapping the replay uses to balance ancestors.
        assert_eq!(spatial_layer_close(&push_clip()), Some(DisplayCommand::PopClip));
        assert_eq!(
            spatial_layer_close(&DisplayCommand::PushClipRoundedRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                radii: [0.0; 4],
            }),
            Some(DisplayCommand::PopClip)
        );
        assert_eq!(spatial_layer_close(&push_transform()), Some(DisplayCommand::PopTransform));
        assert_eq!(
            spatial_layer_close(&push_scroll_layer()),
            Some(DisplayCommand::PopScrollLayer)
        );
    }

    #[test]
    fn spatial_close_is_none_for_non_spatial_opens() {
        // Every ancestor captured by `plan_overlays_nested` is a spatial open, so a
        // compositing open, an overlay bracket, or a leaf must return None (they are
        // never handed to `spatial_layer_close`, but the guard is belt-and-suspenders).
        assert_eq!(spatial_layer_close(&push_opacity()), None);
        assert_eq!(spatial_layer_close(&begin_sticky()), None);
        assert_eq!(spatial_layer_close(&leaf()), None);
        assert_eq!(spatial_layer_close(&DisplayCommand::PopClip), None);
    }

    #[test]
    fn spatial_close_covers_every_spatial_open_predicate() {
        // Guard against drift: any command `is_spatial_layer_open` accepts must have a
        // matching close, or the replay would leak an unbalanced Push.
        for cmd in [
            push_clip(),
            DisplayCommand::PushClipRoundedRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                radii: [0.0; 4],
            },
            DisplayCommand::PushClipPath {
                shape: crate::display_list::ResolvedClipShape::Circle { cx: 0.0, cy: 0.0, r: 1.0 },
            },
            push_transform(),
            push_scroll_layer(),
        ] {
            assert!(is_spatial_layer_open(&cmd));
            assert!(
                spatial_layer_close(&cmd).is_some(),
                "spatial open {cmd:?} has no matching close"
            );
        }
    }
}
