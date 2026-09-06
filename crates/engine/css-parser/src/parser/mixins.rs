//! CSS Functions and Mixins L1 — `@mixin`/`@apply`/`@contents`: types,
//! parsing, and the stylesheet-level pass that turns a nested style rule
//! inside a mixin's `@result` into standalone top-level `Rule`s once
//! combined with each `@apply` call site's own selector.
//!
//! Split out of `at_rules.rs` (BUG-518 срез 2) purely to keep that file
//! under the 2000-line cap — no behaviour change to anything moved here
//! verbatim; the nested-rule support itself is new.

use super::*;

/// Marker `Declaration::property` value the parser pushes into a rule's
/// (or `@result` block's) declaration list at the exact source position of
/// an `@apply <name>(<args>) [{ <block> }];` statement (CSS Mixins L1),
/// keeping `Rule`'s existing `Vec<Declaration>` shape rather than growing a
/// second, position-correlated list. `@` can never start a real CSS
/// property name, so this cannot collide with an author-declared one.
/// `Declaration::value` holds the exact raw source text following `@apply`
/// (name, optional `(args)`, optional `{block}`) — [`super::parse_apply_call`]
/// re-parses it back into an [`ApplyRule`] at cascade time (layout crate),
/// the same "raw text, re-parsed on demand" shape `--name(args)` calls
/// already use inside a property value.
pub const MIXIN_APPLY_MARKER: &str = "@apply";

/// `@mixin <dashed-ident>(<params>) { <mixin-body> }` — CSS Functions and
/// Mixins L1 §mixin-rule. Declares a reusable named set of declarations,
/// invoked from a style rule body (or another mixin's `@result` block) via
/// `@apply <name>(<args>)`.
///
/// A nested style rule inside `@result` (`&.foo { ... }`, as real mixins
/// use to re-target `@apply`'s expansion at a different selector) is
/// parsed into [`MixinResultItem::NestedRule`] but, unlike a plain
/// `Decl`/`Apply`/`Contents` item, is **not** expanded by the layout
/// crate's per-element `@apply` splice (`expand_mixin_apply`) — a nested
/// rule's selector is combined with the *calling* rule's own selector
/// (CSS Nesting's `&`-combination), which can target a different element
/// entirely (a descendant, a sibling), so it cannot become part of the
/// *same* element's flat declaration list the way every other `@result`
/// item can. Instead, [`expand_mixin_nested_rules`] runs once per
/// stylesheet, right after the whole thing is parsed, and materializes
/// each one into its own standalone top-level `Rule` (see that function's
/// doc comment for the current scope limits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixinRule {
    /// Dashed-ident name, e.g. `--centered`. Matched against `@apply <name>`.
    pub name: String,
    /// Positional parameters in declared order.
    pub parameters: Vec<MixinParameter>,
    /// Local custom-property declarations at the top level of the mixin
    /// body — both before and after `@result`; per CSS Mixins L1 all are
    /// visible when evaluating `@result` regardless of source position
    /// (confirmed against `mixin-locals.html`'s "Locals after `@result`
    /// are seen" case). A non-custom-property declaration at this level
    /// (e.g. a stray `font-size: 200px;`) belongs to no selector and is
    /// parsed but discarded, same as source authors are told to expect
    /// ("will be ignored" comment in the vendored `mixin-basic.html`).
    pub locals: Vec<Declaration>,
    /// Body of the mixin's own `@result { ... }` block, in source order.
    /// `None` if the mixin has no `@result` (a no-op mixin — `@apply`
    /// expands to zero declarations either way).
    pub result: Option<Vec<MixinResultItem>>,
}

/// One parameter of an `@mixin` rule: `--name [type(<syntax>)]? [: <default>]?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixinParameter {
    /// Dashed-ident parameter name, e.g. `--my-color`.
    pub name: String,
    /// Raw `type(<syntax>)` descriptor, if present. Stored but not
    /// validated — Phase 0, same deferral as `@function`'s `returns`
    /// (`var()` against a typed parameter does plain untyped substitution
    /// here, not the registered-custom-property-style numeric resolution
    /// the spec gives a `type()`-annotated one).
    pub type_syntax: Option<String>,
    /// Optional default value, substituted when `@apply` omits this
    /// positional argument. Evaluated against the **call site's** scope,
    /// not the mixin's own locals (CSS Mixins L1 — a default is written at
    /// the mixin's definition site but is a stand-in for a caller-supplied
    /// value, so it resolves like one).
    pub default: Option<String>,
}

