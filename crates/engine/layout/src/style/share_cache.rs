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
//! The key is `(tag, the cascade-relevant attribute set sorted, the address
//! of the `inherited` [`ComputedStyle`] this call received, plus the
//! per-node dynamic bits — sibling position, `:focus-visible`, CSP-blocked
//! `style`, the dynamic-selector fingerprint)`. The attribute set is
//! *projected*: only names something in the cascade can actually read are
//! pinned ([`sheet_relevant_attr_names`] ∪ [`DIRECT_READ_ATTRS`]) — analytics
//! `data-*`/`aria-*` attributes no rule, hint or `attr()` references cannot
//! change the result, so pinning them would only split keys that should
//! collide. `style=""` and presentation attributes still feed the cascade
//! and stay in the key via [`DIRECT_READ_ATTRS`].
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
//! exact conditions. Eligibility was later extended from SVG presentational
//! elements to all HTML-namespace elements ([`share_key_eligible`]) once the
//! dynamic-state reads (`:focus-visible`, CSP-blocked `style`, the
//! attribute projection) were pinned in the key — `LUMEN_NO_SHARE_CACHE=1`
//! rolls the whole cache back, `LUMEN_SHARECACHE_VERIFY=1` recomputes every
//! hit and reports any divergence.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId};

use super::cascade::compute_style_shareable;
use super::invalidation::MatchedRule;
use super::share_safety::{compound_is_bare_universal, dynamic_fingerprint, sheet_has_fingerprintable_selector};
use super::ComputedStyle;

/// One node's cascade-relevant identity, see the module doc for why it is a
/// sound proxy for "would produce the same `ComputedStyle`".
#[derive(PartialEq, Eq, Hash)]
struct ShareKey {
    tag: Box<str>,
    /// `(name, value)` pairs, sorted by name — the attribute set **projected
    /// to names that can influence the cascade** ([`sheet_relevant_attr_names`]
    /// ∪ [`DIRECT_READ_ATTRS`]), not the full set: analytics/`data-*`/`aria-*`
    /// attributes no selector, hint or `attr()` ever reads cannot change the
    /// result, so pinning them would only split keys that should collide.
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
    /// `doc.is_style_attr_csp_blocked(node)` — a `style=""` attribute a
    /// `style-src-attr` policy forbids contributes no declarations
    /// (`cascade.rs`), so two nodes equal on every other field still compute
    /// different styles when only this bit differs.
    style_csp_blocked: bool,
    /// THREAD-4 срез N: whether [`super::matching::forms::matches_focus_visible`]
    /// holds for this node *right now* — the one piece of live, unkeyed state
    /// the UA phase reads outside selectors (`apply_ua_focus_ring`'s
    /// `outline: 2px auto`). Cheap (a TLS focus-id compare plus, only for the
    /// focused node, a tag/type check) and computed for every eligible node:
    /// a focused element and its identical sibling must not share a style.
    focus_visible: bool,
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

/// BUG-1112 срез 4 (`:first-child`/`:last-child`/`:only-child`), срез 11
/// (`* + subject` — see `share_safety::compound_is_bare_universal`'s doc
/// comment): `true` when `sheet` (or any of its `@layer`/`@media`/
/// `@supports` blocks — the same set `cascade.rs`'s `shareable &=
/// rule.selectors.iter().all(selector_is_share_safe)` loop walks) contains a
/// selector whose *subject* compound needs the real `is_first_child`/
/// `is_last_child` value to be judged share-safe. Scans every rule's every
/// selector once per `ShareCache` lifetime (one pass), not per node — see
/// [`ShareCache::compute`]'s caller.
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
    // BUG-1112 срез 11: `* + subject` needs the same `is_first_child` field —
    // see `share_safety::compound_is_bare_universal`'s doc comment.
    fn subject_preceded_by_bare_universal_sibling(c: &lumen_css_parser::ComplexSelector) -> bool {
        let Some((comb, _)) = c.tail.last() else { return false };
        if !matches!(comb, lumen_css_parser::Combinator::NextSibling) {
            return false;
        }
        let prev = if c.tail.len() >= 2 { &c.tail[c.tail.len() - 2].1 } else { &c.head };
        compound_is_bare_universal(prev)
    }
    fn rules_have_it(rules: &[lumen_css_parser::Rule]) -> bool {
        rules.iter().any(|r| {
            r.selectors
                .iter()
                .any(|s| subject_has_position_pseudo(s) || subject_preceded_by_bare_universal_sibling(s))
        })
    }
    rules_have_it(&sheet.rules)
        || sheet.layers.iter().any(|l| rules_have_it(&l.rules))
        || sheet.media_rules.iter().any(|m| rules_have_it(&m.rules))
        || sheet.supports_rules.iter().any(|s| rules_have_it(&s.rules))
}

