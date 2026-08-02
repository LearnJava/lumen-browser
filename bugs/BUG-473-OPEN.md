# BUG-473: `CSSStyleDeclaration.cssText` doesn't collapse longhands into shorthand

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`, инлайновый `style`-литерал
`_lumen_make_style`/`_lumen_serialize_style`, ~`dom.rs:4257-4300`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Механизм

`_lumen_serialize_style(obj)` (`dom.rs:4257`) сериализует хранимые
объявления построчно (`Object.keys(obj).map(k => k+': '+obj[k]).join('; ')`)
— наивная конкатенация без CSSOM §6.7.2 (serialize a CSS declaration block)
алгоритма схлопывания лонгхендов в шорткод, без финального `;` после
последнего объявления, без учёта порядка групп логических свойств
(`padding-block-start` vs `padding-top`), без сохранения буквальной формы
значений с `var()`.

## Симптом

```
FAIL Shorthand serialization with just longhands.
  - assert_equals: expected "margin: 10px;" but got "margin-right: 10px; margin-left: 10px; margin-top: 10px; margin-bottom: 10px;"
FAIL inline style - text - delimiters - one declaration
  - assert_equals: expected "left: 10px;" but got "left: 10px"
FAIL Longhand with variable preserves original serialization but trims whitespace
  - assert_equals: expected "font-size: var(--a);" but got "font-size:var(--a);"
```

Три независимых, но родственных симптома одного и того же самодельного
сериализатора: (1) четыре лонгхенда `margin-*`/`border-*`/`padding-*` не
схлопываются в один шорткод, даже когда для этого достаточно данных; (2)
отсутствует завершающий `;` после последней декларации; (3) исходные пробелы
вокруг `:`/значения теряются вместо буквального сохранения записи (для
значений с `var()` спека требует trim, но не полную ре-сериализацию).

## Масштаб находки

32 файла массового прогона `css/cssom` (`cssstyledeclaration-*`,
`*-serialization.html`, `serialize-*.html`) падают на этом сериализаторе.
(Смежный, но не кластеризуемый по одному наблюдению файл того же семейства,
`cssstyledeclaration-csstext-setter.window.html`, гоняется под двумя
глобалами `.window.js`-конвенции и репортит **дублирующиеся** имена
сабтестов — `wptrunner` сам валит его гарнес `ERROR` на этой дубликации до
всякого сравнения с ожиданиями; оставлен без `.ini`, см.
`docs/wpt-status.md` → строка `css`.)

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для всех 32
атрибутированных файлов, `expected: FAIL`/`TIMEOUT` по фактическому
результату прогона.
