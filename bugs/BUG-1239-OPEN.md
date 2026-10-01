# BUG-1239 — youtube: после BUG-1207 страница пуста, `ytd-page-manager` без детей, ошибок в логе нет

**Статус:** OPEN
**Компонент:** js — не локализовано (youtube не строит `ytd-browse` главной)
**Найден:** P3, при закрытии [BUG-1207](BUG-1207-FIXED.md), 2026-10-02

## Симптом

Видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, dev-release, `--proxy` (прямое соединение с youtube в этой
сети рвётся на TLS), `https://www.youtube.com/`, 30 с: `[JS error]` нет, `ytd-app` и `ytd-page-manager`
апгрейднуты, но `ytd-page-manager.children` пуст, `ytd-browse`/`ytd-rich-grid-renderer` нет,
`document.body.innerText` пуст, ~1300 элементов (Chrome: 1529–1868). `ytInitialData` определён.

## Что сделать

Выяснить, кто должен создать `ytd-browse` в `ytd-page-manager` (обработчик `yt-page-data-fetched`/навигация
`ytd-app`), и где цепочка обрывается без исключения: `.tmp`-проба с трассой `define`/upgrade/connected, как в
BUG-1207, затем сравнить с Chrome.