/// Attribute names `compute_style_shareable` can read **outside** selector
/// matching — the union of every literal `get_attr("…")` under
/// `style/{presentational,ua,cascade,adjust,pseudo}.rs` plus the
/// pseudo-class matchers' own read set
/// ([`super::restyle::pseudo_class_read_attr_names`]). Kept deliberately
/// over-inclusive: an extra name only splits a key that could have collided,
/// a missing one hands a node a style computed for different attributes.
/// All entries are lowercase; membership tests are ASCII case-insensitive
/// (see [`attr_is_relevant`]).
const DIRECT_READ_ATTRS: &[&str] = &[
    "align", "background", "baseline-shift", "bgcolor", "border", "bordercolor", "cellpadding",
    "cellspacing", "checked", "class", "clip-path", "clip-rule", "color", "color-interpolation-filters",
    "contenteditable", "data-lumen-fullscreen", "data-lumen-interest-source",
    "data-lumen-interest-target", "data-lumen-modal", "data-lumen-popover-open", "dir", "disabled",
    "dominant-baseline", "exportparts", "face", "fill", "fill-opacity", "fill-rule", "filter",
    "flood-color", "flood-opacity", "headingoffset", "headingreset", "height", "hidden", "href",
    "hspace", "id", "inert", "lang", "lighting-color", "mask", "max", "maxlength", "min", "minlength",
    "multiple", "name", "opacity", "open", "part", "placeholder", "readonly", "required", "reversed",
    "selected", "size", "slot", "start", "stroke", "stroke-dasharray", "stroke-dashoffset",
    "stroke-linecap", "stroke-linejoin", "stroke-miterlimit", "stroke-opacity", "stroke-width",
    "style", "text", "text-anchor", "type", "valign", "value", "vspace", "width", "xml:lang",
];

/// `data-lumen-state-<name>` is the one attribute family whose name is built
/// at match time (`PseudoClass::State`, `restyle::pseudo_class_reads_attr`)
/// — it cannot be enumerated, so projection keeps every attr carrying the
/// prefix. These attributes are engine-internal and rare outside `:state()`
/// usage, so the unconditional keep costs almost nothing.
const DYNAMIC_STATE_ATTR_PREFIX: &str = "data-lumen-state-";

/// Membership test for [`ShareKey::attrs`] projection: `local` is an
/// attribute's `QualName::local` as stored in the DOM (HTML parser output is
/// lowercase, SVG/MathML may keep camelCase such as `viewBox`). The relevant
/// set is all-lowercase, so a name that is not already lowercase is retried
/// lowercased — matching in this engine is ASCII case-insensitive for these
/// reads (`get_attr`, `matches_attribute`), so keeping the attr whenever
/// EITHER case form is relevant is the conservative direction.
fn attr_is_relevant(relevant: &HashSet<String>, local: &str) -> bool {
    if relevant.contains(local) {
        return true;
    }
    if !local.bytes().any(|b| b.is_ascii_uppercase()) {
        return false;
    }
    let lower = local.to_ascii_lowercase();
    relevant.contains(lower.as_str())
}

/// Extracts the attribute names of every `attr(<name> …)` call in a
/// declaration value. Over-inclusive by design: a hit inside a string
/// literal or comment only adds a name to the relevant set (costing a few
/// potential key collisions), while a miss would let two nodes with
/// different attributes share a style — `cascade.rs` expands `attr()` at
/// apply time, so the name must be pinned whenever it can appear.
fn collect_attr_call_names(value: &str, out: &mut HashSet<String>) {
    // ASCII-lowercase copy keeps `match_indices` positions identical (the
    // transform never changes byte length) while catching `ATTR(`/`Attr(`.
    let lower = value.to_ascii_lowercase();
    let bytes = value.as_bytes();
    let mut pos = 0;
    while let Some(rel) = lower[pos..].find("attr(") {
        let mut i = pos + rel + 5;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name = if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i + 1;
            let end = bytes[start..].iter().position(|&b| b == quote).map(|p| start + p).unwrap_or(bytes.len());
            &value[start..end]
        } else {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_' | b':'))
            {
                i += 1;
            }
            &value[start..i]
        };
        if !name.is_empty() {
            out.insert(name.to_ascii_lowercase());
        }
        pos += rel + 5;
    }
}

