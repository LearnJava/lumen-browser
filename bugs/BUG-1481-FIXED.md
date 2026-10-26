# BUG-1481 — `justify-self`/`align-self`/`place-self` не выравнивают `position:absolute` бокс внутри содержащего блока (CSS Box Alignment 3 §«Absolute-Position»)

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — абсолютно позиционированный бокс: `justify-self`/`align-self`/`place-self` для бокса с обоими инсетами)

## Симптом

Для абсолютного бокса с `inset:0` и заданными шириной/высотой `justify-self:center`, `align-self:end`, `place-self:end center` игнорируются: бокс остаётся в `(0,0)`. `margin:auto` тот же бокс центрирует верно. Блок 50×50 в контейнере 200×100 (`inset:0`): `justify-self:center; align-self:center` → `(0,0)` вместо `(75,25)`; `end`/`end` → `(0,0)` вместо `(150,50)`; `place-self:end center` → `(0,0)` вместо `(75,50)`. Связано с якорным позиционированием (`anchor-center` — кластер `anchor-center` среза, 14 id: `justify-self: anchor-center` не читается обратно из `element.style`). 4 id `css-position/position-absolute-center-001/002/006/007` — по имени файла и пробе на `div`; `-003…005` проходят.

## Проба

Проба (`--mcp`, контейнер `position:relative;width:200px;height:100px`, цель `position:absolute;inset:0;width:50px;height:50px`):

| стили | у нас | ожидается |
|---|---|---|
| `margin:auto` | `(75,25)` | `(75,25)` |
| `justify-self:center; align-self:center` | `(0,0)` | `(75,25)` |
| `justify-self:end; align-self:end` | `(0,0)` | `(150,50)` |
| `place-self:end center` | `(0,0)` | `(75,50)` |

## Как найдено

WPT-RUN-14 срез 21: `css-position/position-absolute-center-001.html`, `-002.html`, `-006.html`, `-007.html`.

## Что делать

После определения размера абсолютного бокса применять `justify-self`/`align-self` (`normal`/`stretch` → начало, `center`, `start`/`end`, `safe`/`unsafe`) к свободному месту между инсетами, когда оба инсета по оси не `auto`. Тот же код нужен `anchor-center`.

## Как проверить

Таблица выше; `css/css-position/position-absolute-center-001.html`.

## Повторное измерение: WPT-RUN-14 срез 24 (2026-10-08)

`css/css-align/abspos/*` (67 id, 856 из 1 086 сабтестов): `{align,justify}-self-*-{htb,vlr,vrl}-*`, `*-default-overflow-*`, `safe-*-self-*`, `stretch-intrinsic-size-*`, `table-*-self-stretch`. Тот же код нужен для статической позиции ([BUG-1528](BUG-1528-OPEN.md)) и размера по `justify-self` без `width` ([BUG-1526](BUG-1526-FIXED.md)).

## Исправление

`place_abs_child` (`crates/engine/layout/src/box_tree/multicol_abspos.rs`): ось с двумя заданными инсетами и
определённым размером (`width`/`height` либо `anchor-size()`) раньше всегда брала начало (`left`/`top`) и сдвигала
бокс только `auto`-полями. Теперь без `auto`-полей по оси бокс выравнивается `justify-self` / `align-self` /
`place-self` внутри блока, суженного инсетами (`abs_self_aligned_start`): `start`/`end`/`center`/`self-start`/`self-end`/
`left`/`right`, `safe` при переполнении прижимает к началу, `last baseline` → `end`; `auto`/`normal`/`stretch` не
сдвигают. Оси — физические по `writing-mode` и `direction` самого бокса (они наследуются от содержащего блока);
`auto`-поля по-прежнему перекрывают выравнивание. Тесты: `box_tree/tests/abs_auto_margins.rs` (таблица пробы из
симптома, rtl, поля, `safe`).

**Остаток (не закрыт этим багом):** `position-absolute-center-006/007` — это `display: table` с `margin: auto` и
нулевыми инсетами, а не `justify-self` (имя файла в заявке указывало на него ошибочно); `auto` значения
`justify-self`/`align-self` не берутся из `justify-items`/`align-items` содержащего блока; размер бокса без `width`
по `justify-self` — [BUG-1526](BUG-1526-FIXED.md), статическая позиция — [BUG-1528](BUG-1528-OPEN.md). Проба в юнит-тесте
показала, что `display:table` с единственным блочным потомком без табличных частей даёт ширину 0 даже в потоке —
проверить на настоящей странице отдельно.
