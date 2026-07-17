# BUG-305: конструктор Image (HTMLImageElement) отсутствует в DOM-шиме

**Статус:** OPEN
**Дата:** 2026-07-17
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** перф-аудит `/lumen-perf-audit` 2026-07-17 на ria.ru

## Симптом

```
script error: JS runtime error: Image is not defined
```

В `WEB_API_SHIM` нет ни `Image`, ни `HTMLImageElement`
(`grep -n "function Image\|HTMLImageElement" crates/js/src/dom.rs` — пусто;
есть только `DataTransfer.setDragImage` и `createImageBitmap`).

`new Image()` — один из самых частых легаси-паттернов (предзагрузка картинок,
трекинг-пиксели, canvas-исходники: `const img = new Image(); img.src = …;
img.onload = …`). Его отсутствие валит скрипты целиком (как BUG-302 на HN),
маскируя дальнейшие несовместимости сайта.

## Воспроизведение

```bash
cargo run -p lumen-shell --profile dev-release -- --dump-layout https://ria.ru/
# stderr: script error: JS runtime error: Image is not defined
```

## Ожидание

`Image(width?, height?)` = алиас `document.createElement('img')` с
проставленными width/height (HTML Standard §4.8.3); `HTMLImageElement`
экспонирован как глобаль. `onload`/`onerror` при подключённом сетевом слое
картинок; в headless-шиме допустимо событие после установки `src` в очереди
микрозадач — главное, чтобы конструктор существовал и скрипты не падали.