fn collect_selector_attr_names(sel: &lumen_css_parser::ComplexSelector, out: &mut HashSet<String>) {
    fn compound(c: &lumen_css_parser::CompoundSelector, out: &mut HashSet<String>) {
        for part in &c.parts {
            match part {
                lumen_css_parser::SimpleSelector::Attribute(a) => {
                    out.insert(a.name.to_ascii_lowercase());
                }
                lumen_css_parser::SimpleSelector::PseudoClass(pc) => pseudo(pc, out),
                // `::slotted()`'s argument lives in the shadow sheet, never in
                // the document `sheet` this scan walks — but a `::part()`-style
                // nested list is cheap to include if it ever appears.
                lumen_css_parser::SimpleSelector::PseudoElement(
                    lumen_css_parser::PseudoElementKind::Slotted(Some(list)),
                ) => {
                    for c in list {
                        collect_selector_attr_names(c, out);
                    }
                }
                _ => {}
            }
        }
    }
    fn pseudo(pc: &lumen_css_parser::PseudoClass, out: &mut HashSet<String>) {
        use lumen_css_parser::PseudoClass as P;
        match pc {
            P::Not(list) | P::Is(list) | P::Where(list) | P::Host(Some(list)) => {
                for c in list {
                    collect_selector_attr_names(c, out);
                }
            }
            P::NthChild(_, Some(list)) | P::NthLastChild(_, Some(list)) => {
                for c in list {
                    collect_selector_attr_names(c, out);
                }
            }
            P::Has(rels) => {
                for r in rels {
                    collect_selector_attr_names(&r.selector, out);
                }
            }
            // `:state(<ident>)` reads `data-lumen-state-<ident>` — a name built
            // at match time; projection keeps the whole prefix unconditionally
            // (see `DYNAMIC_STATE_ATTR_PREFIX`), so nothing to collect here.
            P::State(_) => {}
            other => {
                for name in super::restyle::pseudo_class_read_attr_names(other) {
                    out.insert((*name).to_string());
                }
            }
        }
    }
    compound(&sel.head, out);
    for (_, c) in &sel.tail {
        compound(c, out);
    }
}

/// The attribute names a node's computed style can possibly depend on, for
/// this pass's `sheet` — see [`ShareKey::attrs`]. Union of:
///
/// * [`DIRECT_READ_ATTRS`] — every literal attribute the engine reads outside
///   selector matching (UA sheet, presentational hints, inline `style`,
///   `lang`/`xml:lang` case folding, CSP-blocked style attrs are covered by
///   the separate [`ShareKey::style_csp_blocked`] bit instead);
/// * every attribute-selector name in the sheet, at any nesting depth
///   (`:is()`/`:not()`/`:where()`/`:has()`/`:nth-child(… of …)`/`::slotted()`);
/// * every attribute name a pseudo-class matcher in the sheet reads
///   ([`super::restyle::pseudo_class_read_attr_names`]) — redundant with the
///   dynamic fingerprint for the match *result*, kept anyway so the key does
///   not silently rely on that machinery staying enabled;
/// * every `attr(<name>)` referenced by any declaration value.
///
/// Anything else — analytics `data-*`, `aria-*`, framework bookkeeping —
/// provably cannot reach [`compute_style_shareable`]'s output, so two nodes
/// differing only there MUST share a key (that is the whole point of the
/// projection: github/habr markup repeats structure while varying exactly
/// these attributes).
fn sheet_relevant_attr_names(sheet: &Stylesheet) -> HashSet<String> {
    let mut out: HashSet<String> = DIRECT_READ_ATTRS.iter().map(|s| (*s).to_string()).collect();
    fn rule(r: &lumen_css_parser::Rule, out: &mut HashSet<String>) {
        for sel in &r.selectors {
            collect_selector_attr_names(sel, out);
        }
        for d in &r.declarations {
            collect_attr_call_names(&d.value, out);
        }
    }
    fn rules(rs: &[lumen_css_parser::Rule], out: &mut HashSet<String>) {
        for r in rs {
            rule(r, out);
        }
    }
    rules(&sheet.rules, &mut out);
    for l in &sheet.layers {
        rules(&l.rules, &mut out);
    }
    for m in &sheet.media_rules {
        rules(&m.rules, &mut out);
    }
    for s in &sheet.supports_rules {
        rules(&s.rules, &mut out);
    }
    out
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
    relevant_attrs: &HashSet<String>,
) -> Option<ShareKey> {
    let NodeData::Element { name, attrs, .. } = &doc.get(node).data else {
        return None;
    };
    if !share_key_eligible(doc, node) {
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
        .filter(|a| {
            let local = a.name.local.as_str();
            attr_is_relevant(relevant_attrs, local)
                || (local.len() >= DYNAMIC_STATE_ATTR_PREFIX.len()
                    && local[..DYNAMIC_STATE_ATTR_PREFIX.len()].eq_ignore_ascii_case(DYNAMIC_STATE_ATTR_PREFIX))
        })
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
        focus_visible: super::matching::forms::matches_focus_visible(doc, node),
        style_csp_blocked: doc.is_style_attr_csp_blocked(node),
        dynamic_fingerprint_sig,
    })
}

