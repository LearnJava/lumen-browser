# WPT vendor notes — `scroll-performance-timing`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-06 (коммит `35be3b44`, `tests/wpt/scroll-performance-timing/tentative/supported-entry-types.window.js`, 1 файл). Прогон `run_report.py --all --root scroll-performance-timing --recursive` (~29 с): 1/1 harness OK, 0/2 сабтестов. Оба FAIL — `'scroll'` отсутствует в `PerformanceObserver.supportedEntryTypes` и `window.PerformanceScrollTiming` не существует; API нестандартный (tentative, MSEdgeExplainers) и не реализован вовсе, согласуется с уже задокументированным пробелом `CAPABILITIES.md:152` (общий PerformanceObserver не заполняет большинство entry types). Новых багов не заведено
