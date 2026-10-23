//! Итератор кликабельных элементов дерева раскладки (оверлей подсказок
//! кликов P3, §12.14 task 7B.2). Вынесено из `lib.rs` без изменений.

use crate::style::PointerEvents;
use crate::{box_tree, LayoutBox};

// ─── Clickable elements iterator (for P3 click-hint overlay, §12.14 task 7B.2) ──

/// Classification of an interactive element found during layout-tree traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickableKind {
    /// `<a href="…">` hyperlink (block-level or inline).
    Link {
        /// Raw `href` value, not yet resolved against base URL.
        href: String,
    },
    /// `<button>` or `<input type=submit|button|reset>`.
    Button,
    /// Text/number/file/etc. `<input>`, `<textarea>`, `<select>`.
    Input,
    /// `<details>` disclosure element (opening/closing the summary).
    Details,
    /// Element with `tabindex` >= 0 that doesn't fit other categories.
    Generic,
}

/// An interactive element with its screen-space bounding rect.
///
/// `rect` is the border-box of the element in CSS px, as computed by layout.
/// Used by P3's click-hint overlay to render keyboard-navigable hint badges.
#[derive(Debug, Clone)]
pub struct ClickableElement {
    /// DOM node that owns this interactive region.
    pub node_id: lumen_dom::NodeId,
    /// Border-box rectangle in CSS px (document-relative, before scroll).
    pub rect: lumen_core::geom::Rect,
    /// Short text label for the hint badge (link text, button label, etc.).
    /// `None` when no usable label could be extracted.
    pub hint_text: Option<String>,
    /// Interaction kind — used by P3 to assign the correct hint key and action.
    pub kind: ClickableKind,
}

/// Collect all interactive elements from the layout tree in document order.
///
/// Walks the layout tree and returns every element that the user can
/// activate: links, buttons, form controls, and elements with `tabindex`.
/// Skipped boxes (`display: none`) and their children are omitted entirely.
///
/// For inline `<a href>` links, the returned `rect` is a bounding box of
/// all inline fragments belonging to that link on its first line; multi-line
/// links produce one entry per distinct link element (full-line bbox).
pub fn collect_clickable_elements(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
) -> Vec<ClickableElement> {
    let mut out = Vec::new();
    collect_clickable_rec(root, doc, &mut out);
    out
}

/// Explicit heap-stack pre-order walk (LAYOUT-2 срез 2), not native
/// recursion: nothing after the loop over `children` reads a value the walk
/// produced, so this is the same safe mechanical class LAYOUT-1 already
/// converted. Children are pushed in reverse to keep the LIFO stack's pop
/// order identical to the old left-to-right recursion (the inline-link
/// merging logic below relies on visiting frags of one `InlineRun` in order,
/// which is unaffected — it iterates `lines`/`line`, not `children`).
fn collect_clickable_rec(
    root: &LayoutBox,
    doc: &lumen_dom::Document,
    out: &mut Vec<ClickableElement>,
) {
    use box_tree::{BoxKind, FormControlKind};
    use lumen_core::geom::Rect;

    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
    if matches!(b.kind, BoxKind::Skip) {
        continue;
    }

    // `[inert]` → `interactivity: inert` (inherited, `apply_ua_inert_important`) makes
    // `effective_pointer_events` `none` too; a descendant may reset it to `auto`.
    // CSS Pointer Events L1: `pointer-events: none` on a block box excludes the box
    // itself from the clickable set. Children are always visited — a child's
    // pointer-events is independent (the property is not inherited).
    //
    // InlineRun boxes carry the BLOCK CONTAINER'S style (not the inline element's
    // own style), so we cannot use `b.style.pointer_events` to gate InlineRun
    // processing. Instead, each inline link is gated by `frag.style.pointer_events`,
    // which reflects the actual inline element's computed value.
    let block_pe_none = b.style.effective_pointer_events() == PointerEvents::None;

    match &b.kind {
        BoxKind::FormControl { kind } if !block_pe_none => {
            let ck = match kind {
                FormControlKind::Button => ClickableKind::Button,
                FormControlKind::Input { .. }
                | FormControlKind::Select { .. }
                | FormControlKind::Textarea { .. }
                | FormControlKind::Range { .. }
                | FormControlKind::Progress { .. }
                | FormControlKind::Meter { .. } => ClickableKind::Input,
            };
            out.push(ClickableElement {
                node_id: b.node,
                rect: b.rect,
                hint_text: None,
                kind: ck,
            });
        }
        BoxKind::Block | BoxKind::FlowRoot if !block_pe_none => {
            if let Some(href) = element_href(doc, b.node) {
                out.push(ClickableElement {
                    node_id: b.node,
                    rect: b.rect,
                    hint_text: first_text_content(doc, b.node),
                    kind: ClickableKind::Link { href },
                });
            } else if is_details_element(doc, b.node) {
                out.push(ClickableElement {
                    node_id: b.node,
                    rect: b.rect,
                    hint_text: first_text_content(doc, b.node),
                    kind: ClickableKind::Details,
                });
            } else if has_tabindex(doc, b.node) {
                out.push(ClickableElement {
                    node_id: b.node,
                    rect: b.rect,
                    hint_text: first_text_content(doc, b.node),
                    kind: ClickableKind::Generic,
                });
            }
        }
        BoxKind::InlineRun { lines, .. } => {
            // Collect rects for inline <a href> links by walking frag source_nodes.
            // Groups consecutive frags with the same link ancestor into one entry.
            // Skip links whose frag.style.pointer_events is None (the frag carries
            // the inline element's own computed style, not the block container's).
            let line_y_offset = b.rect.y;
            let line_x_offset = b.rect.x;
            for line in lines {
                let mut cur_link_node: Option<lumen_dom::NodeId> = None;
                let mut cur_href = String::new();
                let mut cur_rect: Option<Rect> = None;
                for frag in line {
                    // Treat pointer-events:none inline elements as if they have no link.
                    let link = if frag.style.effective_pointer_events() == PointerEvents::None {
                        None
                    } else {
                        link_ancestor(doc, frag.source_node)
                    };
                    if link == cur_link_node {
                        if let Some(ref mut r) = cur_rect {
                            let fx = line_x_offset + frag.x;
                            let fw = frag.width;
                            let left = r.x.min(fx);
                            let right = (r.x + r.width).max(fx + fw);
                            r.x = left;
                            r.width = right - left;
                        }
                    } else {
                        // Flush previous link entry.
                        if let (Some(nid), Some(r)) = (cur_link_node, cur_rect) {
                            out.push(ClickableElement {
                                node_id: nid,
                                rect: r,
                                hint_text: Some(cur_href.clone()),
                                kind: ClickableKind::Link { href: cur_href.clone() },
                            });
                        }
                        cur_link_node = link;
                        if let Some(nid) = link {
                            cur_href = element_href(doc, nid).unwrap_or_default();
                            let line_height = line
                                .iter()
                                .map(|f| f.style.font_size)
                                .fold(0.0_f32, f32::max);
                            let fy = line_y_offset;
                            cur_rect = Some(Rect::new(
                                line_x_offset + frag.x,
                                fy,
                                frag.width,
                                line_height,
                            ));
                        } else {
                            cur_rect = None;
                        }
                    }
                }
                // Flush the last link.
                if let (Some(nid), Some(r)) = (cur_link_node, cur_rect) {
                    out.push(ClickableElement {
                        node_id: nid,
                        rect: r,
                        hint_text: Some(cur_href.clone()),
                        kind: ClickableKind::Link { href: cur_href },
                    });
                }
            }
        }
        _ => {}
    }

    for child in b.children.iter().rev() {
        stack.push(child);
    }
    }
}

