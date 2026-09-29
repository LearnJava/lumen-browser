# BUG-1221 — Performance Timeline: `first-paint` в системе отсчёта процесса, `startTime` ресурсов `link`/`css` равен 0

**Статус:** OPEN
**Компонент:** js (Paint Timing / Resource Timing)
**Найден:** 2026-09-29, стенд bankruptcy-platform, сборка `main` 22a782d55

## Симптом

На странице `/dashboard` (живое окно):

- `performance.getEntriesByType('paint')` → `first-paint = first-contentful-paint = 310203` мс при `performance.now() ≈ 10273`
  и `navigation.domContentLoadedEventEnd = 1696`. Метка в системе отсчёта процесса/окна, а не навигации: значение больше
  текущего `performance.now()`, что невозможно по спецификации (Paint Timing).
- Из 84 записей `resource` у **24** `startTime` не положителен (записи `link` для CSS и `css` для `@font-face` `woff2`).
  У `script` и `xmlhttprequest` времена есть. Таблицы стилей вообще не измеряются: ни один инструмент, читающий
  Resource Timing, не может оценить загрузку CSS в Lumen.

## Ожидание

`paint.startTime` ≤ `performance.now()` и отсчитывается от `timeOrigin` навигации; у каждой записи `resource` заполнены
`startTime`, `responseEnd`, `duration`.
