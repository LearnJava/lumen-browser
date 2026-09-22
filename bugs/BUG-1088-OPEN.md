# BUG-1088 — у `AudioContext`/`OfflineAudioContext` нет `renderSizeHint` и `renderQuantumSize`

**Статус:** OPEN
**Тип:** пробел реализации — черновик Web Audio API (`renderSizeHint` в опциях конструктора, атрибут `renderQuantumSize`) в шиме не реализован.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 52, `webaudio`)
**Область:** js — `crates/js/src/web_audio.rs` (`WEB_AUDIO_SHIM`: `AudioContext`, `OfflineAudioContext`, размер кванта рендера зашит как 128)
**Владелец:** P3.

## Симптом

Проба `--dump-layout` (2026-09-22, dev-release от 00:43):

```
new AudioContext().renderQuantumSize                                   -> undefined
new AudioContext({renderSizeHint: 256}).renderQuantumSize              -> undefined
new AudioContext({renderSizeHint: 'bogus'})                            -> не бросает
new OfflineAudioContext({numberOfChannels:1, length:128, sampleRate:44100, renderSizeHint:256}).renderQuantumSize -> undefined
```

В прогоне `webaudio` это даёт 20 сообщений `renderQuantumSize should be exactly N expected (number) N but got (undefined) undefined` и подтесты
`audiocontext-rendersizehint.html` (18 в `.ini`) и `offlineaudiocontext-rendersizehint.html` (17). Файлы `*-rendersizehint*.https.html`
(`analysernode`, `biquadfilter`, `convolver`, `scriptprocessor`, `audioworklet`, `offlineaudiocontext-suspend`, `rendersizehint-smoke-tests`) — `ERROR` на https-origin
([BUG-1069](BUG-1069-OPEN.md)), их вклад появится после починки BUG-1069.

## Ожидание

`renderQuantumSize` — число (по умолчанию 128); `renderSizeHint` — `'default'` | `'hardware'` | положительное целое, невалидное значение бросает `TypeError`;
рендер `OfflineAudioContext` идёт квантами `renderQuantumSize`, а не зашитыми 128.

## Связанное

- [BUG-828](BUG-828-FIXED.md) — pull-граф в квантах по 128 кадров, которому размер кванта придётся сделать параметром.
- `docs/tasks/p2-test-track.md#test-3-срез-52-2026-09-22`.

## Не проверялось

- Какая часть из 35 подтестов двух `.html`-файлов держится только на `renderQuantumSize`, а какая — на смене длины кванта в рендере.
