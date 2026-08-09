# BUG-719 — `MIDIPort`/`MIDIInput`/`MIDIOutput`/`MIDIAccess` are directly constructible with `new`, though the spec defines no constructor

**Статус:** OPEN
**Компонент:** js (`crates/js/src/web_midi.rs` — `WEB_MIDI_SHIM`)
**Найден:** P2, WPT-VENDOR-webmidi, 2026-08-09

## Симптом

Категория `webmidi` (`tests/wpt/webmidi/`, 1 файл, `idlharness.https.window.js`)
— вендорена и прогнана целиком (`run_report.py --all --root webmidi
--recursive`, ~1:30, 1 id): **0/1 harness OK**, single TIMEOUT. Not a new
finding — same already-documented gap as `webgl`'s `idlharness.any.html`
(`WPT-VENDOR-webgl` row, `BUGS.md`): `/resources/WebIDLParser.js` and
`/resources/idlharness.js` are referenced by `// META: script=` but never
vendored anywhere under `tests/wpt/resources/`, so the classic script throws
`ReferenceError: idl_test is not defined` before `testharness.js` ever
registers a test — no functional signal from the run itself.

Web MIDI is actually implemented as a Phase 0 stub
(`crates/js/src/web_midi.rs`, `navigator.requestMIDIAccess()` resolves
immediately with empty `inputs`/`outputs` maps — matches its own doc-comment).
Live probe (`--mcp-live-port`) found an independent WebIDL-shape defect,
directly reproducing what the idlharness test itself declares as expected
usage — `idl_array.add_objects({ MIDIPort: [] })` etc., with no constructor
arguments, meaning the spec never exposes a constructor for these four
interfaces (they are only ever produced by the UA via `requestMIDIAccess()`):

```json
{
  "MIDIPort": "function", "MIDIInput": "function",
  "MIDIOutput": "function", "MIDIAccess": "function",
  "new_MIDIPort": "CONSTRUCTED: MIDIPort",
  "new_MIDIInput": "CONSTRUCTED: MIDIInput",
  "new_MIDIOutput": "CONSTRUCTED: MIDIOutput",
  "new_MIDIAccess": "CONSTRUCTED: MIDIAccess"
}
```

All four are implemented as plain ES6 `class X extends EventTarget` /
`class X extends _ETBase`, so `new window.MIDIPort('id','man','name','type',
'1.0')` / `new window.MIDIAccess(false)` etc. construct successfully from any
page, no engine-provided factory required. Per WebIDL, an interface with no
constructor operation must make calling its interface object with `new`
throw `TypeError`. (`MIDIMessageEvent`/`MIDIConnectionEvent`, the two
remaining interfaces of the module, *do* have spec constructors — matches
the idlharness test's own `'new MIDIMessageEvent(...)'`/`'new
MIDIConnectionEvent(...)'` usage — not affected.)

## Масштаб

Same class of defect already open for `Report`/`ReportingObserver`
([BUG-629](BUG-629-OPEN.md)), `FileSystemFileHandle`
([BUG-374](BUG-374-OPEN.md)), `Serial`/`SerialPort`
([BUG-672](BUG-672-OPEN.md)) and `HIDManager`/`HIDDevice`
([BUG-713](BUG-713-OPEN.md)) — a forged instance, indistinguishable via
`instanceof MIDIPort`/`instanceof MIDIAccess` etc. from one legitimately
returned by `requestMIDIAccess()`. Fifth independent surface of the same
systemic pattern (no `new.target`/constructor guard). No functional WPT
signal exists for this category (single id, infra-gap TIMEOUT) — the finding
is live-probe-only.

## Причина

Not investigated in detail (out of scope for the WPT-VENDOR task). Same
structure as the other four surfaces: `WEB_MIDI_SHIM`
(`crates/js/src/web_midi.rs:55-124`) declares each class with a public
constructor and exports it directly on `window`, with no check that
construction originated from the engine rather than page script.

## Дальше

Fix scope: block public `new MIDIPort(...)`/`new MIDIInput(...)`/
`new MIDIOutput(...)`/`new MIDIAccess(...)` (same guard pattern proposed for
[BUG-672](BUG-672-OPEN.md)/[BUG-713](BUG-713-OPEN.md); worth fixing all five
surfaces together once a guard helper exists — common root, same V8-port
era). Does not require the infra gap (`WebIDLParser.js`/`idlharness.js`) to
reproduce or verify — the live `--mcp-live-port` probe is sufficient.
