# BUG-648: `PerformanceObserver.observe()`/notification pipeline implements neither call-signature validation nor the spec's task-queued delivery model

**Статус:** FIXED 2026-09-26 (P6)
**Компонент:** js (`crates/js/src/dom.rs:8258-8335` — `PerformanceObserver` constructor/`observe`/`disconnect`/`takeRecords`/`_perf_observer_notify`, and every `_lumen_deliver_*`/`performance.mark`/`performance.measure` call site that invokes it)
**Найден:** P2, WPT-VENDOR-performance-timeline, 2026-08-05

## Симптом

`performance-timeline` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root performance-timeline --recursive`, ~4.5 мин,
51 id): **36/51 harness OK, 19/69 сабтестов**. Unusually high signal for
this backlog (most 🚫/⬜ categories in this stretch return 0-2 subtests) —
`PerformanceObserver` is a real, wired-up implementation, so its
conformance gaps are directly observable rather than masked by an
unimplemented API.

Two distinct, both spec-normative, defects account for essentially every
`PerformanceObserver`-specific subtest failure (as opposed to the
already-known unrelated gaps listed under Реконфирмации below):

**1. `observe()` performs no call-signature validation at all**
(`po-observe-type.any.html`, 4/5 subtests FAIL; `po-observe.any.html`,
1/3 subtests FAIL — `entryTypes must be a sequence or throw a
TypeError`):

```js
obs.observe({});                                    // must throw TypeError, doesn't
obs.observe({entryTypes: ["mark"]});
obs.observe({type: "measure"});                      // must throw InvalidModificationError, doesn't
obs.observe({type: "mark", entryTypes: ["measure"]}); // must throw TypeError, doesn't
```

Per Performance Timeline L2 §6.2.2 "register a performance observer",
`observe()` must throw `TypeError` when neither `type` nor `entryTypes`
is present (or a non-array `entryTypes`), and `InvalidModificationError`
when an observer that already registered via one form (single-`type` vs
multi-`entryTypes`) is re-registered via the other. Lumen's implementation
(`dom.rs:8273-8303`) has no throw statement anywhere in the function — any
input silently normalizes to an (possibly empty) type list.

**2. Observer notification runs synchronously, in-line at entry-creation
time, instead of being queued as a task** (`po-disconnect.any.html`
"An observer disconnected after a mark must not have its callback
invoked" FAIL, "Reached unreachable code"; `po-disconnect-removes-
observed-types.any.html` FAIL; `po-callback-mutate.any.html` FAIL;
`po-takeRecords.any.html` "expected 3 but got 5" FAIL;
`buffered-does-not-sync-invoke.html` TIMEOUT; `po-mark-measure.any.html`
TIMEOUT):

```js
mark: function(name, opts) {
    ...
    _perf_entries.push(entry);
    _perf_observer_notify([entry]);   // dom.rs:8194 — fires the callback right here
    return entry;
},
```

`_perf_observer_notify` is called directly from `performance.mark()`
(`dom.rs:8194`), `.measure()` (`dom.rs:8214`), and every `_lumen_deliver_*`
native-entry-point (paint/LCP/layout-shift/…, `dom.rs:8342` onward) — none
of these wrap the call in `queueMicrotask`/a task queue, even though
`queueMicrotask` already exists and is used elsewhere in the same file
(e.g. mutation observers, `dom.rs:6957`). Per §5.1/§10.3 ("queue a
PerformanceObserverCallback"), delivery must happen via a queued task, not
inline in the same synchronous turn that created the entry. Concretely
reproduced by `po-disconnect.any.html`'s second case: `observer.observe();
performance.mark("mark1"); observer.disconnect(); performance.mark
("mark2")` expects the callback to fire **zero** times (the queued task
for mark1's notification hasn't run yet when `disconnect()` executes, so
it's cancelled) — Lumen fires it immediately inside `mark()`, before
`disconnect()` gets a chance to run, hitting `assert_unreached`. The same
in-line-delivery model also explains the `buffered: true` case
(`PerformanceObserver.prototype.observe`, `dom.rs:8295-8302`, calls
`_perf_deliver_to_observer` directly inside `observe()` — literally what
`buffered-does-not-sync-invoke.html`'s title says must not happen) and the
`takeRecords()` overcount (an observer that already received an entry via
synchronous delivery has no "pending, undelivered records" queue distinct
from `_perf_entries`, so `takeRecords()` re-returns entries the callback
already saw).

## Причина

Both defects trace to the same shortcut in `dom.rs:8253-8335`: the shim
implements `PerformanceObserver` as straightforward JS convenience code
(collect types into an array, filter `_perf_entries` by type, call the
callback) rather than the spec's normative algorithm, which requires (a)
validating the *shape* of the `observe()` argument against the observer's
prior registration state, and (b) maintaining a separate queued-task
delivery path with its own pending-records buffer, distinct from the
synchronous `_perf_entries` push.

## Реконфирмации (не новые)

- `PerformanceObserverEntryList is not defined` (`po-observe.any.html`) —
  same class as [BUG-645](BUG-645-FIXED.md)/[BUG-624](BUG-624-FIXED.md)/
  [BUG-637](BUG-637-OPEN.md)/[BUG-589](BUG-589-FIXED.md): WebIDL
  interface objects absent as globals even where the underlying behavior
  (the plain-object "list" passed to callbacks, `dom.rs:8319-8326`) works.
- `case-sensitivity.any.html` (`resources/square.png?id=1` never loads,
  "fetch error: invalid url: invalid url: missing scheme") — same class
  as [BUG-347](BUG-347-FIXED.md) (`fetch()`/resource loading doesn't
  resolve relative URLs — fixed 2026-08-06).
- `timing-removed-iframe.html` (`Cannot read properties of null (reading
  'performance')` on a detached iframe's `contentWindow`) — same class as
  the already-documented `<iframe>` no-separate-browsing-context gap
  (BUG-480 lineage, per `STATUS-PN`/`focus` session notes).
- `navigation-id-*.tentative.html`, `not-restored-reasons/*.window.html`
  (`ReferenceError: RemoteContext is not defined` / `token is not
  defined`) — `/common/dispatcher/dispatcher.js` is category-external and
  not vendored (`tests/wpt/common/` doesn't exist), same established gap
  as `navigation-timing`/`mixed-content`.
- `supportedEntryTypes` listing types with no real delivery mechanism
  (`element`/`event`/`first-input`/`longtask`/`soft-navigation`) — already
  filed as [BUG-354](BUG-354-FIXED.md).

## Новая, не реконфирмационная находка вне PerformanceObserver

`not-restored-reasons/abort-block-bfcache.window.html` FAILs on
`window.stop is not a function` — `window.stop()` (HTML LS §7.4.1) does
not exist on the `window` shim at all. Unrelated to the two defects above;
noted here rather than filed separately since it's a single-file,
single-line finding with no further investigation performed.

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root performance-timeline --recursive
```
or a live probe:
```js
var o = new PerformanceObserver(function(){ throw new Error('called'); });
o.observe({entryTypes:['mark']});
performance.mark('m1');
o.disconnect();
// spec: callback must NOT have run by here. Lumen: it already threw.
```


## Реальный сайт (2026-09-24): cnbc

Синхронная доставка buffered-записей из `observe()` ломает шаблон web-vitals, где колбэк отчёта
присваивается после `observe()`: cnbc — `Uncaught TypeError: n is not a function`, стек через
`PerformanceObserver._cb` → `_perf_deliver_to_observer`. Репро `.tmp/compat/g6/site/perfobs.html`:
Lumen `['cb:1','after-observe']` + `report is not a function`, Chrome `['after-observe','cb:1',
'report:1']`. Передан P6 по решению пользователя.

## Реальный сайт (2026-09-25, P6): imdb

Тот же шаблон на imdb (3 из 3 прогонов, видимое окно, `LUMEN_NO_ADBLOCK=1`): `Uncaught TypeError:
a is not a function` из колбэка `PerformanceObserver._cb` → `_perf_deliver_to_observer` →
`PerformanceObserver.observe` — колбэк вызван синхронно изнутри `observe()`, до того как страница
присвоила функцию отчёта. Chrome этой ошибки не даёт. Найдено по ходу закрытия
[BUG-493](BUG-493-FIXED.md).

## Исправление (2026-09-26, P6)

`crates/js/src/shim/web_api_shim_tail.js`, блок `PerformanceObserver` переписан по алгоритму
Performance Timeline L2 §4–5 (текущий черновик), а не по удобной фильтрации:

- **Модель наблюдателя.** Наблюдатель хранит `observer type` (`undefined` → `single`/`multiple`),
  список опций и **буфер наблюдателя** — записи, ещё не отданные колбэку. `_perf_observer_notify`
  стал §5.1 «queue a PerformanceEntry»: он кладёт запись в буферы заинтересованных наблюдателей и
  ставит одну задачу §5.3 на глобал (флаг «performance observer task queued», очередь —
  `_perf_queue_task`, та же, что у `resourcetimingbufferfull`). Колбэк больше не вызывается ни
  внутри `mark()`/`measure()`/`_lumen_deliver_*`, ни внутри `observe({buffered: true})`.
  Задача отдаёт каждому наблюдателю его буфер и опустошает его.
- **Валидация `observe()`.** Словарь `PerformanceObserverInit` конвертируется по WebIDL
  (`entryTypes` — последовательность, строка → `TypeError`). Нет ни `type`, ни `entryTypes` →
  `TypeError`; оба сразу → `TypeError`; смена формы → `InvalidModificationError`. Форма
  фиксируется первым вызовом, даже если он потом отменён из-за неизвестного типа, и
  переживает `disconnect()`. `entryTypes` заменяет прежнюю подписку, `type` добавляет к ней, а
  повторный `type` заменяет одноимённый. `buffered` рядом с `entryTypes` игнорируется с
  предупреждением: спека говорит «любой другой член → TypeError», но WPT
  (`buffered-flag-with-entryTypes-observer.tentative`) и все движки требуют игнорирования.
- **`takeRecords()`** возвращает буфер наблюдателя и опустошает его: запись доходит до страницы
  ровно один раз. **`disconnect()`** снимает регистрацию и очищает буфер и опции, поэтому уже
  поставленная задача этому наблюдателю ничего не отдаст.
- **`PerformanceObserverEntryList`** стал интерфейсом (`window.PerformanceObserverEntryList`,
  `new` → `TypeError`). Колбэк получает его экземпляр, а `getEntries*` сортируют по `startTime`
  (§5.5). `this` колбэка — сам наблюдатель.
- **Форма интерфейсов по WebIDL.** Проверки `idlharness.any.html` раньше не выполнялись вовсе:
  `idl_test setup` падал на наблюдателе. После фикса они заработали и показали форму.
  `PerformanceObserver`/`PerformanceObserverEntryList` — неперечислимые глобалы, `prototype`
  неизменяем, члены перечислимы, `Symbol.toStringTag`, у операций IDL-`name`/`length`, проверка
  бренда (`this` не того типа → `TypeError`), `TypeError` без обязательного аргумента.
  `supportedEntryTypes` — один замороженный массив (`[SameObject] FrozenArray`). То же для
  `performance.getEntries*` (`crates/js/src/shim/performance_shim.js`).
- `droppedEntriesCount` считается в момент доставки, поэтому записи, выброшенные до
  выполнения задачи, тоже учитываются. Счётчик `dropped_entries_count_reported_once_per_observe`
  исправлен с 1 на 2: это то же «2», что проверяет `droppedentriescount.any.js`.

Не тронуты `long_tasks.rs`/`long_animation_frames.rs`/`soft_navigation.rs`: их собственные
`PerformanceObserver` — заглушки для их же юнит-тестов (`PERF_STUB`), а живые записи идут через
`_perf_observer_notify` страничного шима. `soft_navigation.rs` пишет в несуществующие
`performance._observers`, но тип `soft-navigation` не входит в `supportedEntryTypes` (BUG-354).

**Тесты.** `crates/js/src/dom/tests/v8_perf_observers.rs` — 11 новых `bug648_*` (форма
интерфейсов по WebIDL, оба вида
`TypeError`, `InvalidModificationError` в обе стороны, колбэк не внутри `mark()`, `disconnect()`
после `mark()`, buffered-шаблон web-vitals, `takeRecords()` опустошает буфер,
`entryTypes` заменяет / `type` накапливает, `disconnect()` забывает типы, интерфейс
`PerformanceObserverEntryList` с сортировкой, `buffered` рядом с `entryTypes` не
воспроизводит прошлое). 20 существующих тестов ждали синхронной доставки. В них добавлен
`_lumen_tick_timers()` между созданием записи и проверкой, а buffered-случаи переведены на форму
`type`: в форме `entryTypes` флаг `buffered` по спеке ничего не делает.

**Живая проверка** (dev-release, `--mcp-live-port`, видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`, `.tmp/compat/probe.py`). Репро из раздела про cnbc (`perfobs.html`):
Lumen `['after-observe','cb:1','report:1']` без ошибок, как у Chrome; до фикса было
`['cb:1','after-observe']` + `report is not a function`. Сводная проба: колбэк
отключённого после `mark()` наблюдателя не вызван, `observe({})` → `TypeError`, смена формы →
`InvalidModificationError`. cnbc: 2650 узлов, заголовок страницы, ошибок `PerformanceObserver` нет.
imdb, 4 прогона: ни одного `a is not a function`. В двух прогонах страница дошла до приложения
(1965 и 5565 узлов), в двух через 15–22 с ещё стоял челлендж на 13 узлов. Это
[BUG-1179](BUG-1179-OPEN.md), а не этот баг.

Попутно: `window.stop()` из раздела «Новая находка» всё ещё отсутствует (`typeof window.stop`
→ `'undefined'`). Эта находка жила только внутри этого бага, поэтому заведена отдельно:
[BUG-1186](BUG-1186-OPEN.md). На cnbc две `rel=preload`-загрузки упали с `H2 I/O: peer closed
connection without sending TLS close_notify` без повтора на новом соединении. Это класс
[BUG-1177](BUG-1177-OPEN.md), сайт дописан туда.

**WPT `performance-timeline`** (`run_report.py --all --root performance-timeline --check`, тот же
бинарь): 28 новых PASS (`po-observe-type`, `po-disconnect*`, `po-takeRecords`, `po-callback-mutate`,
`po-entries-sort`, `droppedentriescount`, `buffered-flag-observer`, `buffered-does-not-sync-invoke`,
`po-mark-measure`, `supportedEntryTypes` «caches result», `performanceentry-tojson`,
`idl_test setup`). Baseline переснят `--update-expected`, следующий `--check` дал 0 настоящих
регрессий. Два результата разошлись между двумя прогонами одного бинаря и помечены как
нестабильные: `idlharness.any.sharedworker.html` (`[OK, TIMEOUT]`) и
`webtiming-resolution.any.html` (`[FAIL, PASS]`). Второй — проверка разрешения `performance.now()`,
этот фикс её не касается. «Регрессии» `--check` до пересъёмки
разобраны по одной:

- 13 подтестов `PerformanceEntry interface: …` в `idlharness.any.html`. Раньше `idl_test setup`
  падал до них, и baseline записал их PASS в тихом режиме. Теперь они выполняются и падают, потому что
  интерфейса `PerformanceEntry` нет. Заведено [BUG-1189](BUG-1189-OPEN.md).
- `not-clonable.html` и `idlharness.any.serviceworker.html` → TIMEOUT. Проверено A/B на бинаре с
  текущего `main` (`run_smoke.py`): там оба теста так же TIMEOUT, так что это не результат этого фикса.
  `not-clonable` висит, потому что ребёнок шлёт ответ через `window.top.postMessage`, а во фрейме
  `window.top === window` ([BUG-1187](BUG-1187-OPEN.md)). Попутно найдено: `postMessage` из
  синхронного скрипта фрейма родителю не доходит ([BUG-1188](BUG-1188-OPEN.md)).
- Подтесты операций (`getEntries*`, `disconnect`, `takeRecords`) требовали проверки бренда — сделана
  (см. выше).
