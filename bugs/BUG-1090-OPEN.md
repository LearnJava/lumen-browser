# BUG-1090 — конструкторы аудио-узлов ничего не валидируют: `new AnalyserNode()` без контекста не бросает, невалидные опции принимаются

**Статус:** OPEN
**Тип:** пробел реализации — WebIDL-проверка аргументов конструкторов `AudioNode`-наследников отсутствует.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 52, `webaudio`)
**Область:** js — `crates/js/src/web_audio.rs` (`WEB_AUDIO_SHIM`, конструкторы узлов: `opts = opts || {}` и присвоение полей без проверки)
**Владелец:** P3.

## Симптом

Проба `--dump-layout` (2026-09-22):

```
new AnalyserNode()                                     -> не бросает   (ожидается TypeError: первый аргумент BaseAudioContext обязателен)
new AnalyserNode(1)                                    -> не бросает   (ожидается TypeError: 1 не BaseAudioContext)
new PannerNode(new AudioContext(), {refDistance: -1})  -> не бросает   (ожидается RangeError)
```

В прогоне `webaudio` по файлам `ctor-*.html` — 197 подтестов в `.ini` (20 файлов), из них с текстом `did not throw`: `ctor-panner` 21/44, `ctor-analyser` 13/20,
`ctor-audiobuffer` 11/15, `ctor-offlineaudiocontext` 9/21, `ctor-oscillator` 9/15, `ctor-stereopanner` 9/14, `ctor-waveshaper` 7/12, `ctor-delay` 7/12. Все сообщения — `assert_true: expected true got false`
из `should_throw`-обёртки `audit`-хелпера, поэтому по тексту причина не видна — проба выше даёт её напрямую.

## Ожидание

Конструктор `AudioNode`-подкласса бросает `TypeError` без `BaseAudioContext` первым аргументом и `TypeError`/`RangeError`/`InvalidStateError`/`NotSupportedError` по таблицам
опций каждого интерфейса (диапазоны `refDistance`, `maxDistance`, `coneOuterGain`, `fftSize`, `delayTime`…).

## Связанное

- [BUG-708](BUG-708-OPEN.md) — то же по сеттерам свойств (`fftSize`, `channelCountMode`, `ConvolverNode.buffer`); конструкторы там не покрыты, но чинить их разумно вместе — общий набор проверок.
- `docs/tasks/p2-test-track.md#test-3-срез-52-2026-09-22`.

## Не проверялось

- Разбивка остальных подтестов `ctor-*` (не `did not throw`): часть — дефолты и отсутствующие члены (`IIRFilterNode`, [BUG-707](BUG-707-OPEN.md); `renderSizeHint`, [BUG-1088](BUG-1088-OPEN.md)).
