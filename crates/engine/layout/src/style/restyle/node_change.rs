//! BUG-341 S17 — корни рестайла после мутации DOM: [`NodeChange`] /
//! [`OwnedNodeChange`], [`RestyleRoots`] и их вычисление поверх
//! [`NodeRestyleIndex`]. Вынесено из `style/restyle.rs` без правок тел —
//! файл перерос потолок размера (docs/lint-policy.md §5.1).

use super::*;

/// BUG-341 S17 — one reported DOM mutation, as
/// [`restyle_root_set_for_node_change`] needs to see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeChange<'a> {
    /// The attribute named `.0` was written to, or removed from, the node. The
    /// name is what lets the root-set ask which selectors could possibly react.
    Attr(&'a str),
    /// BUG-935 срез 68 — like [`Self::Attr`], and the value the attribute had *before*
    /// the write (`""` when it was absent), which is what lets the root-set name the
    /// class/id tokens that changed. Only a source that knows the old value reports it.
    AttrFrom {
        /// The attribute written.
        name: &'a str,
        /// Its value at the start of the interval the change covers.
        old: &'a str,
    },
    /// BUG-935 срез 60 — the node's child list changed (an element was appended,
    /// removed, moved, or its text replaced) and nothing else about it did.
    /// [`restyle_roots_for_node_changes`] can restyle the node and its direct
    /// children instead of the parent's whole subtree; the single-set
    /// [`restyle_root_set_for_node_change`] reads it as [`Self::Unattributed`].
    ChildList,
    /// BUG-935 срез 89 — like [`Self::ChildList`], and the source knows every edit of the list was
    /// an insertion or a removal of an *element* (text nodes do not count for `:first-child`).
    /// `front`/`back`: some edit happened with no element before/after it, i.e. it can have
    /// changed which element is first/last; `edits`: how many element edits there were. An
    /// element then changed its first/last state only if it now sits within `edits + 1` of
    /// that end — each edit moves the old first/last by at most one place.
    ChildListEnds {
        /// Some edit left no element before the edited node.
        front: bool,
        /// Some edit left no element after the edited node.
        back: bool,
        /// Number of element insertions/removals the change covers.
        edits: u32,
    },
    /// Something else changed, or the source cannot name what changed: a child
    /// list moved (`:nth-child`, `:empty` and sibling combinators all react to
    /// that, and no attribute name describes it), or the mutation came from a
    /// tracker that does not record names (page-side JS). Always takes the
    /// conservative widen-to-parent path — the pre-S17 behaviour for everything.
    Unattributed,
}

/// BUG-935 срез 81: what happened to one node since a consumer's watermark — the owned
/// form of [`NodeChange`], so it can leave the tracker's lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedNodeChange {
    /// A change the root-set cannot narrow (structural, or a touch with no name on record).
    Unattributed,
    /// Only the node's child list changed.
    ChildList,
    /// Only the node's child list changed, by element edits that touched these ends
    /// ([`NodeChange::ChildListEnds`]).
    ChildListEnds {
        /// Some edit left no element before the edited node.
        front: bool,
        /// Some edit left no element after the edited node.
        back: bool,
        /// Number of element insertions/removals.
        edits: u32,
    },
    /// The attribute was written; its earlier value is not on record.
    Attr(Box<str>),
    /// The attribute was written, and this is the value it had at the watermark.
    AttrFrom {
        /// The attribute written.
        name: Box<str>,
        /// Its value at the watermark (`""` — absent).
        old: Box<str>,
    },
}

impl OwnedNodeChange {
    /// The borrowed form the root-set takes.
    pub fn as_node_change(&self) -> NodeChange<'_> {
        match self {
            Self::Unattributed => NodeChange::Unattributed,
            Self::ChildList => NodeChange::ChildList,
            Self::ChildListEnds { front, back, edits } => {
                NodeChange::ChildListEnds { front: *front, back: *back, edits: *edits }
            }
            Self::Attr(name) => NodeChange::Attr(name),
            Self::AttrFrom { name, old } => NodeChange::AttrFrom { name, old },
        }
    }
}

