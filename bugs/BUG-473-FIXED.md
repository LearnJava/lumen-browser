# BUG-473: `CSSStyleDeclaration.cssText` doesn't collapse longhands into shorthand

**Статус:** FIXED 2026-09-01
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js`, инлайновый
`style`-литерал `_lumen_make_style`/`_lumen_serialize_style`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Механизм

`_lumen_serialize_style(obj)` сериализовала хранимые объявления построчно
(`Object.keys(obj).map(k => k+': '+obj[k]).join('; ')`) — наивная
конкатенация без CSSOM §6.7.2 (serialize a CSS declaration block) алгоритма
схлопывания лонгхендов в шорткод, без финального `;` после последнего
объявления. Геттер `cssText` до фикса вообще не проходил через сериализатор
— отдавал сырой текст атрибута `style=""` как есть, поэтому даже
буквальные пробелы вокруг `:`/значения из разметки утекали наружу
непричёсанными.

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

## Фикс

`_lumen_serialize_style` (`web_api_shim_mid.js`) переписана: перед
построением строки собирает для каждой из пяти схлопываемых TRBL-групп
(`margin`/`padding`/`border-width`/`border-style`/`border-color`) шортхенд
по CSS2.1 §8.3 (1/2/3/4-значное правило: `right≠left` → все 4, иначе
`top≠bottom` → 3, иначе `top≠right` → 2, иначе 1), но только когда
присутствуют ВСЕ четыре лонгхенда группы с равными буквальными значениями;
`!important`, шорткод `all` и парсинг значения самого шортхенда
(`style.margin = '1px 2px'`) — сознательно отдельный, более крупный
CSSOM-гэп, не тронут. Каждая эмитируемая декларация закрывается `;`, в т.ч.
последняя. Геттер `cssText` переведён на `_lumen_serialize_style(getParsed())`
вместо сырого текста атрибута — это заодно нормализует любой инлайновый
`style`, пришедший прямо из разметки и никогда не тронутый JS. Сеттер
`cssText` и `getPropertyValue` (резолвит шортхенд, если лонгхенды не
запрашивались напрямую) обновлены в паре с сериализатором. Значения с
`var()` отдельного кода не получили — парсер и так не трогает внутренние
пробелы значения, только обрезает внешние, что и требует спека для
unparsed-значений.

Регресс-тест `inline_style_serialization_collapses_shorthand_and_normalizes_text`
(`crates/shell/src/tests/page_pipeline.rs`) — строки утверждений взяты
дословно из трёх FAIL-примеров выше.

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` (32 атрибутированных
файла) **намеренно не тронуты** — категория `css/cssom` пересекается с
другими, ещё не закрытыми гэпами того же семейства (BUG-471/CSSOM-1:
`document.styleSheets` отсутствует, BUG-472/CSSOM-3: `getComputedStyle()`
покрывает не всё, `!important`/`all`/шортхенд-значения вне скоупа этого
фикса), так что часть из 32 файлов останется FAIL по несвязанным причинам.
Точное перераспределение PASS/FAIL по сабтестам требует свежего
`run_report.py` по всей категории — не сделано в этой сессии, оставлено
следующему WPT-прогону (P2).

## Гейты

`cargo check -p lumen-js --features v8-backend` ok; `cargo test -p
lumen-shell --features v8` — весь `tests::page_pipeline` 85/85 ok; `cargo
clippy -p lumen-shell --all-targets --features v8 -- -D warnings` чисто.
