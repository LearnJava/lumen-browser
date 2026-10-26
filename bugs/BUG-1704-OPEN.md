# BUG-1704 — `window.stop()` не определён

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-NAV-SCOUT)
**Область:** js/shell (глобальный объект `window`; отмена текущей навигации/загрузки документа)

## Симптом

`typeof window.stop` → `"undefined"`; вызов даёт `TypeError: window.stop is not a function`. HTML LS §7.4.3 (`window.stop()`): отменяет текущую навигацию документа и останавливает загрузку подресурсов. Без метода падают тесты Navigation API, которые проверяют, что `stop()` прерывает перехваченную навигацию (`navigation.transition` отклоняется с `AbortError`, `navigateerror`).

## Как найдено

WPT-NAV-SCOUT, кластер F (`docs/wpt-vendor-notes/navigation-api.md`): `ordering-and-transition/navigate-intercept-stop.html?currententrychange` и `?no-currententrychange`, `precommit-handler/precommitHandler-window-stop-before-commit.html` (3 id, 1/5 сабтестов). Потолок небольшой (+2.7 балла), но метод входит в базовый HTML API и нужен не только этим тестам.

## Что делать

Добавить `window.stop()`: отменить ожидающую/идущую навигацию (для перехваченной через Navigation API — прервать с `AbortError`, как это делает `navigation` при замене запроса), остановить подгрузку ресурсов документа. Вне навигации — no-op.

## Как проверить

`typeof window.stop === "function"`; `ordering-and-transition/navigate-intercept-stop.html?currententrychange` проходит.
