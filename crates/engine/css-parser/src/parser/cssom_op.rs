//! Журнал CSSOM-записей [`CssomOp`] для воспроизведения на каскадном листе.
//!
//! Вырезано из `parser.rs` (SPLIT-CP2) без изменения поведения.

/// One recorded CSSOM write against an owned sheet — CSSOM-8 вариант C.
///
/// The page cascade is an independent parse of every `<style>`/`<link>` body
/// concatenated together, so a write applied to one node's own
/// `Stylesheet` (which is what `document.styleSheets[i]` hands out) does not
/// reach it. Rather than serialising the mutated node back to CSS text and
/// re-parsing the page — which would need a byte-exact writer for every
/// at-rule CSSOM cannot represent, and would corrupt the whole page's styles
/// if that writer were ever wrong — each write is also recorded here and
/// **replayed** onto the freshly parsed cascade sheet on demand
/// ([`Stylesheet::replay_cssom_ops`]). A wrong address can then only misplace
/// the one edit it describes, and the page's own CSS is never rewritten.
///
/// Indices are in the owning node's own `cssRules` space; the base that maps
/// them into the cascade's space is resolved at replay time, so a recorded op
/// survives any number of cascade rebuilds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssomOp {
    /// `CSSStyleSheet.insertRule(text, index)`.
    InsertRule {
        /// Position in the owning node's own `cssRules`.
        index: usize,
        /// The rule text exactly as JS passed it.
        text: String,
    },
    /// `CSSStyleSheet.deleteRule(index)`.
    DeleteRule {
        /// Position in the owning node's own `cssRules`.
        index: usize,
    },
    /// A top-level `CSSStyleRule.style` write.
    SetRuleStyle {
        /// Position in the owning node's own `cssRules`.
        index: usize,
        /// The rule's whole new declaration list.
        css_text: String,
    },
    /// A `CSSStyleRule.style` write on a rule nested in a top-level `@media`.
    SetMediaChildStyle {
        /// The `@media` block's own position in the node's `cssRules`.
        media_index: usize,
        /// The rule's position inside that block (not rebased — a `@media`
        /// block's children are addressed relative to the block itself).
        child_index: usize,
        /// The rule's whole new declaration list.
        css_text: String,
    },
    /// A `.style` write on a node inside a top-level `@mixin`'s `@result`
    /// tree (CSSOM-8, вложенные правила).
    SetMixinResultStyle {
        /// The `@mixin`'s own position in the node's `cssRules`.
        mixin_index: usize,
        /// Path from `@result`'s own children down to the target node —
        /// not rebased, structural (see [`Stylesheet::set_mixin_result_style`]'s
        /// doc comment).
        path: Vec<usize>,
        /// The node's whole new declaration list.
        css_text: String,
    },
    /// `CSSGroupingRule.insertRule` of an `@apply` statement into a
    /// TOP-LEVEL style rule's own body.
    InsertRuleBodyApply {
        /// The owning style rule's position in the node's `cssRules`.
        rule_index: usize,
        /// Position in that rule's own `@apply`-marker sub-list — see
        /// [`Rule::insert_apply_marker`].
        index: usize,
        /// The rule text exactly as JS passed it.
        text: String,
    },
}
