# BUG-708: Web Audio node/param properties perform no WebIDL validation at all — invalid values are silently accepted instead of throwing or being ignored

**Статус:** OPEN
**Компонент:** js (`crates/js/src/web_audio.rs` — `WEB_AUDIO_SHIM`)
**Найден:** WPT-VENDOR-webaudio (`ROADMAP.md`), живая проба через `--mcp-live-port`

## Симптом

Several `AudioNode` properties that the spec defines as either range-checked
(throw `IndexSizeError`/`NotSupportedError` on an invalid value) or
WebIDL-enum-typed (silently *ignore* an unrecognized string, keeping the old
value) are implemented in the shim as plain, unchecked data fields — any
value assigned is stored verbatim:

* `AnalyserNode.fftSize` (`web_audio.rs:267`, `this.fftSize = opts.fftSize ?
  ... : 2048`, no setter) — spec requires a power of 2 in `[32, 32768]`,
  else `IndexSizeError`. Live probe: `an.fftSize = -1` → no throw, `fftSize`
  reads back `-1`; `an.fftSize = 3` → no throw, reads back `3`.
* `ConvolverNode.buffer` (`web_audio.rs:428`, plain field) — spec requires
  `NotSupportedError` for a buffer whose channel count isn't 1, 2, or 4.
  Live probe: assigning a 3-channel `AudioBuffer` does not throw.
* `AudioNode.channelCountMode`/`channelCount`/`channelInterpretation`
  (`web_audio.rs:115-117`, all plain fields set once in the constructor,
  no `Object.defineProperty` at all) — `channelCountMode` and
  `channelInterpretation` are WebIDL enums; assigning an unrecognized
  string must be a no-op that leaves the previous value in place. Live
  probe: `gain.channelCountMode = 'foobar'` after `= 'max'` → reads back
  `'foobar'`, not `'max'`.

Notably this is *not* uniform across the file: `OscillatorNode.type`
(`web_audio.rs:181-188`) and `BiquadFilterNode.type`
(`web_audio.rs:246-253`) already use `Object.defineProperty` with an
allow-list check that throws `InvalidStateError` on an unrecognized value —
so the pattern for "validate an enum-like setter" exists and is applied
inconsistently, not absent as a matter of Phase-0 design. `CAPABILITIES.md`'s
"Web Audio (graph only, no DSP)" caveat covers audio-processing correctness,
not property-setter validation, so this isn't the already-documented gap.

## Как воспроизвести

```js
var ac = new AudioContext();
var an = ac.createAnalyser();
an.fftSize = -1; an.fftSize;               // -1, expected: throw IndexSizeError

var conv = ac.createConvolver();
conv.buffer = ac.createBuffer(3, 100, 44100);  // no throw, expected: NotSupportedError

var g = ac.createGain();
g.channelCountMode = 'max';
g.channelCountMode = 'foobar';
g.channelCountMode;                        // "foobar", expected: still "max"
```

## Масштаб в WPT

`WPT-VENDOR-webaudio` run: 25 `FAIL Setting fftSize to N did not throw`, 29
`FAIL ConvolverNode with buffer of N channels did not throw`, 4 `FAIL
node.channelCountMode/channelInterpretation after invalid setter is not
equal to <old value>. Got foobar.`, plus related `setValueCurveAtTime`/
`AudioBuffer` constructor "did not throw" clusters from the same
no-validation pattern on `AudioParam`/`AudioBuffer`.

## Дальше

Convert the affected fields to `Object.defineProperty` accessors following
the existing `type` pattern on `OscillatorNode`/`BiquadFilterNode`: range
+ power-of-2 check for `fftSize`, enum allow-list (silently ignore, don't
throw) for `channelCountMode`/`channelCount`/`channelInterpretation`, and a
channel-count check in `ConvolverNode.buffer`'s setter.
