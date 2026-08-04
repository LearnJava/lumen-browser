# BUG-578: `ToggleEvent` interface missing — popover/`<details>` toggle fires a plain `Event` with hand-bolted `oldState`/`newState`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:14888-14937` popover/`<details>`
toggle dispatch, `dom.rs:15013-15042` popover show/hide toggle dispatch — no
`ToggleEvent` constructor is defined anywhere in the file)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL the event is an instance of ToggleEvent - ToggleEvent is not defined
FAIL ToggleEvent constructed with no arguments throws - ToggleEvent is not defined
...
```

154 occurrences (`popovers/toggleevent-interface.html` and every
`interactive-elements/the-details-element/*toggle*` test).

## Причина

Both dispatch sites that fire a `toggle`/`beforetoggle` event
(`<details>` at `dom.rs:14903/14924`, popover show/hide at
`dom.rs:15013/15042`) construct a plain `Event` and then manually assign
`oldState`/`newState` as ordinary own properties:

```js
var toggleEvt = new Event('toggle', { bubbles: false, cancelable: false });
toggleEvt.oldState = oldState;
toggleEvt.newState = newState;
_lumen_dispatch(pid, toggleEvt);
```

Per HTML LS, both events must be real `ToggleEvent` instances
(`ToggleEvent extends Event`, with `oldState`/`newState` as constructor-init
read-only accessors, `cancelable` defaulting `true` for `beforetoggle`). No
`ToggleEvent` constructor/prototype exists anywhere in `dom.rs` — grep for
`ToggleEvent` returns zero hits. Consequences beyond the interface-identity
check: `new ToggleEvent(...)` throws `ReferenceError` (breaks any test or
page script that constructs one directly, e.g. to dispatch a synthetic
toggle), `event instanceof ToggleEvent` always throws before it can even be
false, and `oldState`/`newState` are plain writable own properties instead
of the spec's read-only accessors.

## Масштаб

Medium, self-contained: every listed subtest lives in either
`popovers/toggleevent-interface.html` or
`interactive-elements/the-details-element/*toggle*`, i.e. the two dispatch
sites named above are the only two that need to switch constructors. The
functional show/hide/open/close behavior itself works today (confirmed:
`popover_toggle_event_fired`/`popover_beforetoggle_event_fired` unit tests
pass at `dom.rs:31137-31148`) — this is purely an event-*type* gap, not a
missing feature.
