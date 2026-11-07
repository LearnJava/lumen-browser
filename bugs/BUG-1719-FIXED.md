# BUG-1719 — `<video controls>` / `<audio controls>`: нет нативной панели управления медиа

**Статус:** FIXED 2026-11-07 (P3)
**Заведён:** 2026-10-26 (по живой проверке, main 677ee5487, Windows 10, dev-release)
**Область:** shell/paint/layout (`crates/shell/src/lumen/media_controls.rs` + события ввода), js (`crates/js/src/video_bindings.rs`, `crates/js/src/audio_element.rs`, шимы `video_element.js` / `web_api_shim_tail_b.js`)

## Симптом

`LUMEN_NO_ADBLOCK=1 target/dev-release/lumen.exe --maximized https://www.w3schools.com/html/html5_video.asp` (или локальный файл с `<video controls src=x.mp4 width=320>`). Первый кадр видео рисуется (H.264 через Media Foundation декодируется: лог `video: загружен nid=… (320×176, 10026мс)`), но панели управления нет вовсе: ни кнопки play/pause, ни временной шкалы, ни громкости/mute, ни fullscreen. Без `autoplay` пользователь не может запустить воспроизведение. Chrome показывает свою панель управления.

## Ожидание

HTML LS §4.8.11.13 «User interface»: при атрибуте `controls` агент пользователя должен показывать UI (play/pause, seek, громкость, mute, fullscreen…). Минимум: нативная панель для `<video controls>` / `<audio controls>` — переключатель play/pause, текущее время/длительность, полоса перемотки, mute; рисуется поверх нижней части бокса видео, клики маршрутизируются в медиаэлемент; без `controls` панель скрыта.

## Диагностика

- Отражение атрибута `controls` есть (`video_bindings.rs`), но на отрисовку и ввод не влияет; UA-теневого дерева у медиаэлементов нет.
- Страничный display-list кэшируется между кадрами, а состояние воспроизведения меняется каждый тик (позиция, пауза) — панель нельзя положить в кэшируемый список. У полосы прокрутки та же проблема, и решается она так же: overlay-буфер, дополняемый шеллом на каждом RedrawRequested.
- Для `<video>` состояние уже живёт в `VideoGifStore::playback` (сечения декода пишут его на стороне шелла), для `<audio>` декод-сессии у шелла нет вообще — provider-сторона аудио живёт только в JS-модели (`audio_element.rs`), и общий store для аудио никто не заполнял.
- Клик по панели обязан не доходить до страницы (mousedown/click/drag-select), а мутации play/pause обязаны порождать события `play`/`pause` с правильным `wasPaused` — значит шелл не имеет права мутировать модель напрямую: если он поставит `paused` в store до вызова JS, шимовский `el.pause()` увидит «уже на паузе» и пропустит событие.

## Исправление

Срезы 1+2 (рендер и клики) + срез 3 (drag seek, громкость, fullscreen).

- **`crates/shell/src/lumen/media_controls.rs`** — вся панель: `panel_ui` (раскладка по ширине бара: play слева; при ≥190px — время; при ≥330px — громкость; mute; fullscreen справа при ≥148px; seek-полоса занимает остаток, при <24px панель не строится), `emit_panel` (фон, дорожки seek/громкости с заливкой доли, иконки, текст времени), hit-зоны `MediaHit` (дорожки с вертикальным запасом ±10px под палец). Press перехватывается до DOM: `handle_media_panel_click` разбивает по зонам; play/pause идёт **только** через JS-хук `__lumen_media_ui_toggle` (паритет событий), mute/громкость/seek пишут store напрямую и уведомляют JS хуками `__lumen_media_ui_muted/volume/seek`. Drag: `MediaDrag` в `AppState`, `cursor_moved.rs` тянет дорожку, `mouse_input.rs` на release фиксирует финальную позицию (для аудио — хуком seek), переключение вкладки/навигация сбрасывают drag. `toggle_media_fullscreen`: `winit Fullscreen::Borderless` + ручка `_lumen_notify_fullscreen_enter/exit` для синхронизации JS-состояния.
- **`crates/shell/src/lumen/state.rs`, `window_mode.rs`** — поля `media_panel_hits` / `media_panel_drag`.
- **`crates/js/src/shim/video_element.js`** — хуки `__lumen_media_ui_toggle/seek/volume/muted` на элементе: каждый проходит через то же свойство/метод, что вызвал бы скрипт (`el.play()`/`el.pause()`, `el.currentTime`, `el.volume`, `el.muted`), поэтому провайдер, модель элемента и события остаются согласованными.
- **`crates/js/src/audio_element.rs`** — те же четыре хука плюс натив `__lumen_audio_ui_sync(nid, pos, dur, paused, now)`: write-through provider-состояния в общий `VideoGifStore::playback` (ключ — `el.__nid__`, тот же, что шелл берёт из `b.node.index()`); шим вызывает его на тиках pump/tupdate, которые уже опрашивает провайдер. Пауза пишется без ре-якоря эпохи (позиция — истина провайдера), wake шелла — только на переходе пауза↔play или пока идёт воспроизведение, иначе paused-элемент будил бы перерисовку на каждом тике.
- **`crates/js/src/shim/web_api_shim_tail_b.js`** — `_lumen_notify_fullscreen_enter(nid)`: шелл инициирует fullscreen не через JS `requestFullscreen`, поэтому атрибут `data-lumen-fullscreen` (мачится псевдоклассом `:fullscreen`), `fullscreen_nid` и событие `fullscreenchange` выставляются той же функцией, что и для JS-пути, с вытеснением предыдущего fullscreen-элемента.

Ограничения (задокументированы в `CAPABILITIES.md`): `<video>` внутри `<iframe>` — BUG-1720 (панель не достроится поверх чужого бокса); fullscreen растягивает окно, а не сам элемент — UA-позиционирование `:fullscreen` (CSS Fullscreen §5, top-layer) отдельная работа; во время drag курсорные события странице не доходят, вне drag — проходят мимо панели (кроме зон самих кнопок).

## Проверка

- `cargo test -p lumen-shell media_controls` — 8/8: раскладка на широком/среднем (300px, без громкости)/узком (120px) баре, порядок hit-зон, вертикальный запас дорожек, 8 контуров fullscreen-иконки.
- `cargo test -p lumen-js --features v8-backend --lib` — см. итог прогона в коммите: хуки video (`media_ui_hooks_drive_video_element_model`), хуки audio + write-through store (`media_ui_hooks_drive_audio_element_model`, `ui_sync_mirrors_provider_state_into_playback_store`), регрессия load-событий (`load_events_complete_once_when_ready`: первая версия syncPanelStore бросала ReferenceError по отсутствующему `nid`, `_lumen_pump_media` молча выкидывал такой pump по `catch (e) { keep = false }`, и `loadeddata`/`canplay` переставали приходить).
- Полный graphic-прогон + goldens — в коммите.
