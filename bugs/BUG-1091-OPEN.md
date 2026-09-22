# BUG-1091 — `PannerNode`, `ConvolverNode`, `DynamicsCompressorNode` в офлайн-рендере пропускают вход без обработки

**Статус:** OPEN
**Тип:** пробел реализации — DSP-часть трёх узлов не написана; «осознанный остаток» [BUG-828](BUG-828-FIXED.md), под который задачи не заведено.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 52, `webaudio`)
**Область:** js — `crates/js/src/web_audio.rs` (шапка модуля: «**Not rendered:** `DynamicsCompressorNode`, `PannerNode`, `ConvolverNode` and `AudioWorkletNode` pass their input through unchanged»)
**Владелец:** P3.

## Симптом

BUG-828 перевёл `OfflineAudioContext.startRendering()` на настоящий pull-граф в квантах по 128 кадров, но три узла остались пропусканием (`BUG-828-FIXED.md`, «осознанный остаток, а не забытая ветка»).
Отдельного бага или роадмап-задачи на остаток нет. Замер `webaudio` (2026-09-22):

- `the-pannernode-interface/distance-{linear,inverse,exponential}.html` — по 103 подтеста в `.ini` (309), все вида `X 1 is not close to 0.7071067811865476 within a relative error of 0.000002272 … Got 1.` —
  выход панера с ненулевым расстоянием равен входу (усиление 1), модель расстояния не применяется.
- Тесты `the-convolvernode-interface`/`the-dynamicscompressornode-interface`, проверяющие числа на выходе, падают, по-видимому, тем же образом; в срезе по ним не разбирались (в `.ini` они смешаны с [BUG-708](BUG-708-OPEN.md)).

## Ожидание

`PannerNode` — модель расстояния (`linear`/`inverse`/`exponential`), конус, `equalpower`/`HRTF`-панорама; `ConvolverNode` — свёртка с `buffer`/`normalize`; `DynamicsCompressorNode` — сжатие с `reduction`.

## Связанное

- [BUG-779](BUG-779-OPEN.md) — `AudioWorkletNode` пропускает вход по другой причине (`addModule()` — no-op).
- [BUG-708](BUG-708-OPEN.md) — валидация тех же узлов.
- `docs/tasks/p2-test-track.md#test-3-срез-52-2026-09-22`.

## Не проверялось

- Доля остальных `the-pannernode-interface` (`panner-automation-basic` 49 подтестов) и `the-convolvernode-interface` в этом дефекте — по файлам не разбиралось.
