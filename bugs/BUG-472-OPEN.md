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

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для всех 37
атрибутированных файлов, `expected: FAIL`/`TIMEOUT` по фактическому
результату прогона.
