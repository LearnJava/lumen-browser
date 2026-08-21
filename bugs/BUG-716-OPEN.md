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

## Диагностическая ценность (P3, 2026-08-09, разбор BUG-703)

Даже без полного диспатча событий один только колбэк, печатающий отклонение
в stderr, оказался решающим инструментом: [BUG-703](BUG-703-FIXED.md)
(`tbank.ru` молча не рендерится) был неразличим по консоли — асинхронный
бутстрап глотал всё, — а временный колбэк за один прогон выдал и точку
падения, и стек. Рабочий минимум (проверен на `v8 = 150.1.0`):

```rust
extern "C" fn diag_promise_reject_callback(msg: v8::PromiseRejectMessage) {
    v8::callback_scope!(unsafe scope, &msg);
    let event = msg.get_event();
    let Some(value) = msg.get_value() else { return };
    v8::scope!(let scope, scope);
    let text = value.to_rust_string_lossy(scope);
    let stack = value.to_object(scope)
        .and_then(|o| { let k = v8::String::new(scope, "stack")?; o.get(scope, k.into()) })
        .filter(|v| !v.is_undefined())
        .map(|v| v.to_rust_string_lossy(scope))
        .unwrap_or_default();
    eprintln!("[unhandled-rejection] event={event:?} value={text}\n{stack}");
}
// в v8_thread_main, сразу после install_dynamic_import_hook:
isolate.set_promise_reject_callback(diag_promise_reject_callback);
```

Замечание к шагу 4 плана: reason'ы, не являющиеся `Error`, печатаются как
`[object Object]` — при реализации стоит сериализовать их (JSON) хотя бы для
консольного вывода. Стоит рассмотреть вывод необработанных отклонений в
`resource://console` как часть этого бага: сейчас страница может отказать
полностью, не оставив ни одной диагностической строки.

## Цена в WPT (P2, WPT-RUN-6 срез 10, 2026-08-21)

`testharness.js` докладывает провал внутри промис-цепочки **только** через это
событие (`addEventListener("unhandledrejection", …)` → `error_handler`,
`resources/testharness.js:5085`). Раз оно не диспатчится, обычный провал
ассерта в Lumen не даёт FAIL: гарнесс замолкает, лог браузера остаётся
чистым, и wptrunner убивает тест по таймауту. Вердикт меняется с FAIL на
TIMEOUT, а цена вердикта — 2.71 с настенного времени против 0.05 с у
разрешившегося теста (замер WPT-RUN-5 срез 15).

Измерено инструментированным прогоном (`tests/wpt/rejection_trace.py --on`,
JS-реализация трекинга из HTML LS §8.1.7.5 поверх нашего
`testharnessreport.js`; отклонение без обработчика печатает
`LUMEN_UNHANDLED_REJECTION: <reason>`, что классифицирует
`tests/wpt/timeout_audit.py`):

* `html/syntax/speculative-parsing` — **69 из 124 TIMEOUT (55.6 %)** оказались
  обычными провалами ассерта, дословно
  `assert_not_equals: speculative case did not fetch got disallowed value ""`.
  Корневая причина у них — [BUG-480](BUG-480-OPEN.md) (тест строит `<iframe>`,
  тот не грузится, сабресурс не запрашивается), но **вердикт** портит именно
  этот баг: с работающим `unhandledrejection` те же 69 id дали бы быстрый FAIL
  с внятным сообщением вместо девяти секунд молчания.
* `html/semantics/scripting-1` — 0 из 133: там цепочки не отклоняются, а
  никогда не разрешаются (ожидание `postMessage` из `<iframe>`). То есть
  механизм категорийный, а не универсальный; экстраполировать долю на весь
  корпус нельзя.

Побочно, тем же срезом подтверждён и умалчиваемый близнец из
[BUG-591](BUG-591-OPEN.md): пробой `--dump-layout` в три строки — верхнеуровневый
`throw` печатает `script error: …` в stderr, но `addEventListener('error', …)`
на window не срабатывает, поэтому и синхронный провал вне `test()` тоже
деградирует до TIMEOUT.
