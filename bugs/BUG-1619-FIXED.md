# BUG-1619 — Медиа-запросы возможностей отвечают «да» без декодера

**Статус:** FIXED 2026-10-18 (P3)
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** js/core (`crates/js/src/media_capabilities.rs:33`, `core/src/ext.rs:5227`, `audio_element.rs:713`)

## Симптом и место в коде

navigator.mediaCapabilities.decodingInfo/encodingInfo на любой конфиг со строковым type резолвят {supported:true, smooth:true, powerEfficient:false} (media_capabilities.rs:31-53). contentType, type 'media-source' (MSE в движке нет) и keySystemConfiguration не проверяются. Это противоречит собственным ответам движка: MediaRecorder.isTypeSupported → false (media_stream_recording.rs:187-188), WebCodecs isConfigSupported → false. Строка WPT-VENDOR-media-capabilities описывает это как «явную Phase 0 заглушку» (10/94 сабтестов), бага нет. <audio>.canPlayType идёт через дефолтную реализацию трейта (ext.rs:5227-5233: PlatformAudioPlayer её не переопределяет, audio_player.rs:293) или через JS-дубль без провайдера (audio_element.rs:713-723). Ответ 'probably' для audio/ogg, audio/wav, audio/flac даётся без учёта параметра codecs, поэтому audio/ogg;codecs=opus тоже получает 'probably'. Для audio/webm, audio/aac и audio/mp4 ответ 'maybe', хотя rodio собран только с wav/mp3/flac/vorbis (shell/Cargo.toml:139), то есть Opus/AAC/MP4/WebM декодировать нечем. <video>.canPlayType в штатной сборке честен (см. CANPLAY-VIDEO). Чинить одной таблицей возможностей — поэтому это один баг, а не два. По домену баги ведёт P3, но его нет в списке ролей, поэтому указан P6.

## Где смотреть

- crates/js/src/media_capabilities.rs:31-53
- crates/js/src/media_stream_recording.rs:186-188
- crates/core/src/ext.rs:5223-5233
- crates/js/src/audio_element.rs:713-723
- crates/shell/src/platform/audio_player.rs:9-15
- crates/shell/src/platform/audio_player.rs:293
- crates/shell/Cargo.toml:139
- ROADMAP.md WPT-VENDOR-media-capabilities

## Объём и приёмка

Баг. Сейчас mediaCapabilities.decodingInfo/encodingInfo всегда отдают {supported:true, smooth:true} (crates/js/src/media_capabilities.rs:33-53), в т.ч. type 'media-source' без MSE; audio canPlayType даёт 'maybe' на audio/webm|aac|mp4 и 'probably' на audio/ogg;codecs=opus (crates/core/src/ext.rs:5227, дубль audio_element.rs:713), а rodio собран только с wav/mp3/flac/vorbis (crates/shell/Cargo.toml:139). Объём: общая таблица реальных декодеров с учётом codecs. Приёмка: decodingInfo media-source → supported:false; canPlayType('audio/aac') === ''; canPlayType('audio/ogg; codecs=vorbis') === 'probably'. Источник: docs/research/site-compat/report.md §8.1; areas/04

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.

## Исправление

Единая таблица декодеров — `lumen_core::ext::audio_can_play_type` (wav/mp3/flac/vorbis, как собран rodio; учитывает `codecs`). Её используют дефолт `AudioPlaybackProvider::can_play_type`, нативный `__lumen_audio_can_play_type` (без провайдера тоже) и новый `__lumen_media_audio_can_play` для `navigator.mediaCapabilities`. JS-дубль в `audio_element.rs` удалён. `decodingInfo`: `media-source`, `keySystemConfiguration` и контейнеры без декодера (аудио — таблица, видео — `__lumen_video_can_play_type`) → `supported:false`; `encodingInfo` → `supported:false` (кодировщиков нет); конфиг без `audio`/`video` или с неизвестным `type` → `TypeError`. Тесты: `crates/core/src/ext.rs` (`default_can_play_type_*`), `crates/js/src/media_capabilities.rs`.
