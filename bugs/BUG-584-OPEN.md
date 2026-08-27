# BUG-584: Interest Invokers API (`interestfor`, `InterestEvent`) not implemented at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — grep for `interestFor`/
`InterestEvent` returns zero hits anywhere in the file)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - InterestEvent is not defined
```

36 occurrences, entirely within `interestfor/` (all `.tentative.` files —
27 of 31 id's pull `testdriver.js`, per this run's harness pass rate 22/31).

## Причина

Interest Invokers (WHATWG HTML draft addition, hover/focus-triggered
"preview" popovers via `interestfor="idX"` on `<a>`/`<area>`/`<button>`, the
hover/focus analogue of the existing `command`/`commandfor` click-driven
mechanism — see [BUG-582](BUG-582-OPEN.md), same shape of gap) has no
implementation: no `interestFor` IDL reflection, no
`InterestEvent`/`interest`/`loseinterest` dispatch, no hover/focus-delay
timers driving it.

## Масштаб

Whole feature, self-contained to `interestfor/`. Entirely `.tentative.` —
an early-stage draft, not yet broadly implemented even in other engines —
flagging for scope triage rather than implying priority, same posture as
[BUG-583](BUG-583-OPEN.md) (`<permission>`) found in the same run.
