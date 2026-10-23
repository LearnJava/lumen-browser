# BUG-1498 — Выбор контейнера для `@container`: вложенный `inline-size` не пропускается при запросе по высоте, `display:none`/`contents`/top-layer/таблица неверно участвуют

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/box_tree/container_anchor.rs` — выбор контейнера по `container-type`/имени/осям)

## Симптом

Запрос по оси, которую ближайший контейнер не отдаёт (`container-type:inline-size` и запрос `min-height`), должен идти к следующему предку, умеющему эту ось; у нас запрос ложен. Затем отдельные случаи «контейнер без principal box»: `display:contents`, `display:none`, контейнер внутри multicol, диалог в top layer, `display:table` (непригоден для containment). Измерено на вложенном контейнере; остальные — по именам файлов и первому сообщению сабтеста.

## Проба

Проба (`--mcp`, `.c{container-type:size;width:200px;height:100px}`, внутри `.d{container-type:inline-size}`, запрос `@container (min-height:50px){#t{color:green}}`):

| случай | у нас | ожидается |
|---|---|---|
| `#t` внутри `.d` внутри `.c`, запрос `(min-height:50px)` | `red` | `green` (`.d` не отдаёт высоту → `.c`) |
| `.c{container-type:inline-size}`, запрос `(min-height:50px)` | `red` | `red` |
| `.c{display:contents;container-type:inline-size}`, `(min-width:1px)` | `red` | `red` |
| `#t{container-type:size}` — запрос к самому себе | `red` | `red` |

## Как найдено

WPT-RUN-14 срез 22: `container-queries/{container-selection,container-nested,nested-query-containers,query-container-name(-dynamic),unsupported-axis,display-none,display-contents,ineligible-containment,top-layer-dialog(-container),size-container-no-principal-box,container-inside-multicol-with-table}` — 13 id, 90 из 129 сабтестов.

## Что делать

Выбирать ближайшего предка, чей `container-type` отдаёт нужную ось (и, при указанном имени, совпадает по `container-name`); учесть eligibility по `display` и top layer.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/container-selection.html`, `unsupported-axis.html`.

## Срез 1 (2026-10-23, P3)

Сделано (`style/container.rs`, `box_tree/container_anchor.rs`, `css-parser` `ContainerRule::enclosing`):

- выбор контейнера по оси: `Needs { inline, block, scroll_state }` вместо одного `size`; блоковая ось есть только у `container-type: size` (`unsupported-axis` 16/16, `container-selection` 21/21);
- «ineligible» контейнер (`display: contents`/`inline`/`table`/внутренние табличные) выбирается, но запрос по размеру ложен и к внешнему не переходит (`ContainerKinds::ineligible`; `ineligible-containment` 2/2, `display-contents` 3/3); боксовые потомки бокс-less контейнера не получают правил внешнего (`shadowed_by_boxless_container`);
- вложенный `@container` хранит охватывающие прелюдии (`ContainerRule::enclosing`), правило применяется при истинности всех (`container-nested` 14/14);
- `@container --name { }` без запроса — по `container-name` без `container-type` (`query-container-name(-dynamic)`: 4/5 и 3/3);
- `%`-высота потомков `inline-size`-контейнера с заданной `height` разрешается от неё при втором проходе.

WPT `css/css-conditional/container-queries`: 784 → 798 из 1414 подтестов.

Остаток (оставить открытым):

- `nested-query-containers` (0/32): запрос сам меняет `display` контейнера на `inline`/`contents`/`table*`, что должно перевести вложенные контейнеры в «unknown» и обратно (нужен повторный проход с новым `display`);
- `display-none` (3 подтеста): `getComputedStyle(el, '::before')` и `display:none` у самого элемента-цели — псевдоэлементная ветка вычисленного стиля не знает контейнерного контекста;
- `top-layer-dialog-container`: после `dialog.showModal()` контейнер должен перемеряться по вьюпорту до чтения стиля;
- `query-container-name`: именованный контейнер в shadow-дереве (область видимости имён).
