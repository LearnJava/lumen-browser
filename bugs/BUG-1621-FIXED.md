# BUG-1621 — MediaRecorder изображает запись и отдаёт пустой Blob

**Статус:** FIXED 2026-10-18 (P3)
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** js (`crates/js/src/media_stream_recording.rs:82`)

## Симптом и место в коде

Найдено сверх исследования. MediaRecorder объявлен: присутствие вернул BUG-549 ради проверок наличия, установка — v8_runtime.rs:1111. isTypeSupported всегда возвращает false (media_stream_recording.rs:187-188). При этом конструктор принимает любой mimeType, хотя по спецификации неподдержанный тип должен давать NotSupportedError (:82-89). start() переводит в 'recording' и шлёт событие start (:129-137). stop() шлёт dataavailable с Blob из _chunks (:147), а _chunks нигде не наполняется (:96, :135, :150) — Blob всегда 0 байт. getUserMedia({audio}) при этом рабочий (CAPABILITIES.md:172), так что запись голоса изображает успех и отдаёт пустой файл. Это ложный успех в реализованном коде, поэтому баг. По домену баги ведёт P3, его нет в списке ролей, поэтому указан P6.

## Где смотреть

- crates/js/src/media_stream_recording.rs:1-7
- crates/js/src/media_stream_recording.rs:82-96
- crates/js/src/media_stream_recording.rs:129-150
- crates/js/src/media_stream_recording.rs:180-188
- crates/js/src/v8_runtime.rs:1111
- CAPABILITIES.md:172

## Объём и приёмка

Баг (сверх исследования). Сейчас MediaRecorder.isTypeSupported всегда false (crates/js/src/media_stream_recording.rs:187), но конструктор принимает любой mimeType (:82-89), start() переводит в 'recording' и шлёт start (:129-137), а stop() отдаёт dataavailable с пустым Blob (:147): запись голоса с рабочего getUserMedia({audio}) молча даёт файл 0 байт. Объём: неподдержанный mimeType в конструкторе и start() без кодировщика → NotSupportedError (или не объявлять MediaRecorder — отмена BUG-549, тогда срез COMPAT-API-01). Приёмка: после start/stop нет dataavailable с size 0; baseline WPT mediacapture-record. Источник: docs/research/site-compat/report.md §8.1 (класс), §11 п.4

## Исправление

Конструктор `MediaRecorder` бросает `NotSupportedError`, если mimeType не входит в `SUPPORTED_TYPES` (сейчас пуст — кодировщиков нет), а при пустом mimeType — если нет ни одного типа по умолчанию. Поэтому «записывающий» объект с пустым Blob создать нельзя. `isTypeSupported` читает тот же список. Объявление `MediaRecorder` сохранено (проверки наличия, BUG-549). Тесты: `media_stream_recording::tests::constructor_*`.

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
