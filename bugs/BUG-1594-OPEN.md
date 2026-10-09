# BUG-1594 — Media Queries: известная фича с недопустимым значением (`(prefers-color-scheme: 0)`) — `not all`, а не сохранённый запрос; `resolution` сериализуется в `dppx`; `--custom` и экранирование не сохраняются

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** css-parser/js (`parser/media.rs`; `matchMedia(q).media`, `MediaList` — `crates/js/src/shim/web_api_shim_mid.js:11866`)

## Симптом

Media Queries 4 §3.3: если запрос синтаксически допустим, но имя фичи или её значение неизвестно (`general-enclosed`), он остаётся запросом: `matches` ложно, но `media` и `mediaText` повторяют исходный текст. Мы заменяем его на `not all`, и тесты вида `query_is_js_parseable` (`match.media == query`) падают в каждом файле по фиче: `prefers-color-scheme`, `prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-data`, `prefers-reduced-transparency`, `forced-colors`, `inverted-colors`, `dynamic-range`, `display-mode`, `display-state`, `overflow-block/-inline`, `scripting`, `update`, `resizable`, `navigation-controls`.

## Проба

`--dump-layout` + `console.log(matchMedia(q).media)`:

| запрос | `media` у нас | ожидается |
|---|---|---|
| `(prefers-color-scheme: 0)`, `(prefers-color-scheme: 10px)`, `(prefers-color-scheme: dark 0)` | `not all` | как написано |
| `(prefers-contrast: increase)`, `(update: ?)`, `(scripting: invalid)`, `(display-mode: random)` | `not all` | как написано |
| `(--FOO: bar)` | `not all` | `(--FOO: bar)` |
| `(min-resolution: 1x)`, `(resolution: 2x)`, `(max-resolution: 7x)` | `(min-resolution: 1dppx)` и т. д. | `(min-resolution: 1x)` |
| `(resolution: 600dpi)` | `(resolution: 6.25dppx)` | `(resolution: 600dpi)` |
| `(prefers-contrast)`, `(prefers-color-scheme: dark)` (контроль) | как написано | как написано |
| `for (const q of mediaList)` для `sheet.cssRules[0].media` | `TypeError: queries is not iterable` | итерация (BUG-1553) |

## Как найдено

WPT-RUN-14 срез 27: `css/mediaqueries/{prefers-*,forced-colors,inverted-colors,dynamic-range,display-mode,display-state,overflow-media-features,scripting,update-media-feature,resizable.tentative,navigation-controls.tentative,match-media-parsing,mq-unknown-feature-custom-property,mq-escaped-serialization,mq-invalid-media-type-005,mq-invalid-media-type-layer-002}.html`.

## Что делать

Ввести в разбор ветку `general-enclosed`: неизвестное имя/значение даёт `MediaQuery::Unknown(text)` с исходным текстом, вычисляется как ложь; `resolution` хранит единицу записи, а не только `dppx`; `<dashed-ident>` как имя фичи; серилизация экранирует идентификаторы. Одной правки с BUG-1593 не требуется, но они пересекаются в `media.rs`.

## Как проверить

`css/mediaqueries/prefers-color-scheme.html`, `match-media-parsing.html`, `mq-unknown-feature-custom-property.html`.
