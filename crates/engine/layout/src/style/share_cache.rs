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
//! does not pin anywhere in the induced chain, at either the *subject* node
//! itself (attribute selectors, BUG-1112 срез 3, and
//! `:first-child`/`:last-child`/`:only-child`, срез 4, are pinned directly
//! by `attrs`/`is_first_child`/`is_last_child` — no induction needed) or an
//! ancestor (an attribute selector is pinned too, BUG-1112 срез 5, by the
//! SAME induction that already covers `Type`/`Class`/`Id` there — see the
//! cascade doc comment; a dynamic pseudo-class the induction cannot prove
//! state-identical, like `:hover`, whether it sits on an ancestor compound or
//! on the SUBJECT compound itself (BUG-1112 срез 10 — `node:hover`, a fact
//! about the key node, not an ancestor), is instead pinned directly by
//! [`ShareKey::dynamic_fingerprint_sig`] — BUG-1112 срез 8/10 — the real,
//! current match result of that one selector against `node`, folded into the
//! key the same way `is_first_child` is), plus Shadow DOM and `@scope`, still
//! disqualify it (a sibling combinator has no field here at any depth); see
//! [`crate::style::cascade::compute_style_shareable`]'s doc comment for the
//! exact conditions, including why this slice is scoped to SVG
//! presentational elements only.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};

use super::cascade::compute_style_shareable;
use super::share_safety::{dynamic_fingerprint, sheet_has_fingerprintable_selector};
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
    /// BUG-1112 срез 4: whether `node` is its parent's first/last
    /// element-child (same predicates `matching/forms.rs`'s
    /// `:first-child`/`:last-child` matcher uses) — but only when
    /// [`sheet_has_position_dependent_subject`] found the current stylesheet
    /// actually contains a `:first-child`/`:last-child`/`:only-child` subject
    /// selector anywhere; `false`/`false` otherwise, unconditionally, on
    /// every node. That gate matters: this field is a direct per-node fact
    /// (no `inherited_ptr`-style induction needed to prove two colliding
    /// keys agree on it), so it soundly lets such a subject pseudo-class
    /// stop disqualifying sharing (see `cascade::selector_is_share_safe`) —
    /// but adding it to the key *unconditionally* costs real sharing on
    /// every OTHER document, splitting siblings that used to collide (most
    /// repeated groups have exactly one true first-child and one true
    /// last-child) even when no rule in the sheet cares about position at
    /// all. Gating on a one-time-per-pass sheet scan keeps that cost paid
    /// only where it buys something: measured on github.com/lenta.ru,
    /// `repeated_svg_icons_share_the_cascade_without_changing_the_result`
    /// (a plain `.octicon { fill }`-only sheet, no position selectors) is
    /// the regression this gate exists to prevent — first version of this
    /// срез broke it by omitting the gate entirely.
    is_first_child: bool,
    /// See [`Self::is_first_child`].
    is_last_child: bool,
    /// BUG-1112 срез 8 (ancestor), срез 10 (subject): [`dynamic_fingerprint`]'s
    /// output for this node — one bit per candidate selector
    /// `share_safety::selector_is_share_safe` treats as `is_fingerprintable`
    /// (a dynamic pseudo-class like `:hover`/`:focus` the key otherwise
    /// cannot pin, on an ancestor compound OR on the subject compound
    /// itself), or empty when [`sheet_has_fingerprintable_selector`] found
    /// the sheet has none at all — same gating shape as
    /// [`Self::is_first_child`], and for the same reason: computing this
    /// unconditionally would cost a `RuleIndex::candidates` walk on every
    /// eligible node even on sheets that never need it. Two colliding nodes
    /// only share when this vector is also equal, i.e. when their real,
    /// current match result for every such selector agrees — see
    /// `share_safety::selector_is_share_safe`'s doc comment for why that is
    /// exactly the condition sharing needs.
    dynamic_fingerprint_sig: Vec<bool>,
}

