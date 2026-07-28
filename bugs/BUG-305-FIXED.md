# BUG-305: конструктор Image (HTMLImageElement) отсутствует в DOM-шиме

**Статус:** FIXED 2026-07-19 (P3)
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

## Решение (2026-07-19, P3)

В `WEB_API_SHIM` (`crates/js/src/dom.rs`) добавлены два глобальных объявления
перед литералом `document`:

- `function HTMLImageElement() {}` — голый интерфейс-глобаль (как и остальные
  DOM-интерфейсы, `instanceof` не заведён: враппер элемента — обычный объект).
- `function Image(width, height)` — создаёт `document.createElement('img')`,
  проставляет `width`/`height` из аргументов конструктора и **возвращает**
  элемент. Возвращённый объект перекрывает `this`, поэтому `new Image()` даёт
  нативный `<img>`-враппер с `__nid__`, участвующий в layout/paint как любой
  распарсенный `<img>`.

Попутно у враппера элемента (`_lumen_build_element`) появилось отражение
`src`-атрибута (`get/set src`, общее для `<img>/<script>/<iframe>/<source>/…`),
чтобы `img.src = …` доходил до content-атрибута и его видел layout; чтение
возвращает сырую строку атрибута (та же упрощённая семантика, что у
`getAttribute`; резолюция в абсолютный URL отложена), пустая строка при отсутствии.

Регрессия закрыта 4 юнит-тестами (`image_constructor_creates_img_element`,
`image_constructor_applies_width_height_args`,
`image_src_reflects_content_attribute`, `html_image_element_is_a_global`).
Событийная модель `onload`/`onerror` при JS-инициированной загрузке подресурса
остаётся отложенной (не в объёме бага).
