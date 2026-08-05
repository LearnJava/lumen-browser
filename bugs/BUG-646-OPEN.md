# BUG-646: `PaymentRequest` constructor performs zero input validation (spec requires 4+ throw cases)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/payment_request.rs`, Payment Request API shim)
**Найден:** P2, WPT-VENDOR-payment-method-basic-card, 2026-08-05

## Симптом

`payment-method-basic-card` (скоуп 🚫, Payment Request API — нет платёжного
пайплайна) — вендорена и прогнана целиком (`run_report.py --all --root
payment-method-basic-card --recursive`, ~1 мин 21 с, 4 отобранных id из 6
вендоренных файлов — 2 `-manual` в имени файла раннер исключает): **0/4
harness OK**. Все четыре — TIMEOUT: два `.https.` файла (`historical.https.html`,
`payment-request-canmakepayment-method.https.html`) на уже задокументированном
TLS-гэпе `UnknownIssuer`; два оставшихся (`apply_the_modifiers.html`,
`steps_for_selecting_the_payment_handler.html`) не имеют `-manual` в имени
(значит `run_report.py` их не исключает и id засчитывает), но фактически
являются ручными тестами — `promise_test` вызовы навешаны только на
`onclick`-обработчики кнопок, ничего не запускается автоматически, поэтому
`testharness.js` никогда не завершается и wptrunner бьёт по внешнему таймауту
(тот же класс, что уже задокументированный `manual/`-каталог без суффикса в
имени файла).

Прогон сам по себе сигнала не дал (всё упирается в TLS-гэп/безавтозапускные
тесты раньше кода API). Живая проба через `--mcp-live-port` (ретрай-цикл на
`eval`, страница с `<script>window.__ready=1;</script>`) нашла реальный
дефект шима — первое вендорение любой категории семейства Payment Request
API (`payment-request`/`payment-method-id` пока не вендорены), и это первая
проверка `window.PaymentRequest` живым кодом.

## Причина

`crates/js/src/payment_request.rs:44-59` — конструктор `PaymentRequest`
проверяет только то, что `methodData`/`details` — объекты (`typeof ===
'object'`), и ничего больше:

```js
var PaymentRequest = function(methodData, details, options) {
    if (!methodData || typeof methodData !== 'object') {
      throw new TypeError('methodData is required');
    }
    if (!details || typeof details !== 'object') {
      throw new TypeError('details is required');
    }
    // Store minimal state (Phase 0: no actual processing)
    ...
};
```

Живая проба (4 независимых кейса, каждый — обязательный `throw` по спеке
[Payment Request API §3.1](https://www.w3.org/TR/payment-request/#constructor),
[§4.9 `checkAndCanonicalizeTotal`/`checkAndCanonicalizeAmount`](https://www.w3.org/TR/payment-request/#validity-checkers)):

```json
{
  "empty_methoddata": "NO_THROW (spec violation: must TypeError)",
  "negative_total": "NO_THROW (spec violation: must throw on negative total)",
  "invalid_amount": "NO_THROW (spec violation: must TypeError)",
  "invalid_currency": "NO_THROW (spec violation: must RangeError)",
  "done": true
}
```

Все четыре конструктора отработали без исключения:
- `new PaymentRequest([], {...})` — пустой `methodData` (это объект — массив
  проходит `typeof === 'object'`) должен бросать `TypeError` (§3.1 шаг 3:
  "If methodData is empty, then throw a TypeError."), не бросает.
- `total.amount.value: "-5.00"` (отрицательная сумма) должна бросать при
  канонизации total (§4.9), не бросает.
- `total.amount.value: "not-a-number"` (не decimal monetary value) должна
  бросать `TypeError`, не бросает.
- `total.amount.currency: "US"` (не well-formed ISO 4217, 2 буквы вместо 3)
  должна бросать `RangeError`, не бросает.

## Масштаб

Не WPT-раннер-сигнал (все 4 id TIMEOUT раньше исполнения) — находка живой
пробы поверх Phase 0 заглушки, задокументированной как "accepts but no
processing" в собственном doc-комментарии модуля (`payment_request.rs:1-9`).
Category-скоуп 🚫 (нет платёжного пайплайна, `.show()`/`canMakePayment()`
корректно не поддерживают реальные платежи) — конструктор всё равно
достижим и WebIDL-контракт нарушен независимо от отсутствия backend'а.

## Дальше

Fix scope: добавить валидацию в конструктор `PaymentRequest` (пустой
`methodData`, `checkAndCanonicalizeAmount`-подобная проверка `total.amount.value`
на decimal-формат и неотрицательность, `currency` на 3-буквенный ISO 4217
формат) — вне скоупа этой WPT-VENDOR-задачи (только вендоринг + прогон).
