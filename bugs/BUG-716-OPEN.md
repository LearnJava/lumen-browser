# BUG-716: `unhandledrejection`/`rejectionhandled` никогда не диспатчатся

**Статус:** OPEN
**Компонент:** js (`crates/js/src/v8_runtime.rs` — уровень изолята; интерфейс события живёт в `crates/js/src/dom.rs`, `WEB_API_SHIM`)
**Найден:** P3, при закрытии [BUG-702](BUG-702-FIXED.md), 2026-08-09

## Симптом

Отклонённый промис, у которого нет обработчика, не порождает никакого
события: `window.onunhandledrejection` и слушатель
`addEventListener('unhandledrejection', …)` не вызываются никогда.
То же для `rejectionhandled` (обработчик, добавленный к уже отклонённому
промису). Страница молча теряет ошибку, а сайты, чья телеметрия/показ
ошибок построены на этом событии (типовой паттерн у Sentry и подобных),
не видят ни одной асинхронной ошибки.

```html
<script>
  window.onunhandledrejection = function (e) { console.log('caught', e.reason); };
  Promise.reject(new Error('boom'));   // никакого вывода
</script>
```

## Что уже есть

[BUG-702](BUG-702-FIXED.md) добавил в шим сам интерфейс:
конструктор `PromiseRejectionEvent` (наследник `Event`, поля
`promise`/`reason`), `window.PromiseRejectionEvent`, свойства
`window.onunhandledrejection`/`onrejectionhandled` (по умолчанию `null`).
Это сделано не ради галочки: core-js считает нативный `Promise`
недоверенным, если конструктора нет, и подменяет `globalThis.Promise`
своим полифиллом на каждом сайте, где он есть — что и вешало
`tbank.ru/auth/login/`. То есть **детекция теперь честна по факту
существования интерфейса, но не по факту доставки событий**.

## Что нужно сделать

Диспатч требует уровня изолята, до которого JS-шим не дотягивается:

1. `v8::Isolate::set_promise_reject_callback` при создании изолята
   (`crates/js/src/v8_runtime.rs:178`).
2. Ведение списка «about-to-be-notified rejected promises» по HTML LS
   §8.1.7.5: `kPromiseRejectWithNoHandler` — добавить,
   `kPromiseHandlerAddedAfterReject` — убрать (и, если промис уже был
   объявлен необработанным, запланировать `rejectionhandled`).
3. Оповещение в конце микрозадачного чекпоинта, а не немедленно —
   иначе будут ложные срабатывания на промисах, обработчик к которым
   добавляется в том же тике.
4. Мост в JS: собрать `PromiseRejectionEvent` и прогнать его через
   обычный путь доставки событий окна (включая `preventDefault()`,
   который подавляет вывод ошибки в консоль).

Слой регистрации нативных функций (`reg!` в `install_dom`) работает на
уровне `JsValue`-замыканий и до изолята/контекста не достаёт — сначала
понадобится способ вызвать JS из статического V8-колбэка.

Найден P3, 2026-08-09.
