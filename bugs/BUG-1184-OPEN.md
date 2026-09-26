# BUG-1184 — `getComputedStyle` видит `<style>`, заблокированный CSP

**Статус:** OPEN
**Заведён:** 2026-09-26 (P3, по ходу [BUG-1183](BUG-1183-FIXED.md); живая проба против Chrome 153).
**Область:** не локализовано — снимок стилей для JS (`crates/shell/src/page_pipeline.rs:842`
`collect_js_layout_snapshot`, `update_computed_styles`) или CSSOM-листы (`update_stylesheet_nodes`,
`crates/shell/src/persistent_js.rs:1288`).

## Симптом

`Content-Security-Policy: style-src 'none'`, `<style>#b{color:rgb(0,0,255)}</style><div id=b>`.

| | Lumen dev-release | Chrome 153 |
|---|---|---|
| `getComputedStyle(b).color` | `rgb(0, 0, 255)` | `rgb(0, 0, 0)` |
| отрисовка (`--dump-display-list`) | `#000000ff` | чёрный |

Каскад страницы лист исключает (`ParsedPage::rule_count == 0`, отрисовка чёрная), а скрипт видит
синий. Так и во время разбора (`DOMContentLoaded`, тест `parse_and_layout_for_test`), и после
загрузки (MCP `eval` через 4 с). То же при `style-src-elem 'none'`.

## Репро

Сервер `target/bug1183/server.py` (worktree `p3-work`), страница `/p7.html`; локально —
`target/bug1183/p7local.html` с `<meta http-equiv>`.

## Что сделать

Найти, из какого листа JS берёт стиль, и исключить из него заблокированные CSP `<style>`/`<link>`.
Критерий: `/p7.html` даёт `rgb(0, 0, 0)`, как Chrome.
