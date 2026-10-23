# BUG-1496 — `@container style(--x: y)` срабатывает только на предке с `container-type`; по спецификации любой элемент — style-контейнер

**Статус:** FIXED 2026-10-23 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/style/container.rs`, `crates/engine/layout/src/box_tree/container_anchor.rs` — выбор контейнера для `style()`)

## Симптом

CSS Conditional 5 §container-queries: запрос `style()` вычисляется на ближайшем предке, у которого есть контейнер-имя, а без имени — на ближайшем предке-элементе вообще (любой элемент является style-контейнером). У нас `style()` находит только элемент с `container-type`. Без `container-type` (с `container-name` или без) — не срабатывает, ни на родителе, ни на самом элементе, ни на `:root`. Отдельно: boolean-форма `style(--x)` и `style(color:red)` верны, когда `container-type` есть. Сопутствующее: 22 теста `container-queries/*style*` завершаются `ERROR` — `assert_implements_style_container_queries()` читает `CSSStyleSheet.replaceSync('@container STYLE(--foo: bar){}').cssRules[0].containerQuery`, а `cssRules` у такого листа пуст (BUG-1455) — после CSSOM-10 они откроются и покажут эту причину.

## Проба

Проба (`--mcp`, цвет `#t`, `@container ⟨q⟩{#t{color:green}}`):

| контейнер | запрос | у нас | ожидается |
|---|---|---|---|
| `.c{--x:y}` (без `container-type`) | `style(--x: y)` | `red` | `green` |
| `.c{container-name:n;--x:y}` | `n style(--x: y)` | `red` | `green` |
| `<div style="--x:y"><div id=t>` | `style(--x: y)` | `red` | `green` |
| `:root{--x:y}` | `style(--x: y)` | `red` | `green` |
| `.c{container-type:inline-size;--x:y}` | `style(--x: y)` | `green` | `green` |
| `.c{container-type:size;…;--x:y}` | `(min-width:100px) and style(--x:y)` | `green` | `green` |
| `.c{color:red;container-type:size}` | `style(color:red)` | `green` | `green` |

## Как найдено

WPT-RUN-14 срез 22: `css-conditional/container-queries/{custom-property-style-queries,style-query-*,style-container-*,multiple-style-containers-comma-separated-queries,registered-color-style-queries}` — 36 id (22 ERROR, 12 OK с падениями, 2 FAIL).

## Что делать

Определять style-контейнер как ближайший предок-элемент без требования `container-type` (для запроса с именем — с совпавшим `container-name`); `size`-запросы по-прежнему требуют `container-type`. Затем CSSOM-10 (BUG-1455) для `containerQuery`.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/style-query-no-cycle.html`, `custom-property-style-queries.html`.

2026-10-21 (CQ-SCROLL-STATE): запрос выбирает контейнер по цепочке (`ContainerContext::parent`) — по типу и по имени, а не только ближайший.

## Исправление

- Если в листе есть `@container` со `style(…)`, второй проход (`apply_container_styles`) не ищет контейнеры по `container-type`, а обходит дерево целиком (`StyleSweep` в `box_tree/container_anchor.rs`): каждый элемент получает `ContainerContext` (`kinds` пусты у обычного элемента), контексты связаны в цепочку предков (`ContainerContext::parent`), и каждый элемент перестилизуется против контекста **родителя**. Безымянный `style()` отвечает родитель, именованный — ближайший предок с этим `container-name` (с `container-type` или без), размерный признак — ближайший `container-type`. Элементы без бокса (inline, `display: none/contents`) идут отдельным обходом по документу (`boxless_below`) и тоже в цепочке; боксовый потомок бокслесс-родителя берёт его контекст (`through_boxless`).
- Без `style()` в листе путь прежний (`re_style_subtree` + `restyle_boxless`) — `container-type`-контейнеры, ни одного нового контекста. Серийный `computed_style_to_map` на элемент делается только если запрос читает стандартное свойство (`query_reads_standard_property`).
- Сопутствующее для тех же 36 id: `containerQuery` отдаёт имя `STYLE(` в нижнем регистре (`cssom_tree::parse::lowercase_style_function`) — `assert_implements_style_container_queries()` перестал падать; значение запроса подставляет `var()` против контейнера (недостижимая ссылка = guaranteed-invalid, равна неустановленному свойству) и игнорирует `!important`.
- Тесты: `tests/style_container_any_element.rs` (12: таблица симптома, ближайший предок, именованный, size+style, inline-родитель, `var()`/`!important`), `cssom_tree::tests::style_query_function_name_is_lower_cased`. WPT (подмножество 12 файлов `container-queries/*style*`): `custom-property-style-queries` 67/78, `query-evaluation-style` 42/91 (старый бинарий корня: 36), `style-negation-with-container-name` PASS.
- Ограничение: геометрия за style-only контейнером не пересчитывается (как у scroll-state-контейнера) — запрос меняет стиль, но не раскладку.
- Остаток (не выбор контейнера): диапазонная форма `style(--x < 3)`, `initial`/`inherit`/`unset`/`revert` в значении запроса, зарегистрированные `@property` (`<length>` с `em`/`cqi`), `style(--green)` при `--green: ;` (пустое значение — не guaranteed-invalid), `style()` в shadow-дереве (`::part`, `::slotted`, `:host`), `(unknown)` как `<general-enclosed>` в `or`/`not`.
