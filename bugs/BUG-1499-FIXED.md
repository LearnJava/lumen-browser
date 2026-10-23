# BUG-1499 — Единицы `cqw`/`cqh`/`cqi`/`cqb`/`cqmin`/`cqmax` разрешаются только если в листе есть хотя бы одно правило `@container`; в `padding` и вне контейнера — не разрешаются никогда

**Статус:** FIXED 2026-10-23
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/box_tree/container_anchor.rs:30` — раннее возвращение при `sheet.container_rules.is_empty()`; `crates/engine/layout/src/style/env.rs` `CONTAINER_CQ`)

## Симптом

`apply_container_styles` возвращается сразу, если в листе нет `@container`, — а `cq*`-единицы разрешает именно этот проход через `CONTAINER_CQ`. Поэтому `width:10cqw` в контейнере без единого `@container` в листе остаётся неразрешённой (`--dump-layout`: `rect` 200 вместо 20, `w=10.00cqw`). Когда правило `@container` есть — `width`/`height` разрешаются, а `padding-left:10cqw` и `element.style.padding='10cqi'` — всё равно `0px`. Без контейнера единица должна равняться `small viewport` (`10cqw` → `10svw`), у нас `10cqw` не разрешена (`1024px`).

## Проба

Проба (`--mcp` и `--dump-layout`, `.c{container-type:size;width:200px;height:100px}`):

| случай | у нас | ожидается |
|---|---|---|
| `#t{width:10cqw;height:10cqh}`, `@container` в листе нет | `--dump-layout`: `rect=(0,0,200,17.72)` | `(0,0,20,10)` |
| то же, `@container (min-width:1px){…}` в листе есть | `rect=(0,0,20,10)` | `(0,0,20,10)` |
| `#t{padding-left:10cqw}` | `getComputedStyle(t).paddingLeft` = `0px` | `20px` |
| `t.style.padding='10cqi'` | `0px` | `20px` |
| `#t{width:10cqw}` без контейнера | `1024px` | `102.4px` (small viewport) |
| `CSS.supports('width','10cqw')` | `true` | `true` |

## Как найдено

WPT-RUN-14 срез 22: `container-queries/container-units-*` (21 id, 103 из 115 сабтестов: `container-units-basic` 2/2, `-auto`, `-content-box`, `-invalidation`, `-selection`, `-svglength`, `-typed-om`, `-media-queries`, `-gradient*`, `-small-viewport-fallback`).

## Что делать

Запускать проход единиц независимо от наличия `@container` (или резолвить `cq*` при раскладке через явный контекст — см. примечание ADR-008 в `env.rs`); подключить `cq*` в `padding`/`margin`/`border`; запасной вариант без контейнера — `sv*`.

## Как проверить

Таблица выше; `css/css-conditional/container-queries/container-units-basic.html`.

## Исправление (2026-10-23, P3)

- `Length::resolve` (`style/values/length.rs`): `cq*` без контейнера (и ось, которую контейнер не
  скрывает — `inline-size` прячет блочную) — small viewport (`cqw`→`svw`, `cqh`→`svh`), а не `None`.
  Базис `CONTAINER_CQ` стал пооcным: ширина ближайшего контейнера, содержащего inline-размер, и
  высота ближайшего `container-type: size` — так `inline-size` внутри `size` берёт `cqb` у внешнего
  (`container-units-basic.html`).
- `apply_container_styles` (`box_tree/container_anchor.rs`): проход идёт и без `@container` в листе,
  если где-либо разобрана `cq*`-длина (`cq_units_seen`, монотонный флаг); рестайл потомков при пустом
  списке правил пропускается. Высота ближайшего `size`-контейнера едет по стеку обхода.
- `getComputedStyle` (`resolved_geometry.rs`): `GeomCtx.container` держит тот же поосный базис, и
  `apply_used_geometry` ставит его на время разрешения `padding`/`margin`/`width`/inset — `padding-left:
  10cqw` и `element.style.padding='10cqi'` дают `20px`, а не `0px`.
- Тесты: `tests/container_units.rs` (6) и обновлённые `cq_units_*` в `style/tests/values.rs`.
- **Не проверено:** сами WPT `container-queries/container-units-*` после правки не прогонялись; что
  останется красным (typed-om, svglength, invalidation при смене размера, selection) — новые баги по
  следующему прогону P2.