/// One item inside an `@mixin`'s `@result { ... }` block, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixinResultItem {
    /// A plain declaration, substituted (`var()`/`--fn()`) against the
    /// mixin's local scope (bound parameters + its own `--x:` locals) at
    /// `@apply` time.
    Decl(Declaration),
    /// A nested `@apply` call (`apply-within-mixin.html`) — expands
    /// recursively against the same local scope as the enclosing `@result`.
    Apply(ApplyRule),
    /// `@contents [{ <fallback declarations> }];` — placeholder replaced at
    /// `@apply` time by the block the *caller's* `@apply ... { ... }`
    /// supplied, or by `fallback` (evaluated against the mixin's own local
    /// scope) when the caller gave no block.
    Contents {
        /// Flat declarations to use when the invoking `@apply` supplied no
        /// `{ ... }` block of its own. Empty if `@contents` had none.
        fallback: Vec<Declaration>,
    },
    /// A nested style rule (`&.foo { ... }`, `.foo { ... }` implicit
    /// descendant, `> .foo { ... }`, or bare `& { ... }`) found directly
    /// inside `@result` — same grammar CSS Nesting itself uses inside an
    /// ordinary style rule. `selectors` is stored **relative and
    /// unexpanded**: a `@mixin` block has no selector of its own to
    /// combine with yet (unlike CSS Nesting's `&`, which always has a
    /// concrete enclosing rule at parse time) — only the eventual `@apply`
    /// call site supplies that, so combination is deferred to
    /// [`expand_mixin_nested_rules`]. `selectors` empty means a bare
    /// `& { ... }` (same selector as the call site, no additional
    /// constraint). `body` recurses through this same grammar one level
    /// down (a nested rule may itself contain `@contents`/further nested
    /// rules — `contents-rule.html`'s `&.a { @contents {...} }`).
    NestedRule {
        /// `None` — compound join (`&.foo`, or a bare `& { }`, which has
        /// no `selectors` at all). `Some(c)` — relative combinator
        /// (`& span` = `Descendant`, `.foo` implicit = `Descendant`,
        /// `> .foo` = `Child`, etc.), mirroring
        /// [`super::expand_nesting`]'s own parameter.
        combinator: Option<Combinator>,
        /// The nested rule's own selector list, before combination with
        /// any call site. Empty for a bare `& { ... }`.
        selectors: Vec<ComplexSelector>,
        /// The nested rule's body, in source order.
        body: Vec<MixinResultItem>,
    },
}

/// `@apply <name>[(<args>)] [{ <block> }] [;]` — invokes a previously
/// defined `@mixin`, splicing its `@result` declarations (recursively
/// resolved against `args`) into the position `@apply` occupied. Appears
/// either directly in a style rule's declaration block, or inside another
/// mixin's own `@result` (nested mixin calls). `name` is not required to be
/// a dashed-ident at parse time — an `@apply` of a name no `@mixin` ever
/// registered under (dashed or not) simply resolves to nothing at cascade
/// time, the same "unknown call" outcome as a made-up dashed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyRule {
    /// Mixin name to look up (last same-name `@mixin` registration wins).
    pub name: String,
    /// Raw positional argument expressions (unexpanded — `var()`/`--fn()`
    /// substitution happens against the call site's scope at cascade time),
    /// in source order. A bare `@apply --name;` (no parens at all) and an
    /// explicit `@apply --name();` both parse to an empty `Vec` here.
    pub args: Vec<String>,
    /// `Some(decls)` when `@apply` supplies a `{ ... }` block (fills the
    /// invoked mixin's `@contents` placeholder, if any); `None` when no
    /// block was given at all (the mixin's own `@contents` fallback, if
    /// any, applies instead).
    pub block: Option<Vec<Declaration>>,
}

