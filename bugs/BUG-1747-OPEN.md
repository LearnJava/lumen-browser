# BUG-1747 — нет интерфейсных объектов `ServiceWorker`, `ServiceWorkerContainer`, `ServiceWorkerRegistration`, `NavigationPreloadManager`

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-SW-SCOUT)
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `_sw_container`, `_sw_make_registration`, объекты `ServiceWorker`; `crates/js/src/sw_worker.rs` — глобал SW)

## Симптом

```js
typeof ServiceWorker;              // "undefined"
typeof ServiceWorkerContainer;     // "undefined"
navigator.serviceWorker instanceof ServiceWorkerContainer; // ReferenceError
typeof NavigationPreloadManager;   // "undefined"
```

`navigator.serviceWorker`, регистрации и воркеры — простые объекты из фабрик шима (`_sw_make_event_target`, `_sw_make_registration`), без конструкторов на глобале и без общих прототипов. Тесты, сверяющие `instanceof` или прототип, падают с `ReferenceError` (`state.https.html`: «ServiceWorker is not defined», `registration-end-to-end`: «ServiceWorkerContainer is not defined», `navigation-preload/navigationPreload`: «NavigationPreloadManager is not defined»). `idlharness.https.any.html` — 118 из 410 сабтестов («self does not have own property "ServiceWorker"»). Нет `[LegacyUnforgeable]`-неизменяемого прототипа глобала SW (`immutable-prototype-serviceworker` — TIMEOUT), проверки `interface-requirements-sw` не доходят до конца.

## Как найдено

WPT-SW-SCOUT, кластер G (`docs/wpt-vendor-notes/service-workers.md`): 8 id, потеряно 7.1 балла (2.1 п.п. категории). Представители: `idlharness.https.any.html`, `service-worker/state.https.html`. `idlharness.https.any.worker.html`/`.sharedworker.html` сейчас падают раньше — на `self.caches` ([BUG-1746](BUG-1746-OPEN.md)).

## Что делать

1. Классы `ServiceWorker`, `ServiceWorkerContainer`, `ServiceWorkerRegistration`, `NavigationPreloadManager` на глобале окна (и воркеров по `[Exposed]`), наследники `EventTarget`; конструкторы бросают `TypeError` («Illegal constructor»).
2. Фабрики шима создают экземпляры через эти прототипы; атрибуты — геттеры на прототипе, а не собственные поля.
3. В глобале SW — `ServiceWorkerGlobalScope`, `ExtendableEvent`, `FetchEvent`, `Clients`, `Client`, `WindowClient` с той же формой; прототип глобала неизменяем.

## Как проверить

`run_corpus.py --prefixes service-workers/idlharness --processes 6`: `idlharness.https.any.html` заметно выше 118/410; `service-worker/state.https.html` без `ReferenceError`.
