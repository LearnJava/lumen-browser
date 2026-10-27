# BUG-1758 — Отсоединённые `<audio src>` и `<img srcset>` не шлют ни `load`, ни `error`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-FETCH-SCOUT, кластер M в [`docs/wpt-vendor-notes/fetch.md`](../docs/wpt-vendor-notes/fetch.md))
**Область:** js (`crates/js/src/audio_element.rs::startLoad`, `crates/js/src/shim/*` — загрузка `<img>` по `srcset` для элемента вне документа)

## Симптом

Проба (временная `tests/wpt/fetch/zz-scout-probe.https.sub.html`, удалена; таймаут 3 с на случай):

```js
const a = document.createElement('audio');            // не вставлен в документ
a.onload = a.onerror = done;
a.setAttribute('src', '/common/blank.html');          // не медиа → ожидается error
// событие не приходит

const i = document.createElement('img');              // не вставлен
i.onload = i.onerror = done;
i.setAttribute('srcset', '/images/green.png');        // ожидается load
// событие не приходит; тот же img с src= — load приходит
```

Там же проверено, что `<embed>`, `<object>`, `<input type=image>`, `<track>` и `<img src>` (отсоединённый) событие шлют.

Генерированные тесты `fetch/metadata/generated/element-audio*.html`, `element-img*.html`, `element-picture*.html` ждут именно `onload`/`onerror` такого элемента: первый же сабтест висит до таймаута теста, остальные — NOTRUN (`element-img.https.sub.html`: 1/45, `element-audio.https.sub.html`: 0/23, `element-picture.https.sub.html`: 3/66).

## Причина

Не установлена. `<audio>`: выбор ресурса (HTML §4.8.11.5 «resource selection algorithm») запускается и для элемента вне документа — `src` при установке ставит задачу загрузки; похоже, `startLoad` привязан к вставке в дерево (ср. [BUG-955](BUG-955-OPEN.md) — ранний возврат на пустом `src`). `<img srcset>`: «update the image data» для отсоединённого элемента выполняется (HTML §4.8.4.3.4), но путь выбора кандидата `srcset`, вероятно, живёт в раскладке (нужен viewport/density) и для элемента без бокса не вызывается.

## Что делать

1. `<audio>`/`<video>`: установка `src` на элементе вне документа запускает выбор ресурса; не-медиа ответ → `error` (`MEDIA_ERR_SRC_NOT_SUPPORTED`).
2. `<img>`: «update the image data» по `srcset` без бокса — плотность 1x/ширина viewport по умолчанию, затем `load`/`error`.

## Как проверить

- `run_smoke.py /fetch/metadata/generated/element-img.https.sub.html /fetch/metadata/generated/element-audio.https.sub.html` — первый сабтест не висит (дальнейшие кросс-сайтовые сабтесты зависят от хоста `not-web-platform.test`, см. `WPT-HOST-RESOLVE`).
- Ожидаемый прирост `fetch` — единицы баллов сам по себе; основной эффект — снятие TIMEOUT-а в 6 id `metadata/generated`.
