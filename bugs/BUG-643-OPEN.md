# BUG-643: `deviceorientation`/`devicemotion` synthetic event fires only for the first-ever listener, never for any other

**Статус:** OPEN
**Компонент:** js (`crates/js/src/device_sensors.rs`, `DEVICE_SENSORS_SHIM`)
**Найден:** P2, WPT-VENDOR-orientation-event, 2026-08-05

## Симптом

`orientation-event` (скоуп 🚫, вне скоупа — legacy DeviceOrientation
sensor) — вендорена и прогнана целиком (`run_report.py --all --root
orientation-event --recursive`, ~7 мин, 27 отобранных id из 33
вендоренных файлов, 10 `-manual` исключены раннером): **0/27 harness
OK, 0/0 сабтестов**. 26/27 — известный TLS-гэп `UnknownIssuer` (все
`.https.`); 1/27 (`idlharness.https.window.html`) — реконфирмация
[BUG-380](BUG-380-FIXED.md) (browsing context переиспользуется, отдаёт
результаты предыдущего теста). Прогон не даёт нового сигнала сам по
себе — находка получена живой пробой поверх реализации, по правилу
«категория 🚫 не значит находок нет» ([[reference_wpt_run_report_invocation_recipe]]).

`DeviceOrientationEvent`/`DeviceMotionEvent` реально реализованы как
Phase 0 заглушка (module doc-comment: «default values»). Живая проба
через `--mcp-live-port` (два слушателя `deviceorientation`,
зарегистрированные один за другим):

```js
window.__l1_fired = 0;
window.__l2_fired = 0;
window.addEventListener('deviceorientation', () => { window.__l1_fired++; });
window.addEventListener('deviceorientation', () => { window.__l2_fired++; });
```

Результат после ожидания: `l1_fired = 1`, `l2_fired = 0`. Третий
слушатель, зарегистрированный ещё позже (после того как синтетическое
событие уже отстрелялось для первого), тоже никогда не получает
событие: `l3_fired = 0`.

## Причина

`crates/js/src/device_sensors.rs:67`-`100`, monkey-patch
`window.addEventListener`:

```js
window.addEventListener = function(type, listener, options) {
  if (type === 'deviceorientation' && !firedOrientationEvent) {
    firedOrientationEvent = true;
    setTimeout(() => {
      if (deviceOrientationListeners.has(listener)) {
        const evt = new DeviceOrientationEvent('deviceorientation', {...});
        listener(evt);
      }
    }, 0);
    deviceOrientationListeners.add(listener);
  } else if (type === 'devicemotion' && !firedMotionEvent) {
    ...
  }
  return originalAddEventListener.call(this, type, listener, options);
};
```

Внешний `if`/`else if` гейтится глобальным флагом `firedOrientationEvent`
(соответственно `firedMotionEvent`), который выставляется в `true` при
самой первой регистрации слушателя данного типа за всё время жизни
страницы. Любой последующий вызов `addEventListener('deviceorientation',
...)` — второй, третий и далее, независимо от того, до или после
`setTimeout`-колбэка первого — падает в `else`-ветку общего условия и
просто делегируется в `originalAddEventListener`: слушатель
регистрируется как обычный DOM-листенер, но синтетическое событие
`deviceorientation`/`devicemotion` никогда не диспатчится через обычный
`dispatchEvent`-путь (движок не эмулирует непрерывный поток данных
датчика — только этот один хардкоженный колбэк-вызов). В результате
второй и все следующие слушатели молча висят вечно — не ошибка, не
таймаут на уровне JS, просто отсутствие события.

Одной строкой воспроизводится и вне WPT (два addEventListener подряд,
второй никогда не срабатывает — см. проба выше).

## Масштаб

Затрагивает любой код, который регистрирует более одного слушателя
`deviceorientation`/`devicemotion` (в WPT — явно проверяется
`motion/multiple-event-listeners.https.html`,
`orientation/multiple-event-listeners.https.html`,
`motion/add-during-dispatch.https.html`,
`orientation/add-listener-from-callback.https.html`), но эти файлы сами
не давали чистого сигнала в прогоне — все `.https.` и упирались в
TLS-гэп раньше, чем в этот баг; находка подтверждена только прямой
живой пробой поверх шима, не через сам WPT-прогон.

Смежная (не заводится отдельным багом, тот же модуль): и
`DeviceOrientationEvent.requestPermission()`, и
`DeviceMotionEvent.requestPermission()` безусловно резолвятся в
`'granted'` (`device_sensors.rs:30-33`, `:48-51`) без какой-либо модели
разрешений — тот же класс «нет валидации», что и [BUG-386](BUG-386-OPEN.md)
(`navigator.permissions.query()`), но другой, не связанный с ним
механизм (собственный статический метод класса, а не
`navigator.permissions`). Не выносится в отдельный баг: спека Device
Orientation допускает `requestPermission()` как no-op-«always granted»
на платформах без реального permission gate для этого API (в отличие
от `Permissions.query()`, которая обязана валидировать имя разрешения)
— задокументировано здесь на случай, если решится иначе.

## Дальше

Fix scope: заменить булев флаг «стрельнуло один раз глобально» на
диспатч синтетического события каждому новому слушателю независимо
(например — хранить единственный «текущий снапшот» данных датчика и на
каждой регистрации слушателя данного типа планировать `setTimeout` с
диспатчем именно этому listener, а не только самому первому). Вне
скоупа этой WPT-VENDOR-задачи (только вендоринг + прогон + находка);
категория `orientation-event` сама вне скоупа продукта (🚫, legacy
sensor), так что приоритет фикса — низкий, но баг реальный и
затрагивает Phase 0 заглушку, которую код может использовать как есть.
