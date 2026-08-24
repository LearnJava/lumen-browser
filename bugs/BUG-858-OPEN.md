# BUG-858 — `navigator.sendBeacon`: относительный URL молча уходит в никуда, а `ArrayBuffer`/`TypedArray` отправляются пустым телом с чужим `Content-Type`

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `beacon`)
**Область:** `crates/js/src/dom.rs:7379`–`7397` (`navigator.sendBeacon` — цепочка `if` по типу тела: строка, `URLSearchParams`, `FormData`, `Blob`; ветки для `ArrayBuffer`/`ArrayBufferView` нет, поэтому `body` остаётся `''`), `crates/js/src/v8_runtime.rs:3449` (`_lumen_send_beacon` — URL уходит в `fetch_with_body_sync` как есть, без разрешения относительно документа)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.
**Родственный:** [BUG-780](BUG-780-FIXED.md) — та же форма у `XMLHttpRequest.open` (URL сохраняется дословно и умирает в сетевом слое как `invalid url: missing scheme`).

## Симптом

```js
navigator.sendBeacon('/collect', 'payload');            // true, но запроса нет
navigator.sendBeacon(abs, new Uint8Array([1,2,3]));     // true, тело пустое,
                                                        // Content-Type: text/plain;charset=UTF-8
```

Возвращаемое значение `true` означает по спеке «запрос поставлен в очередь», и
страница не может отличить его от реальной отправки.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant beacon`
(2026-08-23, dev-release, Linux, `main` = `530d0a444`, `--seconds 5`).
Свидетельство — запись запросов **на стороне сервера пробы**, не в браузере
(лог браузера здесь не доказательство, [BUG-826](BUG-826-FIXED.md)):

| вызов | `sendBeacon` вернул | сервер увидел |
|---|---|---|
| относительный `/vfma-beacon?rel-string` | `true` | **ничего** |
| абсолютный, тело — строка | `true` | `POST … content-type=text/plain;charset=UTF-8` ✔ |
| абсолютный, тело — `ArrayBuffer(8)` | `true` | `POST` с **пустым телом**, `content-type=text/plain;charset=UTF-8` (спека: Content-Type не ставится вовсе) |
| абсолютный, тело — `Uint8Array([1,2,3])` | `true` | то же |
| абсолютный, тело — `Blob(type=text/plain)` | `true` | `POST … content-type=text/plain` ✔ |
| абсолютный, тело — `FormData` | `true` | `content-type=application/x-www-form-urlencoded;charset=UTF-8` (спека: `multipart/form-data; boundary=…`) |
| контроль `fetch(POST)` на тот же абсолютный URL | — | `POST` дошёл |

Отдельно измерено и вынесено в [BUG-859](BUG-859-OPEN.md): ни один из этих
запросов (включая контрольный `fetch`) не несёт `Origin` и `Referer`.

## Масштаб

Механизм `beacon-request-gaps` в `tests/wpt/timeout_audit.py` — **4 id**
остатка снимка WPT-RUN-5: `beacon/headers/header-content-type-and-body.html`
(зависшие подтесты `Test content-type header for a body string`,
`… ArrayBufferView`, `… ArrayBuffer`, `… Blob`),
`beacon/headers/header-origin-same-origin.html`,
`header-referrer-no-referrer.html`, `header-referrer-same-origin.html`.
Все четыре бьют в один и тот же хелпер `beacon/resources/beacon.py`,
который отражает полученные заголовки обратно, — тест ждёт ответа про
запрос, которого либо нет, либо он не тот.

## Направление починки (не предписание)

1. Разрешать URL относительно `document.baseURI` до передачи в
   `_lumen_send_beacon` (та же поправка закроет и `XHR`-половину BUG-780,
   если сделать общий хелпер).
2. Добавить ветки `ArrayBuffer`/`ArrayBufferView` (тело — байты, `Content-Type`
   не ставится) и `multipart/form-data` для `FormData`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant beacon` — сервер должен увидеть и `rel-string`, и непустое тело
   у `abs-arraybuffer`/`abs-view`.
2. WPT: `run_report.py --all --root beacon --recursive`.
