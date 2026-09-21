# BUG-1087 — интерфейсы Trusted Types не WebIDL-формы: нет глобала `TrustedTypePolicyFactory`, `window.trustedTypes` — обычный объект

**Статус:** OPEN
**Тип:** дефект реализованного кода — объектная модель Trusted Types собрана из литералов/`function`, а не из интерфейсов с прототипами.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 51, `trusted-types`)
**Область:** js — `crates/js/src/trusted_types.rs` (`TRUSTED_TYPES_SHIM`: `var factory = { createPolicy: … }`, строка ~106)
**Владелец:** P3.

## Симптом

Проба страницы (`--dump-layout`, 2026-09-22):

- `typeof self.trustedTypes` → `object`, но `typeof self.TrustedTypePolicyFactory` → `undefined`; `trustedTypes.constructor.name` → `Object`.
- `Object.getOwnPropertyNames(self)` содержит `TrustedHTML`, а `TrustedTypePolicyFactory` — нет.
- `Object.prototype.toString.call(trustedTypes)` → `[object Object]`, а не `[object TrustedTypePolicyFactory]`.

`trusted-types/idlharness.window.html`: **39/100** подтестов, 61 `FAIL`. Разбивка:

По типу проваленной проверки (61 `FAIL`):

| Проверка | Подтестов | Что означает |
|---|---|---|
| `assert_own_property` | 16 | нет глобала `TrustedTypePolicyFactory` |
| `assert_inherits` | 16 | методы `createPolicy`/`isHTML`/`isScript`/`isScriptURL`/`getAttributeType`/`getPropertyType` и атрибуты `emptyHTML`/`emptyScript`/`defaultPolicy` лежат на самом объекте, а не в цепочке прототипов; `name` не на прототипе `TrustedTypePolicy` |
| `assert_equals` | 11 | `length` интерфейсного объекта (2/3 вместо 0; 4 подтеста), пустое `.name` у операций (`toJSON`, `createHTML`, …), у `Window.trustedTypes` нет getter'а |
| `assert_false` | 8 | интерфейсный объект enumerable, `prototype` writable |
| `assert_class_string` | 5 | `Symbol.toStringTag` экземпляров/фабрики не совпадает |
| `assert_throws_js` | 4 | стрингификатор с `this = null`, вызов `createHTML` без аргументов не бросают `TypeError` |
| `assert_true` | 1 | — |

## Ожидание

Интерфейсы по WebIDL: `TrustedTypePolicyFactory`, `TrustedTypePolicy`, `TrustedHTML`, `TrustedScript`, `TrustedScriptURL` — не enumerable, с корректными `length`/`name`, неписаемым `prototype`, `Symbol.toStringTag`,
конструкторы бросают `TypeError` (`Illegal constructor`); `window.trustedTypes` — accessor на `Window.prototype`, возвращающий экземпляр `TrustedTypePolicyFactory`.

## Связанное

- [BUG-946](BUG-946-OPEN.md) — sink'и не читают политику.
- [BUG-1086](BUG-1086-OPEN.md) — в воркерах нет Trusted Types вообще.
- `docs/tasks/p2-test-track.md#test-3-срез-51-2026-09-22`.
