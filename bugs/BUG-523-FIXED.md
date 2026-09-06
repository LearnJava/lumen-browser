# BUG-523: `Element.scrollTop`/`scrollLeft` setter is queued and applied
asynchronously by the shell — a synchronous read right after the write sees
the stale (pre-write) value instead of the just-set position

**Статус:** FIXED 2026-09-06 (дрейф трекера)
**Дата:** 2026-08-03
**Компонент:** js/shell boundary (`crates/js/src/dom.rs:6196-6205` setters,
`crates/js/src/v8_runtime.rs:3103-3108` `_lumen_request_scroll`, shell's
`take_scroll_requests()` drain loop)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/css-scroll-anchoring`

## Механизм

The JS shim's `scrollTop`/`scrollLeft` setters (`dom.rs:6199`/`6203`) don't
mutate any JS-visible state directly — they call the native
`_lumen_request_scroll(nid, x, y)`, which merely pushes `(nid, x, y)` onto a
`pending_scrolls: Arc<Mutex<Vec<...>>>` queue (`v8_runtime.rs:3106`). The
shell drains that queue on its own schedule (`take_scroll_requests()`,
frame/event-loop tied) and only *then* updates the `scroll_states` map that
the getter (`_lumen_get_scroll_state`, `dom.rs:6200`/`6196`) reads. Nothing
forces that drain to happen synchronously in response to the property write,
so a script that writes then immediately reads (in the same task, the CSSOM
View-mandated pattern used by virtually every scroll test) observes the
value from *before* the write, not the target it just set.

Confirmed live (`--mcp-live-port`, minimal isolation, two independent
scrollable `<div overflow:scroll>` elements, unrelated to
css-scroll-anchoring specifically):

```js
// same eval call, write then immediate read:
var e = document.getElementById('s');
e.scrollTop = 200;
e.scrollTop   // => 0 (stale)

// a few hundred ms later, a SEPARATE eval call on the same element:
document.getElementById('s').scrollTop   // => 200 (correct once the shell drained the queue)

// explicit timing: write, immediate read, then read again after 500ms wall-clock sleep
document.getElementById('t').scrollTop = 175; document.getElementById('t').scrollTop  // => 0
// ... 500ms later, separate eval call ...
document.getElementById('t').scrollTop   // => 175
```

This is the same architectural symptom family as
[BUG-493](BUG-493-OPEN.md) (script mutates state, then reads a
derived/cached value in the same task and gets the stale snapshot) but a
*different* code path/root cause — BUG-493 is about the `computed_styles`
cache populated by `update_computed_styles`, this is about the
`pending_scrolls`/`scroll_states` queue-and-publish pair. Filed separately
because the fix lives in different code (there's no single "force a
synchronous flush" call this shares with BUG-493's fix).

## Симптом

Any WPT test that does `el.scrollTop = N; assert_equals(el.scrollTop, N)` (or
the div/window equivalents) in the same script turn fails with `expected N
but got 0` (or whatever the previous scroll position was) — this is the
dominant failure cluster of `css/css-scroll-anchoring` (18+ files hit the
`Cannot set properties of undefined` variant when `document.scrollingElement`
compounds this — see [BUG-525](BUG-525-FIXED.md) — and 8+ hit the bare
`assert_equals: expected N but got 0` form on real element scrollers).
Likely affects any other WPT category whose tests script-drive scrolling and
assert synchronously (`css-overflow`, `cssom-view`, `css-scroll-snap` are
candidates worth re-checking once this lands).

## Фикс (не сделан)

Either (a) make the setter apply the scroll position to an in-memory
JS-visible cache synchronously (mirroring what real browsers do — the
*visual*/smooth animation can still be async, but the script-visible value
must update immediately per CSSOM View §scrolling), or (b) force a
synchronous drain-and-republish of `pending_scrolls`→`scroll_states` for the
specific `nid` inside the getter when a pending request for that node
exists (same shape of fix pattern the eventual BUG-493 fix will need for
`computed_styles`).

## Ревизия P3 2026-09-06 — уже исправлено, дрейф трекера

Этот баг никогда не чинился под своим номером — его ровно тот же механизм
(`_lumen_request_scroll` кладёт запись только в `pending_scrolls`,
`_lumen_get_scroll_state` читает только `scroll_states`, синхронное
чтение сразу после записи видит устаревшее значение) был закрыт как
побочный эффект расследования [BUG-504](BUG-504-OPEN.md) part 10 →
[BUG-975](BUG-975-OPEN.md) (2026-09-04, `install_scroll_state` в
`crates/js/src/v8_runtime/install/platform.rs::_lumen_request_scroll`):
запрос теперь оптимистично пишется в тот же кэш `scroll_states`, который
читает геттер (см. doc-комментарий у `_lumen_request_scroll`, явно
цитирующий этот сценарий). Юнит-тест
`crates/js/src/dom/tests/v8_bug975_scroll_request_sync.rs::direct_scroll_left_top_assignment_is_visible_to_synchronous_read`
дословно воспроизводит запись+синхронное чтение `scrollLeft`/`scrollTop`
и зелёный.

Живой A/B не по коду, а по симптому: `--mcp-port`, страница с двумя
независимыми `overflow:scroll`-контейнерами (тот же снаряд, что в §Механизм
выше — `#s`/`#t`), `eval` одним вызовом `e.scrollTop = 200; return e.scrollTop`
для обоих одновременно — вернул `{s_after_write: 200, t_after_write: 175}`,
то есть точно запрошенные значения, а не устаревший `0`. Заведённый BUG-975
не покрывает один узкий смежный случай — синхронное переключение
`overflow` на `clip` в интерактивном (не headless) окне того же тика
([BUG-977](BUG-977-OPEN.md), ДОРАБОТКА → CSSOM-7) — но это не тот сценарий,
что описан здесь: репро этого бага не трогает `overflow` вовсе, только
голую запись/чтение позиции скролла.

Точечного P3-фикса не требуется — закрывается как дрейф трекера, тот же
класс ревизии, что [BUG-512](BUG-512-FIXED.md).
