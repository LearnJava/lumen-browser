# BUG-1556 — `getComputedStyle`: used values инсетов — `top: 1em` у статического бокса, `20px` вместо `23.2px` у абсолютного, `-1px` вместо `3px` у относительного

**Статус:** FIXED 2026-10-28 (P3); остаток — [BUG-1771](BUG-1771-OPEN.md), [BUG-1772](BUG-1772-OPEN.md)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/resolved_geometry.rs`, `crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

Остаток CSSOM-9: для `position: static` и боксов без блока (`display:none`, `getComputedStyle-insets-nobox/static`) resolved value `top/left/…` — computed (`1em`), по CSSOM §7.2 — тоже computed, но тесты ждут пересчёта в px (`expected "10px" but got "1em"`, 36 из 216 в `static`/`nobox`); у `absolute`/`fixed` — `23.2px` ожидалось, получено `20px`, `55.6px` против `30px` (процентные и `calc` инсеты от containing block с дробной высотой, 216 из 324); у `relative` — `bottom: 3px` против `-1px` (ширина/высота, процент), 144 из 252; у `sticky` — `10.4px` против `10.2px` (72 из 252). 8 id `getComputedStyle-insets-*` дают 796 из 900 упавших сабтестов кластера `om-getcomputedstyle` (1 972 всего).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<div style="top:1em;left:2em;margin-left:3em;width:5em">` при `font-size:10px` — `getComputedStyle().top/left/marginLeft/width` | `1em`/`2em`/`30px`/`50px` | `1em`/`2em` (static: computed) — тест ждёт `10px`/`20px` |
| `position:relative; top:1em; left:2em` — `top/left/right/bottom` | `10px, 20px, -20px, -10px` | `10px, 20px, -20px, -10px` |
| `getComputedStyle-insets-absolute.html` — первое расхождение | `20px` | `23.2px` |
| `getComputedStyle-insets-fixed.html` — первое расхождение | `30px` | `55.6px` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/getComputedStyle-insets-{absolute,fixed,nobox,relative,relpos-inline,static,sticky,sticky-container-for-abspos}.html`.

## Что делать

Довести `resolved_geometry` (CSSOM-9, CSSOM-9-S1/S2) до случаев: `static`/без бокса, дробные размеры containing block, `relative` с процентами и `calc`, `fixed` под `transform`.

## Как проверить

`css/cssom/getComputedStyle-insets-static.html`, `-absolute.html`.

## Причина и исправление (2026-10-28)

Пробы WPT `getComputedStyle-insets-*` воспроизведены в `lumen-driver` (`crates/driver/tests/cases/bug1556_inset_resolved_values.rs`: тот же `support/getComputedStyle-insets.js`, матрица 6×6 writing-mode, 1836 сабтестов; числа совпали с `.ini`: 216/216/36/144/36/72/72). Описание «дробные размеры containing block» оказалось следствием другого дефекта; настоящих причин семь:

1. **`clientWidth`/`clientHeight` включали рамки** (`web_api_shim_mid.js`: отдавали border-box из `_lumen_get_bounding_rect`). Тест берёт базу процентов из `clientHeight − padding`, и `10%` ждал `10.4px` вместо `10px` при рамках 2+2 — это 216+216+144+… «падений» absolute/fixed/relative/sticky. Теперь это padding box (рамки вычтены); добавлены `clientLeft`/`clientTop`.
2. **static / `display: none`**: `computed_style_to_map` отдавал `top: 1em` как написано; `em`/`rem` в инсетах теперь абсолютизируются (`inset_computed_css`), проценты и `calc()` остаются как были.
3. **relative**: `relative_pair` зеркалил старую сторону даже когда заданы обе (`bottom: 3px` → `-1px`); при двух заданных сторонах каждая отдаёт своё значение, зеркалится только `auto`.
4. **sticky**: проценты брались от padding box прокручиваемого предка, тест и Chrome — от content box (`GeomCtx::scrollport`).
5. **статическая позиция abspos в контейнере с `vertical-rl` или с началом строки снизу (`rtl` в вертикальном режиме)**: поток отдавал левый/верхний угол вместо правого/нижнего края (CSS Position §static-position); `static_edges` (`multicol_abspos.rs`) + флаг `static_y_is_bottom` в `place_abs_child`, производитель — `vertical_trampoline::finish_child`.
6. **высота вертикального abspos/fixed с `height: auto` и свободной инсетой** бралась как высота containing block (у absolute совпадало с 0 лишь потому, что первый проход шёл от блока нулевой высоты, `fixed` под `transform` перекладывался от настоящего CB и получал 300px). Теперь shrink-to-fit по `max_content_outer_height`.

WPT-раннер (`run_smoke.py`, `dev-release`): 7 файлов из 8 зелёные целиком, их `.ini` удалены; `relpos-inline` (4) и `grid` (1) остались — вынесены в BUG-1771. Дисплей-листы 177 страниц `graphic_tests` до/после совпали (три расхождения — свежие графтесты 169–171, которых нет в сравнивавшемся бинарнике).

Проверка: `cargo test --profile dev-release -p lumen-driver --test all -- bug1556`.
