# BUG-707: `ConstantSourceNode`/`IIRFilterNode` are entirely absent from the Web Audio shim; `createMediaStreamDestination()` returns a plain `AudioNode`, not a `MediaStreamAudioDestinationNode`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/web_audio.rs` — `WEB_AUDIO_SHIM`)
**Найден:** WPT-VENDOR-webaudio (`ROADMAP.md`), живая проба через `--mcp-live-port`

## Симптом

Three spec-mandated `AudioNode` subtypes are missing or misrepresented:

1. **`ConstantSourceNode`** — the global constructor and
   `BaseAudioContext.prototype.createConstantSource` are both entirely absent.
   The module doc comment at `web_audio.rs:6-9` lists the supported `AudioNode`
   subclasses and does not include it — this is not an oversight in the shim's
   own bookkeeping, it was simply never added.
2. **`IIRFilterNode`** — same: no global constructor, no
   `createIIRFilter(feedforward, feedback)` factory. Also absent from the
   doc-comment list.
3. **`MediaStreamAudioDestinationNode`** — the global constructor is absent,
   but unlike the first two, the factory method
   `AudioContext.prototype.createMediaStreamDestination` (`web_audio.rs:595`)
   *does* exist and returns a plain `AudioNode` instance with an ad-hoc
   `.stream` object (`{ id, active, getTracks() }`) instead of a
   `MediaStreamAudioDestinationNode`. The returned stream stub is also
   incomplete — it has `getTracks()` but not `getAudioTracks()`, which every
   caller that follows the spec's typical `dest.stream.getAudioTracks()[0]`
   pattern throws on.

Live probe (`--mcp-live-port`, `about:blank` navigated to a page with one
`<script>`, per the shim's live-eval gotcha) confirmed all three directly:

```json
{
  "ConstantSourceNode": "undefined",
  "IIRFilterNode": "undefined",
  "MediaStreamAudioDestinationNode": "undefined",
  "createConstantSource": "undefined",
  "createIIRFilter": "undefined",
  "createMediaStreamDestination": "function"
}
```

## Как воспроизвести

```js
var ac = new AudioContext();
typeof window.ConstantSourceNode;       // "undefined" — should be "function"
typeof ac.createConstantSource;         // "undefined" — should be "function"
typeof window.IIRFilterNode;            // "undefined" — should be "function"
typeof ac.createIIRFilter;              // "undefined" — should be "function"
ac.createMediaStreamDestination().stream.getAudioTracks;  // undefined — should be a function
```

## Масштаб в WPT

`WPT-VENDOR-webaudio` run (`--all --root webaudio --recursive --processes=4`,
321 harness ids, 205/321 OK): 84 occurrences of
`ReferenceError: ConstantSourceNode is not defined` / `.createConstantSource is
not a function`, 49 of `IIRFilterNode`/`.createIIRFilter`, 6 of
`MediaStreamAudioDestinationNode is not defined` — together the single
largest cluster of harness-level failures in the category (poisons every
subsequent assertion in each affected test file, not just the one directly
touching the missing API).

## Дальше

Add both node types (`ConstantSourceNode` with an `offset` `AudioParam`,
`IIRFilterNode` with `getFrequencyResponse`) following the existing pattern
used by `GainNode`/`BiquadFilterNode` in the same file, plus their
`BaseAudioContext.prototype.create*` factories. For
`MediaStreamAudioDestinationNode`, promote `createMediaStreamDestination`'s
ad-hoc object to a real subclass of `AudioNode` (mirroring
`MediaStreamAudioSourceNode` at `web_audio.rs:545`) and add
`getAudioTracks()` to the returned stream stub. This is additive-only —
doesn't require the DSP pipeline these tests otherwise depend on (Phase 0,
CAPABILITIES.md "Web Audio (graph only, no DSP)"), since these are pure
graph-node/constructor gaps, not audio-processing correctness gaps.
