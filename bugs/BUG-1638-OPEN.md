# BUG-1638 — ServiceWorker `statechange` приходит без `target`

**Статус:** OPEN
**Заведён:** 2026-10-18 (P6, при разборе [BUG-1239](BUG-1239-FIXED.md))
**Область:** js (`navigator.serviceWorker.register`, заглушка `ServiceWorker`)

## Симптом

youtube (`k6E` в `kevlar_base/main.js`) слушает `statechange` у `ServiceWorker` и читает `event.target.state`;
у нас событие приходит с `target === null`: `[JS error] Cannot read properties of null (reading 'state')`.
Единственная `[JS error]` на главной youtube после BUG-1239.

## Что сделать

Проверить заглушку `navigator.serviceWorker.register` / `ServiceWorker` в `web_api_shim_*`: `dispatchEvent`
должен выставлять `target` и `currentTarget` на сам `ServiceWorker`. Тест в `crates/js/src/dom/tests/`.
