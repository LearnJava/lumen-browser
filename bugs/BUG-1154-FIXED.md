# BUG-1154: `@font-face` из `<style>`, вставленного скриптом после загрузки, никогда не загружается

**Статус:** FIXED 2026-09-30
**Дата:** 2026-09-25
**Компонент:** shell (`crates/shell/src/relayout.rs::refresh_dynamic_css` пересобирает
`Stylesheet`, но не порождает загрузку новых `@font-face url()`; загрузка шрифтов
запускается только из `page_load.rs` по `page.pending_web_fonts` первичной сборки)
**Найден:** P3 2026-09-25, при закрытии [BUG-1021](BUG-1021-FIXED.md)

## Симптом

`tests/wpt/css/fetching/fetch-resources.sub.html`, подтест «WebFonts should be
fetched with cors»: скрипт добавляет `<style>@font-face { font-family: SomeFont;
src: url(...) }</style>` в `<head>` и `<p style="font-family: SomeFont">` в
`<body>`, затем ждёт Resource Timing-запись для URL шрифта. Лог `run_smoke.py`
(2026-09-25, изолированный вариант файла, `--timeout-multiplier=5`): после
`CSS пересобран после правки <style>` запроса к `echo-headers.py?…&location=/fonts/pass.woff`
нет вовсе — подтест TIMEOUT, а не FAIL.

## Механизм (по коду)

Первичная загрузка: `page_pipeline.rs` собирает `pending_web_fonts` из каскада,
`page_load.rs` (~стр. 2253) на каждый порождает поток `fetch_font_bytes` →
`FontLoaded` → релейаут. Путь поздно вставленного `<style>` —
`refresh_dynamic_css` (BUG-743) — только пересобирает CSS-текст и `Stylesheet`;
новые `@font-face` правила в нём никто не собирает и не загружает, поэтому шрифт
не применится и на реальной странице (CSS-in-JS, вставляющий `@font-face`,
остаётся на системном шрифте).

## Что нужно

После пересборки в `refresh_dynamic_css` собрать `@font-face url()`-источники,
которых ещё нет в `self.web_fonts`/в полёте, и запустить для них тот же путь,
что `page_load.rs` (с тем же CSP `font-src`-гейтом и `upgrade-insecure-requests`).

## Исправление (P3, 2026-09-30)

Загрузка вынесена из `apply_loaded_page` в `Lumen::spawn_web_font_fetches` (тот же CSP `font-src`/UIR-гейт). `refresh_dynamic_css` после пересборки листа собирает `@font-face url()` через `load_font_faces` и зовёт её; уже запрошенные источники отсекает `Lumen::requested_web_fonts` (сброс на навигации и при возврате на вкладку). Живой прогон: `<style>` с `@font-face`, вставленный через 300 мс, → `@font-face async загружен`. Ограничение: `local()`-источники поздних правил не регистрируются (как и раньше).
