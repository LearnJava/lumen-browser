# BUG-163

**Статус:** OPEN
**Компонент:** shell / layout
**Файл:** `crates/shell/src/main.rs` (preload-обработка), `crates/engine/layout/src/box_tree.rs:1302` (`collect_image_requests`)

## Описание

`<link rel="preload" as="image" href="...">` хинты парсятся и логируются
(`⤷ preload img [low] ...`), но реальные картинки по ним не дозагружаются и не
рендерятся. На lenta.ru это 94 хинта с настоящими URL превью статей
(`icdn.lenta.ru/images/.../owl_article_250_*.jpg`), но в DOM нет `<img>` с
этими src (контент строит JS), поэтому `collect_image_requests` возвращает
`0 картинок`, а preload-хинты остаются неиспользованными.

## Воспроизведение

```
./target/release/lumen.exe https://lenta.ru
```

В stdout: `Распарсено: ... 0 картинок, 120 preload-хинтов`.
В stderr: ~94 строки `⤷ preload img [low] https://icdn.lenta.ru/images/...jpg`,
ни одной `Загружена картинка`.

## Анализ

Сейчас shell дозагружает только картинки, привязанные к `<img>`-узлам
(`collect_image_requests` ходит по DOM и ищет `name.local == "img"`).
Preload-as-image хинты собираются отдельно (preload scanner), но не
превращаются ни в `Renderer::register_image`, ни в layout-боксы.

Полное отображение lenta.ru этим не чинится (превью расставляет SPA-бандл,
который движок не исполняет), но дозагрузка preload-картинок — корректный
самостоятельный шаг к поддержке сайтов с `rel=preload as=image`.

## Не дубль

BUG-158 — про геометрию карточек lenta.ru (контейнеры height=0). Этот баг —
про отсутствие самой загрузки картинок из preload-хинтов.