/// BUG-341 S3/S17 — restyle root-set (brief §4) for DOM attribute/class/
/// structural changes (chrome `bind_model` diff or a JS DOM mutation).
///
/// Class/attribute/structural selectors don't match ancestors *by themselves*,
/// so the changed node's own subtree covers the node itself and every
/// descendant selector rooted at it (`node X`). What reaches *outside* that
/// subtree is a sibling combinator (`node + X`, `node ~ X`), which the pre-S17
/// version covered by unconditionally invalidating the parent's whole subtree.
///
/// S17 asks the S14 question of that widening: which selectors could react to
/// *this* attribute on *this* node? A `value` write on chrome's omnibox input
/// re-cascaded all 12 elements of `div.omnibox` — and the census found all 12
/// produced a byte-identical `ComputedStyle`, because `chrome.html` has no
/// sibling combinator that could match `#omniInput` (in fact none at all).
/// `index` — from [`restyle_node_index`] — is what answers that per node and
/// per attribute name; a change reported as [`NodeChange::Unattributed`], or a
/// sheet the index declares conservative, keeps the old widen-to-parent
/// behaviour exactly.
///
/// A node with no parent (the document root) invalidates itself either way.
///
/// **Fixed gap (BUG-348/BUG-349):** `:has()` — which this engine does
/// implement (`PseudoClass::Has`, `style.rs`'s `matches_relative`) — lets a
/// change on a node flip some ancestor `E`'s `:has(...)` result, where `E` can
/// sit arbitrarily far above the node's own parent. The parent-only widening
/// below cannot express that reach, so while [`NodeRestyleIndex::has_has_dependency`]
/// is set (`sheet` contains a `:has()` selector anywhere) each change also adds
/// the ancestors (and, for forward-sibling arguments, their previous siblings)
/// that could match a compound carrying a `:has()` — see
/// [`NodeRestyleIndex::has_reach_roots`]. BUG-349 first widened every change to
/// the whole document; BUG-935 s58 replaced that with this index, because on a
/// real page every forced reflow restyled and re-collected all of it. A
/// document with an author shadow root (shadow sheets are not scanned) still
/// widens to the whole document.
///
/// **Known gap, same family:** `:indeterminate` on a radio group and
/// `:default` on a form's submit button read *other* elements' `name`/`checked`/
/// `type` attributes across the whole form. Like `:has()`, that reach is
/// document-shaped rather than subtree-shaped, so neither the pre-S17
/// widen-to-parent nor S17's narrowing expresses it — pre-existing, not
/// introduced here.
pub fn restyle_root_set_for_node_change<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
) -> HashSet<NodeId> {
    root_set_impl(doc, changes, index, false, None).deep
}

/// BUG-935 срез 60 — the restyle roots of a batch of DOM changes, split by how much of
/// the subtree each one has to take with it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RestyleRoots {
    /// Nodes whose whole subtree is recascaded — [`restyle_root_set_for_node_change`]'s
    /// answer, and what `RestyleDelta::dirty_roots` means.
    pub deep: HashSet<NodeId>,
    /// Nodes whose child list changed: the node and its direct children are
    /// recascaded, and a child's subtree only when its style changed, it moved from
    /// another parent, or it is in [`Self::deep`] (`RestyleDelta::shallow_roots`).
    pub shallow: HashSet<NodeId>,
    /// BUG-935 срез 73 — descendants of a [`Self::shallow`] root that a selector reads the
    /// root's written token for: each is recascaded on its own (`RestyleDelta::point_roots`),
    /// the rest of the shallow root's subtree is left alone.
    pub point: HashSet<NodeId>,
}

/// BUG-935 срез 60 — like [`restyle_root_set_for_node_change`], but a
/// [`NodeChange::ChildList`] yields a shallow root instead of the parent's whole subtree.
///
/// A child-list change on `C` reaches only: `C` itself (`:empty`, `:has()`), its direct
/// children (positional pseudo-classes, sibling combinators), and the subtree of a child
/// whose own position a selector reads on the way down (`li:first-child a` —
/// [`NodeRestyleIndex::add_structure_reach`], which lands what such a child feeds in `point`).
/// What goes beyond that is a sibling combinator on `C` (`C:empty + X`), answered by also
/// making `C`'s parent a shallow root, and `:has()`, answered by the same
/// [`NodeRestyleIndex::has_reach_roots`] as before. `:nth-child(… of S)` and shadow roots
/// switch it off (`conservative`): the change then widens to the parent, as before.
pub fn restyle_roots_for_node_changes<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
) -> RestyleRoots {
    root_set_impl(doc, changes, index, true, None)
}