/// Splits `s` at its first top-level `:` (outside `(...)`/strings) into
/// `(before, Some(after))`, or `(s, None)` if there is none. Used for
/// `@mixin` parameters (`--name type(<syntax>): <default>`) where a plain
/// `str::split_once(':')` would wrongly match a `:` that could in principle
/// appear inside a parenthesized `type(...)` descriptor.
fn split_top_level_colon(s: &str) -> (&str, Option<&str>) {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut in_string: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = in_string {
            if b == q {
                in_string = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => in_string = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b':' if depth == 0 => return (&s[..i], Some(&s[i + 1..])),
            _ => {}
        }
    }
    (s, None)
}

/// Strips one layer of balanced `{...}` wrapping from an `@apply` argument
/// (CSS Mixins L1 allows `{ <value> }` around an argument to protect a
/// top-level comma it would otherwise contain — `@apply --m({green})`
/// resolves the argument to `green`). Only strips when the braces are
/// actually a matching outer pair (depth returns to exactly 0 at the last
/// character), not e.g. `{a},{b}` that happened to survive as one segment.
fn strip_brace_wrapping(s: &str) -> String {
    if !s.starts_with('{') || !s.ends_with('}') || s.len() < 2 {
        return s.to_string();
    }
    let inner = &s[1..s.len() - 1];
    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return s.to_string();
                }
            }
            _ => {}
        }
    }
    if depth == 0 { inner.trim().to_string() } else { s.to_string() }
}

