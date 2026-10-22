# BUG-1673 — UA-стили HTML: `source` скрыт, `option` закрытого `select` — `none`, замещаемые без `overflow: clip`

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), crates.io, ya.ru, apple)
**Область:** layout/UA-стили (`html.css` движка)

## Симптом

```html
<!doctype html><picture><source id=s srcset=x.png><img></picture>
<!doctype html><img id=i width=10 height=10><select id=s><option id=o>a</option></select>
```

| `getComputedStyle(...)` | Chrome | Lumen | спецификация |
|---|---|---|---|
| `source.display` | `inline` | `none` | HTML §15.3.1: `source` в списке `display:none` **нет** — Lumen неправ |
| `option.display` (закрытый `select`) | `block` | `none` | spec? — HTML описывает выпадающий `select` как виджет; WPT `html/rendering/widgets/select-*` |
| `img.overflow`, `select.overflow` | `clip` | `visible` | spec? — Chrome ставит `overflow: clip` замещаемым и `select` (`overflow-clip-margin: content-box`), в HTML §15.4 это есть для `img, video, canvas` и пр. |

Визуально почти ничего не меняет, но в отчёте geom-diff даёт «дерево разошлось» (`picture>source`
есть у Chrome, у Lumen скрыт) и строки стиля `overflow: clip → visible`.

## Что делать

1. `source` — убрать из `display:none` UA-таблицы (оставить, как у прочих неотображаемых, `display: inline`
   без бокса — `source` пуст и боксов не даёт).
2. Сверить с HTML §15.4.1 правило `overflow: clip; overflow-clip-margin: content-box` для `img, video,
   canvas, iframe, embed, object` и добавить, если оно там есть (Chrome 108+).
3. `option` внутри закрытого `select`: решить по WPT `html/rendering/widgets/` — Chrome отдаёт `block`.

## Как проверить

Репро выше. `python scripts/geom_diff.py --only crates` — нет `picture:1>source:*` в «нет у Lumen».
