# BUG-1824 — `PermissionStatus instanceof EventTarget` красный на чистом `main`: `scoped-test.sh` роняет `lumen-js` в каждом прогоне

**Статус:** OPEN
**Заведён:** 2026-11-18 (P6, гейтом `scripts/scoped-test.sh` при закрытии LOAD-8)
**Область:** js (`crates/js/src/permissions.rs` — `PERMISSIONS_SHIM`/`install_permissions_api_v8`; порядок установки — `crates/js/src/v8_runtime.rs:1207`; тест — `crates/js/src/dom/tests/v8_idle_message_clipboard.rs:440`)

## Симптом

`cargo test -p lumen-js --lib --features v8-backend -- v8_idle_message_clipboard` — 41 из 42
зелёные, красный один:

```
thread 'dom::tests::v8_idle_message_clipboard::navigator_permissions_status_is_a_real_event_target'
  panicked at crates\js\src\dom\tests\v8_idle_message_clipboard.rs:440:5:
assertion failed: bool_eval(&rt, "status instanceof EventTarget")
```

До следующих проверок теста (`status instanceof PermissionStatus`,
`typeof status.addEventListener`, `navigator.permissions instanceof Permissions`) дело не
доходит. Тот же файл рядом проверяет, что `navigator.permissions.query({name:'local-fonts'})`
резолвится в `state === 'denied'` — и та проверка зелёная, то есть `navigator.permissions`
есть и `query()` промис исполняется; расходится именно наследование `PermissionStatus` от
`EventTarget`.

## Воспроизведение

```
cargo test -p lumen-js --lib --features v8-backend -- navigator_permissions_status_is_a_real_event_target
```

Воспроизведено на **чистом `main`** `712b66462` (`git stash` всех правок LOAD-8 — результат
тот же: `0 passed; 1 failed`), поэтому к LOAD-8 отношения не имеет. В полном
`scripts/scoped-test.sh` это единственный красный тест целевого `-p lumen-js --lib`, кроме
флаков [BUG-1677](BUG-1677-OPEN.md); в том же прогоне `compat_hang::*` зелёный поодиночке.

## Что делать

1. Пробой в самом тесте: `typeof EventTarget`, `typeof navigator.permissions`,
   `status && status.constructor.name`, `Object.getPrototypeOf(status) ===
   PermissionStatus.prototype`, `status instanceof PermissionStatus`, а также сравнение
   `EventTarget` до и после `v8_runtime_with_dom(make_doc())`.
2. Две гипотезы, обе проверяемые этим пробоем:
   * `PERMISSIONS_SHIM` выходит на `if (typeof ET !== 'function') return;`
     (`permissions.rs:106`) — тогда `navigator.permissions` не установился бы вовсе, что
     противоречит зелёной проверке `local-fonts`, но пробой это подтвердит/снимет за минуту;
   * тестовый хелпер ставит `WEB_API_SHIM` повторно после `install_permissions_api_v8`,
     подменяя `globalThis.EventTarget` новым конструктором: `PermissionStatus` держит
     прототип **прежнего**, поэтому `instanceof` по глобальному ложно. Если так, дефект —
     в порядке установки/тестовом хелпере, а не в продукте: в живом пути
     `install_dom` права ставятся после шима (`v8_runtime.rs:1207`).
3. Если пробой покажет, что порядок установки действительно переставлен и в живом пути —
   `git bisect` между `e10f71ae43` (тест добавлен 2026-10-09, зелёный) и `712b66462`:
   первая точка, где тест покраснел, укажет мерж-виновника.

## Как проверить

Команда из «Воспроизведения» — зелёная; `scripts/scoped-test.sh` перестаёт держать
`error: test failed, to rerun pass -p lumen-js --lib` на 5096 зелёных тестах.

## Связи

- Флаки того же модуля и code cache — [BUG-1677](BUG-1677-OPEN.md) (там же описан
  параллельный прогон, но не этот тест: он падает и поодиночке).
- Модуль `PERMISSIONS_SHIM` родился из [BUG-624](BUG-624-FIXED.md)/[BUG-650](BUG-650-FIXED.md)
  (валидация имён вместо `_perm_denied`).
