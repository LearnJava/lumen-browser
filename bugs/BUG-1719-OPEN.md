# BUG-1719 — `<video controls>` / `<audio controls>`: нет нативной панели управления медиа

**Статус:** OPEN
**Заведён:** 2026-10-26 (по живой проверке, main 677ee5487, Windows 10, dev-release)
**Область:** shell/paint/layout (UA-панель управления медиаэлемента; `crates/js/src/video_bindings.rs`, `crates/shell/src/video_tick.rs`)

## Симптом

`LUMEN_NO_ADBLOCK=1 target/dev-release/lumen.exe --maximized https://www.w3schools.com/html/html5_video.asp` (или локальный файл с `<video controls src=x.mp4 width=320>`). Первый кадр видео рисуется (H.264 через Media Foundation декодируется: лог `video: загружен nid=… (320×176, 10026мс)`), но панели управления нет вовсе: ни кнопки play/pause, ни временной шкалы, ни громкости/mute, ни fullscreen. Без `autoplay` пользователь не может запустить воспроизведение. Chrome показывает свою панель управления.

## Ожидание

HTML LS §4.8.11.13 «User interface»: при атрибуте `controls` агент пользователя должен показывать UI (play/pause, seek, громкость, mute, fullscreen…). Минимум: нативная панель для `<video controls>` / `<audio controls>` — переключатель play/pause, текущее время/длительность, полоса перемотки, mute; рисуется поверх нижней части бокса видео, клики маршрутизируются в медиаэлемент; без `controls` панель скрыта.

## Факты по коду

- `grep -rn -iE "media_controls|video.{0,40}controls"` по `crates/` ничего содержательного не находит; у UA-теневого дерева видео нет слота (`CAPABILITIES.md`, строка ~35).
- Отражение атрибута `controls` есть: `crates/js/src/video_bindings.rs` (тест `controls_and_loop_write_through_to_the_content_attribute`) — но на отрисовку и ввод оно не влияет.
- Нативы состояния воспроизведения: `__lumen_video_play` / `__lumen_video_pause` / `__lumen_video_seek` в `crates/js/src/video_bindings.rs:144-184`; кадры на стороне шелла — `crates/shell/src/video_tick.rs::tick_video_sessions`.

## Заметки

Размер — фича, а не точечный дефект; при взятии в работу, вероятно, разбить на три задачи: (1) отрисовка панели (display list поверх бокса видео, состояние play/paused/время), (2) обработка ввода (hit-test по панели, клик → play/pause/mute), (3) seek-полоса и громкость (drag, `currentTime`/`volume`/`muted`). Для аудио без видео-бокса (`<audio controls>`) нужен собственный бокс панели.
