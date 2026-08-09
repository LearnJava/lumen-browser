# BUG-714 — `DOMException` doesn't match the WebIDL "legacy exception" binding shape

**Статус:** OPEN
**Компонент:** js (`crates/js/src/v8_runtime.rs:120-163` — `DOM_EXCEPTION_POLYFILL`)
**Найден:** P2, WPT-VENDOR-webidl, 2026-08-09

## Симптом

Категория `webidl` (`tests/wpt/webidl/`, 45 файлов) — вендорена и прогнана
целиком (`run_report.py --all --root webidl --recursive`, ~1:43, 45 id,
ни одного `.https.`, ноль `testdriver`, ноль variant-fanout): **37/45
harness OK, 134/324 сабтестов**. Один из редких случаев в бэклоге, где
почти нет TLS/testdriver-гэпа и сигнал целиком в исполнившихся тестах.

Пять `es-exceptions/DOMException-*.any.html` файлов дали **25 unexpected**
из 30 их сабтестов (0/3, 39/46, 7/15, 0/1, 3/9) — доминирующий кластер
категории. Все пять бьют в один и тот же полифилл
`DOM_EXCEPTION_POLYFILL` (`v8_runtime.rs:120-163`), устанавливаемый один
раз глобально при старте изолята. Конкретные расхождения со спекой
(WebIDL §4.3 "DOMException"):

1. **`name`/`message`/`code` — собственные data-свойства инстанса, а не
   accessor-свойства прототипа.** Конструктор (`v8_runtime.rs:133-139`)
   пишет `this.message = ...; this.name = ...; this.code = ...;` прямо в
   `this`. Спека требует их как геттеры на `DOMException.prototype`
   поверх внутренних слотов — WPT проверяет это явно
   (`e.hasOwnProperty("name") === false`,
   `Object.getOwnPropertyDescriptor(DOMException.prototype, "name").get`
   существует). Следствие: `DOMException-constructor-behavior.any.js`
   "inherited-ness" (4 сабтеста) и весь `DOMException-custom-bindings.any.js`
   descriptor-блок (message/name/code property descriptor + brand-check на
   геттере, 6 сабтестов).

2. **Устаревшие коды в `LEGACY_CODES`** (`v8_runtime.rs:122-132`):
   `DOMStringSizeError: 2`, `NoDataAllowedError: 6`, `ValidationError: 16`.
   Апстримный webidl-тест явно требует `code === 0` для этих трёх имён —
   они были удалены из legacy error names table
   ([webidl#946](https://github.com/heycam/webidl/pull/946)), таблица
   `CONSTANTS`/`LEGACY_CODES` полифилла всё ещё содержит их с ненулевыми
   кодами. 3 прямых FAIL в `DOMException-constructor-behavior.any.js`
   (`"Should have matching legacy code from error names table expected 0
   but got 2/6/16"`).

3. **Можно вызвать без `new`.** `DOMException()` не бросает `TypeError` —
   обычная JS-функция, не class-конструктор с guard на `new.target`.
   1 FAIL (`DOMException-custom-bindings.any.js`, "Cannot construct without
   new").

4. **`globalThis.DOMException = DOMException` — перечислимое, `.prototype`
   — writable, `.prototype.constructor` — перечислимый.** Три прямых FAIL
   в `DOMException-constructor-and-prototype.any.js` (все три — "expected
   false got true" на enumerable/writable).

5. **Нет `Symbol.toStringTag`.** `Object.prototype.toString.call(new
   DOMException())` даёт `"[object Object]"` вместо `"[object
   DOMException]"` — 1 FAIL (`DOMException-custom-bindings.any.js`) плюс
   тот же паттерн у `Object.prototype.toString` брошен из `.toString()`
   применённого к `DOMException.prototype` напрямую — должен бросать
   `TypeError` (brand-check name/message геттеров), сейчас не бросает —
   ещё 1 FAIL.

6. **`Error.isError(new DOMException())` — `false`.** `DOMException-is-error.any.js`,
   0/1. `Error.call(this, message)` внутри обычного вызова функции (не
   `new Error(...)`) не проставляет движковый внутренний слот
   `[[ErrorData]]` на произвольном `this` — сам факт того, что прототип
   ведёт к `Error.prototype` (через `Object.create`), инстансу не хватает.
   В отличие от пунктов 1-5 это может быть архитектурным потолком
   чисто-JS полифилла без нативного V8-байндинга (настоящий `DOMException`
   в браузерах — нативный класс с реальным `[[ErrorData]]`); упомянуто как
   часть той же находки, чинить отдельно от 1-5, если вообще возможно
   в рамках текущей архитектуры (см. «Дальше»).

## Причина

Единственная точка установки — `DOM_EXCEPTION_POLYFILL`
(`v8_runtime.rs:120-163`), инсталлируется один раз при старте изолята
(`install_dom`), поэтому фикс в одном месте закрывает весь кластер
categorii (кроме пункта 6). Полифилл писался как минимальный
`function`+`this.foo=` конструктор без WebIDL-биндинг-слоя, который есть
у остального шима (`WEB_API_SHIM` в `dom.rs` для большинства других
интерфейсов использует `Object.defineProperty` геттеры — см. паттерн
`EventSource` из [BUG-363](BUG-363-FIXED.md)) — та же переделка (accessor
на прототипе поверх приватного поля, `new.target`-guard, `Symbol.toStringTag`,
`Object.defineProperty(globalThis, 'DOMException', {enumerable:false,...})`)
применима здесь один-в-один.

## Дальше

Fix scope: переписать `DOM_EXCEPTION_POLYFILL` — три приватных инстанс-поля
(`_name`/`_message`/`_code`, не enumerable, не через `this.foo=`) +
accessor-геттеры на `DOMException.prototype` с brand-check
(`if (!(this instanceof DOMException)) throw new TypeError(...)`),
убрать `DOMStringSizeError`/`NoDataAllowedError`/`ValidationError` из
`LEGACY_CODES`, `if (!new.target) throw new TypeError(...)` первой строкой
конструктора, `Object.defineProperty(DOMException.prototype,
Symbol.toStringTag, {value: 'DOMException', ...})`,
`Object.defineProperty(globalThis, 'DOMException', {value: DOMException,
enumerable: false, writable: true, configurable: true})`,
`Object.defineProperty(DOMException, 'prototype', {writable: false, ...})`.
Пункт 6 (`Error.isError`) — исследовать отдельно, возможен upstream-потолок
чисто-JS реализации.
