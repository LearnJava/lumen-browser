//! Read-only walks over a built layout tree.
//!
//! Sibling of [`crate::layout_metrics`] (which counts boxes) and of
//! [`crate::display_list_metrics`] (which measures the emitted display list):
//! everything here traverses the `LayoutBox` tree itself, collecting styles for
//! the transition scheduler, promoting `will-change` nodes to GPU layers and
//! finding the `<video>` the picture-in-picture window embeds.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// Р РµРєСѓСЂСЃРёРІРЅРѕ СЃРѕР±РёСЂР°РµС‚ `ComputedStyle` РІСЃРµС… СѓР·Р»РѕРІ layout-РґРµСЂРµРІР°.
/// Р РµР·СѓР»СЊС‚Р°С‚ РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ `transition_scheduler.sync()` РґР»СЏ СЃСЂР°РІРЅРµРЅРёСЏ
/// РїСЂРµРґС‹РґСѓС‰РµРіРѕ Рё РЅРѕРІРѕРіРѕ СЃС‚РёР»СЏ РїРѕСЃР»Рµ РєР°Р¶РґРѕРіРѕ relayout-Р°.
pub(crate) fn collect_box_styles(lb: &LayoutBox, map: &mut HashMap<NodeId, ComputedStyle>) {
    // BUG-341 S12: `LayoutBox::style` is now an `Arc`, but the transition
    // scheduler owns its snapshot (it diffs it against the next frame), so this
    // stays a deep copy. Sharing it here would need `prev_styles` to hold `Arc`s
    // too вЂ” a page-pipeline follow-up, not part of this slice's measured path.
    map.insert(lb.node, (*lb.style).clone());
    for child in &lb.children {
        collect_box_styles(child, map);
    }
}

/// Traverse the layout tree and promote nodes with `will-change: transform/opacity/filter`
/// to their own GPU layers via `RenderBackend::promote_layer`.
///
/// Called after every relayout so the promoted-layer set stays current.
/// Nodes removed from the DOM are cleaned up automatically by `sync_promoted_layers`
/// (called by each backend's `promote_layer` impl via `LayerCache`).
pub(crate) fn promote_will_change_layers(lb: &LayoutBox, renderer: &mut dyn RenderBackend) {
    promote_will_change_rec(lb, renderer);
}

fn promote_will_change_rec(lb: &LayoutBox, renderer: &mut dyn RenderBackend) {
    let needs_layer = lb.style.will_change.iter().any(|p| {
        matches!(p.as_str(), "transform" | "opacity" | "filter")
    });
    if needs_layer {
        let w = lb.rect.width.max(1.0) as u32;
        let h = lb.rect.height.max(1.0) as u32;
        renderer.promote_layer(lb.node.index() as u32, w, h);
    }
    for child in &lb.children {
        promote_will_change_rec(child, renderer);
    }
}

/// Find the first `<video>` element in the layout tree (depth-first, document
/// order) and return its `(src, poster)` URLs.  Used by the picture-in-picture
/// window (task #21) to pick a video to embed.  Returns `None` when the page
/// has no `<video>`.
pub(crate) fn find_video_source(lb: &LayoutBox) -> Option<(String, String)> {
    if let lumen_layout::BoxKind::Video { src, poster } = &lb.kind {
        return Some((src.clone(), poster.clone()));
    }
    for child in &lb.children {
        if let Some(found) = find_video_source(child) {
            return Some(found);
        }
    }
    None
}
