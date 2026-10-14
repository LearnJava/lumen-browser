# BUG-1573 — `el.style.<свойство> = …` на скроллере (или его потомке, или предке) молча сбрасывает `scrollTop` в 0

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** layout/shell (повторная раскладка после мутации стиля теряет смещение прокрутки вложенного контейнера; точное место не изолировано)

## Симптом

Контейнер `overflow:auto` 200×200 с четырьмя детьми по 150 px. После `c.scrollTop = 100` и записи в инлайн-стиль **любого** элемента из цепочки «сам / потомок / предок (`body`)» `scrollTop` становится 0. Сброс виден не сразу: синхронное чтение сразу после записи ещё отдаёт 100, через 50 мс — 0.

## Проба

`tests/wpt/run_smoke.py` + testharness, контейнер `.c{width:200px;height:200px;overflow:auto}`, `.c div{height:150px;width:150px}`; в каждой строке перед записью `scrollTop = 100` и пауза 100 мс, замер через 300 мс:

| действие после `scrollTop = 100` | `scrollTop` у нас | ожидается |
|---|---|---|
| `c.style.color = "red"` | 0 | 100 |
| `c.style.background = "#eee"` | 0 | 100 |
| `c.setAttribute("style", "background:#eee")` | 0 | 100 |
| `c.children[0].style.background = "red"` | 0 | 100 |
| `document.body.style.margin = "3px"` | 0 | 100 |
| `c.className = "c y"` | 100 | 100 |
| `c.setAttribute("data-x", "1")` | 100 | 100 |
| `style` соседнего (не предок) элемента | 100 | 100 |
| вставка постороннего `<p>` в `body` | 100 | 100 |

То есть сбрасывает путь «инлайн-стиль → пересчёт стиля поддерева контейнера», а не любая перераскладка.

## Как найдено

WPT-RUN-14 срез 26: `css/css-scroll-snap/snap-after-relayout/*` и `resnap-on-*` читают `scrollTop` после смены стиля скроллера и получают 0 вместо ожидаемой позиции привязки (`assert_equals: expected N but got N`).

## Что делать

Найти, где после мутации инлайн-стиля пересобирается состояние прокрутки контейнера (`collect_scroll_containers_for_js_state`, `crates/engine/layout/src/lib.rs`, и перенос смещения между раскладками) и сохранять смещение, зажимая его по новому `scrollHeight`, а не обнуляя.

## Как проверить

Проба выше; затем `run_corpus.py --prefixes css/css-scroll-snap/snap-after-relayout`.