/// BUG-935 срез 82 — [`restyle_roots_for_node_changes`] for a caller that knows which
/// elements the cascade it updates already has a style for (`has_style`).
///
/// An element without one — created after that cascade ran — is recascaded in full the
/// moment the walk reaches it, together with its subtree, and the walk reaches it because
/// its parent's child list changed (the attach is a [`NodeChange::ChildList`] on the
/// parent, whose shallow root also covers the siblings' positional and sibling-combinator
/// reach). Its own attribute writes (`class` set while a framework builds the node) add
/// nothing to that, yet used to widen to the parent's whole subtree because a sibling
/// combinator mentions the token: a page that builds its DOM from script recascaded the
/// container once per node it inserted. Such a write is dropped here, apart from what no
/// recascade of the node covers — what a `:has()` reads
/// ([`NodeRestyleIndex::has_reach_roots`]) and the elements that act outside their own
/// subtree ([`fresh_node_change_is_covered`]).
pub fn restyle_roots_for_node_changes_with_basis<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
    has_style: &dyn Fn(NodeId) -> bool,
) -> RestyleRoots {
    root_set_impl(doc, changes, index, true, Some(has_style))
}

/// BUG-935 срез 82 — whether `change` on `n` is subsumed by the recascade `n` gets anyway
/// because the cascade holds no style for it ([`restyle_roots_for_node_changes_with_basis`]).
///
/// Only a change that names its kind (an unnamed one may be anything) on an element that
/// stays inside its own subtree: the document-level metadata elements change what *other*
/// nodes resolve to (`<base href>`, `<link>`/`<style>` sheets, `<meta name=color-scheme>`),
/// `<slot>` and the `slot` attribute move nodes between composed parents without a
/// child-list change.
fn fresh_node_change_is_covered(doc: &Document, n: NodeId, change: &NodeChange<'_>) -> bool {
    let attr = match change {
        NodeChange::Unattributed => return false,
        NodeChange::ChildList | NodeChange::ChildListEnds { .. } => "",
        NodeChange::Attr(name) | NodeChange::AttrFrom { name, .. } => name,
    };
    attr != "slot"
        && matches!(doc.get(n).data, NodeData::Element { .. })
        && !tag_is(doc, n, &["html", "head", "body", "base", "link", "meta", "style", "slot", "template"])
}

/// The attribute `change` wrote and, when the source knows it, the value it had before.
fn change_written<'c>(change: &NodeChange<'c>) -> Option<(&'c str, Option<&'c str>)> {
    match *change {
        NodeChange::Attr(name) => Some((name, None)),
        NodeChange::AttrFrom { name, old } => Some((name, Some(old))),
        NodeChange::Unattributed | NodeChange::ChildList | NodeChange::ChildListEnds { .. } => None,
    }
}

