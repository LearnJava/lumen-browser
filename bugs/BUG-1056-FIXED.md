# BUG-1056: `ResizeObserver` delivery races `requestAnimationFrame`/focus-fixup instead of running inside the same "update the rendering" step

**Статус:** FIXED 2026-09-30
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid_b.js` -- `_ro_schedule_initial`/`_ro_initial_pass`, `_lumen_timers`)
**Найден:** P3, BUG-600 follow-up, 2026-09-16

## Симптом

```
FAIL #button1 - assert_true: requestAnimationFrame should run before ResizeObserver expected true got false
FAIL #div - assert_equals: activeElement shouldn't have changed yet (ResizeObserver) expected object "[object Object]" but got object "[object Object]"
```
(`processing-model/focus-fixup-rule-one-no-dialogs.html`, 6/8 subtests --
found while fixing BUG-600, see that bug's file for the mechanism this test
is actually about)

## Причина

Resize Observer §3.2 requires the observation loop to run as part of the
same "update the rendering" step as animation frame callbacks (rAF first,
then resize observations, both before any later step like focus fixup).
This engine's `ResizeObserver` delivery (`_ro_schedule_initial`/
`_ro_initial_pass`, added by BUG-661) is scheduled through `_lumen_timers` --
a plain event-loop timer task, entirely decoupled from
`_lumen_run_raf_callbacks` (the shim's stand-in for "update the rendering",
driven by the shell only when a rAF is pending). A timer task's relative
order against a same-frame rAF batch is not guaranteed the way the spec's
single rendering-step ordering is, so a test asserting "rAF ran before my
ResizeObserver callback" or "the DOM hasn't changed yet by the time my
ResizeObserver callback runs" sees the two race unpredictably instead of in
the fixed spec order.

## Масштаб

Blocks 6 of `focus-fixup-rule-one-no-dialogs.html`'s 8 subtests from ever
reaching their real assertion (`#button1`/`#button2`/`#button4`/`#button5`
fail on the rAF-vs-RO ordering check itself; `#div`/`#editable` fail on the
"unchanged yet" check one step later) -- independent of BUG-600's fixup
mechanism, which is implemented and unit-tested correctly
(`v8_runtime::tests::dom_suspend_focus::focus_fixup_rule_moves_focus_to_body`).
Any other WPT file or real page relying on "rAF, then ResizeObserver, in
that fixed order within one frame" is equally affected -- this is a general
`ResizeObserver` scheduling gap, not specific to focus.

Fixing it properly means moving `ResizeObserver` delivery off `_lumen_timers`
and into `_lumen_run_raf_callbacks` (or a sibling step invoked in the same
shell tick, after rAF callbacks and before focus fixup) while preserving
BUG-661's "guaranteed first delivery even with no relayout" property --
out of scope for a one-line fix, hence filed separately rather than folded
into BUG-600.

## Срез 1 (P6, 2026-09-30)

Первая доставка `ResizeObserver` больше не забирается таймер-задачей раньше rAF: `_ro_initial_pass` откладывается (до 8 кадров), пока есть ожидающие rAF, а `_lumen_run_raf_callbacks` вызывает `_ro_frame_pass` после пакета rAF и до `_lumen_focus_fixup`. Юнит-тест `resize_observer_first_delivery_follows_raf_in_same_frame`. WPT `focus-fixup-rule-one-no-dialogs.html` не перепрогнан — баг остаётся OPEN до перепроверки.

## Срез 2 (P6, 2026-09-30) — закрыт

Причина остатка: пост-relayout проход шелла (`deliver_layout_observers` → `_lumen_deliver_resize_observers`) доставлял RO до rAF того же кадра. Добавлен `_lumen_deliver_resize_observers_layout`: при очереди rAF откладывает доставку до `_ro_frame_pass`. Юнит-тест `resize_observer_layout_delivery_follows_raf_in_same_frame`. WPT `focus-fixup-rule-one-no-dialogs.html`: 6/8 (`.ini` обновлён); #button3 (синхронный fixup) и #button6 падают по другим причинам, не по порядку RO.
