# BUG-1846 — grid: дорожка `auto`/`minmax(0,auto)` получает 0, если в шаблоне есть дорожка `fr` — на portal.nextera.ru невиден заголовок чата и схлопнуты кнопки поля ввода

**Статус:** OPEN
**Заведён:** 2026-11-26 (ZCode, portal.nextera.ru, сравнением с Chrome 158.0.8127.37)
**Область:** layout (grid, размерирование дорожек; файл не локализован — область
[`box_tree/grid.rs`](../crates/engine/layout/src/box_tree), `grid_auto_cols.rs`, `intrinsic.rs`,
см. BUG-1394; сериализация computed — [`grid_computed.rs`](../crates/engine/layout/src/grid_computed.rs))

## Симптом на живой странице

`div.pp-chat-head` — `display: grid; grid-template-columns: minmax(max-content,1fr) minmax(0,auto) minmax(max-content,1fr)`;
`div.chat-input-box` — `auto auto minmax(0,1fr) auto auto auto auto`:

| | Chrome 158 | Lumen a0666c3564+патч |
|---|---|---|
| `.pp-chat-head`, дорожки | `736.77px 70.45px 736.78px`, заголовок чата (имя агента) виден | `790px 0px 790px`, `div.pp-head-title` 0×33 — заголовка нет |
| `.chat-input-box`, дорожки | `44px 0px 616px 40px 40px 0px 48px` | `0px 0px 788px 0px 0px 0px 0px` — кнопки поля ввода схлопнуты |
| computed `grid-template-columns` | разрешён в px | на живой странице отдаёт шаблон, не px |

## Локализация условия (синтетическая проба, видимая область, 600px-контейнер)

Полностью разложившийся повтор (двухфазный замер: вставка → ≥1 с → замер; см. ловушку метода —
свежие узлы в Lumen синхронно не меряются):

| шаблон | Lumen (средняя дорожка) | Chrome | верно? |
|---|---|---|---|
| `minmax(max-content,1fr) minmax(0,auto) minmax(max-content,1fr)` | `0` (дети `[292,0,292]`) | `117.55` (`[233.22,117.55,233.23]`) | нет |
| `1fr minmax(0,auto) 1fr` | `0` (`[292,0,292]`) | `117.55` | нет |
| `minmax(max-content,1fr) auto minmax(max-content,1fr)` | `0` | `117.55` | нет |
| `auto auto minmax(0,1fr) auto auto auto auto` | `0` (`[0,0,552]`) | `117.55` (`[23.94,117.55,410.52,0,0,0,0]`) | нет |
| `100px minmax(0,auto) 100px`, `100px auto 100px` | `384` | `384` | да |
| `auto auto auto` | `254.04` (`[160.44,254.04,169.53]`) | `254.05` | да |
| `minmax(max-content,1fr) min-content minmax(max-content,1fr)` | `49.66` | `49.67` | да |

Условие: **дорожка с `auto`-размерированием обнуляется тогда и только тогда, когда в шаблоне есть
дорожка `fr`** — гибкие дорожки забирают всё свободное место, не оставив `auto`-дорожкам
контентного вклада (по CSS Grid: не-гибкие дорожки размерятся до распределения `fr`).

## Что делать

Порядок размерирования: сначала не-гибкие дорожки (`auto` → max-content), затем распределение
остатка по `fr`; в `resolved_track_list` отдавать использованные значения. Свежая раскладка
вставленных узлов — отдельная ловушка метода, к этому дефекту не относится.

## Как проверить

Проба выше (двухфазно); на портале — заголовок чата виден, кнопки поля ввода своих ширин.
Смежный: BUG-1394 (subgrid, другой аспект той же области).
