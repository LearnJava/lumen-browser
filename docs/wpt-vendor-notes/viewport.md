# WPT vendor notes — `viewport`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/viewport/`, 3 файла: `META.yml`, `WEB_FEATURES.yml`, `viewport-segments.html` — апстримная категория состоит из ровно одного теста). `run_report.py --all --root viewport --recursive` (~20 с) — **1/1 harness OK, 0/1 сабтестов**. Единственный тест целиком про `viewport.segments` (глобальный объект Viewport Segments API, `drafts.csswg.org/css-viewport-1/`, черновик для складных устройств) — `ReferenceError: viewport is not defined`, тот же класс «API отсутствует целиком, ожидаемо», что `navigator.vibrate` в `vibration`/`requestVideoFrameCallback` в `video-rvfc`. Несмотря на пометку ⬜ у самой категории, весь её реальный контент — материал складных устройств, тот же скоуп, что уже 🚫-категория `viewport-segments` (строка ниже). Новый номер бага не заводился
