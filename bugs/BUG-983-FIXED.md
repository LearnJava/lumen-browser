# BUG-983: tokenizer's RAWTEXT tag table is missing `<iframe>`, `<noembed>`,
`<noframes>`, `<xmp>`

**Статус:** FIXED 2026-09-15 (P1, ветка `p1-gap-xmldoc-srez24`)
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

## Исправление (2026-09-15)

Найден повторно, независимо, живым прогоном `tests/wpt/run_report.py --all
--root html/syntax/parsing --recursive` (213 файлов, 211/213 harness OK,
2339/7876 сабтестов) во время поиска следующего среза GAP-XMLDOC (BUG-786):
`html-integration-point.html` падал на "SVG desc should be an HTML
integration point"/"SVG title should be an HTML integration point" —
`<noembed>`/`<noframes>` внутри HTML integration point (`<svg><desc>`/
`<svg><title>`) декодировали entity вместо того, чтобы оставить их
литеральными. Репродукция через `lumen_html_parser::parse()` подтвердила
тот же дефект и для голого `<iframe>`/`<xmp>` вне foreign content — ровно
тот класс, что описан выше.

**Фикс — точечное расширение таблицы**, как и предполагал раздел
«Механизм»: `is_raw_text_element` (`tokenizer.rs`) получил `iframe`/
`noembed`/`noframes`/`xmp` рядом с уже бывшими там `script`/`style`.
`noscript` осознанно не тронут — отдельный, зависящий от scripting-флага
случай, не входящий в этот баг. Сам механизм text-only режима
(`is_raw_text_element`/`is_rcdata_element`, переключение в `consume_start_tag`)
не менялся — только таблица тегов, ровно как и предполагалось при заводе
бага.

Проверено: `tree_builder::current_context_forbids_text_only` отменяет
RAWTEXT/RCDATA только для элементов, реально созданных в foreign
(SVG/MathML) неймспейсе — у `iframe`/`noembed`/`noframes`/`xmp` неймспейс
всегда HTML (в том числе внутри HTML integration point, где
`start_tag_namespace` уже сбрасывает неймспейс на HTML), так что отмена
никогда не срабатывает для этой четвёрки и полноценный RAWTEXT остаётся в
силе везде, включая integration points.

Тесты: `tokenizer::tests::iframe_noembed_noframes_xmp_are_rawtext` (все
четыре тега по отдельности — стартовый тег, литеральный текст с
недекодированными entity, парный конечный тег), в `tree_builder.rs` —
`iframe_noembed_xmp_content_stays_inert_text` (буквальный пример из
раздела «Механизм»: `<iframe><div id="lost">abc</div></iframe>` больше не
даёт живой `<div>`) и `svg_desc_noembed_stays_rawtext` (прямая регрессия на
упавший WPT-сабтест). `cargo test -p lumen-html-parser --lib` — 492/492
зелёные (было 488). `cargo clippy -p lumen-html-parser --all-targets --
-D warnings` — чисто. `cargo test -p lumen-js --lib` (228/228) и
`cargo test -p lumen-shell` (1785/1785 + отдельные test-бинарники) без
изменений в счёте — обратные зависимости не задеты. `graphic_tests/
dump_golden.py` — 4/12 несовпадений, тот же базовый дрейф, что и на
`main` (не связан с этой правкой). `tree_builder.rs` пересёк собственный
baseline (5710 → 5803 строк из-за новых тестов), `scripts/
file-size-baseline.tsv` обновлён тем же коммитом только для этой строки.
Полный `scripts/scoped-test.sh` не досчитан до конца — завис на известном
[BUG-805](BUG-805-OPEN.md) (`lumen-network`, разделяемое состояние тестов);
`lumen-driver --test all` прогнан отдельно и падает только на уже
задокументированном чужом дрейфе CPU-эталонов
([BUG-1008](BUG-1008-OPEN.md), та же сигнатура из 7 файлов), не на этой
правке.

Отдельно найдено при этом же прогоне: GAP-XMLDOC (BUG-786) не дал нового
XML-специфичного кандидата на срез 24 — лёгкая жила корпусного grep
исчерпана (см. BUG-786 срез 23), а этот прогон вскрыл только общий,
не-XML-специфичный HTML-гэп (этот баг).
