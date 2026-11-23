//! Sequential focus navigation order (HTML Standard §6.6.6) — which DOM
//! nodes Tab/Shift+Tab reach and in what order (FRAME-7 срез 2: page-level
//! Tab focus; срез 4: the same walk reused, non-wrapping, for a frame's OWN
//! document — see [`next_focus_target_no_wrap`] and `crate::lumen::focus_tab`).
//!
//! Pure tree walk over a `Document` + its `FlatTree` (composed order, so
//! Shadow DOM content participates) — no layout dependency, so it says
//! nothing about elements hidden by CSS (`display:none`/`visibility:hidden`)
//! or disabled only through an ancestor `<fieldset disabled>`; both are
//! DOM-attribute checks, the same scope `lumen_a11y`'s `AXState` already
//! keeps.
//!
//! Used by [`crate::lumen::focus_tab`].

use lumen_dom::{find_ancestor_form, Document, FlatTree, InputType, NodeData, NodeId};
use std::collections::HashMap;

/// Effective `tabindex` for sequential (Tab-key) navigation, or `None` if the
/// node is not reachable that way at all.
///
/// `tabindex="-1"` is focusable by click/script (`focused_node` already
/// treats any clicked node as focused — BUG-480 срез 23) but excluded from
/// the Tab order per HTML Standard §6.6.6 step 1; this returns `None` for it
/// same as for a node that is not focusable at all.
fn sequential_tab_index(node: &lumen_dom::Node) -> Option<i32> {
    if let Some(raw) = node.get_attr("tabindex") {
        return match raw.trim().parse::<i32>() {
            Ok(v) if v >= 0 => Some(v),
            _ => None,
        };
    }
    implicitly_focusable(node).then_some(0)
}

/// The element set focusable without an explicit `tabindex` — shared by the
/// Tab order ([`sequential_tab_index`]) and click-focusability below.
fn implicitly_focusable(node: &lumen_dom::Node) -> bool {
    let NodeData::Element { name, .. } = &node.data else {
        return false;
    };
    let disabled = node.get_attr("disabled").is_some();
    match name.local.as_ref() {
        "a" | "area" => node.get_attr("href").is_some(),
        "button" | "select" | "selectlist" | "textarea" => !disabled,
        "input" => !disabled && node.input_type() != Some(InputType::Hidden),
        "iframe" | "embed" | "object" => true,
        _ => node
            .get_attr("contenteditable")
            .is_some_and(|v| v != "false"),
    }
}

/// Whether a pointer press landing on `node` counts as "mouse focus" for
/// `:focus-visible` modality (CSS UI L4 §3.3). Click-focusable elements —
/// the sequential-focus set plus any valid `tabindex` (negatives included,
/// which are click/script-focusable but not tabbable) — reset the keyboard
/// modality on click; presses on non-focusable content leave it alone
/// (WPT `focus-visible-007`, `focus-visible-script-focus-*`).
pub(crate) fn click_focusable(doc: &Document, node: NodeId) -> bool {
    let n = doc.get(node);
    match n.get_attr("tabindex") {
        Some(raw) => raw.trim().parse::<i32>().is_ok(),
        None => implicitly_focusable(n),
    }
}

/// Radio-group tab-stop collapse (HTML LS §6.6.6 sequential focus
/// navigation): a radio button group contributes ONE stop — its checked
/// member, or its first member when none is checked. Every other member
/// stays in the sequence (so «next after a script-focused member» still
/// lands correctly) but is not itself a stop.
///
/// Group identity mirrors the checkedness rule: same non-empty `name`,
/// same form owner (nearest `<form>` ancestor — the `form=` attribute is
/// not consulted here, matching `find_ancestor_form`'s scope). Radios
/// without a `name` are singletons and always their own stop.
fn collapse_radio_stops(doc: &Document, zero: &[NodeId]) -> (Vec<NodeId>, Vec<bool>) {
    // Group key → member indices into `zero`, in tree order.
    let mut groups: HashMap<(String, Option<NodeId>), Vec<usize>> = HashMap::new();
    for (i, &id) in zero.iter().enumerate() {
        let node = doc.get(id);
        if !matches!(node.input_type(), Some(InputType::Radio)) {
            continue;
        }
        let name = node.get_attr("name").unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let owner = find_ancestor_form(doc, id);
        groups
            .entry((name.to_owned(), owner))
            .or_default()
            .push(i);
    }
    let mut stop = vec![true; zero.len()];
    for (_, members) in groups {
        if members.len() < 2 {
            continue;
        }
        // Representative: first checked member in tree order, else the first.
        let rep = members
            .iter()
            .copied()
            .find(|&i| doc.control_checked(zero[i]))
            .unwrap_or(members[0]);
        for &i in &members {
            if i != rep {
                stop[i] = false;
            }
        }
    }
    (zero.to_vec(), stop)
}

