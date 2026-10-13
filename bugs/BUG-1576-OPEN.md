# BUG-1576 — CSS Scroll Snap 2: `scrollsnapchange`/`scrollsnapchanging`/`SnapEvent` не существуют, `scroll-initial-target` не применяется к вложенным скроллерам

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** js/layout (`SnapEvent` и события `scrollsnapchange`/`scrollsnapchanging` — ноль совпадений по `crates/`; `crates/engine/layout/src/scroll_initial_target.rs`)

## Симптом

События Scroll Snap 2 в актуальной редакции черновика (`scrollsnapchange` после завершения привязки, `scrollsnapchanging` во время жеста) и интерфейс `SnapEvent` (`snapTargetBlock`, `snapTargetInline`) в движке отсутствуют; реализованы устаревшие `snapchanging`/`snapchanged` (`SnapChangeEvent`, `crates/engine/layout/src/lib.rs:1201`, оговорка в `v8_css_storage_nav_misc.rs`). `scroll-initial-target` по `CSS-SPECS.md` применяется при загрузке документа; при вставке контейнера скриптом — нет (проба ниже).

## Проба

`run_smoke.py` + testharness:

| вызов | у нас | ожидается |
|---|---|---|
| `typeof SnapEvent` | `undefined` | `function` |
| `"onscrollsnapchange" in window` / `in document` / `in element` | `false` / `false` / `false` | `true` |
| `new SnapEvent("scrollsnapchange")` | `ReferenceError` | событие с `snapTargetBlock === null` |
| слушатель `scrollsnapchange` на скроллере, `scrollTo(0,160)` | не вызван (пришли `scroll`, `scrollend`) | вызван |
| вложенный `overflow:auto` 200 px, ребёнок на 150 px с `scroll-initial-target: nearest`, контейнер вставлен скриптом после загрузки | `scrollTop` 0 | 150 |

## Как найдено

WPT-RUN-14 срез 26: `css/css-scroll-snap/snap-events/*`, `snapevent-constructor.html`, `scroll-initial-target/*`.

## Что делать

Это новый интерфейс и события; делать после BUG-1574 (привязка при программной прокрутке даёт сам момент «привязка завершилась»). `scroll-initial-target` — применять и к контейнерам, вставленным после загрузки (CSS-SPECS: 🟡); долю 12 id, объяснённую именно этим, не измеряли.

## Как проверить

`snap-events/scrollsnapchange/scrollsnapchange-on-programmatic-scroll.tentative.html`, `scroll-initial-target/scroll-initial-target-nested-container.tentative.html`.
