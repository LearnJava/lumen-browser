# BUG-1743 — `iframe.contentWindow.<глобал>` пересекает границу изолятов по значению: `navigator.serviceWorker`, промис `fetch()`, `performance` теряются

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-SW-SCOUT)
**Область:** js (`crates/js/src/frame_bridge.rs` — `wrapWinFacadeGlobals`; `crates/js/src/frame_bridge_globals.rs` — `_lumen_f_global_get`/`_lumen_f_global_call`; `crates/js/src/v8_runtime/eval.rs:587` — `peer_global_get`/`peer_global_call`)

## Симптом

Страница и её same-origin iframe:

```js
const w = frame.contentWindow;
w.navigator.serviceWorker;               // undefined — navigator пришёл копией без геттеров
typeof w.fetch("x").then;                // "undefined" — Promise ребёнка сплющен в {} ("[object Object]")
w.performance.getEntriesByType;          // undefined
new w.XMLHttpRequest();                  // конструктор родителя, а не ребёнка
```

Фасад окна фрейма (`winFacade`) — объект родителя. Запрос любого глобала не из его фиксированного набора (BUG-979) уходит в `peer_global_get`/`peer_global_call` изолята ребёнка. Результат возвращается конвертом `{"kind":"value","value":V}` с `JsValue` (`frame_peer_bridge.rs:44`). Объекты копируются как данные, без прототипов, геттеров и методов. Функции дают `{"kind":"function"}`, а результат вызова снова копируется по значению: Promise, Response, ServiceWorkerContainer и Performance превращаются в пустые объекты. Живых ссылок на объекты чужого изолята нет.

## Как найдено

WPT-SW-SCOUT, кластер J (`docs/wpt-vendor-notes/service-workers.md`): 64 id (54 OK с провалами, 7 TIMEOUT, 3 ERROR), 25 из 256 сабтестов, потеряно 58.7 балла (17.7 п.п.), второй по величине кластер `service-workers`. Почти каждый тест SW открывает `with_iframe(scope)` и проверяет контролируемый фрейм:

- `active.https.html`: `frame.contentWindow.navigator.serviceWorker.controller` → «Cannot read properties of undefined (reading 'controller')»;
- `fetch-event-respond-with-custom-response.https.html`: `iwin.fetch('?type=string')` → «response.text is not a function»;
- `navigation-timing*.https.html`, `resource-timing-cross-origin`: «contentWindow.performance.getEntriesByType is not a function»;
- `fetch-request-redirect`, `fetch-event-within-sw`: «.then is not a function».

Тот же класс, вероятно, роняет iframe-тесты и в других категориях. Например, `about-blank-replacement` падает на «reading 'body'».

## Что делать

Для объектов и функций из `peer_global_get`/`peer_global_call` возвращать удалённую ссылку, а не копию:

- конверт `{"kind":"object","id":N}` и прокси в родителе, у которого `get`/`set`/`apply`/`construct` проксируются в изолят ребёнка по `id`;
- Promise ребёнка завершает Promise родителя через тот же мост;
- хэндлы держатся в таблице изолята ребёнка и освобождаются при выгрузке документа фрейма.

Это ADR-масштаб: проверить [`docs/invariants.md`](../docs/invariants.md) и модель «фрейм = отдельный изолят» (`frame_bridge.rs`, //! шапка) до начала работы. Если живые ссылки отвергнуты, то минимум — `navigator` (с `serviceWorker`), `fetch`/`Response`, `performance` и `XMLHttpRequest` через специализированные фасады, по образцу `docFacade`.

## Как проверить

Проба выше в `*.https.html` рядом с тестом (`run_smoke.py`). `run_corpus.py --prefixes service-workers` — кластер J без сообщений «reading 'controller'» / «.then is not a function» / «response.text is not a function».
