# BUG-608: `fetchPriority` IDL attribute missing on `<script>`/`<img>` (likely `<link>`/`<iframe>` too)

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — reflection tables for `HTMLImageElement`/`HTMLScriptElement`/`HTMLLinkElement`/`HTMLIFrameElement`)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL default fetchpriority attribute on <script> elements should be 'auto' - assert_equals: expected (string) "auto" but got (undefined) undefined
FAIL fetchPriority of new Image() is 'auto' - assert_equals: expected (string) "auto" but got (undefined) undefined
```
(`scripting/the-script-element/attr-script-fetchpriority.html`,
`embedded-content/the-img-element/attr-img-fetchpriority.html` — the
first subtest in each file is additionally masked by
[BUG-384](BUG-384-FIXED.md), named access on `window`, since it references
elements by bare `id`-derived identifiers; the second subtest constructs
the element directly and fails independently of that gap)

## Причина

The `fetchpriority` content attribute (Fetch Priority spec, referenced
from HTML LS on `<script>`/`<img>`/`<link>`/`<iframe>`) is a limited
enumerated reflection (`"high"`/`"low"`/invalid-or-missing → `"auto"`).
Lumen's element IDL reflection table (built for BUG-383) has no
`fetchPriority` entry for either interface, so the property is simply
absent — `script.fetchPriority`/`img.fetchPriority`/`new Image().fetchPriority`
are all `undefined` instead of reflecting the attribute (or defaulting to
`"auto"`).

## Масштаб

Confirmed on `<script>` and `<img>` (2 files, 2 subtests independent of
BUG-384). Not checked here, but likely the same gap on `<link>` and
`<iframe>`, which also carry `fetchpriority` per spec — not vendored/tested
in this slice, so unconfirmed.

## Срез P3 2026-09-16

Одна общая enum-таблица `_LUMEN_FETCH_PRIORITY` (`{ def: 'auto', keys:
['high', 'low', 'auto'] }`), заведённая рядом с `_LUMEN_REFERRER_POLICY` в
`crates/js/src/shim/web_api_shim_tail_b.js`, и одна строка
`['fetchPriority', 'fetchpriority', 'enum', _LUMEN_FETCH_PRIORITY]`,
добавленная в `_lumen_install_reflection`-таблицы всех четырёх интерфейсов,
которым HTML LS §2.5.3 даёт `fetchpriority`: `HTMLImageElement`,
`HTMLScriptElement`, `HTMLLinkElement`, `HTMLIFrameElement`. Предполагавшийся
в заявке остаток (`<link>`/`<iframe>` "not checked") закрыт в этом же
слайсе, а не оставлен как отдельная задача — проверено по спеке, не по
аналогии. Существующий `'enum'`-вид рефлексии уже даёт нужную семантику
(missing/invalid content attribute → `def`), новый код в
`_lumen_define_reflection` не потребовался.

Новые тесты `crates/js/src/dom/tests/v8_bug608_fetch_priority_reflection.rs`
(2/2 зелёных): дефолт `'auto'` и отказ невалидного значения на всех четырёх
тегах, плюс валидные `high`/`low` в обе стороны (getter отражает атрибут,
setter пишет атрибут обратно). `cargo test -p lumen-js --features
v8-backend` зелёный (3737/3737), `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` чист.
