//! Layout-tree counters used as render-health metrics (PERF-6).
//!
//! Sibling of [`crate::display_list_metrics`], which measures the *display list*;
//! these two walk the laid-out box tree instead, so a change to one says nothing
//! about the other. Moved out of `main.rs` by the SPLIT track (batch SH-3a);
//! behaviour and signatures are unchanged.

/// PERF-6: recursively count every box in a laid-out tree (render-health metric).
pub(crate) fn count_layout_boxes(b: &lumen_layout::LayoutBox) -> usize {
    1 + b.children.iter().map(count_layout_boxes).sum::<usize>()
}

/// PERF-6: count "rendered units" вЂ” things that actually paint: non-whitespace
/// characters across inline text runs plus replaced elements
/// (`<img>`/`<canvas>`/`<video>`/`<iframe>`). Zero means the page painted
/// nothing visible, which for a content-bearing DOM signals a white screen.
pub(crate) fn count_rendered_units(b: &lumen_layout::LayoutBox) -> usize {
    use lumen_layout::BoxKind;
    let mut n = match &b.kind {
        BoxKind::InlineRun { segments, .. } => segments
            .iter()
            .map(|s| s.text.chars().filter(|c| !c.is_whitespace()).count())
            .sum(),
        BoxKind::Image { .. }
        | BoxKind::Video { .. }
        | BoxKind::Canvas { .. }
        | BoxKind::Iframe { .. } => 1,
        _ => 0,
    };
    for c in &b.children {
        n += count_rendered_units(c);
    }
    n
}