impl<'a> Parser<'a> {
    /// Парсит `@mixin <name>(<params>) { <mixin-body> }` — CSS Functions and
    /// Mixins L1. Same prelude grammar as `@function` (dashed-ident
    /// immediately followed by `(`, no whitespace); `None` on a missing
    /// `(` or missing/malformed body, matching `@function`'s "whole rule is
    /// simply not registered" outcome for invalid syntax (confirmed against
    /// `mixin-basic.html`'s `invalid-name`/`--missing-argument-list` cases).
    pub(crate) fn parse_mixin_rule(&mut self) -> Option<MixinRule> {
        self.skip_ws_and_comments();
        let name = self.parse_ident()?;
        if !name.starts_with("--") || self.peek() != Some('(') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '('
        let params_str = self.read_balanced_parens()?;
        let parameters: Vec<MixinParameter> = split_top_level_commas(&params_str)
            .into_iter()
            .filter_map(|raw| self.parse_mixin_parameter(raw))
            .collect();

        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.skip_until_block_end();
            return None;
        }
        self.consume(); // '{'
        let (locals, result) = self.parse_mixin_body();
        Some(MixinRule { name, parameters, locals, result })
    }

    /// Parses one `@mixin` parameter: `--name`, `--name: <default>`,
    /// `--name type(<syntax>)`, or `--name type(<syntax>): <default>`.
    /// `raw` is one already-comma-split, not-yet-trimmed segment of the
    /// parameter list. `None` for a non-dashed-ident name (`self` here is
    /// only used to reuse no state — parameters are pure string parsing,
    /// kept as a method for symmetry with the rest of this grammar).
    fn parse_mixin_parameter(&self, raw: &str) -> Option<MixinParameter> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        // Split off an optional trailing `: <default>` first (top-level —
        // a default value itself may contain `:` only inside balanced
        // parens/strings, which `split_top_level_commas`-style scanning
        // would need; a plain `split_once` is safe here because the
        // `type(...)` descriptor that could otherwise contain `:` is
        // parenthesized, and `:` cannot appear elsewhere in `--name`/`type`).
        let (head, default) = split_top_level_colon(raw);
        let head = head.trim();
        let default = default.map(|d| d.trim().to_string());
        let mut parts = head.splitn(2, |c: char| c.is_ascii_whitespace());
        let name = parts.next().unwrap_or("").trim();
        if !name.starts_with("--") {
            return None;
        }
        let type_syntax = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .and_then(|s| {
                let inner = s.strip_prefix("type(")?.strip_suffix(')')?;
                Some(inner.trim().to_string())
            });
        Some(MixinParameter { name: name.to_string(), type_syntax, default })
    }

    /// Parses the inside of an `@mixin`'s body (cursor already past the
    /// opening `{`, consumes the matching `}`): collects `--x:` locals
    /// (both before and after `@result`) and the single `@result { ... }`
    /// block, if present. Non-custom-property declarations at this level,
    /// and any other `@`-rule or nested-selector token, are parsed/skipped
    /// but otherwise discarded — see [`MixinRule`]'s doc comment.
    fn parse_mixin_body(&mut self) -> (Vec<Declaration>, Option<Vec<MixinResultItem>>) {
        let mut locals = Vec::new();
        let mut result = None;
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                }
                Some('@') => {
                    let at_start = self.pos;
                    self.consume();
                    let ident = self.parse_ident().unwrap_or_default();
                    if ident.eq_ignore_ascii_case("result") {
                        self.skip_ws_and_comments();
                        if self.peek() == Some('{') {
                            self.consume();
                            result = Some(self.parse_mixin_result_body());
                        } else {
                            self.skip_until_block_end();
                        }
                    } else {
                        self.pos = at_start;
                        self.skip_at_rule();
                    }
                }
                _ => match self.parse_declaration() {
                    Some(d) => {
                        if d.property.starts_with("--") {
                            locals.push(d);
                        }
                    }
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        (locals, result)
    }

    /// Parses the inside of a mixin's `@result { ... }` block (cursor
    /// already past the opening `{`, consumes the matching `}`), or of a
    /// nested style rule one level inside it (same grammar — see
    /// [`MixinResultItem::NestedRule`]'s doc comment).
    fn parse_mixin_result_body(&mut self) -> Vec<MixinResultItem> {
        let mut items = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                None => break,
                Some('}') => {
                    self.consume();
                    break;
                }
                Some(';') => {
                    self.consume();
                }
                Some('&') | Some('.') | Some('#') | Some('[') | Some(':') | Some('*')
                | Some('>') | Some('+') | Some('~') => {
                    if let Some(item) = self.parse_mixin_nested_rule() {
                        items.push(item);
                    }
                }
                Some('@') => {
                    let at_start = self.pos;
                    self.consume();
                    let ident = self.parse_ident().unwrap_or_default();
                    if ident.eq_ignore_ascii_case("contents") {
                        items.push(MixinResultItem::Contents { fallback: self.parse_contents_fallback() });
                    } else if ident.eq_ignore_ascii_case("apply") {
                        if let Some(apply) = self.parse_apply_rule() {
                            items.push(MixinResultItem::Apply(apply));
                        }
                    } else {
                        self.pos = at_start;
                        self.skip_at_rule();
                    }
                }
                _ => match self.parse_declaration() {
                    Some(d) => items.push(MixinResultItem::Decl(d)),
                    None => self.recover_to_decl_boundary(),
                },
            }
        }
        items
    }

    /// Parses a nested style rule inside a mixin's `@result` block (cursor
    /// at the selector-start token, not yet consumed) — the same grammar
    /// CSS Nesting itself uses inside an ordinary style rule (explicit
    /// `&`, implicit descendant, explicit relative combinator; see
    /// `Parser::parse_declaration_block_with_nesting`), except the nested
    /// selector list is stored relative and unexpanded — a `@mixin` block
    /// has no selector of its own to combine with yet (see
    /// [`MixinResultItem::NestedRule`]'s doc comment). `None` only on a
    /// malformed selector list or missing `{`, mirroring the top-level
    /// nesting parsers' "recover to block end, contribute nothing"
    /// behaviour.
    fn parse_mixin_nested_rule(&mut self) -> Option<MixinResultItem> {
        let (combinator, selectors) = match self.peek() {
            Some('&') => {
                self.consume(); // '&'
                let had_ws = self.skip_ws_and_comments_track();
                let combinator = match self.peek() {
                    Some('>') => {
                        self.consume();
                        self.skip_ws_and_comments();
                        Some(Combinator::Child)
                    }
                    Some('+') => {
                        self.consume();
                        self.skip_ws_and_comments();
                        Some(Combinator::NextSibling)
                    }
                    Some('~') => {
                        self.consume();
                        self.skip_ws_and_comments();
                        Some(Combinator::LaterSibling)
                    }
                    Some('{') => None, // bare `& { }` — same as the call site.
                    _ if had_ws => Some(Combinator::Descendant),
                    _ => None, // `&.class` / `&[attr]` / `&#id` — compound join.
                };
                let selectors = if self.peek() == Some('{') {
                    Vec::new() // bare `& { }`.
                } else {
                    let s = self.parse_selector_list();
                    if s.is_empty() {
                        self.recover_to_block_end();
                        return None;
                    }
                    s
                };
                (combinator, selectors)
            }
            Some('>') | Some('+') | Some('~') => {
                // SAFETY: we just peeked this char, consume() cannot return None here.
                let c = self.consume().unwrap_or('>');
                let combinator = match c {
                    '+' => Combinator::NextSibling,
                    '~' => Combinator::LaterSibling,
                    _ => Combinator::Child, // '>'
                };
                self.skip_ws_and_comments();
                let selectors = self.parse_selector_list();
                if selectors.is_empty() {
                    self.recover_to_block_end();
                    return None;
                }
                (Some(combinator), selectors)
            }
            _ => {
                // Implicit descendant — `.foo`, `#id`, `[attr]`, `:pseudo`, `*`.
                let selectors = self.parse_selector_list();
                if selectors.is_empty() {
                    self.recover_to_block_end();
                    return None;
                }
                (Some(Combinator::Descendant), selectors)
            }
        };
        self.skip_ws_and_comments();
        if self.peek() != Some('{') {
            self.recover_to_block_end();
            return None;
        }
        self.consume(); // '{'
        let body = self.parse_mixin_result_body();
        Some(MixinResultItem::NestedRule { combinator, selectors, body })
    }

    /// Parses `@contents`'s optional `{ <fallback> }` (cursor right after
    /// the `contents` ident) and its optional trailing `;` — CSS Mixins L1
    /// allows both `@contents;` and a bare `@contents` immediately before
    /// the enclosing block's own closing `}` (no semicolon needed there,
    /// confirmed against `contents-rule.html`'s "Implicit semicolon"
    /// case). Returns the fallback declarations (empty if none given).
    fn parse_contents_fallback(&mut self) -> Vec<Declaration> {
        self.skip_ws_and_comments();
        let fallback = if self.peek() == Some('{') {
            self.consume();
            self.parse_declaration_block()
        } else {
            Vec::new()
        };
        self.skip_ws_and_comments();
        if self.peek() == Some(';') {
            self.consume();
        }
        fallback
    }

    /// Парсит `@apply <name>[(<args>)] [{ <block> }] [;]` (cursor right
    /// after the `apply` ident). `None` only when no ident follows `@apply`
    /// at all (fully empty/malformed prelude) — an unresolvable mixin name
    /// still parses fine and simply expands to nothing at cascade time
    /// (see [`ApplyRule`]'s doc comment).
    pub(crate) fn parse_apply_rule(&mut self) -> Option<ApplyRule> {
        self.skip_ws_and_comments();
        let Some(name) = self.parse_ident() else {
            self.recover_to_decl_boundary();
            return None;
        };
        self.skip_ws_and_comments();
        let args = if self.peek() == Some('(') {
            self.consume();
            let Some(raw) = self.read_balanced_parens() else {
                self.recover_to_decl_boundary();
                return None;
            };
            split_top_level_commas(&raw)
                .into_iter()
                .map(|a| strip_brace_wrapping(a.trim()))
                .filter(|a| !a.is_empty())
                .collect()
        } else {
            Vec::new()
        };
        self.skip_ws_and_comments();
        let block = if self.peek() == Some('{') {
            self.consume();
            Some(self.parse_declaration_block())
        } else {
            None
        };
        self.skip_ws_and_comments();
        if self.peek() == Some(';') {
            self.consume();
        }
        Some(ApplyRule { name, args, block })
    }
}

