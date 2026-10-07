# BUG-1396 — `contain: layout` / `paint`: baseline, независимый форматирующий контекст, stacking context, scrollable overflow

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout (`crates/engine/layout/src/box_tree/bfc.rs` — `establishes_bfc`; `box_tree/baseline.rs`; `lib.rs:1508` `contributes_to_scrollable_overflow`), paint (`display_list/stacking`)

## Симптом

`--dump-layout` / `--screenshot`, `body{margin:0;font:20px/1 Ahem}`:

| проба | получено | ожидается (CSS Containment L1 §3.2, §3.3) |
|---|---|---|
| `<div style="contain:layout"><div style="margin-top:40px;height:10px"></div></div><div style="height:10px"></div>` (то же с `paint`) | обёртка `y=40`, высота 10 | обёртка `y=0`, высота 50: поля не схлопываются наружу, она — независимый форматирующий контекст |
| `div{contain:paint;position:relative;background:green}` с абсолютным ребёнком `z-index:-1; background:red` | пиксель (10,10) — зелёный | красный: `contain:paint`/`layout` создают stacking context, ребёнок с `z-index:-1` рисуется над фоном этого бокса |
| `display:inline-flex; align-items:baseline`: `<canvas style="height:100px">` и `<div style="contain:layout;width:50px;height:100px">item</div>` | элемент на `y=84`, контейнер 184 | элемент на `y=0`, контейнер 100: baseline элемента с `contain:layout` синтезируется по нижней границе |
| `div{overflow:auto;width:100px;height:100px}` > `div{contain:layout;height:0}` > `div{width:300px;height:300px}` | `scrollWidth`/`scrollHeight` = 300 | 100: переполнение — ink overflow, прокручиваемую область не увеличивает |

Верно уже сейчас: `contain:layout` и `contain:paint` создают containing block для `position:absolute` и `position:fixed` (проба: абсолютный ребёнок `bottom:0;right:0` приклеен к рамке контейнера), `contain:paint` клипует по padding box (`PushClipRect` совпадает с `overflow:hidden`).

## Как найдено

WPT-RUN-14 срез 17: `contain-layout-independent-formatting-context-001/002`, `contain-paint-independent-formatting-context-001/002`, `contain-layout-stacking-context-001`, `contain-paint-stacking-context-001a/b`, `contain-layout-baseline-001…006`, `contain-layout-suppress-baseline-001/002`, `contain-layout-ink-overflow-013…020`, `contain-content-002` — 24 id, все `thick`.

## Что делать

`establishes_bfc` (`bfc.rs`): добавить `ContainFlags::LAYOUT | PAINT`. Stacking context — там же, где строятся контексты для `opacity`/`transform`. `baseline.rs`: при `LAYOUT` у flex-/grid-элемента и ячейки возвращать `None`. `contributes_to_scrollable_overflow`: при `LAYOUT` на потомке не учитывать его переполнение.

## Как проверить

`css/css-contain/contain-layout-independent-formatting-context-001.html`, `contain-layout-baseline-002.html`, `contain-layout-ink-overflow-016.html`, `contain-paint-stacking-context-001a.html`.
