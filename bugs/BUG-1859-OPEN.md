# BUG-1859 — portal.nextera.ru: DEVX-8a в debug-сборке — автоширинный block-ребёнок на 30px шире родителя (сайдбар)

**Статус:** OPEN (заведён 2026-11-27, ZCode; найден при живом замере BUG-1858 — паника
убила сессию до навигации в чат. Репродюс флейм: из двух одинаковых живых прогонов
с логином паниковал только первый; финальный DOM сайдбара чист от нарушения —
нарушение живёт в промежуточном streaming-тике)
**Область:** layout (блочный сайзинг: auto-width block-ребёнок в plain-block-родителе)

## Симптом

Залогиненный портал, сразу после отрисовки сайдбара, debug-сборка паникует
(`crates/engine/layout/src/invariants.rs:101`):

```
DEVX-8a: in-flow block child escapes parent border box horizontally,
without overflow/positioning:
  parent node=NodeId(307) rect=Rect { x: 55.0, y: 795.0, width: 257.0, height: 32.195923 }
  child  node=NodeId(312) rect=Rect { x: 55.0, y: 811.9, width: 287.0, height: 15.295898 }
```

Ребёнок — auto-width (`style.width.is_none()`, условие инварианта), тот же x, но
287 против 257: ровно +30px вправо, без overflow/positioning. По геометрии — однострочный
текст (h≈15.3) внутри элемента сайдбара (родитель h≈32.2, y=795 — нижняя часть левой
колонки); сразу перед паникой пришли `/api/v2/platform/notice` и
`/api/portal/projects/84/topics`.

## Почему это дефект движка, а не сайта

§10.3.3: auto-width block-ребёнок в block-контейнере заполняет containing block
(margin-solving), а не сжимается по min-content. Child-бокс 287 > родителя означает, что
движок применил к блочному ребёнку shrink-to-fit / min-content-пол (как у flex-элементов),
либо разрешил ширину от чужого containing block. Известный родственник — утечка fit-content
в `column_item_avail_cross` (BUG-1853, дефект 3), но там родитель был flex; здесь родитель —
plain block (`BoxKind::Block` + `Display::Block`).

## Последствия

- Любой живой залогиненный прогон портала в debug-сборке умирает на этом месте
  (паника `lumen-v8` → PoisonError во всех нативных биндингах → main-поток,
  `about_to_wait.rs:1756`, процесс гаснет). Замеры BUG-1858 не идут.
- `dev-release`/release не паникует (`check_geometry` под `cfg(debug_assertions)`) —
  дефект геометрии остаётся, просто молча.

## Воспроизведение

Живой портал: залогиниться (см. `.tmp/bug1853/bubble_probe.py` — сам логинится), дождаться
сайдбара, debug-сборка (`target/debug/lumen.exe`). Лог упавшего прогона:
`.tmp/bug1853/bubble_lumen.log` (первичная паника — строка 310).

## Как проверять

Снять DOM-поддерево родителя (NodeId(307) ≈ элемент сайдбара) на dev-release-инстансе
(без паники), построить синтетический репродюс, починить сайзинг, юнит-тест в
`flex_axes`/блочных тестах + живой прогон debug-сборкой без паники.

## Смежные

BUG-1858 (пузыри чата — причина живого замера), BUG-1853 (дефект 3 — соседняя утечка
fit-content в колоночном flex), BUG-1846/1851 (портальная серия).