/// THREAD-4 срез N — elements whose computed style may be shared across
/// distinct nodes with an equal [`ShareKey`]. The SVG presentational set
/// (срез 2, audited ancestor-independent) plus **HTML-namespace** elements:
/// every UA hint/presentational pass in `compute_style_shareable` was audited
/// for reads the key does not capture — ancestor walks (`cellpadding`,
/// nested-list margins, `rtc > rt`) are covered by the `inherited_ptr`
/// induction (equal keys ⇒ pairwise key-equal ancestor chains ⇒ identical
/// ancestor attributes), sentinel attributes (`data-lumen-modal`,
/// `data-lumen-fullscreen`, `open`, `inert`, `hidden`) are ordinary
/// attributes, and the single live-state read — `:focus-visible` — is pinned
/// by [`ShareKey::focus_visible`]. Excluded: UA-shadow internals
/// (`ua_slot_role`/`ua_vtt_role`), whose style keys on a DOM role no key
/// field captures, and every non-HTML/SVG-eligible namespace.
pub(crate) fn share_key_eligible(doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return false;
    };
    if super::presentational::is_svg_presentational_element(name.local.as_ref()) {
        return true;
    }
    if name.namespace != lumen_dom::Namespace::Html {
        return false;
    }
    doc.ua_slot_role(node).is_none() && doc.ua_vtt_role(node).is_none()
}

// PERF-SHARE-PROBE — allocation-free negative filter in front of `build_key`.
// `compute` used to build a full `ShareKey` (tag `Box<str>`, sorted
// `Vec<(Box<str>,Box<str>)>`, fingerprint `Vec<bool>`) for EVERY node before
// it could learn the cache had no equal entry — on habr inserts outnumber
// hits ~5:1, so most of those builds were pure waste. `key_hashes` stores a
// u64 hash of every inserted key; `cheap_key_hash` recomputes the same hash
// from the node with zero allocations (attribute order-insensitive — the
// stored key's `attrs` are sorted, so the hash sums per-pair mixes). A hash
// absent from the set proves no equal key was ever inserted, so the node
// skips `build_key` entirely unless `compute_style_shareable` reports the
// result shareable and an insert actually needs the key. Hash collisions
// only cost a full lookup — never a wrong answer.
fn mix_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn mix_str(h: u64, s: &str) -> u64 {
    mix_bytes(h, s.as_bytes())
}

/// Whether `local` survives the [`build_key`] attribute projection — kept
/// in sync with the `filter` there; a drift only weakens the probe (extra
/// attributes in the hash can split hits, missing ones cannot happen
/// because both sides call this same predicate).
fn attr_survives_projection(relevant: &HashSet<String>, local: &str) -> bool {
    attr_is_relevant(relevant, local)
        || (local.len() >= DYNAMIC_STATE_ATTR_PREFIX.len()
            && local[..DYNAMIC_STATE_ATTR_PREFIX.len()].eq_ignore_ascii_case(DYNAMIC_STATE_ATTR_PREFIX))
}

