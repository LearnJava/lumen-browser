# BUG-158

**Статус:** FIXED 2026-06-15
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

На реальных сайтах (воспроизводится на `https://lenta.ru/`) карточки новостей
рисуются друг поверх друга: заголовки разных новостей оказываются на одинаковых
координатах. В display list видно две независимые карточки с `DrawText` на
идентичных y:

```
DrawText (40, 669, ...) "Москвичам назвали время пика жары"
DrawText (40, 685, ...) "жары"
...
DrawText (40, 669, ...) "Михалков поднял проблему лжеветеранов СВО"   ← тот же y=669
DrawText (40, 685, ...) "лжеветеранов СВО"
```

Корневая причина: контейнеры-обёртки карточек схлопываются в `height=0`, поэтому
курсор главной оси не продвигается и следующий сиблинг рисуется поверх
предыдущего. В `--dump-layout` много блоков с `h=0.00` там, где должен быть
ненулевой контент (`Block rect=(0,0,1024,0) position=relative`,
`Block rect=(0,0,300,0) w=300`). Высота контента не пробрасывается вверх через
какой-то тип контейнера (предположительно flex/grid с абсолютными детьми либо
блок, чья intrinsic-высота не считается).

## Воспроизведение

```bash
./target/debug/lumen.exe --dump-display-list https://lenta.ru/ 2>/dev/null \
  | grep -nE "DrawText.*669" | head
```

Несколько разных заголовков на одном y → подтверждение.

## Старт расследования

Найти контейнер карточки в `--dump-layout` (h=0 при непустом контенте), проверить
почему его высота не растёт от детей в `lay_out` / `lay_out_flex` / `lay_out_grid`
(`crates/engine/layout/src/box_tree.rs`). Связь с BUG-159 (abs-дети) вероятна:
если контент карточки — out-of-flow, родитель честно получает h=0, но тогда
сами карточки должны позиционироваться раскладкой более высокого уровня.

## Корневая причина (расследование 2026-06-15)

Карточки новостей `lenta.ru` — `<a class="card-mini _topnews">` — являются flex-items
column-flex контейнера `.topnews__column` (`flex-direction:column`). CSS:

```css
.card-mini._topnews { align-items:center; flex:1 }          /* flex-basis:0 */
.card-mini._topnews:first-child, :last-child { flex:none }  /* content-sized */
```

`flex:1` раскрывается в `flex-grow:1; flex-shrink:1; flex-basis:0`. В column-flex с
**неопределённой** высотой контейнера свободного места нет → flex-grow ничего не
добавляет, и used main size (высота) item-а оставалась равной flex-basis = **0**.
Отсутствовал CSS Flexbox §4.5 *automatic minimum size*: item с `min-height:auto` и
видимым overflow не может стать ниже своего content size. Первый/последний ребёнок
(`flex:none` → flex-basis:auto) считались по контенту и работали — потому ломались
только «средние» карточки, а сиблинги рисовались на одном `y`.

Второй слой: `lay_out_flex` пишет вычисленный px-`height` обратно в стиль item-а
(`children[i].style.height = Px(inner_main)`). Grandparent (row-flex `.topnews`)
раскладывает column дважды (prelim + final), поэтому на втором проходе у карточки
уже был ненулевой `style.height`, и наивный guard `height.is_none()` отключал floor
и снова схлопывал её в 0.

## Фикс

В `all_hyp` (ветка `FlexBasis::Length`, `is_column`) пол высоты берётся как
`item.rect.height` из предварительного прохода — это content height, уже
ограниченный реальным явным `height` (спецификационный «specified size suggestion»),
и устойчивый к самозаписи стиля между проходами. Guard: только `min_height:auto` +
`overflow_y == visible` (scroll-контейнеры по §4.5 имеют auto-min = 0).

Регресс-тест `flex_column_basis_zero_item_keeps_content_height` (lib.rs) строит
row-flex > column-flex > `flex:1`, чтобы воспроизвести двухпроходный путь.
Проверено на живом `lenta.ru`: совпадений `DrawText` на `y=669` стало 1 (было 3+),
карточки складываются по `y` (643→715→803). Layout 2894 + paint 737 тестов зелёные.
