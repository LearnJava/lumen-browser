# BUG-1429 — `ex`, `cap`, `ic`, `ch` не берут метрики веб-шрифта из OS/2 (`sxHeight`, `sCapHeight`): `10ex` у шрифта с `sxHeight` = em/8 даёт 437 px вместо 100

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/font (`crates/engine/layout/src/style/values/length.rs` — `FONT_CH_EX`/`FONT_LH`)

## Симптом

`--screenshot`, `body{margin:0}`, `<div style="height:10px;background:green;font:Npx F; width:<L>">`, число зелёных в строке:

| страница | получено | ожидается |
|---|---|---|
| `ExTest.woff` (`sxHeight` = em/8), `font:80px ExTest; width:10ex` | **437** | 100 |
| `ExTest.woff`, `width:1em` (контроль) | 80 | 80 |
| `ChTestShortZero.woff`, `font:100px C; width:1ch` | 100 | 100 |
| `IcTestFullWidth.woff2`, `font:100px I; width:1ic` | 100 | 100 |
| Ahem 20px: `5ch` | 100 | 100 |
| Ahem 20px: `5ex` | 55 | 80 (x-height Ahem 0.8 em → 16·5) |
| Ahem 20px: `5cap` | 70 | 80 (cap-height Ahem 0.8 em) |

`ch`/`ic` верны, `ex` и `cap` — нет (метрика берётся не из шрифта страницы). Тот же механизм у `FONT_CH_EX` в `length.rs:166–174`.

## Как найдено

WPT-RUN-14 срез 19: `css-values/ex-unit-00{1…3}.html`, `cap-unit-001.html`, `calc-ch-ex-lang.html`, `ch-unit-0xx.html`, `ic-unit-0xx.html`,
`lh-unit-00x.html` (25 reftest `thick`), `*-invalidation.html`/`*-recalc-on-font-load.html` (12 testharness). `ch-unit-004` и
`ic-unit-016` пробой не проверялись: в вертикальном режиме (`ch-unit-004`) ширина `5ch` у Ahem — 100×60 (верно) — причина падения не
в единице; в `ic-unit-016` (`ic` у шрифта без CJK-глифа) ожидается откат на `1em`.

## Что делать

Брать `ex`/`cap` из OS/2 `sxHeight`/`sCapHeight` шрифта первой подходящей грани (и откат 0.5 em/… только если метрики нет),
пересчитывать после загрузки шрифта.

## Как проверить

Страница из таблицы (`ExTest.woff` лежит в `css/css-values/resources/`); `css/css-values/ex-unit-003.html`, `cap-unit-001.html`.

## Исправление

`ex` к моменту работы уже читал `OS/2.sxHeight` гарнитуры бокса (BUG-1340, `x_height_px_with_families`): проба `10ex` с `ExTest.woff` даёт 100 px, Ahem `5ex` — 80. Оставался `cap`: парсер подставлял `Length::Em(n * 0.7)`, метрики шрифта не читались (Ahem `5cap` — 70 вместо 80).

Добавлена единица `Length::Cap`. `TextMeasurer::cap_height_px{,_with_families}` ([`lib.rs`](../crates/engine/layout/src/lib.rs)) читают `OS/2.sCapHeight` (при отсутствии или нуле — `0.7 em`) у `FontMeasurer` и `OwnedFontMetrics` ([`paint/src/lib.rs`](../crates/engine/paint/src/lib.rs)); `FontContext::enter` публикует тройку `(ch, ex, cap)` в `FONT_CH_EX`, `Length::resolve` берёт `cap` оттуда (запасной вариант вне раскладки — `0.7em`). `calc()`, анимация, сериализация и `font-size` знают новый вариант.

Тесты: `cap_height_with_families_uses_registered_font_ahem_is_0_8_em` (paint), `length_resolve_ch_ex_*` (layout). Проба: Ahem `5cap` = 80, `5ex` = 80, `5ch` = 100, `ExTest` `10ex` = 100, `ic` = 100.

## Остаток

`rcap`/`ric`/`ic` по-прежнему приближены (`rcap` = `0.7 rem`, `ic` = `1em`) — метрики корневого шрифта и CJK-глифа не читаются; пересчёт после загрузки `@font-face` в headless-проходе не проверен (`document.fonts.ready` там не срабатывает).
