# BUG-1134 — SVG-картинка, начинающаяся с XML-комментария, не распознаётся декодером

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** image (`crates/engine/image/src/lib.rs:68-93` `is_svg` — после BOM/пробелов ждёт только `<svg`, `<!doctype svg`, `<?xml`; `Content-Type: image/svg+xml` не учитывается)

## Симптом

Логотипы `s3-symbol-logo.tradingview.com` (`Content-Type: image/svg+xml`) начинаются с
`<!-- by TradingView --><svg …>`. `is_svg` пропускает только BOM и пробелы, поэтому ведущий
комментарий даёт `UnknownFormat`: 158 строк «Не декодируется …svg», 155 битых `<img>` из 246.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

Страница `.tmp/compat/g5/api.html` грузит `comment.svg` (`<!-- by TradingView --><svg …>`) и `plain.svg` (тот же SVG без комментария).

**Результат:** Lumen: `comment.svg` → `UnknownFormat`; `plain.svg` → «Загружена картинка: plain.svg (18×18, Rgba8)». Chrome рисует обе.

## Что сделать

MIME Sniffing §7: для `<img>` сначала доверять `Content-Type: image/svg+xml` (SVG не
сниффится по байтам); в самом `is_svg` пропускать пролог XML — комментарии `<!-- -->`,
`<?xml …?>`, `<!DOCTYPE …>` — до первого элемента. Критерий: `comment.svg` декодируется;
tradingview перемерить.
