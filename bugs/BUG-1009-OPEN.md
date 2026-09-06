# BUG-1009 — regular (non-`:host`/`::slotted`) selectors in a shadow tree's own stylesheet never match its own descendants

**Статус:** OPEN
**Заведён:** 2026-09-06 (P3, побочно при работе над [BUG-518](BUG-518-OPEN.md), срез `mixin-shadow-dom.html`)
**Компонент:** layout (`crates/engine/layout/src/style/cascade.rs::compute_style`, the `SHADOW_SHEETS`
`own_shadow`/`host_shadow` block; `crates/engine/layout/src/box_tree/entry.rs::build_shadow_sheets`)

## Механизм

Each shadow host's own author stylesheet is collected separately
(`build_shadow_sheets`, keyed by shadow-host `NodeId`) and installed into a
thread-local, `SHADOW_SHEETS`, once per layout pass. `compute_style` reads
that thread-local, but **only** for two specific cross-boundary cases (CSS
Scoping L1 §6.1-6.2):

- (a) `node` is itself a shadow host → its own shadow sheet's `:host`/`:host()`
  rules apply to it (`cascade.rs`, `own_shadow` block, gated on
  `complex_has_host(complex)`);
- (b) `node`'s DOM parent is a shadow host (a slotted light child) → that
  host's shadow sheet's `::slotted()` rules apply to it (`host_shadow` block).

There is no third case for **an element that lives inside a shadow tree**,
matched against **regular selectors** (`#id`, `.class`, bare type selectors —
anything without `:host`/`::slotted`) written in that same shadow tree's own
`<style>`. Such an element's DOM parent is the `ShadowRoot` node (or another
shadow-interior element), never the host itself, so it fails both `(a)` and
`(b)` and falls through to the ordinary `sheet` parameter — which is always
the single document-level `Stylesheet` built by the shell's
`extract_style_blocks` (`crates/shell/src/doc_extract.rs`), and which by
design never descends into a shadow root at all (`build_shadow_sheets`'s own
doc comment: "the two collections never overlap"). The declaration is parsed,
stored in `SHADOW_SHEETS[host]`, and then never looked up by anything.

## Симптом

Confirmed with a layout-crate unit test driving the real entry point
(`lumen_layout::box_tree::layout`) on a full declarative-shadow-DOM document:

```html
<div id="host">
  <template shadowrootmode="open">
    <style>#e1 { color: red; }</style>
    <div id="e1">x</div>
  </template>
</div>
```

The resulting box tree has **no node anywhere with `color: rgb(255, 0, 0)`** —
`#e1`'s box keeps the inherited default (black). The exact same rule written
with `:host` (`:host #e1 { color: red; }` from the *document's* `<style>`, or
`:host { color: red; }` targeting the host itself) already works — this is
specifically the shadow tree's own non-`:host`/`::slotted` rules against its
own descendants that never reach the cascade.

## Масштаб

This is the CSS half of Shadow DOM tree-scoped styling (CSS Scoping L1's
core §6 use case: "author a component's internal styles once, scoped to its
own tree"), independent of any particular WPT category — it affects every
shadow-root `<style>` block whose selectors aren't `:host`/`::slotted`, which
is the dominant case in real-world shadow DOM usage (most component styles
target the component's own internal markup, not the host or slotted
content). Found via [BUG-518](BUG-518-OPEN.md)'s `mixin-shadow-dom.html`
follow-up (`#e1`/`#e2`/`#e3`/`#e4` in that file all use plain `id` selectors,
not `:host`), but the gap is upstream of mixins entirely — a mixin can only
be as visible as the plain declaration sharing its rule, and here the whole
rule never matches. `tests/wpt/shadow-dom/`'s own vendored run
(`docs/wpt-vendor-notes/shadow-dom.md`, 2026-08-06) didn't surface this: that
category's failures are almost entirely DOM/JS API gaps (named access,
`Node.contains`, etc.), not `getComputedStyle()` assertions on shadow-interior
elements — the category doesn't happen to exercise this path.

## Почему это не point-fixed здесь

Not attempted as part of BUG-518 — this is a layout-crate cascade gap with no
connection to `@mixin`/`@apply`, and fixing it correctly needs its own design
pass: a shadow-interior node's rule-index lookup would need to switch from the
document `sheet` to its enclosing shadow root's own `Stylesheet` (found by
walking up through `ShadowRoot` nodes to the nearest shadow host, then
`SHADOW_SHEETS[host]`), while still letting `:host`/`::slotted` cross into the
*outer* scope as they already do — i.e. a node's applicable stylesheet is
tree-scoped, not a single global one, which the current `compute_style(doc,
node, sheet: &Stylesheet, ...)` signature does not model (`sheet` is one
value for the whole document, not looked up per-node). `ensure_cascade_index`/
`with_front_cascade_index`'s thread-local rule-index cache is also
pointer+length-keyed off a *single* sheet at a time, so per-shadow-tree
lookups would need either one cache entry per shadow host (plausible — there
are already per-host stylesheets in `SHADOW_SHEETS`) or a scope-aware cache
key. Left as a dedicated task; blocks `mixin-shadow-dom.html`'s 3 of 4
subtests that use plain selectors inside a shadow's own `<style>` (the
`:host`-adjacent case, if any existed, would already work).

## Воспроизведение

Layout-crate unit test (not committed as a permanent test here — this bug's
job is to track the gap, not carry its regression test; a fix should add one
under `crates/engine/layout/src/style/tests/shadow_dom_selectors.rs`):

```rust
let html = r#"<div id="host"><template shadowrootmode="open">
    <style>#e1 { color: red; }</style><div id="e1">x</div>
</template></div>"#;
let doc = lumen_html_parser::parse(html);
let sheet = lumen_css_parser::parse("");
let root = lumen_layout::box_tree::layout(&doc, &sheet, Size::new(800.0, 600.0));
// No box in `root` has color == Color { r: 255, g: 0, b: 0, a: 255 }.
```
