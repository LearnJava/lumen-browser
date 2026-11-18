# BUG-1824 — `PermissionStatus instanceof EventTarget` красный на чистом `main`: тест держал статус в глобальной `status`

**Статус:** FIXED 2026-11-18
**Заведён:** 2026-11-18 (P6, гейтом `scripts/scoped-test.sh` при закрытии LOAD-8)
**Починен:** 2026-11-18 (P6)
**Область:** js — тест `crates/js/src/dom/tests/v8_idle_message_clipboard.rs`; продуктовый код (`crates/js/src/permissions.rs`) исправен и не менялся

## Симптом

`cargo test -p lumen-js --lib --features v8-backend -- navigator_permissions_status_is_a_real_event_target`
падал на чистом `main` `712b66462`:

```
thread 'dom::tests::v8_idle_message_clipboard::navigator_permissions_status_is_a_real_event_target'
  panicked at crates\js\src\dom\tests\v8_idle_message_clipboard.rs:440:5:
assertion failed: bool_eval(&rt, "status instanceof EventTarget")
```

`scripts/scoped-test.sh` из-за него держал `-p lumen-js --lib` красным на 5096 зелёных тестах
(единственный красный, кроме флаков BUG-1677).

## Причина

Тест складывал объект `PermissionStatus` в **глобальную** переменную:

```js
var status = null;
navigator.permissions.query({ name: 'camera' }).then(function(ps) { status = ps; });
```

В `WEB_API_SHIM` ([`shim/web_api_shim_tail_b.js:8721`](../../crates/js/src/shim/web_api_shim_tail_b.js))
`window.status` объявлен как WebIDL-атрибут типа DOMString с сеттером `statusText = String(v)`
(BUG-1801) — именно так он ведёт себя и в браузерах. `var` в eval на глобальном объекте нового
связывания не создаёт (свойство `status` уже есть как аксессор), поэтому присваивание уходило в
сеттер и объект превращался в строку `"[object PermissionStatus]"`.

Проба (временный тест с `println!` значений) подтвердила, что продукт исправен:

```
typeof status                  => "string"
String(status)                 => "[object PermissionStatus]"
typeof status.addEventListener => "undefined"
Object.getPrototypeOf(PermissionStatus.prototype) === EventTarget.prototype        => true
Object.getPrototypeOf(PermissionStatus) === EventTarget                            => true
PermissionStatus.prototype.addEventListener === EventTarget.prototype.addEventListener => true
```

Две гипотезы из первой редакции файла: `if (typeof ET !== 'function') return`
(`permissions.rs:106`) не срабатывает (`navigator.permissions` установлен), а второго
`WEB_API_SHIM` после `install_permissions_api_v8` никто не ставит — `EventTarget` в глобальном
объекте один (`value/writable/enumerable/configurable`), и `PermissionStatus` наследует именно его.

## Фикс

Переменная теста переименована в `ps`; продуктовый код не тронут. Поведение Lumen совпадает с
браузерным (`window.status = {}` и в Chrome даёт строку), дефект был только в тесте.

## Проверка

```
cargo test -p lumen-js --lib --features v8-backend -- navigator_permissions
```

10 из 10 зелёные, включая `navigator_permissions_status_is_a_real_event_target`
(`ps instanceof EventTarget`, `ps instanceof PermissionStatus`, `typeof ps.addEventListener`).

## Связи

- [BUG-1801](BUG-1801-FIXED.md) — `Window.status`, чей сеттер обнажил коллизию имён в тесте.
- [BUG-1677](BUG-1677-OPEN.md) — флаки того же модуля (параллельный прогон), к этому дефекту отношения не имеют.