/// DOM order of every sequentially-focusable node under `root`, plus a
/// parallel `is_stop` flag per entry. Sorted per HTML Standard §6.6.6:
/// positive `tabindex` first (ascending, ties broken by tree order), then
/// `tabindex="0"`/implicitly-focusable elements in tree order with radio
/// groups collapsed to their representative (see [`collapse_radio_stops`]).
/// A `hidden` element (and its whole subtree — HTML: `hidden` means not
/// rendered) is skipped entirely.
fn build_focus_seq(doc: &Document, flat_tree: &FlatTree, root: NodeId) -> (Vec<NodeId>, Vec<bool>) {
    let mut positive: Vec<(i32, usize, NodeId)> = Vec::new();
    let mut zero: Vec<NodeId> = Vec::new();
    let mut seq: usize = 0;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if node.get_attr("hidden").is_some() {
            continue;
        }
        if let Some(tabindex) = sequential_tab_index(node) {
            seq += 1;
            if tabindex > 0 {
                positive.push((tabindex, seq, id));
            } else {
                zero.push(id);
            }
        }
        // Reverse push so children pop in document order (plain DFS via an
        // explicit stack instead of recursion).
        for &child in flat_tree.children_of(doc, id).iter().rev() {
            stack.push(child);
        }
    }
    positive.sort_by_key(|&(tabindex, seq, _)| (tabindex, seq));
    let (zero_seq, zero_stops) = collapse_radio_stops(doc, &zero);
    let mut out_seq: Vec<NodeId> = Vec::with_capacity(positive.len() + zero_seq.len());
    let mut out_stop: Vec<bool> = Vec::with_capacity(positive.len() + zero_seq.len());
    for (_, _, id) in positive {
        out_seq.push(id);
        out_stop.push(true);
    }
    out_seq.extend(zero_seq);
    out_stop.extend(zero_stops);
    (out_seq, out_stop)
}

/// Stops-only view of [`build_focus_seq`] — what the unit tests assert on.
#[cfg(test)]
fn build_focus_order(doc: &Document, flat_tree: &FlatTree, root: NodeId) -> Vec<NodeId> {
    let (seq, stops) = build_focus_seq(doc, flat_tree, root);
    seq.into_iter()
        .zip(stops)
        .filter_map(|(id, stop)| stop.then_some(id))
        .collect()
}

/// First element in tree order under `root` whose `accesskey` lists one of
/// `chars` (HTML Standard §6.6.2: a space-separated list of single-character
/// tokens, compared case-insensitively) and that can take focus — `hidden`
/// subtrees and elements that are not focusable at all (a disabled control, a
/// plain `<div>`) are skipped. `chars` is ordered best first: the first
/// character with any matching element wins, so a page that declares
/// `accesskey="ф"` beats the physical-position fallback `a`.
pub(crate) fn find_accesskey_target(
    doc: &Document,
    flat_tree: &FlatTree,
    root: NodeId,
    chars: &[char],
) -> Option<NodeId> {
    let mut candidates: Vec<(NodeId, Vec<char>)> = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        if node.get_attr("hidden").is_some() {
            continue;
        }
        if let Some(keys) = node.get_attr("accesskey") {
            let focusable = sequential_tab_index(node).is_some()
                || node.get_attr("tabindex").is_some_and(|v| v.trim().parse::<i32>().is_ok());
            if focusable {
                let tokens = keys
                    .split_whitespace()
                    .filter_map(|t| {
                        let mut it = t.chars();
                        let c = it.next()?;
                        it.next().is_none().then(|| c.to_lowercase().next().unwrap_or(c))
                    })
                    .collect();
                candidates.push((id, tokens));
            }
        }
        for &child in flat_tree.children_of(doc, id).iter().rev() {
            stack.push(child);
        }
    }
    chars.iter().find_map(|c| {
        candidates
            .iter()
            .find(|(_, tokens)| tokens.contains(c))
            .map(|(id, _)| *id)
    })
}

/// Next (`forward = true`) or previous (`forward = false`) node in `root`'s
/// Tab order relative to `current`, wrapping around at either end.
/// `current = None`, or a `current` no longer in the order (e.g. it left the
/// document), starts from the first (forward) or last (backward) entry.
/// `None` iff the document has no focusable node at all.
/// Index of the first stop at or after `from` in `seq` (wrapping when
/// `wrap`), or `None` when there is none.
fn next_stop(seq: &[NodeId], stops: &[bool], from: usize, step: isize, wrap: bool) -> Option<usize> {
    let n = seq.len();
    if n == 0 {
        return None;
    }
    let mut i = from;
    for _ in 0..n {
        i = if step > 0 {
            if i + 1 >= n {
                if !wrap {
                    return None;
                }
                0
            } else {
                i + 1
            }
        } else if i == 0 {
            if !wrap {
                return None;
            }
            n - 1
        } else {
            i - 1
        };
        if stops[i] {
            return Some(i);
        }
    }
    None
}

