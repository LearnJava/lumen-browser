# BUG-983: tokenizer's RAWTEXT tag table is missing `<iframe>`, `<noembed>`,
`<noframes>`, `<xmp>`

**Статус:** OPEN
**Дата:** 2026-09-04
**Компонент:** engine (`crates/engine/html-parser/src/tokenizer.rs:561` —
`is_raw_text_element`)
**Найден:** P3, 2026-09-04, при разборе устаревшей ветки, закрывавшей BUG-413
(измерение застало этот баг как побочный шум); подтверждён повторным чтением
кода на актуальном `main`

## Механизм

```rust
// crates/engine/html-parser/src/tokenizer.rs:558-562
/// Элементы, чьё содержимое в HTML5 — RAWTEXT (литеральный текст до
/// `</tag` + терминатор; character references **не** декодируются).
fn is_raw_text_element(name: &str) -> bool {
    matches!(name, "script" | "style")
}
```

By HTML LS §13.2.6.4.7 (`<script>` **and** the surrounding table of
text-only elements), RAWTEXT also covers `<iframe>`, `<noembed>`,
`<noframes>`, `<xmp>` (and `<noscript>` when scripting is enabled — a
separate, scripting-flag-gated case not covered by this bug). The tokenizer
only special-cases `script`/`style`, so markup inside any of the four
missing tags is tokenized as regular HTML instead of being swallowed as
literal text, and the tree builder turns it into live DOM:

```js
d.innerHTML = '<iframe><div id="lost">abc</div></iframe>';
// spec: <iframe> content is one text node, invisible to the DOM
// actual: a real <div id="lost"> element, reachable via getElementById,
// participating in style/layout
```

The text-only mechanism itself (`is_raw_text_element`/`is_rcdata_element`
switch in the tokenizer) is already correct and exercised by the
`script`/`style` tests — this is purely a missing table entry, not a
missing feature.

## Цена

Direct: one WPT subtest (`getter.html`'s
`<iframe><div id='target'>abc` case, part of the BUG-413 measurement).
Indirect: any real page markup that puts HTML-like content inside an
`<iframe>` placeholder, `<noframes>` fallback, or `<xmp>` block gets a live,
styled, queryable subtree instead of inert text — a correctness gap wider
than the one WPT id suggests.