/// Order-insensitive hash of the projected `(name, value)` attribute set:
/// equal sets hash equal regardless of DOM attribute order (the stored
/// `ShareKey::attrs` are sorted, so this MUST NOT depend on iteration
/// order).
fn hash_attr_pairs(h: u64, pairs: &[(Box<str>, Box<str>)]) -> u64 {
    let mut acc = 0u64;
    for (n, v) in pairs {
        let mut p = mix_str(0x9e37_79b9_7f4a_7c15, n);
        p = mix_str(p, "\u{1}");
        p = mix_str(p, v);
        acc = acc.wrapping_add(p);
    }
    h.wrapping_add(acc)
}

fn share_key_hash(k: &ShareKey) -> u64 {
    let mut h = mix_str(0xcbf2_9ce4_8422_2325, &k.tag);
    h = hash_attr_pairs(h, &k.attrs);
    h = h.wrapping_add(k.inherited_ptr as u64);
    h = h
        .wrapping_add(u64::from(k.is_first_child))
        .wrapping_add(u64::from(k.is_last_child) << 1)
        .wrapping_add(u64::from(k.style_csp_blocked) << 2)
        .wrapping_add(u64::from(k.focus_visible) << 3);
    for &b in &k.dynamic_fingerprint_sig {
        h = h.wrapping_mul(0x1000_0000_01b3).wrapping_add(u64::from(b));
    }
    h
}

/// Allocation-free [`ShareKey`] hash probe — `Some(h)` only when the node
/// passes every `build_key` early-out (element, eligible, not shadow
/// related) AND the pass tracks neither position nor fingerprint
/// selectors, so `is_first_child`/`is_last_child`/`dynamic_fingerprint_sig`
/// are the canonical `false`/`false`/empty. Anything else returns `None`
/// and the caller takes the full `build_key` path.
fn cheap_key_hash(
    doc: &Document,
    node: NodeId,
    inherited: &ComputedStyle,
    relevant_attrs: &HashSet<String>,
) -> Option<u64> {
    let NodeData::Element { name, attrs, .. } = &doc.get(node).data else {
        return None;
    };
    if !share_key_eligible(doc, node) {
        return None;
    }
    if doc.is_shadow_host(node)
        || doc.get(node).parent.is_some_and(|p| doc.is_shadow_host(p))
        || doc.enclosing_shadow_host(node).is_some()
    {
        return None;
    }
    let mut h = mix_str(0xcbf2_9ce4_8422_2325, name.local.as_str());
    // Same per-pair mix `hash_attr_pairs` applies to the stored key, fed in
    // DOM order — the wrapping_add accumulation makes order irrelevant.
    let mut acc = 0u64;
    for a in attrs.iter() {
        let local = a.name.local.as_str();
        if !attr_survives_projection(relevant_attrs, local) {
            continue;
        }
        let mut p = mix_str(0x9e37_79b9_7f4a_7c15, local);
        p = mix_str(p, "\u{1}");
        p = mix_str(p, a.value.as_str());
        acc = acc.wrapping_add(p);
    }
    h = h.wrapping_add(acc);
    h = h.wrapping_add(inherited as *const ComputedStyle as usize as u64);
    h = h
        .wrapping_add(u64::from(doc.is_style_attr_csp_blocked(node)) << 2)
        .wrapping_add(u64::from(super::matching::forms::matches_focus_visible(doc, node)) << 3);
    Some(h)
}

/// Per-pass structural memo — see module doc.
#[derive(Default)]
pub(crate) struct ShareCache {
    entries: HashMap<ShareKey, (Arc<ComputedStyle>, Arc<[MatchedRule]>)>,
    /// Hashes of every key in `entries` — the negative filter described at
    /// [`cheap_key_hash`]. Inserted and removed together with `entries`.
    key_hashes: HashSet<u64>,
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
    /// [`sheet_relevant_attr_names`]'s result for this pass's `sheet`,
    /// computed on the first [`Self::compute`] call — one sheet walk per
    /// pass, reused for every node's [`build_key`].
    relevant_attrs: Option<HashSet<String>>,
    /// `LUMEN_SHARECACHE_VERIFY=1` — on every hit, recompute the node fresh
    /// and compare `(ComputedStyle, matched rules)` against the cached pair;
    /// counts and prints (capped) mismatches. Verification only, never
    /// changes the returned value.
    verify_checked: usize,
    verify_mismatch: usize,
}

impl ShareCache {
    /// `LUMEN_NO_SHARE_CACHE=1` — compute every node fresh, no lookup, no
    /// insert: the rollback switch for the HTML-eligibility extension.
    fn disabled() -> bool {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *OFF.get_or_init(|| std::env::var_os("LUMEN_NO_SHARE_CACHE").is_some_and(|v| v != "0"))
    }

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

