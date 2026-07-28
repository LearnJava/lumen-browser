# BUG-319: BiDi `script.evaluate` игнорирует `awaitPromise`

**Renumbered 2026-07-18** from `BUG-317` — collided with `origin/main`'s own
`BUG-317` (the next-free slot per another parallel session's BUGS.md), resolved
while merging S6/S7 back into `main`.

**Статус:** FIXED 2026-07-20 (P3)
**Дата:** 2026-07-18
**Компонент:** bidi-server (`crates/bidi-server/src/protocol.rs`, `script_evaluate`)
**Найден:** P2-wpt S6, проверка awaitPromise (`tests/wpt/verify_s6_await_promise.py`)

## Исправление (2026-07-20)

`script_evaluate` при `awaitPromise:true` теперь идёт через новый
`eval_await_promise`, реализующий two-round-trip eval поверх синхронного
`AutomationCommand::Eval`:

1. **Раунд 1** — выражение оборачивается в
   `Promise.resolve((EXPR)).then(onFulfilled, onRejected)`, где хендлеры
   записывают `{state:"fulfilled",value}` / `{state:"rejected",error}` в
   глобаль `globalThis.__lumen_bidi_await` (синхронный throw в `EXPR` ловится
   `try/catch` → `rejected`). V8 авто-гоняет microtask-checkpoint в конце
   каждого eval (`v8_runtime.rs`: «V8 auto-runs microtasks after each
   script/task by default»), поэтому settle-хендлер срабатывает ещё до
   возврата раунда 1.
2. **Раунд 2** — читает `globalThis.__lumen_bidi_await` (и удаляет его),
   транслирует: `fulfilled` → `RemoteValue` через `remote_value_from_json`
   (`Promise.resolve(42)` → `{type:number,value:42}`; отсутствующий `value` =
   `undefined`-fulfillment → `{type:undefined}`); `rejected` → `javascript
   error` с сообщением; `pending` → объект промиса (`{type:string,value:"{}"}`,
   тот же fallback, что и non-await путь).

**Ограничение:** резолвятся только microtask-разрешимые промисы
(`Promise.resolve`, синхронные `async`-функции). Промис, ждущий макротаска/IO
(`setTimeout`, сеть), остаётся `pending` и возвращает объект промиса —
корректная поддержка потребовала бы прокачки полного event loop между
раундами.

Побочно: `eval_result_to_remote_value` разложен на `remote_value_from_json`
(маппинг уже-распарсенного JSON) + строковый фолбэк; добавлен
`undefined_remote_value()`.

4 юнит-теста (`script_evaluate_await_promise_*`) + `fake_live_session_await`
эмулирует two-round-trip в bidi-server-only фейке. `verify_s6_await_promise.py`
флипнут на `EXPECT_AWAIT_PROMISE_RESOLVES=True`.

## Симптом

`script.evaluate` не обрабатывает параметр `awaitPromise`: при `awaitPromise:
true` и выражении, вычисляющемся в `Promise`, сервер возвращает **сам объект
промиса** (сериализуется как `{"type":"string","value":"{}"}`), а не его
разрешённое значение.

Проверено живым probe против `lumen --bidi-port` (навигация на страницу со
скриптом, затем `script.evaluate`):

| выражение | awaitPromise | получено | ожидание BiDi |
|---|---|---|---|
| `1+1` | false | `{type:number, value:2}` | ✅ |
| `Promise.resolve(42)` | false | `{type:string, value:"{}"}` | RemoteValue `promise` |
| `Promise.resolve(42)` | **true** | `{type:string, value:"{}"}` | **`{type:number, value:42}`** |
| `(async () => 42)()` | true | `{type:string, value:"{}"}` | `{type:number, value:42}` |
| `Promise.reject(new Error(...))` | true | `{type:string, value:"{}"}` | `javascript error` |

## Влияние

Пайплайн WPT **не затронут**: `LumenTestharnessExecutor` намеренно использует
`awaitPromise=false` и опрашивает глобаль `window.__lumen_wpt_results` (async-тесты
завершаются через собственный event loop страницы + testharness completion, не
через awaitPromise). Баг относится только к прямому использованию BiDi
`awaitPromise` внешними клиентами.

## Ожидание

BiDi §10.2.4: при `awaitPromise:true`, если результат — промис, ждать его
разрешения и вернуть разрешённое значение (или `javascript error` при reject).
Требует прокачки microtask/event loop внутри eval-пути (`AutomationCommand::Eval`
синхронен и снимает результат до слива microtask-очереди), поэтому корректная
реализация — отдельный срез (нужна асинхронная команда либо two-round-trip
extract после слива microtasks).

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/verify_s6_await_promise.py
```
