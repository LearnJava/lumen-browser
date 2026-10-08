# BUG-1235 — остаток BUG-1097: `script-tag`/`sharedworker-classic`/`a-tag` в `referrer-policy/4K*` и сквозная проверка BMP

**Статус:** OPEN
**Тип:** не локализован / верификация.
**Заведён:** 2026-10-01 (P1, остаток [BUG-1097](BUG-1097-FIXED.md))
**Область:** WPT-инфраструктура `common/security-features/` (`requestViaScript`, `tests/wpt/common/security-features/resources/common.sub.js:626-635`).

## Что осталось

1. **Сквозная проверка BMP.** `lumen-image` теперь декодирует BMP (BUG-1097, срез 1), но
   `python tests/wpt/run_report.py --check --all --root referrer-policy/4K --recursive --processes 7`
   не запускался. Ожидание: 156 `img-tag`-подтестов переходят в unexpected PASS. После — регенерация
   baseline `referrer-policy` и `mixed-content` (и категорий, где `img-tag`/`picture-tag` берётся из
   `security-features`).
2. **Необъяснённый остаток.** `script-tag` 141, `sharedworker-classic` 12, `a-tag` 3 падают с
   `promise_test: Unhandled rejection with value: object "[object Object]"`. `script.py` отдаёт
   `application/javascript`, не изображение, — к BMP не относится. Кандидат: гонка
   `bindEvents2(window, "message", script, "error", window, "error")` в `requestViaScript` —
   возможно, наша доставка `postMessage`/событий ведёт себя иначе.

## Как проверить

Прогон из п. 1 + отдельный пробник на `requestViaScript` для п. 2 (процедура — `docs/probe-method.md`).
