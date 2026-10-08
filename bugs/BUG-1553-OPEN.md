# BUG-1553 — `MediaList`: `appendMedium`, `deleteMedium`, `mediaText =` не меняют список; `matchMedia("bogus").media` — `bogus`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `MediaList`, `matchMedia`)

## Симптом

`styleSheet.media` — `MediaList` с `item`/`appendMedium`/`deleteMedium`, но методы не меняют состояние: у `<style media="screen">` после `appendMedium("print")`, `mediaText = "all, tty"` и `deleteMedium("tty")` `length` остаётся 1, `mediaText` — `screen`; у `<style>` без `media` `appendMedium("print")` оставляет `length` 0 и `mediaText` `""`. 8 id (`MediaList.html`, `MediaList2.xhtml`, `medialist-*`).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<style media="screen">` — `m.appendMedium("print"); m.mediaText="all, tty"; m.deleteMedium("tty")`: `length` и `mediaText` | 1, `screen` на каждом шаге | 2/`screen, print`; 2/`all, tty`; 1/`all` |
| `<style>` без `media` — `appendMedium("print")` | `length` 0, `mediaText` `""` | 1, `print` |
| `Object.prototype.toString.call(m)` | `[object Object]` | `[object MediaList]` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/{MediaList,medialist-appendmedium-parse-single-and-dedup,medialist-deletemedium-parse-and-remove-all,medialist-interfaces-001,002,004,medialist-dynamic-001}.html`, `MediaList2.xhtml`.

## Что делать

Хранить список запросов в `MediaList` и пересчитывать `mediaText` (CSSOM §6.1 «append a medium», «delete a medium», разбор по CSS Media Queries 4, `toString` — `[object MediaList]`).

## Как проверить

`css/cssom/MediaList.html`, `medialist-appendmedium-parse-single-and-dedup.html`.