/// CSS Mixins L1: collects every nested style rule reachable from an
/// `@apply` call site's mixin (`MixinResultItem::NestedRule`, anywhere in
/// its own `@result`) as a standalone top-level `Rule`, combining each with
/// the calling rule's own selector list the same way CSS Nesting combines
/// `&` ([`super::expand_nesting`]) — see [`MixinRule`]'s doc comment for
/// why this can't share the flat, per-element `@apply` splice the layout
/// crate's cascade already does for a mixin's plain declarations.
///
/// Returns the extra rules rather than appending them itself: mutating
/// `sheet.rules` in place is the sanctioned job of [`super::parse`] alone
/// (`revision.rs`'s `every_stylesheet_mutation_in_the_workspace_
/// announces_itself` gate scans the whole workspace for exactly that, and
/// only exempts `parser.rs`) — the caller must still mint a fresh revision
/// or (as `parse` does) simply extend before the sheet's revision is ever
/// observed.
///
/// Called once, right after the whole stylesheet is parsed — a `@mixin`
/// may be defined after its first `@apply` (CSS Mixins L1 forward
/// references), so this cannot run incrementally while parsing does.
/// Scans only `sheet.rules` (the flat top-level list, which already holds
/// every CSS-Nesting-expanded rule too, each its own entry): an `@apply`
/// call site inside `@media`/`@supports`/`@layer`/`@scope`/a shadow-tree
/// sheet is out of scope for this slice — each of those keeps its own
/// separate `Vec<Rule>` (`sheet.media_rules`/`supports_rules`/`layers`/
/// `scope_rules`), unlike CSS Nesting's own expansion, which flattens
/// directly into whichever block it found itself in.
pub(super) fn collect_mixin_nested_rules(sheet: &Stylesheet) -> Vec<Rule> {
    let mut extra = Vec::new();
    if sheet.mixin_rules.is_empty() {
        return extra;
    }
    for rule in &sheet.rules {
        for decl in &rule.declarations {
            if decl.property != MIXIN_APPLY_MARKER {
                continue;
            }
            let Some(apply) = parse_apply_call(&decl.value) else { continue };
            let Some(mixin) = sheet.mixin_rules.iter().rev().find(|m| m.name == apply.name) else {
                continue;
            };
            let Some(result) = &mixin.result else { continue };
            for item in result {
                if let MixinResultItem::NestedRule { combinator, selectors, body } = item {
                    collect_nested_rule(*combinator, selectors, body, &apply, &rule.selectors, &mut extra);
                }
            }
        }
    }
    extra
}

