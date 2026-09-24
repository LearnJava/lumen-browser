# BUG-1081 — в воркерном `XMLHttpRequest` нет `overrideMimeType`: `TypeError: req.overrideMimeType is not a function`

**Статус:** FIXED 2026-09-24 (P1, WORKER-1 срез 5)
**Тип:** дефект — точечно недостающий метод в воркерной реализации XHR (`crates/js/src/worker.rs`, «minimal synchronous port over the same bridge», стр. ~1164); оконный XHR метод имеет (`crates/js/src/xhr.rs`).
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 50, `encoding`)
**Область:** js — воркерный рантайм (`crates/js/src/worker.rs`, `XMLHttpRequest` в воркерной области)
**Владелец:** P3.

## Симптом

Прогон `encoding` (2026-09-22): 26 сообщений `req.overrideMimeType is not a function` — 12 в `replacement-encodings.any.worker.html`, 14 в `unsupported-encodings.any.worker.html`;
их оконные варианты (`.any.html`) метод находят и идут дальше (`replacement-encodings.any.html` 6/12, `unsupported-encodings.any.html` 10/14 — остаток по кодировкам, см. ниже).

## Ожидание

`XMLHttpRequest.prototype.overrideMimeType(mime)` — метод интерфейса `XMLHttpRequest` (XHR Standard), доступен и в воркерах (`[Exposed=(Window,Worker)]`).

## Связанное

- [BUG-1080](BUG-1080-FIXED.md) — в тех же воркерах нет `TextDecoder`; эти два дефекта скрывают друг друга в тестах кодировок.
- `docs/tasks/p2-test-track.md#test-3-срез-50-2026-09-22`.

## Не проверялось

- Остальные члены воркерного XHR (`responseType`, `upload`, `getResponseHeader`…) — сверка с оконным набором не делалась.

## Исправление (WORKER-1 срез 5, 2026-09-24)

Воркерный `XMLHttpRequest` переписан в `crates/js/src/shim/worker_net_shim.js` (модуль
`crates/js/src/worker_net.rs`) поверх страничных `Headers`/`Response`/`Request`, которые
теперь едут в воркер дословным слайсом `FETCH_BODY_SHIM`. `overrideMimeType(mime)` есть,
с проверкой состояния (`InvalidStateError` в `LOADING`/`DONE`); декодирование ответа идёт
через тот же `TextDecoder`, что и у страницы (BUG-1080).

WPT: в `encoding/replacement-encodings.any.worker.html` и
`encoding/unsupported-encodings.any.worker.html` все подтесты проходят — секции воркера
удалены из `.ini` (у оконных вариантов остаток по кодировкам прежний).
