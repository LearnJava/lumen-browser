# BUG-469: zero-width new-formatting-context box not positioned into a zero-width gap between floats

**Статус:** FIXED 2026-09-01
**Дата:** 2026-08-02
**Компонент:** layout (float positioning, CSS2.1 §9.5.1)
**Найден:** WPT-RUN-1 (`docs/wpt-status.md`, точечная проверка на 4 файлах,
2026-08-02) — формализовано в BUG-NNN во время WPT-RUN-3 срез 2

## Симптом

`css/CSS2/floats/zero-space-between-floats-00{1..4}.html` — все 4 файла,
1/1 сабтест FAIL каждый:

```
FAIL <case> - assert_equals:
undefined
offsetLeft expected 100 but got 0
```

Каждый тест зажимает нулевой ширины блок (`overflow:hidden; width:0`,
новый formatting context) между двумя float'ами (либо после float + clearance)
так, что для него остаётся ровно нулевой ширины "щель" в потоке. Спека
(9.5.1) требует разместить такой блок именно в эту щель — тестируемый
`offsetLeft`/`offsetTop` указывает на её позицию (100px после левого float,
или ниже float'ов при `clear`). Lumen вместо этого кладёт блок в (0, 0) —
как будто float'ы вообще не влияют на его позицию.

Обнаружено ещё во время точечной проверки WPT-RUN-1 (см. `docs/wpt-status.md`
→ строка `css`, "Точечная проверка... подтверждает DoD: хелпер теперь
выполняется и даёт содержательные фейлы с числами... похоже на реальный
дефект позиционирования float с нулевым зазором"), тогда не заводился
отдельным BUG-NNN — заведён сейчас как часть группировки провалов
(WPT-RUN-3 срез 2).

## .ini

`tests/wpt/metadata/css/CSS2/floats/zero-space-between-floats-{001,002,003,004}.html.ini`
— по одному сабтесту `expected: FAIL` в каждом. **Оставлены `expected: FAIL`
после фикса** — см. «Фикс» ниже, сабтест по-прежнему красный по другой,
уже заведённой причине (BUG-476).

## Фикс (P3, 2026-09-01)

Живая раскладка (`--dump-layout`) на всех 4 фикстурах показала: геометрия
самого нулевой-ширины BFC-бокса УЖЕ была верна для тестов 001-003 (сужение
через `FloatContext::left_edge_at`/`right_edge_at`, `box_tree/layout_dispatch.rs`,
безусловно применяется к любому ребёнку до входа в BFC-специфичную ветку) —
исходное описание «кладёт блок в (0,0)» устарело относительно текущего кода.
Реальный, воспроизводимый дефект — только в тесте 004 (`clear:right`
приземлялся на y=100 вместо ожидаемых 200).

**Корень:** `layout_dispatch.rs::lay_out_inner`, ветка float (§9.5.1) —
когда у float'а задана явная (в т.ч. процентная) `width`, её базой
(`available_width`, он же `cb` для резолва `%`) служил `probe_avail` —
ширина ТЕКУЩЕГО зазора между уже расставленными float'ами, а не полная
containing-block-ширина контейнера (CSS 2.1 §10.3.5 — процент у float'а
резолвится ровно так же, как у него же не-float, т.е. против того же CB,
что и обычный блок). `float:right; width:100%`, зажатый между двумя уже
стоящими 100px float'ами (нулевой зазор), резолвился в ширину **0** вместо
200px — не проходил rule 8 «не влезает → падает на новую строку»
(`layout_dispatch.rs:1134-1152`), регистрировался в `FloatContext` с
заниженным нижним краем, и именно этот неверный нижний край читал
`clear:right` следующего блока (тест 004). Тот же баг был и во «re-layout
на упавшей строке» (когда float всё же падает по другой причине) —
`avail_w` там тоже была локальным зазором, а не CB.

**Правка:** обе точки резолва явной ширины float'а (`probe_w` и `w` в ветке
`if dropped`) переведены с `probe_avail`/`avail_w` на `content_width`
(полная content-box-ширина родителя — та же величина, что уже
использовалась для не-float-блоков в соседней ветке чуть ниже). Margin'ы
float'а по-прежнему резолвятся против локального зазора (в этой правке не
трогались — ни один из 4 фикстур BUG-469 не использует процентные margin'ы
у float'а, риск регрессии не оправдан без покрывающего теста).
`crates/engine/layout/src/box_tree/layout_dispatch.rs`.

Регресс-тест —
`box_tree::tests::generated_float::bug469_full_width_float_squeezed_between_floats_still_clears_below_it`
(`crates/engine/layout/src/box_tree/tests/generated_float.rs`): float:left
100px + float:right 100px + float:right width:100% + clear:right, проверяет
`clear:right`-блок садится на y=200, а не y=100.

**Остаток — НЕ этот баг.** Живая проверка через `--mcp-port`/`eval`
(`node.offsetLeft`/`offsetTop`, все 4 фикстуры) подтвердила: после фикса
Y-геометрия теста 004 сдвинулась ровно на 100px (108→208 viewport-abs,
т.е. 100→200 относительно контейнера) — фикс работает. Но
`check-layout-th.js` читает `node.offsetLeft`, а Lumen отдаёт
viewport-абсолютные координаты вместо смещения относительно
`offsetParent` (BUG-476) — во всех 4 тестах результат ровно на 8px (body
margin) отличается от ожидаемого `data-offset-x`/`data-offset-y`.
Сабтесты `zero-space-between-floats-{001..004}.html` останутся FAIL до
починки BUG-476; `.ini` не флипнут в PASS.
