//! THREAD-4 срез 2 — intra-pass structural memo for [`crate::style::compute_style`].
//!
//! Срез 1 (`ROADMAP.md` THREAD-4) profiled github.com's layout pass and found
//! `compute_style` running ~1775-2000 times per pass while only ~392 distinct
//! `(tag, sorted classes)` structural keys exist among them — GitHub's Octicon
//! SVG icons repeat the same markup fragment (tag + attributes) throughout the
//! page. [`ShareCache`] recognises a repeat and returns the already-computed
//! [`ComputedStyle`] instead of re-running the cascade for it.
//!
//! This is **not** [`crate::counters::CascadeStyles`] (BUG-341 S24): that
//! cache is keyed on [`NodeId`] and carried *between* passes (its whole point
//! is reusing yesterday's cascade for a node that did not change).
//! [`ShareCache`] is keyed on structural content and lives only *within* one
//! pass — a fresh, empty instance every [`crate::counters::precompute_counters`]
//! / `incremental_precompute_counters` call — because two *different* nodes
//! sharing a key is only meaningful for the one traversal that discovers them
//! side by side; carrying it forward would mean comparing this pass's nodes
//! against a stale key built from a previous pass's document.
//!
//! # Why the key is safe
//!
//! The key is `(tag, every attribute name+value sorted, the address of the
//! `inherited` [`ComputedStyle`] this call received)`. The first two pin
//! everything [`compute_style`](crate::style::compute_style) reads off the
//! node itself — full attributes, not just `class`/`id`, because presentation
//! attributes (`fill`, `stroke`, …) and `style=""` feed the cascade too.
//!
//! The third — `inherited` identity, not equality — is what makes CSS
//! inheritance safe to skip: two calls that received the *very same*
//! [`ComputedStyle`] allocation as `inherited` are guaranteed identical
//! answers for every property that resolves via inheritance (`color`,
//! `font-*`, custom properties, …), because there is only one allocation to
//! disagree with. That identity is not a coincidence to hope for — it
//! *propagates*: `counters::walk` passes each node's own freshly built
//! `Arc<ComputedStyle>` (dereferenced) as `inherited` to every child, so once
//! a repeated container (say, a `<nav>` item template) hits the cache, its
//! repeated children automatically see the same `inherited` pointer too and
//! become eligible in turn.
//!
//! `matches_complex` walks the live DOM tree directly, not `inherited`, so it
//! never consults this cache's key at all — but the key's `inherited_ptr`
//! field turns out to prove something about *selector matching* against
//! ancestors too (BUG-1112): the same "identity, not equality" propagation
//! that makes CSS inheritance safe to skip also means two colliding keys can
//! only arise from a genuinely shared ancestor lineage (see
//! [`crate::style::cascade::selector_is_share_safe`]'s doc comment for the
//! induction). That is why `Descendant`/`Child` combinators are allowed to
//! mark a result shareable — only sibling combinators and anything the key
//! does not pin at the *subject* node itself (pseudo-classes, non-`class`/
//! `id` attribute selectors, Shadow DOM, `@scope`) still disqualify it; see
//! [`crate::style::cascade::compute_style_shareable`]'s doc comment for the
//! exact conditions, including why this slice is scoped to SVG
//! presentational elements only.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};

use super::cascade::compute_style_shareable;
use super::ComputedStyle;

/// One node's cascade-relevant identity, see the module doc for why it is a
/// sound proxy for "would produce the same `ComputedStyle`".
#[derive(PartialEq, Eq, Hash)]
struct ShareKey {
    tag: Box<str>,
    /// `(name, value)` pairs, sorted by name — full attribute set, not just
    /// `class`/`id` (see module doc).
    attrs: Vec<(Box<str>, Box<str>)>,
    /// `&ComputedStyle as *const _ as usize` — allocation identity of the
    /// `inherited` parameter, not its content (see module doc).
    inherited_ptr: usize,
}

/// Builds the structural key for `node`, or `None` when `node` is not an
/// element the cascade's SVG-only eligibility check
/// ([`compute_style_shareable`]) could ever mark shareable — skipping the key
/// build (an attribute-set allocation) on the common case of plain HTML nodes.
///
/// Also `None` for a node CSS Scoping L1 §6.1-6.2 treats specially — a
/// shadow host itself (`:host`), a slotted light child (`::slotted()`), or a
/// node living inside a shadow tree (its own interior selectors) — even
/// though `ShareKey` has no field for any of this. THREAD-4 срез 4: this is
/// not redundant with [`compute_style_shareable`]'s own `shareable` output.
/// That flag only gates *insertion*; [`ShareCache::compute`]'s cache *lookup*
/// keys purely on `build_key`'s output, so if two structural siblings under
/// the same parent (same `tag`+`attrs`+`inherited_ptr`) differed only in one
/// having an attached shadow root, a `None` here is the only thing that
/// stops the plain sibling's cached document-scope style from being handed
/// back for the shadow-host sibling's `:host`-scoped one.
fn build_key(doc: &Document, node: NodeId, inherited: &ComputedStyle) -> Option<ShareKey> {
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return None;
    };
    if !super::presentational::is_svg_presentational_element(name.local.as_ref()) {
        return None;
    }
    if doc.is_shadow_host(node)
        || doc.get(node).parent.is_some_and(|p| doc.is_shadow_host(p))
        || doc.enclosing_shadow_host(node).is_some()
    {
        return None;
    }
    let mut pairs: Vec<(Box<str>, Box<str>)> = attrs
        .iter()
        .map(|a| (Box::from(a.name.local.as_ref()), Box::from(a.value.as_str())))
        .collect();
    pairs.sort();
    Some(ShareKey {
        tag: Box::from(name.local.as_ref()),
        attrs: pairs,
        inherited_ptr: inherited as *const ComputedStyle as usize,
    })
}

/// Per-pass structural memo — see module doc.
#[derive(Default)]
pub(crate) struct ShareCache {
    entries: HashMap<ShareKey, Arc<ComputedStyle>>,
}

impl ShareCache {
    /// The cascade result for `node` — the same [`Arc`] allocation a previous
    /// call already produced for an equal key (a refcount bump, per BUG-341
    /// S9's reasoning: no deep copy on a hit), else a fresh
    /// [`compute_style_shareable`] call wrapped in a new one — cached for
    /// later nodes exactly when that call reports its own result safe to
    /// share.
    pub(crate) fn compute(
        &mut self,
        doc: &Document,
        node: NodeId,
        sheet: &Stylesheet,
        inherited: &ComputedStyle,
        viewport: Size,
        dark_mode: bool,
    ) -> Arc<ComputedStyle> {
        let key = build_key(doc, node, inherited);
        if let Some(hit) = key.as_ref().and_then(|k| self.entries.get(k)) {
            return Arc::clone(hit);
        }
        let (style, shareable) = compute_style_shareable(doc, node, sheet, inherited, viewport, dark_mode);
        let style = Arc::new(style);
        if shareable && let Some(k) = key {
            self.entries.insert(k, Arc::clone(&style));
        }
        style
    }
}
