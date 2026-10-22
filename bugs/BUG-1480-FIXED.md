# BUG-1480 — После `el.style.display = 'inline'` на `div` `'display' in getComputedStyle(el)` — `false` и значение пустое, пока не пройдёт кадр

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout/js (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`, снимок `getComputedStyle` после мутации `style.display`)

## Симптом

`const cs = getComputedStyle(el); 'display' in cs` — `false`, `cs.display` — пустая строка сразу после присваивания `el.style.display = 'inline'` блочному `<div>`; через кадр — `inline`. Не воспроизводится с `block`, `grid`, `table`, `inline-block` (значение сразу верное) и с первым `div` страницы; воспроизводится детерминированно со вторым и третьим `div`, которым в одном синхронном блоке выставлен `inline` (`a:true b:false c:false` на трёх запусках), и при смене `inline-block → inline`. WPT `computed-testcommon.js::test_computed_value` начинается с `assert_true(property in getComputedStyle(target))`, поэтому падает весь файл: `css-display/parsing/display-computed.html` — 105 из 112 сабтестов с «display doesn't seem to be supported in the computed style». Тот же приём в любом тесте на `getComputedStyle(...).display` после присваивания. Родственные: [BUG-1306](BUG-1306-OPEN.md) (устаревший снимок после мутации DOM), [BUG-472](BUG-472-OPEN.md) (`getComputedStyle` как HashMap-lookup).

## Проба

Проба (`--mcp`):

| шаги | у нас | ожидается |
|---|---|---|
| три `div`, каждому `style.display = 'inline'`, сразу `'display' in getComputedStyle(div)` | `true`, `false`, `false` | `true`, `true`, `true` |
| тот же `div` через кадр | `inline` | `inline` |
| `style.display = 'block'` → `'inline'` → `'inline'` (отдельные вызовы) | `true`, `true`, `false` | `true` во всех |
| `style.display = 'table'` | `table` сразу | `table` |

## Как найдено

WPT-RUN-14 срез 21: `css-display/parsing/display-computed.html` (105 из 112).

## Что делать

Найти, почему элемент без блочного бокса (inline) выпадает из карты `computed_style_to_map` до следующего прохода layout, и отдавать запись из пересчитанного стиля, а не из карты.

## Как проверить

Таблица выше; `css/css-display/parsing/display-computed.html`.

## Исправление

Корень — тот же, что у [BUG-1684](BUG-1684-OPEN.md): флаш на инкрементальном пути вытесняет записи стиля
перестроенных боксов (`prev_node_ids`) и пересобирает их обходом боксов; элемент, потерявший бокс в этом
рестайле (`div.style.display = 'inline'` без содержимого — пустой inline, BUG-1191), обходом не
достигается, и его записи нет до следующего полного сбора («через кадр»). Теперь после
`ScopedCollection::collect_computed_styles` флаш (`crates/js/src/v8_runtime/style_flush.rs`) возвращает
такие записи из каскада `CounterMap` через `lumen_layout::publish_boxless_computed_style`
(общий с полным сбором). Тесты: `crates/driver/tests/cases/bug1480_display_inline_computed.rs`.
