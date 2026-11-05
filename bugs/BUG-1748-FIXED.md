# BUG-1748 — cue субтитров не рисуется вне живого окна (`--screenshot`, `--ipc-server`, CPU-снимок)

**Статус:** FIXED 2026-11-05 (P2; срез VTT-RENDER-S5, ветка `zs1-vtt-render-s5`)
**Заведён:** 2026-10-27 (P2, WPT-VTT-SCOUT)
**Область:** shell (`crates/shell/src/app/window_event/redraw_requested.rs:1172` — `tracks::build_cue_overlay`; `crates/shell/src/lumen/automation.rs:430` `render_current_page_to_png`; `dump_mode.rs`)

## Симптом

Страница с `<video>` + `<track default>` и активным cue (`video.textTracks[0].activeCues.length === 1`) в снимке `lumen --screenshot` / через `--ipc-server` даёт пустое видео без текста cue. В живом окне тот же cue виден.

Причина: оверлей cue собирается только в обработчике кадра окна — `tracks::build_cue_overlay(...)` дописывает команды в display list непосредственно перед GPU-кадром (`redraw_requested.rs:1172`). Конвейер снимка (`render_current_page_to_png` растеризует `self.display_list`, однократный путь `--screenshot` в `dump_mode.rs`) этот шаг не вызывает.

## Как найдено

WPT-VTT-SCOUT (`docs/wpt-vendor-notes/webvtt.md`): все 229 FAIL-рефтестов `webvtt/rendering/` — снимок теста с пустым видео, 221 из них побайтно одинаковы. Пробник: в конвейере снимка `activeCues.length=1`, но cue не нарисован.

## Что делать

1. Вынести добавление оверлея cue из `redraw_requested.rs` в общий шаг построения display list, который проходят и окно, и `render_current_page_to_png`, и `--screenshot`/`--dump-display-list`.
2. Долгосрочно оверлей заменяется CSS-боксами WebVTT §7.2 (строка `VTT-RENDER` в `ROADMAP.md`); этот баг — минимальный шаг, чтобы конвейер снимка вообще видел cue.

## Как проверить

`lumen --screenshot out.png tests/wpt/webvtt/rendering/cues-with-video/processing-model/basic.html` — текст cue на снимке; `run_corpus.py --prefixes webvtt/rendering/cues-with-video/processing-model --processes 6` — снимки теста перестают быть одинаковыми.

## Что сделано (VTT-RENDER-S5)

Пункт 1 выполнен длинным путём из п. 2 — оверлея больше нет вообще:

1. Шим (`crates/js/src/shim/video_element.js`) собрал активные cue всех `mode === 'showing'`-треков в JSON (`renderCues`/`pushCueRender`, коалесценция через `setTimeout(…, 0)`, дедуп по строке снимка); `mode` стал аксессором на `TextTrack` (значение в `_mode`), перерисовка — из `checkCueChanges`, смены `mode` и после каждой догрузки cue (`appendCues`).
2. Новый натив `__lumen_video_render_cues(nid, json)` (`crates/js/src/video_bindings.rs`) разбирает JSON в `Vec<CueRender>` и зовёт `Document::set_vtt_cues` (поддерево cue в UA-тени, срез VTT-RENDER-S3), помечая документ изменённым, как соседние DOM-нативы (`DomTouched::unattributed` + флаги relayout/flush-stale) — cue попадает в `self.display_list`, который растеризуют и окно, и `render_current_page_to_png`, и `--screenshot`/`--dump-display-list`.
3. Оверлей удалён: `build_cue_overlay`/`collect_video_rects`/`PageTracks::cues_by_video`/`is_empty` в `crates/shell/src/tracks.rs`, блок в `redraw_requested.rs`, мёртвые `active_cues`/`strip_cue_markup`/`resolve_cue_box`/`CueBox`/`CueTextAlign` в `crates/engine/dom/src/vtt.rs`.

Проверено: `cargo test -p lumen-js --features v8-backend vtt_` (мост: native строит поддерево из двух cue, шим зовёт натив один раз на смену состояния и с пустым списком на выходе из интервала), `--dump-display-list` страницы с cue содержит `DrawText` с текстом cue и `FillRect`-подложку, снимок страницы с `<track>` не равен снимку без него.
