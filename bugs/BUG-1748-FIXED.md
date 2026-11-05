# BUG-1748 — cue субтитров не рисуется вне живого окна (`--screenshot`, `--ipc-server`, CPU-снимок)

**Статус:** OPEN
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
