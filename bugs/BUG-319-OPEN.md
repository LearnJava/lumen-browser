# BUG-319: BiDi `script.evaluate` игнорирует `awaitPromise`

**Renumbered 2026-07-18** from `BUG-317` — collided with `origin/main`'s own
`BUG-317` (the next-free slot per another parallel session's BUGS.md), resolved
while merging S6/S7 back into `main`.

**Статус:** OPEN
**Дата:** 2026-07-18
**Компонент:** bidi-server (`crates/bidi-server/src/protocol.rs`, `script_evaluate`)
**Найден:** P2-wpt S6, проверка awaitPromise (`tests/wpt/verify_s6_await_promise.py`)

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
