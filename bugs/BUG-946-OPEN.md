# BUG-946 — Trusted Types: политика существует, но ни один DOM sink её не спрашивает

**Статус:** OPEN
**Тип:** дефект реализованного кода — объектная модель заведена целиком (`trusted_types.rs`), но не подключена ни к одному потребителю.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 31)
**Область:** js (`crates/js/src/trusted_types.rs` — политика/фабрика; `crates/js/src/shim/web_api_shim_mid_b.js` — `_lumen_timer_string_handler`, единственный найденный sink-подобный путь)
**Владелец:** P3.

## Симптом

`window.trustedTypes.createPolicy(...)`, `.defaultPolicy`, `TrustedScript`/
`TrustedHTML`/`TrustedScriptURL` работают как самодостаточная фабрика —
`trusted_types.rs` реализует их полностью и корректно как объектная модель.
Но ни один DOM sink (innerHTML, `eval`, `setTimeout`/`setInterval` со
строковым обработчиком, `<script>` вставка и т.д.) её не читает: `grep -rn
trustedTypes\|defaultPolicy crates/js/src/shim/` — ноль совпадений в
шиме. `setTimeout(str, …)`/`setInterval(str, …)` со строковым обработчиком
исполняют её через `(0, eval)` безусловно (`_lumen_timer_string_handler`),
поэтому вызов `policy.createScript(...)` внешне «работает» случайно (его
`TrustedScript` стрингифицируется в исходный код), а подтесты именно
*default*-политики — когда голая строка ДОЛЖНА пройти через `defaultPolicy.
createScript` автоматически — никогда не вызывают политику вовсе, и их
колбэк не срабатывает.

Отличие от [BUG-811](BUG-811-OPEN.md) (CSP не enforced): здесь речь не о
CSP-директиве, а о том, что объектная модель Trusted Types сама по себе
никогда не консультируется, независимо от того, задан ли `require-trusted-
types-for` вообще.

## Прямое измерение

`grep -rn trustedTypes crates/js/src/shim/*.js` и
`grep -rn defaultPolicy crates/js/src/shim/*.js` — оба ноль совпадений.

## Кого это держит

`trusted-types/Window-setTimeout-setInterval.html` — тест устанавливает
`defaultPolicy` и ждёт, что голый строковый обработчик `setTimeout`
пройдёт через неё; вместо этого строка исполняется напрямую.

## Направление починки

`_lumen_timer_string_handler` — единственная точка, где строковый
обработчик превращается в исполняемый код: перед `(0, eval)` проверить
`window.trustedTypes.defaultPolicy` и, если задана, прогнать строку через
`defaultPolicy.createScript(str)` (per HTML LS «Rules for parsing a string
to a TrustedScript»), иначе — оставить прежнее поведение. Остальные sinks
(innerHTML, `<script src>`, `eval` напрямую) — отдельные точки того же
шаблона, грепать по каждой, не чинить один раз и считать закрытым (см.
гочу CLAUDE.md про пофичные шимы вне `WEB_API_SHIM*`).
