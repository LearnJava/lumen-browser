# BUG-613: `HTMLInputElement.capture` reflects as plain string, not enum limited to known values

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — IDL reflection table for `HTMLInputElement.prototype`, line ~10775)
**Найден:** P2, WPT-VENDOR-html-media-capture, 2026-08-04

## Симптом

`tests/wpt/html-media-capture/capture_reflect.html` (5/6 subtests, 1 FAIL):

```
FAIL input.capture is "" when the capture attribute is invalid value default
  - assert_equals: expected "" but got "invalid"
```

`<input capture='invalid'>`.capture returns the raw string `"invalid"` instead
of the spec-required `""` (limited-to-known-values default for an
unrecognized keyword). The other four subtests (absent → `""`, boolean
`capture` → `""`, `"user"` → `"user"`, `"environment"` → `"environment"`)
already pass by coincidence — a plain string reflection round-trips valid
keywords correctly and returns `""` for a genuinely absent attribute, it only
diverges from spec on an *invalid* keyword.

## Причина

`crates/js/src/dom.rs`'s `HTMLInputElement.prototype` reflection table wires
`capture` as kind `'string'`:

```js
['capture',        'capture',        'string'],
```

The table already supports exactly the needed kind — `'enum'`, used a few
lines above for `type` and elsewhere for `referrerPolicy`/`dir`
(`_lumen_define_reflection`, same file, `kind === 'enum'` branch: lowercases
the attribute value, returns it only if it matches one of `extra.keys`,
else `extra.def`). `capture` just never got switched to it. Fix is one line:

```js
['capture', 'capture', 'enum', { def: '', keys: ['user', 'environment'] }],
```

## Масштаб

1 file, 1/6 subtests in `html-media-capture` (`--processes=4` not needed,
single-process run: 1/2 harness OK, 5/6 subtests passed). The category's
other automatable file, `idlharness.window.html`, TIMEOUT — not a new
finding, it 404s on `/resources/idlharness.js` and `/resources/WebIDLParser.js`,
the already-documented "common/-helpers not vendored" gap (same class noted
for `history`/`browsers` in `WPT-VENDOR-html-browsers`). The remaining 12 of
14 files in the category are `-manual` (100% manual per-file UI prompts, not
selected by `run_report.py`/wptrunner).
