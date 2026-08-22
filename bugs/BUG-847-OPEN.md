# BUG-847 — `setTimeout`/`setInterval` не приводят задержку к WebIDL `long`: таймер с задержкой больше 2^31-1 (например `Math.pow(2, 32)`) не срабатывает никогда вместо немедленного

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `timer-overflow-delay`)
**Область:** `crates/js/src/dom.rs:7345` (`setTimeout` — `var ms = (typeof delay === 'number' && delay > 0) ? delay : 0;`), `crates/js/src/dom.rs:7361` (`setInterval`, та же строка)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
setTimeout(() => console.log('fired'), Math.pow(2, 32));  // должен сработать сразу
setTimeout(() => console.log('control'), 100);            // срабатывает
```

Ни исключения, ни предупреждения: таймер просто поставлен на 49 суток вперёд.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py --variant timer-overflow-delay`
(2026-08-22, dev-release, Linux, коммит `bafa603d9`, `--seconds 6`, страница
жива — 11 тиков):

| ожидалось | получено |
|---|---|
| `timer-overflow-fired` + `interval-overflow-fired` | `timer-control-fired` — и всё |

## Причина (локализована чтением кода)

```js
// dom.rs:7345
var ms = (typeof delay === 'number' && delay > 0) ? delay : 0;
ms = _lumen_clamp_timeout(ms, nesting);
```

`_lumen_clamp_timeout` поднимает нижнюю границу (вложенные таймеры), верхней
границы нет вовсе. В HTML LS §8.6 аргумент объявлен как `long`, поэтому
`4294967296` при конверсии по WebIDL §3.2.4 («ToInt32», по модулю 2^32)
становится `0`, а `2147483648` — отрицательным и тоже сводится к `0`. То есть
спека требует немедленного срабатывания, а не отложенного на 2^32 мс.

## Масштаб

Маркер `timer-overflow-delay` в `tests/wpt/timeout_audit.py` — **2 id**
остатка снимка WPT-RUN-5: `html/webappapis/timers/type-long-settimeout.any.html`
и `type-long-setinterval.any.html`. Оба построены как
`setup({single_test: true}); setTimeout(done, Math.pow(2, 32));`, поэтому
TIMEOUT: `done` не зовёт никто.

## Направление починки (не предписание)

Перед клампом привести задержку к `long` по WebIDL (`delay | 0` даёт ровно
ToInt32) и уже отрицательное/нулевое значение сводить к `0`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   timer-overflow-delay` — ожидаются `timer-overflow-fired` и
   `interval-overflow-fired`.
2. WPT: `run_report.py --all --root html/webappapis/timers` — пара
   `type-long-*` должна перестать быть TIMEOUT.
