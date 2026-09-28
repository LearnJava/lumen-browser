# BUG-1206 — при `require-trusted-types-for 'script'` не исполняется ни один `<script>`, вставленный через DOM

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_script_execute_classic`, `(0, eval)(text)`; `crates/js/src/v8_runtime/codegen_hook.rs` + `crates/js/src/trusted_types.rs` — `_lumen_tt_get_compliant_script_for_codegen`)
**Найден:** P6, живая проверка youtube к [BUG-1123](BUG-1123-FIXED.md), 2026-09-28

## Симптом

Во фрейме accounts.google.com (пассивный вход youtube) после загрузки скриптов boq-identity:

```
[JS error] Uncaught EvalError: Code generation from strings disallowed for this context
    at eval (<anonymous>)
    at _lumen_script_execute_classic (…:12929:20)
    at job.run (…:13086:21)
    at _lumen_script_exec_drain (…:12982:13)
```

Тело классического скрипта, вставленного через DOM (и внешнего, скачанного по `src`), шим исполняет
непрямым `eval`. Хук кодогенерации V8 (TRUSTEDTYPES-1, срез 7) проверяет каждый `eval` по Trusted Types
и при `require-trusted-types-for 'script'` без политики `default` блокирует его — вместе с собственным
исполнением скрипта движком. Страница с такой CSP (сервисы Google её ставят) теряет все динамически
вставленные скрипты.

## Репро

Юнит-проба в рантайме `install_dom` (как в `crates/js/src/dom/tests/v8_trusted_types.rs`):

```js
_lumen_tt_set_require_script(true);
var errs = [];
window.addEventListener('error', function(e) { errs.push(String(e.message || e.error)); });
var p = trustedTypes.createPolicy('p', { createScript: function(x) { return x; } });
var sc = document.createElement('script');
sc.text = p.createScript('window.__ran = 1');
document.body.appendChild(sc);
String(window.__ran) + '|' + errs.join(';');
```

**Lumen:** `undefined|Code generation from strings disallowed for this context`.
**Ожидается (Chrome):** `1|` — текст уже прошёл проверку TT на стоке `HTMLScriptElement text`, а
исполнение элемента `<script>` (HTML LS §8.1.3.5 «run a classic script») — не `eval` и хук
кодогенерации не проходит.

## Что сделать

Исполнять тело скрипта мимо проверки `eval`: нативная компиляция (`v8::Script::compile`, как у
`eval_and_report` в `crates/js/src/v8_runtime.rs`) вместо `(0, eval)`, либо флаг «идёт исполнение
элемента `<script>`», при котором `_lumen_tt_get_compliant_script_for_codegen` пропускает строку.
Проверить все пути, которые зовут `_lumen_script_execute_classic` (вставка через DOM, внешний `src`,
`document.write`).

## Регрессия подтверждена, репро с настоящими заголовками (P3, 2026-09-28)

Локальный сервер отдаёт страницу с двумя заголовками, как accounts.google.com:
`Content-Security-Policy: require-trusted-types-for 'script'` и
`Content-Security-Policy: script-src 'nonce-abc' 'unsafe-inline'`
(`.tmp/b1123/tt_server.py <port> <file>` и `csp_tt.html` в слоте `p6-work`). Страница создаёт
политику, присваивает `script.textContent = policy.createScript('console.log("INSERTED inline ran")')`
и вставляет скрипт с верным nonce. `lumen --dump-layout http://127.0.0.1:<port>/`:

| Бинарь | Результат |
|---|---|
| main 2026-09-24 (до TRUSTEDTYPES-1 срез 7, `c71846072`) | `INSERTED inline ran` |
| main 2026-09-28 (`2d6fbf4a0`) | `EvalError: Code generation from strings disallowed`, тело не исполнено |

Без `require-trusted-types-for` (одна `script-src` без `'unsafe-eval'`, заголовком или `<meta>`)
вставленный скрипт исполняется на обоих — дело в TT-ветке хука, а не в CSP `unsafe-eval`.
