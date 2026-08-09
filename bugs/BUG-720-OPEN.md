# BUG-720 — `MIDIInputMap`/`MIDIOutputMap` are not exposed globally at all; a single non-standard `MIDIPortMap` stands in for both

**Статус:** OPEN
**Компонент:** js (`crates/js/src/web_midi.rs` — `WEB_MIDI_SHIM`)
**Найден:** P2, WPT-VENDOR-webmidi, 2026-08-09

## Симптом

Same live probe as [BUG-719](BUG-719-OPEN.md) (`webmidi`, run gives no
functional signal — infra gap, `WebIDLParser.js`/`idlharness.js` not
vendored):

```json
{"MIDIPortMap": "function", "MIDIInputMap": "undefined", "MIDIOutputMap": "undefined"}
```

The Web MIDI spec defines two separate maplike global interfaces,
`MIDIInputMap` (`maplike<DOMString, MIDIInput>`) and `MIDIOutputMap`
(`maplike<DOMString, MIDIOutput>`) — `MIDIAccess.inputs`/`.outputs` are typed
as these, respectively. `crates/js/src/web_midi.rs` instead implements one
shared, non-spec class `MIDIPortMap` (`window.MIDIPortMap`) and uses it for
both `MIDIAccess.inputs` and `.outputs`; `window.MIDIInputMap` and
`window.MIDIOutputMap` do not exist. This is exactly the interface pair the
vendored idlharness test itself checks —
`idl_array.add_objects({ MIDIInputMap: ['inputs'] })` / `{ MIDIOutputMap:
['outputs'] }` — so even once the `WebIDLParser.js`/`idlharness.js` infra gap
is fixed, this test would fail on "interface object not found" for both.

## Масштаб

Isolated to this module — `MIDIPortMap`'s own maplike surface (`size`, `get`,
`has`, `entries`, `keys`, `values`, `forEach`, `Symbol.iterator`) matches the
spec's maplike shape correctly; only the two required *global* type names
are missing, replaced by an internal implementation detail leaking as the
public one. `MIDIAccess.inputs instanceof MIDIInputMap` and
`.outputs instanceof MIDIOutputMap` both fail (`instanceof undefined`
throws `TypeError`), which any script feature-testing per spec would trip on.

## Причина

`crates/js/src/web_midi.rs:99-113` (`class MIDIPortMap`) is exported once at
`crates/js/src/web_midi.rs:157` as `window.MIDIPortMap = MIDIPortMap`; no
`MIDIInputMap`/`MIDIOutputMap` subclass or alias is ever created or exported.

## Дальше

Fix scope: split into two named classes (`MIDIInputMap extends MIDIPortMap`,
`MIDIOutputMap extends MIDIPortMap`, or two independent classes sharing a
private mixin) and export both on `window`; construct `MIDIAccess.inputs`/
`.outputs` from the matching one. Independent of
[BUG-719](BUG-719-OPEN.md) (constructor guard) — can be fixed separately.
Does not require the infra gap to reproduce or verify.
