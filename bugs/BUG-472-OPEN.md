# BUG-472: `getComputedStyle()` resolved-value coverage gaps

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/selector_query.rs::
computed_style_to_map`) + js (`crates/js/src/dom.rs`,
`_lumen_get_computed_style`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Механизм

`window.getComputedStyle(el).getPropertyValue(prop)` в шиме (`dom.rs:12772`)
делегирует нативному хуку `_lumen_get_computed_style(nid, prop)`
(`dom.rs:2525`), который — простой `HashMap` lookup без вычисления по
запросу: `computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>`,
заполняемый **один раз за проход layout** функцией
`collect_computed_styles`/`computed_style_to_map` (`selector_query.rs:590`,
ёмкость карты — жёстко заданные ~64 записи). Любое свойство/семантика вне
этого рукописного списка возвращает пустую строку — не ошибку, а тихий
`|| ''` фоллбэк (`dom.rs:12780`).

## Симптом

```
FAIL line-height: 1 - assert_equals: 1 should compute to 16px expected "16px" but got ""
FAIL Resolved value of min-width ... expected "0px" but got ""
FAIL The resolved value for 'background-color' is the used value
  - assert_regexp_match: expected object "/^rgb[a]?\(/" but got ""
FAIL Resolution of width is correct for ::before and ::after pseudo-elements
  - assert_equals: expected "50px" but got ""
```

Пустая строка вместо конкретного значения на широком спектре сценариев,
которых нет в рукописной карте `computed_style_to_map`: разрешённые
(«used») значения `top`/`right`/`bottom`/`left` для позиционированных
элементов относительно фактического layout, `min-width`/`min-height:auto` →
`0px`, computed style псевдоэлементов (`::before`/`::after`/`::picker`),
пользовательские свойства (`--x`) в computed style, `line-height` при
числовом значении, цвета, не сведённые к used-value `rgb()` (может
пересекаться с [BUG-465](BUG-465-OPEN.md), но там про сериализацию заданного
через `style.color =`, здесь — про сами эти свойства отсутствующие в карте
вовсе для отдельных сценариев типа `border-block-end-color`), computed style
у фрагментов, вставленных в IB-split.

## Масштаб находки

37 файлов массового прогона `css/cssom` (`getComputedStyle-*`/
`computed-style-*`) падают исключительно на пустых/отсутствующих значениях
из `getComputedStyle` — не на самой функции (она существует и работает для
свойств, которые в карте есть, см. существующие юнит-тесты
`get_computed_style_*` в `dom.rs`).

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02)** — 25 файлов / ~324
сабтеста: весь `background-attachment`/`-clip`/`-color`/`-image`/`-origin`/
`-position`/`-repeat`/`-size` computed-style кластер
(`parsing/background-*-computed.html`, `parsing/background-computed.html`,
numbered `background-33{1,2,3,5,6}.html`, `background-{size,clip,origin}-001.html`)
плюс новая грань того же гэпа: даже **физические longhand'ы**, давно
реализованные для рендеринга (`border-top-color`, `border-top-width`,
`border-top-style`, `border-top-left-radius` и парные), отсутствуют в карте
не только для составных значений border-shorthand'ов — `parsing/
border-{color,radius,style,width}-computed.html`, все 4 файла падают на
собственных физических longhand-именах, не только на shorthand'ах
(`border-color`/`border-width`/`border-style`/`border-radius`). Подтверждает
исходную гипотезу «рукописная карта ~64 записи» буквально — список
недостающих ключей шире, чем предполагалось на срезе 3.

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для всех 37
атрибутированных файлов, `expected: FAIL`/`TIMEOUT` по фактическому
результату прогона. Срез 9 добавил `.ini` под
`tests/wpt/metadata/css/css-backgrounds/` для ещё 25 файлов (некоторые
делят файл с BUG-463/BUG-495 — общий заголовок `.ini` ссылается на оба).

**WPT-RUN-3 срез 10 (`css/css-variables`, 2026-08-02)** — confirmed missing
keys (full list, checked against `computed_style_to_map` source directly,
`selector_query.rs:625-945`): `border-spacing`, `box-shadow`, `text-shadow`,
`perspective-origin`, `transition-duration`, and the entire
`background-attachment`/`-clip`/`-image`/`-origin`/`-position`/`-repeat`/
`-size` family (only `background-color` is present — `background` itself, as
well as `margin`/`padding`/`border`/`border-radius` shorthands, are likewise
absent; only their physical longhands are in the map). Most of these
co-occur with [BUG-493](BUG-493-OPEN.md) in the same files (a file testing
`border-spacing` via `var()` substitution fails at the BUG-493 layer before
this gap is even reachable — the whole per-node cache entry is empty, not
just this one key) — flagged here as the *compounding, still-present-after-
BUG-493-is-fixed* cause for: `variable-substitution-basic.html`
(`border-spacing`), `variable-substitution-background-properties.html` (7 of
its 8 properties besides `background-color`), `variable-substitution-shadow-properties.html`
(`box-shadow`/`text-shadow`), `variable-substitution-shorthands.html`
(`transition-duration`, 1 of its 51 subtests), `variable-reference-perspective-origin.html`
(`perspective-origin`), `missing-closing-nested-fallback.html` (`box-shadow`,
sole cause — this file doesn't hit BUG-493). `variable-presentation-attribute.html`
additionally exposes a **wider, SVG-specific gap**: none of ~40 SVG
presentation properties it tests (`stroke-width`, `fill`, `clip-rule`,
`dominant-baseline`, `alignment-baseline`, …) are in the map at all — same
mechanism, unmeasured beyond this one file.