/// BUG-1112 срез 4: `true` when `sheet` (or any of its `@layer`/`@media`/
/// `@supports` blocks — the same set `cascade.rs`'s `shareable &=
/// rule.selectors.iter().all(selector_is_share_safe)` loop walks) contains a
/// selector whose *subject* compound carries `:first-child`/`:last-child`/
/// `:only-child`. Scans every rule's every selector once per `ShareCache`
/// lifetime (one pass), not per node — see [`ShareCache::compute`]'s caller.
///
/// Deliberately coarse: does not check whether the selector could ever reach
/// an SVG-presentational element, only whether the pseudo-class exists
/// anywhere in the subject position. A `false` positive here only costs a
/// little extra key granularity (still sound, just less sharing than
/// optimal); a `false` negative would be unsound (a real position rule could
/// then reach a node whose key was never disambiguated).
fn sheet_has_position_dependent_subject(sheet: &Stylesheet) -> bool {
    fn subject(c: &lumen_css_parser::ComplexSelector) -> &lumen_css_parser::CompoundSelector {
        c.tail.last().map(|(_, comp)| comp).unwrap_or(&c.head)
    }
    fn subject_has_position_pseudo(c: &lumen_css_parser::ComplexSelector) -> bool {
        subject(c).parts.iter().any(|p| {
            matches!(
                p,
                lumen_css_parser::SimpleSelector::PseudoClass(
                    lumen_css_parser::PseudoClass::FirstChild
                        | lumen_css_parser::PseudoClass::LastChild
                        | lumen_css_parser::PseudoClass::OnlyChild
                )
            )
        })
    }
    fn rules_have_it(rules: &[lumen_css_parser::Rule]) -> bool {
        rules.iter().any(|r| r.selectors.iter().any(subject_has_position_pseudo))
    }
    rules_have_it(&sheet.rules)
        || sheet.layers.iter().any(|l| rules_have_it(&l.rules))
        || sheet.media_rules.iter().any(|m| rules_have_it(&m.rules))
        || sheet.supports_rules.iter().any(|s| rules_have_it(&s.rules))
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
#[allow(clippy::too_many_arguments)]
fn build_key(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    track_position: bool,
    track_fingerprint: bool,
) -> Option<ShareKey> {
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
    // BUG-1112 срез 4: real values only when the sheet actually has a rule
    // that needs them — see `ShareKey::is_first_child`'s doc comment for why
    // `false`/`false` unconditionally otherwise, not a per-node computation
    // that is merely unused.
    let (is_first_child, is_last_child) = if track_position {
        (
            super::matching::forms::is_first_element_child(doc, node),
            super::matching::forms::is_last_element_child(doc, node),
        )
    } else {
        (false, false)
    };
    // BUG-1112 срез 8/10 — see `ShareKey::dynamic_fingerprint_sig`'s doc comment.
    let dynamic_fingerprint_sig =
        if track_fingerprint { dynamic_fingerprint(doc, node, sheet, viewport, dark_mode) } else { Vec::new() };
    Some(ShareKey {
        tag: Box::from(name.local.as_ref()),
        attrs: pairs,
        inherited_ptr: inherited as *const ComputedStyle as usize,
        is_first_child,
        is_last_child,
        dynamic_fingerprint_sig,
    })
}

/// Per-pass structural memo — see module doc.
#[derive(Default)]
pub(crate) struct ShareCache {
    entries: HashMap<ShareKey, Arc<ComputedStyle>>,
    /// BUG-1112 срез 2 — measurement-only pass counters, see [`Self::stats_enabled`].
    hits: usize,
    inserts: usize,
    misses: usize,
    /// Diagnostic-only split of `misses`: `build_key` returned `None`
    /// (not eligible at all) vs `Some` but `compute_style_shareable`
    /// reported the result unsafe to cache.
    key_none: usize,
    key_some_unshareable: usize,
    /// BUG-1112 срез 4: [`sheet_has_position_dependent_subject`]'s result for
    /// this pass's `sheet`, computed once on the first [`Self::compute`] call
    /// and reused after — every call in one `ShareCache` lifetime receives
    /// the same `sheet` (see `counters::precompute_counters`'s single
    /// `sheet: &Stylesheet` parameter threaded through the whole walk), so
    /// scanning it again per node would be pure waste.
    track_position: Option<bool>,
    /// BUG-1112 срез 8/10: [`sheet_has_fingerprintable_selector`]'s
    /// result for this pass's `sheet`, cached the same way as
    /// [`Self::track_position`] and for the same reason.
    track_fingerprint: Option<bool>,
}

impl ShareCache {
    /// `LUMEN_SHARECACHE_STATS=1` — BUG-1112 срез 2. Reads the env var once per
    /// process (`OnceLock`, same pattern as `shell::relayout`'s
    /// `LUMEN_BUG935_M4_SWAP`): unset changes nothing for anyone who has not
    /// set it, set prints one `[sharecache]` line per pass to stderr so a live
    /// run answers "does `share_insert` become nonzero on a real page" without
    /// a debugger.
    fn stats_enabled() -> bool {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var("LUMEN_SHARECACHE_STATS").ok().as_deref() == Some("1"))
    }

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
        let track_position =
            *self.track_position.get_or_insert_with(|| sheet_has_position_dependent_subject(sheet));
        let track_fingerprint = *self
            .track_fingerprint
            .get_or_insert_with(|| sheet_has_fingerprintable_selector(sheet));
        let key = build_key(doc, node, sheet, inherited, viewport, dark_mode, track_position, track_fingerprint);
        if let Some(hit) = key.as_ref().and_then(|k| self.entries.get(k)) {
            if Self::stats_enabled() {
                self.hits += 1;
            }
            return Arc::clone(hit);
        }
        let key_was_some = key.is_some();
        let (style, shareable) = compute_style_shareable(doc, node, sheet, inherited, viewport, dark_mode);
        let style = Arc::new(style);
        if shareable && let Some(k) = key {
            self.entries.insert(k, Arc::clone(&style));
            if Self::stats_enabled() {
                self.inserts += 1;
            }
        } else if Self::stats_enabled() {
            self.misses += 1;
            if key_was_some {
                self.key_some_unshareable += 1;
            } else {
                self.key_none += 1;
            }
        }
        style
    }
}

impl Drop for ShareCache {
    /// BUG-1112 срез 2 — prints this pass's `share_hit`/`share_insert`/
    /// `share_miss` totals when [`ShareCache::stats_enabled`], mirroring
    /// THREAD-4 срез 5's ad-hoc (uncommitted) instrumentation so it survives
    /// as a reusable, opt-in tool instead of being re-derived every time this
    /// bug needs a live number.
    fn drop(&mut self) {
        if Self::stats_enabled() && (self.hits != 0 || self.inserts != 0 || self.misses != 0) {
            eprintln!(
                "[sharecache] hit={} insert={} miss={} (key_none={} key_some_unshareable={})",
                self.hits, self.inserts, self.misses, self.key_none, self.key_some_unshareable
            );
        }
    }
}