/// Returns the `href` attribute of element `id` if it's an `<a>` element with a non-empty href.
fn element_href(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> Option<String> {
    use lumen_dom::NodeData;
    match &doc.get(id).data {
        NodeData::Element { name, attrs, .. } if name.local == "a" => {
            attrs.iter().find(|a| a.name.local == "href").map(|a| a.value.clone())
        }
        _ => None,
    }
}

/// Returns `true` if element `id` has a non-negative `tabindex` attribute.
fn has_tabindex(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> bool {
    doc.get(id)
        .get_attr("tabindex")
        .and_then(|v| v.trim().parse::<i32>().ok())
        .is_some_and(|n| n >= 0)
}

/// Walk up from `id` to find the nearest `<a href>` ancestor (inclusive).
fn link_ancestor(
    doc: &lumen_dom::Document,
    mut id: lumen_dom::NodeId,
) -> Option<lumen_dom::NodeId> {
    loop {
        if element_href(doc, id).is_some() {
            return Some(id);
        }
        {
            let p = doc.get(id).parent?;
            id = p
        }
    }
}

/// DOM element ancestors of an `InlineSegment`/`InlineFrag::source_node` that the
/// box tree never gives their own `LayoutBox` (BUG-488): plain inline elements
/// (`<span>`, `<em>`, …) are flattened into the enclosing `InlineRun`'s segments,
/// so `collect_computed_styles`/`collect_layout_rects` — which key their output by
/// walking `LayoutBox.node`, not DOM structure — never see the element's own
/// `NodeId` at all.
///
/// Returns every element strictly between `source_node` and `stop_at` (both
/// exclusive), innermost first, plus `source_node` itself when it names an
/// element rather than a text node (`content_to_inline_segments`'s generated-text
/// segments use `source_node = owner_id`, the element being styled — its own
/// generated content must count toward its own box). `stop_at` is normally the
/// `InlineRun`'s own `node` (the containing block/inline-block), which already
/// gets an entry from the ordinary per-`LayoutBox` walk.
pub(crate) fn inline_element_ancestors(
    doc: &lumen_dom::Document,
    source_node: lumen_dom::NodeId,
    stop_at: lumen_dom::NodeId,
) -> Vec<lumen_dom::NodeId> {
    let mut out = Vec::new();
    let mut cur = if matches!(doc.get(source_node).data, lumen_dom::NodeData::Element { .. }) {
        source_node
    } else {
        match doc.get(source_node).parent {
            Some(p) => p,
            None => return out,
        }
    };
    while cur != stop_at {
        out.push(cur);
        cur = match doc.get(cur).parent {
            Some(p) => p,
            None => break,
        };
    }
    out
}

/// Get the text content of the first text-node descendant (for hint labels).
fn first_text_content(
    doc: &lumen_dom::Document,
    id: lumen_dom::NodeId,
) -> Option<String> {
    use lumen_dom::NodeData;
    let node = doc.get(id);
    if let NodeData::Text(t) = &node.data {
        let s = t.trim().to_string();
        return if s.is_empty() { None } else { Some(s) };
    }
    for &child in &node.children {
        if let Some(t) = first_text_content(doc, child) {
            return Some(t);
        }
    }
    None
}

/// Returns `true` if element `id` is a `<details>` element (disclosure widget).
fn is_details_element(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> bool {
    use lumen_dom::NodeData;
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "details"
    )
}
