# BUG-1546 — `<bdi>`, `<bdo>`, `<kbd>`, `<samp>`, `<var>`, `<dfn>`, `<time>`, `<data>`, `<tt>`, `<big>`, `<strike>`, `<font>`, `<nobr>`, `<output>`, `<acronym>` и пользовательские теги получают `display: block`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/style/ua.rs::default_display` — ветка `_ => Display::Block`)

## Симптом

Список inline-элементов в `default_display` (`a`, `span`, `b`, `i`, `em`, `strong`, `code`, `small`, `sub`, `sup`, `label`, `abbr`, `cite`, `q`, `mark`, `u`, `del`, `ins`, `s`, `ruby`…) неполон: всё остальное попадает в `_ => Display::Block`. Текст вокруг такого элемента рвётся на три строки: `<div>A<bdi>B</bdi>C</div>` — три блока. Затронуты также неизвестные и пользовательские теги (`<my-el>`, `<foo>`): HTML LS §15.3.1 и CSS Display 3 даёт им `display: inline`. 154 reftest `css-counter-styles` (эталон — `<div><bdi>i. </bdi>i</div>`) падают, в том числе по этой причине.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<div>A<bdi>B</bdi>C</div>` — `offsetHeight` | 58 (3 строки) | 19 |
| `getComputedStyle(bdi).display`, то же для `bdo`, `kbd`, `samp`, `var`, `dfn`, `time`, `data`, `tt`, `big`, `strike`, `font`, `nobr`, `output`, `acronym`, `wbr`, `<my-el>`, `<foo>` | `block` | `inline` |
| те же элементы: `a`, `abbr`, `b`, `cite`, `code`, `del`, `em`, `i`, `ins`, `label`, `mark`, `q`, `s`, `small`, `span`, `strong`, `sub`, `sup`, `u` | `inline` | `inline` |
| A/B одним бинарём: в эталоне `css3-counter-styles-019-ref.html` `<bdi>` → `<span>`; `reftest_pixdiff` (800×600, Ahem), различающихся пикселей | 4 555 | 0 |

## Как найдено

WPT-RUN-14 срез 25: `css/css-counter-styles/*/css3-counter-styles-NNN.html` (154 id с `<bdi>`/`<bdo>` в тесте или эталоне; кластеры `cs-bdi` и часть `cs-builtin-missing`). A/B одним бинарём: `<bdi>` → `<span>` в эталоне `lower-roman/css3-counter-styles-019` — различающихся пикселей 4 555 → 2 834; во всех 154 эталонах `<bdi>` → `<span style="display:inline-block;min-width:1.5em">` — все 154 остаются `thick`. Остаток — BUG-1548.

## Что делать

Добавить в `default_display` недостающие inline-теги (список HTML Rendering §15.3: `acronym`, `bdi`, `bdo`, `big`, `data`, `dfn`, `font`, `kbd`, `nobr`, `output`, `samp`, `strike`, `time`, `tt`, `var`, `wbr`) и считать `inline` любой неизвестный и автономный пользовательский элемент (HTML LS §15.3.1: `HTMLUnknownElement`/`HTMLElement` без UA-правила — `display: inline`). `<nobr>` — ещё `white-space: nowrap`.

## Как проверить

`<div>A<bdi>B</bdi>C</div>` — `offsetHeight` 19; `css/css-counter-styles/lower-roman/css3-counter-styles-019.html` после BUG-1548 — `identical`. Страница с неизвестным тегом `<foo>x</foo>` в потоке текста не рвёт строку.
