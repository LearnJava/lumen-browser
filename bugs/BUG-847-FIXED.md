# BUG-847 — `setTimeout`/`setInterval` не приводят задержку к WebIDL `long`: таймер с задержкой больше 2^31-1 (например `Math.pow(2, 32)`) не срабатывает никогда вместо немедленного

**Статус:** FIXED 2026-08-25
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `timer-overflow-delay`)
**Область:** `crates/js/src/dom.rs:7345` (`setTimeout` — `var ms = (typeof delay === 'number' && delay > 0) ? delay : 0;`), `crates/js/src/dom.rs:7361` (`setInterval`, та же строка)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.
**Починил:** P1, 2026-08-25 (ветка `p1-bug847-timer-long-delay`)

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

## Починка (P1, 2026-08-25)

Одно выражение `(typeof delay === 'number' && delay > 0) ? delay : 0`
подменено конверсией, которую §8.6 объявляет типом аргумента, — WebIDL `long`,
то есть ToNumber, затем ToInt32, и **только потом** шаг 5 «если timeout меньше
нуля, взять ноль»:

```js
function _lumen_timer_delay(v) {
    var n = Number(v) | 0;
    return n < 0 ? 0 : n;
}
```

Функция дословно повторяет `_toDelay` из `WORKER_TIMERS_SHIM`
(`crates/js/src/worker.rs`, BUG-815): §8.6 — это `WindowOrWorkerGlobalScope`,
поэтому страница и воркер обязаны отвечать одинаково, и расхождение этих двух
шимов уже стоило одного бага ([BUG-831](BUG-831-FIXED.md)).

### Заявка называла один дефект, их было четыре — и все в одном выражении

Замер до правки (одноразовый юнит-тест, печатающий фактический срок через
`panic!`; в дереве не оставлен) показал, что старое выражение не реализует
**ни одной** из двух половин конверсии, и мимо цели промахивались четыре
разных входа:

| вход | было | стало | почему |
|---|---|---|---|
| `Math.pow(2, 32)` | 4294967296 мс (49 суток) | 0 | ToInt32 по модулю 2^32 — заявленный дефект |
| `Math.pow(2, 31)` | 2147483648 мс (24.8 суток) | 0 | тот же модуль с отрицательной стороны, дальше шаг 5 |
| `Infinity` | `deadline === Infinity` | 0 | срок, до которого нельзя дожить: ToInt32 нечисла — ноль |
| `'100'`, `{valueOf: … 250}` | 0, то есть «на следующем тике» | 100 / 250 | конверсия начинается с ToNumber, а `typeof delay === 'number'` отбрасывала всё, что ещё не число |
| `1.9` | 1.9 | 1 | ToInt32 усекает |

Четвёртая строка — самая заметная на живых страницах: `setTimeout(fn, '100')`
раньше выполнялся немедленно, а не через 100 мс, и отличить это от «таймер
сработал вовремя» страница не могла.

### Пятый дефект — тот же самый, но у соседней функции

`clearTimeout`/`clearInterval` сравнивали `_lumen_timers[i].id === id` с
**сырым** аргументом, хотя дескриптор в IDL — такой же `long`. Поэтому
`clearTimeout(String(id))` не отменял ничего и колбэк всё равно срабатывал.
Добавлен `_lumen_timer_handle` (та же конверсия без шага 5 — отрицательный
дескриптор просто ни с чем не совпадёт); ровно этот приём применяет
`cancelAnimationFrame` двумя десятками строк ниже, но до `clearTimeout` он не
дошёл.

Та же дыра нашлась в воркерном шиме: `_toDelay` там был, а дескриптор
сравнивался сырым — исправлено в `WORKER_TIMERS_SHIM` (это же покрывает и
`SharedWorkerGlobalScope`, который вычисляет тот же шим). Синхронную заглушку
скоупа сервис-воркера (`sw_worker.rs`) правка не трогает: там таймер
выполняется немедленно по построению.

### Замер после

`verify_perf_idb_sse_gaps.py --variant timer-overflow-delay` (Windows,
dev-release, `--seconds 6`, страница жива — 3 тика): напечатаны
`timer-overflow-fired`, `interval-overflow-fired` и контрольный
`timer-control-fired`; до правки была одна последняя строка.

A/B по **всей** вендоренной категории `html/webappapis/timers`
(`run_report.py --all --root html/webappapis/timers --recursive`, тот же
бинарник dev-release):

| | было | стало |
|---|---|---|
| подтестов | 12/15 | **14/15** |
| harness OK | 11/13 | 11/13 |

`type-long-settimeout.any.html` и `type-long-setinterval.any.html` — оба
0/1 → **1/1**. Ни один другой id категории не изменил вердикта; в частности
`evil-spec-example` — тест на порядок конверсии аргументов WebIDL — остался
зелёным: обработчик приводится к строке **до** задержки, потому что
`_lumen_timer_string_handler(fn)` по-прежнему стоит первой строкой функции.

Оба `type-long-*` к моменту правки были уже не TIMEOUT, а быстрый FAIL: после
[BUG-591](BUG-591-FIXED.md) `assert_unreached` на 100 мс долетает до harness.
Так что «перестать быть TIMEOUT» из плана проверки выше проверять было не на
чем — считать надо подтесты.

Остаток категории к этому багу не относится: `settimeout-detached-iframe`
(0/1) и два `*-cross-realm-callback-report-exception` (TIMEOUT) требуют
вложенного browsing context — [BUG-480](BUG-480-OPEN.md).

### Тесты

`crates/js/src/dom.rs` — шесть тестов `bug847_*`: срабатывание при `2^32` и
повтор интервала при `2^31`, `Infinity`, конверсия нечисловой задержки
(`'100'`/`valueOf`/`1.9` → 100/250/1 мс — проверяется сам срок, а не факт
срабатывания), нулевой срок для отрицательной задержки и `NaN`,
`clearTimeout(String(id))`. `crates/js/src/worker.rs` —
`v8_worker_clear_timeout_converts_its_handle`.

Значения «до» в каждом взяты из той самой одноразовой пробы, а не из интуиции.
