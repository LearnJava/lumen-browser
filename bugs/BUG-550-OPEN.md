# BUG-550: `audio_bindings.rs`'s richer AudioContext (extra node types + ADR-007 anti-fingerprint noise) was already dead code under QuickJS, before the V8 migration — not a V8 regression, a pre-existing install-order shadow bug

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/audio_bindings.rs` vs `crates/js/src/web_audio.rs`)
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

Two separate modules both install a global `AudioContext`/`OfflineAudioContext`
into the **same** JS context, in `QuickJsRuntime::install_dom`, in this
order:

1. `audio_bindings::install_audio_bindings` (`lib.rs:766`) — the richer
   implementation: `BaseAudioContext`/`AudioContext`/`OfflineAudioContext`
   plus `ConvolverNode`/`WaveShaperNode`/`IIRFilterNode`/
   `ChannelSplitterNode`/`ChannelMergerNode`/`MediaStreamAudioSourceNode`/
   `MediaStreamAudioDestinationNode`/`AudioWorklet` stub/`AudioListener`,
   and per-session LCG noise on `getChannelData`/`copyFromChannel`/
   `getFloatFrequencyData` (ADR-007 Layer 4, 9D.3 anti-fingerprinting).
2. `web_audio::install_web_audio_api` (`lib.rs:1235`) — the simpler Phase-0
   implementation: `AudioContext`/`OfflineAudioContext`/`AudioBuffer`/
   `AudioParam` + `GainNode`/`OscillatorNode`/`AudioBufferSourceNode`/
   `BiquadFilterNode`/`AnalyserNode`/`DelayNode`/`DynamicsCompressorNode`/
   `StereoPannerNode`/`PannerNode`/`AudioDestinationNode`/
   `MediaElementAudioSourceNode`. No anti-fingerprint noise.

Both shims attach via `globalThis.AudioContext = AudioContext` (not a
lexical `class` declaration), so the second `ctx.eval` call **silently
overwrites** the first's globals rather than throwing a redeclaration
`SyntaxError`. Since `web_audio` installs *after* `audio_bindings` in the
same install path, `audio_bindings`'s richer implementation — including the
ADR-007 audio-fingerprint noise — was **already unreachable under QuickJS**,
before the V8 cutover (ADR-018, 2026-07-14) ever happened. Confirmed: `web_audio`
is the one with a V8 port (`install_web_audio_api_v8`, `v8_runtime.rs:4274`);
`audio_bindings` has none.

## Симптом

This is *not* a V8-migration regression — the shadowing predates it. It
means: (1) `ConvolverNode`/`WaveShaperNode`/`IIRFilterNode`/
`ChannelSplitterNode`/`ChannelMergerNode`/`MediaStreamAudioSourceNode`/
`MediaStreamAudioDestinationNode`/`AudioWorklet` have never actually been
reachable from a real page on either engine, despite ~1120 lines and 29
tests exercising them in isolation; and (2) the ADR-007 anti-audio-
fingerprinting noise (`getChannelData` et al.) has never actually applied
to a real page's Web Audio usage either — a privacy-layer feature that has
been silently inert since it was written. `CAPABILITIES.md`'s Media/Devices
bullet already (correctly, if unwittingly) describes only the weaker
reality: "Web Audio (graph only, no DSP)" — no overclaim to fix there.

## Фикс (не сделан)

Two independent decisions, out of scope for a single fix:
1. **S12b disposition** (in scope for the cleanup queue): `audio_bindings.rs`
   is dead code on both engines — delete it outright rather than port it
   (no functional loss, since it was never reachable). Removes it from the
   S12b-G7 port slot in `docs/tasks/p1-s12b-cleanup-queue.md` §4 — done in
   this commit.
2. **Feature gap** (not in scope for S12b, needs its own decision): if the
   missing node types and/or the ADR-007 audio-fingerprint noise are still
   wanted, they need to be merged into `web_audio.rs` (the surviving,
   V8-ported module) as new functionality — not simply re-enabled, since
   they were never live to begin with.