/// One level of [`expand_mixin_nested_rules`]'s recursion: combines
/// `(combinator, selectors)` (one `NestedRule`'s own, unexpanded relative
/// selector) with `call_site_sels` — either the `@apply`'s enclosing rule
/// (top-level call) or an already-combined enclosing `NestedRule`
/// (nested-inside-nested) — and pushes the resulting standalone `Rule`
/// onto `out`, recursing first into `body` for any further `NestedRule`
/// found there.
///
/// `Decl`/`Contents` declarations are copied literally, unsubstituted:
/// `var()`/`--fn()`/mixin-parameter references inside them are left as-is
/// and resolved later by the ordinary per-element cascade against whatever
/// element the *combined* selector ends up matching — correct when they
/// only reference the matched element's own real custom properties (every
/// vendored test today), not when they were meant to resolve against the
/// `@apply` call site's own scope instead (would only differ when the call
/// site's selector and the combined selector can match different elements
/// — e.g. an implicit-descendant nested rule with a `var()`-fed `@contents`
/// block; no vendored test needs it, deferred). A nested `@apply` inside
/// `body` (`MixinResultItem::Apply`) is silently dropped — mirrors the
/// flat per-element path's own documented scope limit for this construct.
fn collect_nested_rule(
    combinator: Option<Combinator>,
    selectors: &[ComplexSelector],
    body: &[MixinResultItem],
    apply: &ApplyRule,
    call_site_sels: &[ComplexSelector],
    out: &mut Vec<Rule>,
) {
    let combined = if selectors.is_empty() {
        call_site_sels.to_vec()
    } else {
        expand_nesting(call_site_sels, combinator, selectors)
    };
    let mut declarations = Vec::new();
    for item in body {
        match item {
            MixinResultItem::Decl(d) => declarations.push(d.clone()),
            MixinResultItem::Apply(_) => {}
            MixinResultItem::Contents { fallback } => {
                let block = apply.block.as_deref().unwrap_or(fallback.as_slice());
                declarations.extend(block.iter().cloned());
            }
            MixinResultItem::NestedRule { combinator: c2, selectors: s2, body: b2 } => {
                collect_nested_rule(*c2, s2, b2, apply, &combined, out);
            }
        }
    }
    out.push(Rule { selectors: combined, declarations });
}