pub(crate) fn next_focus_target(
    doc: &Document,
    flat_tree: &FlatTree,
    root: NodeId,
    current: Option<NodeId>,
    forward: bool,
) -> Option<NodeId> {
    let (seq, stops) = build_focus_seq(doc, flat_tree, root);
    if !stops.iter().any(|&s| s) {
        return None;
    }
    let step: isize = if forward { 1 } else { -1 };
    match current.and_then(|c| seq.iter().position(|&id| id == c)) {
        Some(i) => next_stop(&seq, &stops, i, step, true).map(|j| seq[j]),
        // `current` is not in the sequence at all (e.g. `tabindex=-1` or it
        // left the document): start from the first/last stop.
        None => {
            let idx = if forward {
                stops.iter().position(|&s| s)
            } else {
                stops.iter().rposition(|&s| s)
            };
            idx.map(|j| seq[j])
        }
    }
}

/// Like [`next_focus_target`] but does not wrap: `None` once `current` is the
/// last (`forward`) / first (backward) entry, or `root`'s document has no
/// focusable node at all (FRAME-7 срез 4).
///
/// A frame's own document is a NESTED browsing context (HTML Standard
/// §6.6.6) — wrapping at its own boundary would trap Tab inside a frame that
/// has any focusable field at all, when the correct behaviour is to hand
/// focus back to the CONTAINER's order once the frame's own is exhausted.
/// `crate::lumen::focus_tab::advance_frame_focus` falls back to
/// `advance_page_focus` exactly when this returns `None`.
pub(crate) fn next_focus_target_no_wrap(
    doc: &Document,
    flat_tree: &FlatTree,
    root: NodeId,
    current: Option<NodeId>,
    forward: bool,
) -> Option<NodeId> {
    let (seq, stops) = build_focus_seq(doc, flat_tree, root);
    if !stops.iter().any(|&s| s) {
        return None;
    }
    let step: isize = if forward { 1 } else { -1 };
    match current.and_then(|c| seq.iter().position(|&id| id == c)) {
        Some(i) => next_stop(&seq, &stops, i, step, false).map(|j| seq[j]),
        None => {
            let idx = if forward {
                stops.iter().position(|&s| s)
            } else {
                stops.iter().rposition(|&s| s)
            };
            idx.map(|j| seq[j])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::Document;

    fn parse(html: &str) -> Document {
        lumen_html_parser::parse(html)
    }

    fn find(doc: &Document, id_attr: &str) -> Option<NodeId> {
        (0..doc.len())
            .map(NodeId::from_index)
            .find(|&id| doc.get(id).get_attr("id") == Some(id_attr))
    }

    #[test]
    fn tree_order_for_implicit_tabindex() {
        let doc = parse(
            "<body><a id=a href=/>a</a><input id=b><button id=c>c</button></body>",
        );
        let flat = lumen_dom::build_flat_tree(&doc);
        let order = build_focus_order(&doc, &flat, doc.root());
        assert_eq!(
            order,
            vec![
                find(&doc, "a").unwrap(),
                find(&doc, "b").unwrap(),
                find(&doc, "c").unwrap(),
            ]
        );
    }

    #[test]
    fn positive_tabindex_sorts_before_zero_and_by_value() {
        let doc = parse(
            "<body><input id=first><input id=second tabindex=2><input id=third tabindex=1></body>",
        );
        let flat = lumen_dom::build_flat_tree(&doc);
        let order = build_focus_order(&doc, &flat, doc.root());
        assert_eq!(
            order,
            vec![
                find(&doc, "third").unwrap(),
                find(&doc, "second").unwrap(),
                find(&doc, "first").unwrap(),
            ]
        );
    }

    #[test]
    fn negative_tabindex_and_disabled_and_hidden_link_are_excluded() {
        let doc = parse(
            "<body>\
                <input id=skip tabindex=-1>\
                <input id=dis disabled>\
                <div id=hid hidden><input id=inside></div>\
                <input id=keep>\
             </body>",
        );
        let flat = lumen_dom::build_flat_tree(&doc);
        let order = build_focus_order(&doc, &flat, doc.root());
        assert_eq!(order, vec![find(&doc, "keep").unwrap()]);
    }

    #[test]
    fn href_less_anchor_is_not_focusable() {
        let doc = parse("<body><a id=a>no href</a><input id=b></body>");
        let flat = lumen_dom::build_flat_tree(&doc);
        let order = build_focus_order(&doc, &flat, doc.root());
        assert_eq!(order, vec![find(&doc, "b").unwrap()]);
    }

    #[test]
    fn next_wraps_forward_and_backward() {
        let doc = parse("<body><input id=a><input id=b><input id=c></body>");
        let flat = lumen_dom::build_flat_tree(&doc);
        let (a, b, c) = (
            find(&doc, "a").unwrap(),
            find(&doc, "b").unwrap(),
            find(&doc, "c").unwrap(),
        );
        assert_eq!(next_focus_target(&doc, &flat, doc.root(), Some(a), true), Some(b));
        assert_eq!(next_focus_target(&doc, &flat, doc.root(), Some(c), true), Some(a));
        assert_eq!(next_focus_target(&doc, &flat, doc.root(), Some(a), false), Some(c));
        assert_eq!(next_focus_target(&doc, &flat, doc.root(), None, true), Some(a));
        assert_eq!(next_focus_target(&doc, &flat, doc.root(), None, false), Some(c));
    }

    fn accesskey(html: &str, chars: &[char]) -> Option<String> {
        let doc = parse(html);
        let flat = lumen_dom::build_flat_tree(&doc);
        find_accesskey_target(&doc, &flat, doc.root(), chars)
            .and_then(|id| doc.get(id).get_attr("id").map(str::to_owned))
    }

    #[test]
    fn accesskey_matches_case_insensitively_and_in_tree_order() {
        let html = "<body><a id=x href=/ accesskey=\"B A\">x</a><input id=y accesskey=a></body>";
        assert_eq!(accesskey(html, &['a']).as_deref(), Some("x"));
        assert_eq!(accesskey(html, &['b']).as_deref(), Some("x"));
        assert_eq!(accesskey(html, &['c']), None);
        assert_eq!(
            accesskey("<body><input id=y accesskey=A></body>", &['a']).as_deref(),
            Some("y")
        );
    }

    #[test]
    fn accesskey_prefers_the_earlier_candidate_character() {
        let html = "<body><input id=lat accesskey=a><input id=cyr accesskey=\"ф\"></body>";
        assert_eq!(accesskey(html, &['ф', 'a']).as_deref(), Some("cyr"));
        assert_eq!(accesskey(html, &['я', 'a']).as_deref(), Some("lat"));
    }

    #[test]
    fn accesskey_skips_hidden_disabled_and_unfocusable() {
        assert_eq!(accesskey("<body><div id=d accesskey=a>x</div></body>", &['a']), None);
        assert_eq!(accesskey("<body><button id=b accesskey=a disabled>x</button></body>", &['a']), None);
        assert_eq!(
            accesskey("<body><div hidden><input id=h accesskey=a></div><input id=v accesskey=a></body>", &['a'])
                .as_deref(),
            Some("v")
        );
        assert_eq!(
            accesskey("<body><div id=t tabindex=-1 accesskey=a>x</div></body>", &['a']).as_deref(),
            Some("t")
        );
        // A multi-character token is not a valid accesskey.
        assert_eq!(accesskey("<body><input id=m accesskey=ab></body>", &['a']), None);
    }

    #[test]
    fn no_focusable_elements_returns_none() {
        let doc = parse("<body><p>text only</p></body>");
        let flat = lumen_dom::build_flat_tree(&doc);
        assert_eq!(next_focus_target(&doc, &flat, doc.root(), None, true), None);
    }

    #[test]
    fn no_wrap_stops_at_either_end() {
        let doc = parse("<body><input id=a><input id=b><input id=c></body>");
        let flat = lumen_dom::build_flat_tree(&doc);
        let (a, b, c) = (
            find(&doc, "a").unwrap(),
            find(&doc, "b").unwrap(),
            find(&doc, "c").unwrap(),
        );
        assert_eq!(
            next_focus_target_no_wrap(&doc, &flat, doc.root(), Some(a), true),
            Some(b)
        );
        assert_eq!(
            next_focus_target_no_wrap(&doc, &flat, doc.root(), Some(c), true),
            None
        );
        assert_eq!(
            next_focus_target_no_wrap(&doc, &flat, doc.root(), Some(a), false),
            None
        );
        assert_eq!(
            next_focus_target_no_wrap(&doc, &flat, doc.root(), None, true),
            Some(a)
        );
        assert_eq!(
            next_focus_target_no_wrap(&doc, &flat, doc.root(), None, false),
            Some(c)
        );
    }

    #[test]
    fn no_wrap_no_focusable_elements_returns_none() {
        let doc = parse("<body><p>text only</p></body>");
        let flat = lumen_dom::build_flat_tree(&doc);
        assert_eq!(
            next_focus_target_no_wrap(&doc, &flat, doc.root(), None, true),
            None
        );
    }
}