    /// `LUMEN_SHARECACHE_VERIFY=1` — see [`Self::verify_checked`]. Implies
    /// the stats line too so a run reports hits and mismatches together.
    fn verify_enabled() -> bool {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var("LUMEN_SHARECACHE_VERIFY").ok().as_deref() == Some("1"))
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
    ) -> (Arc<ComputedStyle>, Arc<[MatchedRule]>) {
        if Self::disabled() {
            let mut matched_rules = Vec::new();
            let (style, _) = compute_style_shareable(
                doc, node, sheet, inherited, viewport, dark_mode, None, &mut matched_rules, false,
            );
            return (Arc::new(style), Arc::from(matched_rules));
        }
        let track_position =
            *self.track_position.get_or_insert_with(|| sheet_has_position_dependent_subject(sheet));
        let track_fingerprint = *self
            .track_fingerprint
            .get_or_insert_with(|| sheet_has_fingerprintable_selector(sheet));
        let relevant_attrs = self
            .relevant_attrs
            .get_or_insert_with(|| sheet_relevant_attr_names(sheet));
        // PERF-SHARE-PROBE: на листах без position/fingerprint-селекторов
        // бесаллокационный хэш решает, возможен ли вообще lookup. `None`
        // (трекаемые листы, неподходящий узел) или хэш, присутствующий в
        // `key_hashes`, ведут по полному пути `build_key`; отсутствующий хэш
        // доказывает, что равного ключа нет, — `build_key` запускается,
        // только если свежий результат оказался shareable и требует insert.
        let probe = if track_position || track_fingerprint {
            None
        } else {
            cheap_key_hash(doc, node, inherited, relevant_attrs)
        };
        let mut key = match probe {
            Some(h) if !self.key_hashes.contains(&h) => None,
            _ => build_key(
                doc, node, sheet, inherited, viewport, dark_mode, track_position, track_fingerprint,
                relevant_attrs,
            ),
        };
        if let Some(hit) = key.as_ref().and_then(|k| self.entries.get(k)) {
            if Self::stats_enabled() {
                self.hits += 1;
            }
            if Self::verify_enabled() {
                self.verify_checked += 1;
                let mut fresh_rules = Vec::new();
                let (fresh_style, _) = compute_style_shareable(
                    doc, node, sheet, inherited, viewport, dark_mode, None, &mut fresh_rules, false,
                );
                // Rule order is sheet order, identical for identical matches —
                // compare the vectors directly.
                if fresh_style != *hit.0 || fresh_rules[..] != hit.1[..] {
                    self.verify_mismatch += 1;
                    if self.verify_mismatch <= 20 {
                        eprintln!(
                            "[sharecache-verify] MISMATCH node={} style_eq={} rules_eq={}",
                            node.index(),
                            fresh_style == *hit.0,
                            fresh_rules[..] == hit.1[..]
                        );
                    }
                }
            }
            return (Arc::clone(&hit.0), Arc::clone(&hit.1));
        }
        // `probe == Some` means the node passed every `build_key` early-out,
        // so the key *would* have been `Some` — keeps the stats split honest
        // on the guaranteed-miss path where `build_key` never ran.
        let key_was_some = key.is_some() || probe.is_some();
        let mut matched_rules = Vec::new();
        let (style, shareable) =
            compute_style_shareable(doc, node, sheet, inherited, viewport, dark_mode, None, &mut matched_rules, false);
        let style = Arc::new(style);
        let matched_rules: Arc<[MatchedRule]> = Arc::from(matched_rules);
        // GUARANTEED-MISS probe path left `key` as `None`: build it now,
        // only when the fresh result is actually shareable — unshareable
        // nodes (the majority on real pages) never pay `build_key` at all.
        if shareable && key.is_none() {
            key = build_key(
                doc, node, sheet, inherited, viewport, dark_mode, track_position, track_fingerprint,
                relevant_attrs,
            );
        }
        if shareable && let Some(k) = key {
            self.key_hashes.insert(share_key_hash(&k));
            self.entries.insert(k, (Arc::clone(&style), Arc::clone(&matched_rules)));
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
        (style, matched_rules)
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
        if Self::verify_enabled() && self.verify_checked != 0 {
            eprintln!(
                "[sharecache-verify] checked={} mismatch={}",
                self.verify_checked, self.verify_mismatch
            );
        }
    }
}
