# BUG-1154: `@font-face` из `<style>`, вставленного скриптом после загрузки, никогда не загружается

**Статус:** OPEN
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
