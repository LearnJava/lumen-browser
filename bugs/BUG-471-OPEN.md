# BUG-471: CSSOM stylesheet/rule object model not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Симптом

```
TypeError: Cannot read properties of undefined (reading 'cssRules')
    at ... document.styleSheets[0].cssRules
FAIL CSSStyleSheet is not defined
TypeError: Cannot read properties of undefined (reading 'appendChild')
    at ... document.getElementById("styleElement").sheet.insertRule(...)
```

`document.styleSheets` — не пустой массив, а `undefined`; `<style>`/`<link
rel=stylesheet>` не имеют свойства `.sheet`; глобальные конструкторы
`CSSStyleSheet`, `CSSRule`, `CSSStyleRule`, `CSSGroupingRule`,
`CSSMediaRule`, `CSSNamespaceRule`, `CSSKeyframesRule`, `CSSKeyframeRule`,
`CSSPageRule`, `CSSFontFaceRule`, `CSSFontFeatureValuesRule`, `MediaList`
отсутствуют вовсе. `grep -n "styleSheets\|CSSStyleSheet\|cssRules" crates/js/
src/dom.rs` даёт ноль совпадений — не сломанный геттер, вся ветка CSSOM
(`https://drafts.csswg.org/cssom/`) не подключена к шиму. `insertRule`/
`deleteRule`, конструируемые таблицы стилей (`new CSSStyleSheet()`),
`HTMLLinkElement.disabled`/alternate-переключение и порядок
`document.styleSheets` от того же корня зависят и падают тем же образом.

## Масштаб находки

Крупнейший единичный кластер массового прогона `css/cssom`: из 178 файлов с
непройденными сабтестами **97** используют `document.styleSheets`/`.sheet`/
`CSSStyleSheet`/`CSSRule`-иерархию/`MediaList`/`insertRule`/`deleteRule`
где-либо в теле теста (подтверждено статическим сканом исходников тестов, не
оценкой по частоте строк в логе) — полный список файлов см. в шапках
committed `.ini` под `tests/wpt/metadata/css/cssom/`.

## Влияние вне WPT

`document.styleSheets`/`CSSStyleSheet`/`CSSRule` — стандартный способ
программного чтения и модификации CSS со страницы (theming-движки,
CSS-in-JS библиотеки с constructable stylesheets, dev-инструменты). Полное
отсутствие этой ветки API — не единичный дефект, а целый неподключённый
раздел спеки.

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для всех 97
атрибутированных файлов, `expected: FAIL`/`TIMEOUT` по фактическому
результату прогона.
