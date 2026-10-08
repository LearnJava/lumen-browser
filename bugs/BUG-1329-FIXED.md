# BUG-1329 — `text-transform`: `innerText` при `capitalize` не совпадает с раскладкой; язык (`lang=tr|lt|…`) не учитывается

**Статус:** FIXED 2026-10-08 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 10, `css/css-text`, вторая половина)
**Область:** js/layout (`crates/js/src/shim/*` — `innerText`; `TextTransform::apply` в `crates/engine/layout/src/style/values/typography.rs` не получает язык)

## Симптом

1. Раскладка капитализирует верно (`--dump-layout`: `john's apple foo_bar` → `John's Apple Foo_bar`), а `element.innerText` — `John'S Apple Foo_Bar` (апостроф и подчёркивание сочтены границей слова). WPT `text-transform-capitalize-036.html`: 3 из 6 сабтестов.
2. `uppercase` не учитывает `lang`: `<div lang=tr>iı</div>` → `II` (ожидается `İI`); `lang=lt` — без поведения для `i̇`. `apply(&str)` языка не получает.
3. `text-transform-upperlower-107.html`: `Selection.toString()` для `ß` при `uppercase` даёт `ß`, ожидается `SS` (раскладка при этом рисует `SS`).

## Как найдено

WPT-RUN-14 срез 10: `text-transform-tailoring-*` (8 id, включая `-dynamic`), `text-transform-capitalize-*` (7 id; 6 reftest `thick` — причина не разобрана, пробой подтверждён только `innerText`), `text-transform-upperlower-*` (20 id; пробой подтверждён только `-107`, остальные не разобраны).

## Что делать

Передавать язык элемента в `TextTransform::apply` (tr/az `i`/`İ`, lt, ga); `innerText` и выделение приводить к той же функции, что раскладка.

## Как проверить

`css/css-text/text-transform/text-transform-tailoring-001.html`, `text-transform-capitalize-036.html`.

## Исправление

- `style/values/typography.rs`: `CaseLang` (`tr`/`az` → Turkic, `lt`, `ga`, `nl`; по первому подтегу, без учёта регистра) и `TextTransform::apply_lang` — турецкие `i`/`İ`/`ı`/`I` и `I`+U+0307, литовские точки над `i`/`j`/`į` (SpecialCasing), ирландские приставки `n`/`t` перед заглавной гласной, нидерландское `ij` → `IJ` при `capitalize`. `TextTransformExtra::apply(case, lang, s)`, `TextCssomExtra::transform_text` — единая точка для раскладки.
- `capitalize` режет слова по упрощённому UAX #29 (`is_word_start`), а не по пробелам: `john's`, `foo_bar`, `3.14` — одно слово, `foo-bar` → `Foo-Bar`, `(hello)` → `(Hello)`.
- `cascade.rs`: собственный `lang`/`xml:lang` элемента (в том числе пустой) кладётся в `text_extra.case_lang`, остальное наследуется; ключ share-cache содержит полный набор атрибутов, смена `lang` через `setAttribute` пересчитывает стиль.
- `innerText`: собственная копия `capitalize` в шиме удалена, текст идёт через нативный `_lumen_text_transform` → `style::transform_text_by_value` (те же функции, что у раскладки); язык — ближайший `lang`/`xml:lang` предка.
- `Selection.toString()` (`_lumen_selection_string`): если среди покрытых текстовых узлов есть с `text-transform`, фрагменты пересобираются в шиме и преобразуются (`ß` → `SS`); иначе ответ DOM-стороны без изменений. Срез по UTF-16 вместо байтов — только на этом пути; остальное — [BUG-1464](BUG-1464-OPEN.md).

Тесты: `style/tests/text_cssom_tests.rs::case_lang`, `tests/box_sizing_text_props.rs::text_transform_lang_*`, сквозные `crates/driver/tests/cases/bug1329_text_transform_lang.rs`. WPT: `text-transform-tailoring-001`, `-dynamic`, `-document-lang-dynamic`, `upperlower-039…044`, `-107`, `capitalize-036` (5 из 6 сабтестов).

Остаток (греческий, контекст между текстовыми узлами) — [BUG-1486](BUG-1486-OPEN.md).
