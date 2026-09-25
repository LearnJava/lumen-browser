# BUG-628: `IntersectionObserver.prototype.takeRecords()` missing entirely; `root`/`rootMargin`/`thresholds` are not exposed as IDL attributes at all

**Renumbered 2026-08-05** from `BUG-625` — collided with another parallel
session's `BUG-625` (chrome font measurer, already pushed to `origin/main`
by P3's BUG-128 branch), resolved while merging this branch back into
`main`.

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/dom.rs:7174-7276` — `IntersectionObserver` shim)
**Найден:** P2, WPT-VENDOR-intersection-observer, 2026-08-05

## Симптом

Confirmed live (`--mcp-live-port`, `eval`):

```js
typeof IntersectionObserver.prototype.takeRecords   // "undefined"
var o = new IntersectionObserver(function(){});
typeof o.takeRecords                                // "undefined"
o.root                                               // undefined
o.rootMargin                                         // undefined
o.thresholds                                         // undefined
```

`crates/js/src/dom.rs:7180-7201` defines only the constructor plus
`observe`/`unobserve`/`disconnect`. There is no `takeRecords` method and
no getters for `root`/`rootMargin`/`thresholds` — the constructor stores
the raw options object as `this._options` (`dom.rs:7182`) and never
surfaces it back through the spec's read-only IDL attributes.

## Масштаб

This is the single dominant cause of failure in the `intersection-observer`
category: **68 of 143 test files** call `observer.takeRecords()` as a
matter of course (most WPT `intersection-observer` tests concat pending
records via `entries = entries.concat(observer.takeRecords())` right after
`observe()`, to also cover any synchronous-delivery edge case) and get a
`TypeError: observer.takeRecords is not a function`, aborting the test
before it reaches its actual assertions. A further handful
(`observer-attributes.html` and friends) directly assert on
`observer.root`/`.rootMargin`/`.thresholds` and get `undefined` instead of
the constructed values (e.g. `rootMargin` should default to `"0px 0px 0px
0px"`, `thresholds` to `[0]`).

`CAPABILITIES.md:152` lists `IntersectionObserver` under "Observers/Timing
— ✅" — same drift class as BUG-368 (`innerHTML`): the constructor/
`observe`/callback-delivery path works well enough to pass hand-written
smoke tests (`dom.rs:21114-21330`), but roughly half of the spec's IDL
surface (`takeRecords`, three of five read-only attributes) is absent.

## Fix shape

- `takeRecords()`: return and clear a per-observer pending-records queue.
  The current delivery path (`_lumen_deliver_intersection_observers`,
  `dom.rs:7217-7276`) calls `obs._cb(entries, obs)` directly and never
  retains `entries` afterward — needs to also push to
  `this._records` (or similar) so `takeRecords()` can drain it, and the
  callback path should likewise drain-then-deliver so records aren't
  double-reported.
- `root`/`rootMargin`/`thresholds` getters: trivial — surface
  `this._options.root ?? null`, the normalized/serialized `rootMargin`
  string (defaulting to `"0px 0px 0px 0px"`), and
  `Array.isArray(this._options.threshold) ? this._options.threshold.slice()
  : [this._options.threshold ?? 0]` respectively. Ideally computed once in
  the constructor and stored, not recomputed per access.

See also BUG-626 (constructor/`observe()` perform no argument validation)
and BUG-627 (`root` option is accepted but silently ignored) — filed from
the same run, all three symptoms of the same underlying "IntersectionObserver
is a partial Phase-0 stub" gap.

## Исправление (P3, 2026-09-25)

Код живёт уже не в `dom.rs`, а в `crates/js/src/shim/web_api_shim_mid_b4.js`
(§ IntersectionObserver).

- **Очередь `[[QueuedEntries]]` (§2.2).** Проход
  `_lumen_deliver_intersection_observers` раньше собирал записи в локальный
  массив и сразу звал колбэк — забрать их было неоткуда. Теперь он в два
  этапа: сначала наполняет очереди **всех** наблюдателей, затем для каждого
  забирает очередь непосредственно перед его колбэком (§3.2.4 notify).
  `takeRecords()` возвращает и очищает ту же очередь, поэтому вызов из
  колбэка соседнего наблюдателя забирает его записи, и тот их повторно не
  получает. `disconnect()` очищает очередь.
- **Атрибуты `root`/`rootMargin`/`scrollMargin`/`thresholds`** — getter-only
  аксессоры на прототипе. Значения фиксируются в конструкторе: margin
  разбирается по правилу «parse a margin» (1–4 токена, только px/%, раскладка
  как у `margin`, сериализация `"T R B L"`), `thresholds` — отсортированный
  замороженный массив, пустой → `[0]`, одно число → список из одного.
  Невалидный margin пока откатывается к умолчанию — `SyntaxError`/
  `RangeError`/`TypeError` конструктора остаются в BUG-626.
- **Проход доставки читает нормализованные значения**, а не сырой `options`:
  `%` в `rootMargin` резолвится от высоты (top/bottom) и ширины (left/right)
  корня — прежний `_parse_root_margin` превращал любой не-px токен в 0
  (он остаётся для юнит-тестов lazy-image).
- Попутно: `observe()` после `disconnect()` снова регистрирует наблюдателя —
  раньше `disconnect()` вычёркивал его из `_io_observers` навсегда, и
  повторный `observe()` молчал.

Регрессия — `crates/js/src/dom/tests/v8_bug628_io_take_records.rs`
(7 тестов: умолчания, нормализация, дескрипторы, `takeRecords` до колбэка,
отсутствие двойной доставки, `%`-margin, observe после disconnect).

Живой WPT, `run_report.py --all --root intersection-observer --recursive
--processes 6`, dev-release до/после:

| | До | После |
|---|---|---|
| harness OK | 128/143 | 129/143 |
| сабтесты | 33/229 | 114/381 |
| `observer-attributes.html` | 1/9 | 9/9 |
| `root-margin-rounding.html` | TIMEOUT | OK 1/1 |

Сравнение по сабтестам: ни один PASS не стал не-PASS. Рост знаменателя —
тесты, падавшие на `takeRecords is not a function` до первого утверждения,
теперь доходят до своих утверждений. `.ini`-baseline категории переписан
`--update-expected` (66 файлов, 4 удалены как чистые) — прежний был снят до
BUG-807 и описывал отказы на уровне обвязки.

Что осталось в категории (не этот баг): `entries.length` не совпадает в 66
сабтестах — доставка только по relayout/первому наблюдению и явный `root`
игнорируется (BUG-627); `IntersectionObserverEntry` не глобал (BUG-1131);
валидация аргументов (BUG-626).
