# BUG-1089 — у `AudioContext` нет `sinkId`/`setSinkId()`/`AudioSinkInfo` и `playbackStats`/`playoutStats`

**Статус:** OPEN
**Тип:** пробел реализации — члены `AudioContext` из Web Audio 1.1 (выбор устройства вывода, статистика воспроизведения) отсутствуют.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 52, `webaudio`)
**Область:** js — `crates/js/src/web_audio.rs` (`WEB_AUDIO_SHIM`, `AudioContext`)
**Владелец:** P3.

## Симптом

Проба `--dump-layout` (2026-09-22): `typeof new AudioContext().sinkId`, `.setSinkId`, `.playoutStats`, `.playbackStats`, `typeof AudioSinkInfo`, `typeof AudioPlaybackStats` — всё `undefined`;
`new AudioContext({sinkId: ''}).sinkId` -> `undefined`.

В прогоне `webaudio`:

- `audiocontext-playoutstats.html` (прежнее имя атрибута; рядом лежит `audiocontext-playbackstats.html` с новым именем `playbackStats` — в `.ini` 4 подтеста `FAIL`) — `Cannot read properties of undefined (reading 'totalFramesDuration')` (4) и `(reading 'totalDuration')` (4): восемь `promise_test` падают из-за отсутствующего `playoutStats`.
- `audiocontext-sinkid-{constructor,setsinkid,state-change}.https.html`, `context-time-monotonic-on-setsinkid.https.html`,
  `setSinkId-with-MediaElementAudioSourceNode.https.html` — `ERROR` на https-origin ([BUG-1069](BUG-1069-OPEN.md)); отсутствие `setSinkId` подтверждено пробой, но вклад этих файлов
  в число подтестов не измерен и станет виден только после BUG-1069.

## Ожидание

`AudioContext.sinkId` (строка или `AudioSinkInfo`), `setSinkId(string | AudioSinkOptions)` -> `Promise`, событие `sinkchange`, `AudioContext.playbackStats` (прежнее имя — `playoutStats`; `AudioPlaybackStats`:
`underrunDuration`, `underrunEvents`, `totalDuration`, `averageLatency`, `minimumLatency`, `maximumLatency`, `resetLatency()`, `toJSON()`).

## Связанное

- Отдельное от [BUG-1088](BUG-1088-OPEN.md) — другой набор членов того же интерфейса.
- `docs/tasks/p2-test-track.md#test-3-срез-52-2026-09-22`.

## Не проверялось

- Что вернёт `setSinkId` на машине без устройств вывода (`NotFoundError`/`NotAllowedError`) — зависит от политики разрешений, которой у шима нет.