fn root_set_impl<'a>(
    doc: &Document,
    changes: impl IntoIterator<Item = (NodeId, NodeChange<'a>)>,
    index: &NodeRestyleIndex<'_>,
    allow_shallow: bool,
    has_style: Option<&dyn Fn(NodeId) -> bool>,
) -> RestyleRoots {
    if index.has_dependent && index.has_in_shadow_doc {
        return RestyleRoots { deep: changes.into_iter().map(|_| doc.root()).collect(), ..RestyleRoots::default() };
    }
    let mut roots = RestyleRoots::default();
    let shallow_ok = allow_shallow && !index.conservative;
    // Containers whose every element child was asked `child_needs_deep_restyle`.
    let mut scanned_all: HashSet<NodeId> = HashSet::new();
    for (n, change) in changes {
        // BUG-935 срез 82: a node the cascade has no style for is recascaded in full when
        // its parent's child list change reaches it.
        if shallow_ok
            && index.attr_narrowing
            && has_style.is_some_and(|has| !has(n))
            && fresh_node_change_is_covered(doc, n, &change)
        {
            if index.has_dependent && index.change_can_flip_has(doc, n, change_written(&change)) {
                index.has_reach_roots(doc, n, &mut roots, shallow_ok);
            }
            continue;
        }
        match change {
            NodeChange::ChildList | NodeChange::ChildListEnds { .. } if shallow_ok => {
                let ends = match change {
                    NodeChange::ChildListEnds { front, back, edits } => Some((front, back, edits)),
                    _ => None,
                };
                // A text/comment node reported for a data change: its parent's child
                // list is what `:empty` and the siblings read.
                let container = if matches!(doc.get(n).data, NodeData::Element { .. }) {
                    Some(n)
                } else {
                    doc.get(n).parent
                };
                let Some(n) = container else {
                    roots.deep.insert(n);
                    continue;
                };
                // The parent's list did not change: what reaches it is `n`'s own place among
                // its siblings, which no edit position describes.
                let mut containers = vec![(n, ends)];
                if index.attr_change_needs_fanout(doc, n, "")
                    && let Some(parent) = doc.get(n).parent
                {
                    containers.push((parent, None));
                }
                for (c, ends) in containers {
                    roots.shallow.insert(c);
                    if scanned_all.contains(&c) {
                        continue;
                    }
                    match ends {
                        None => {
                            scanned_all.insert(c);
                            for &child in &doc.get(c).children {
                                if matches!(doc.get(child).data, NodeData::Element { .. }) {
                                    index.add_structure_reach(doc, child, true, true, &mut roots);
                                }
                            }
                        }
                        // BUG-935 срез 89: the edits left the first/last element where an edit
                        // reached, so `:first-child`/`:last-child` flipped only within `edits + 1`
                        // places of that end; the compounds an edit reaches wherever it was
                        // (`:nth-child`, sibling combinators) are asked of every child, as before.
                        Some((front, back, edits)) => {
                            let elements: Vec<NodeId> = doc
                                .get(c)
                                .children
                                .iter()
                                .copied()
                                .filter(|&ch| matches!(doc.get(ch).data, NodeData::Element { .. }))
                                .collect();
                            let reach = edits as usize + 1;
                            for (i, &child) in elements.iter().enumerate() {
                                let at_end = (front && i < reach) || (back && elements.len() - i <= reach);
                                index.add_structure_reach(doc, child, true, at_end, &mut roots);
                            }
                        }
                    }
                }
            }
            other => {
                let written = change_written(&other);
                let needs_fanout = match other {
                    NodeChange::Attr(attr) => index.attr_change_needs_fanout(doc, n, attr),
                    NodeChange::AttrFrom { name, old } => index.attr_change_needs_fanout_from(doc, n, name, Some(old)),
                    NodeChange::Unattributed | NodeChange::ChildList | NodeChange::ChildListEnds { .. } => true,
                };
                // BUG-1211: the write can flip a `:nth-child(… of S)` selector on `n`, which moves the
                // position of siblings on both sides of it.
                if let Some((name, old)) = written
                    && index.attr_change_hits_nth_of(doc, n, name, old)
                {
                    roots.deep.insert(doc.get(n).parent.unwrap_or(n));
                    if index.has_dependent && index.change_can_flip_has(doc, n, written) {
                        index.has_reach_roots(doc, n, &mut roots, shallow_ok);
                    }
                    continue;
                }
                // BUG-935 срез 68: nothing below `n` reads what was written, so `n` and its
                // direct children are restyled and the walk goes deeper only if `n`'s style
                // moved — the shallow-root contract, with the attribute (not the child
                // list) as the reason.
                let local = shallow_ok
                    && !needs_fanout
                    && written.is_some_and(|(name, old)| index.attr_change_stays_local(doc, n, name, old));
                // BUG-935 срез 73: a write some selector *does* read from an ancestor position
                // names the descendants that selector can style; those, and not the whole
                // subtree, are what the cascade has to look at.
                let affected = if local || !shallow_ok || needs_fanout {
                    None
                } else {
                    written.and_then(|(name, old)| index.affected_descendants(doc, n, name, old))
                };
                if local {
                    roots.shallow.insert(n);
                } else if let Some(points) = affected {
                    roots.shallow.insert(n);
                    roots.point.extend(points);
                } else if needs_fanout && written.is_some() && shallow_ok && index.attr_narrowing && doc.get(n).parent.is_some() {
                    // BUG-935 срез 83: a sibling combinator reaches the *following* elements, so
                    // the root is the node and those, not the parent with the siblings before it.
                    roots.deep.insert(n);
                    roots.deep.extend(following_elements(doc, n));
                } else {
                    roots.deep.insert(if needs_fanout { doc.get(n).parent.unwrap_or(n) } else { n });
                }
            }
        }
        if index.has_dependent && index.change_can_flip_has(doc, n, change_written(&change)) {
            index.has_reach_roots(doc, n, &mut roots, shallow_ok);
        }
    }
    roots
}
