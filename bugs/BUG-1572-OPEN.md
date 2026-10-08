# BUG-1572 — `content: "\2022"` — escape-последовательности в строках `content` не раскрываются: на странице печатается `\2022`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/style/parse/content.rs::parse_content_items` — строка берётся срезом `&s[start..i]` без декодирования)

## Симптом

CSS Syntax 3 §4.3.7: в строке `\` + 1–6 hex + необязательный пробел — кодпоинт; `\` + перевод строки — продолжение. `parse_content_items` копирует литерал как есть, поэтому `content: "\2022 x"` печатается `\2022 x`, `"\201C B \201D"` — буквально, `"\00a9"` — буквально, `\"` внутри строки обрывает её (`content: "a\"b"` → `a\`). Тот же разбор нужен `quotes` (там `parse_css_string_sequence` декодирует — тест `\201C` зелёный), `counter-style` `symbols` (декодирует, `counters.rs:2069`), `list-style-type: "…"`. Это самый распространённый приём иконочных шрифтов (`content: "\f101"`). Не регрессия: поведение одинаково у сборки от 17 июля. Скриншот `content:"\2022 A "` → `\2022 A x`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `#n::before{content:"\2022 x"}` — текст псевдоэлемента (`--dump-layout`) | `\2022 x` | `• x` |
| `content:"\201C B \201D"`, `content:"C \00a9"` | буквально | `“ B ”`, `C ©` |
| `content:'D \'q\''` | `D \w` (кавычка потеряна) | `D 'q'` |
| `quotes:"\201C" "\201D"` с `open-quote` | декодируется | декодируется |
| тот же файл через `<link rel=stylesheet>` | `\2022 X` | `• X` |

## Как найдено

WPT-RUN-14 срез 25: найдено при разборе `css-counter-styles/counter-style-at-rule/*` (12 id с `\2023` в `<style>`); не входит в BUG-1380 (там — значения и `url()`, не строки `content`).

## Что делать

В `parse_content_items` разбирать строку по CSS Syntax §4.3.7 (`\HHHHHH `, `\<любой символ>`, `\<перевод строки>`); общая функция с `parse_css_string_sequence` и `parse_css_string_from`.

## Как проверить

`<style>#n::before{content:"\2022 x"}</style><div id=n></div>` — в `--dump-layout` строка `• x`; `css/css-counter-styles/counter-style-at-rule/system-cyclic.html` после BUG-1549.
