# BUG-738: out-of-flow и `display: none` дети раздували intrinsic-ширину родителя

**Статус:** FIXED 2026-08-10
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs` —
`max_content_outer_width`, `min_content_outer_width_of_contents`,
`preferred_inline_block_width`)
**Найден и исправлен:** P3 при разборе [BUG-733](BUG-733-FIXED.md), 2026-08-10

## Симптом

Расчёты max-content / min-content / shrink-to-fit обходили **всех** детей бокса
без разбора. Ребёнок с `position: absolute`/`fixed` меряется от своего
containing block и выкладывается отдельным проходом (CSS 2.1 §10.3.7), поэтому
в intrinsic-ширину родителя входить не должен; у `display: none` бокса
(`BoxKind::Skip`) её нет вовсе — даже его padding/border не имеют права попасть
в счёт.

Сверка с headless Edge (родитель с текстом 35.19 px + абсолютный потомок 300 px):

| Форма | Edge | Lumen до | Lumen после |
|---|---|---|---|
| flex-элемент с абсолютным потомком | 35.19 | 300.0 | 35.19 |
| `float: left` с абсолютным потомком | 35.19 | 300.0 | 35.19 |
| `inline-block` с абсолютным потомком | 35.19 | 35.19 | 35.19 |

(`inline-block` в Lumen «повезло» только потому, что потомком там был
`<span>` — inline-элемент без собственного бокса, [BUG-488](BUG-488-FIXED.md).)

## Корень

Ни одна из трёх функций не фильтровала детей: в блочной ветке шёл `max` по всем
`b.children`, во float-сумме — margin-box любого флоата, в `InlineBlockRow` —
сумма всех. Внутренний `flex_row_intrinsic_sum` из
[BUG-737](BUG-737-FIXED.md) исключение уже делал (там оно совпало с правилом
«абсолютный ребёнок не flex-элемент», Flexbox §4.1), но блочный путь — нет.

## Влияние на `tbank.ru`

Ровно эта форма у верхней навигации: каждый `<li>` пункта меню содержит
выпадающую мега-панель `div.ab2vFRdG2 { position: absolute }`, внутри которой
`div.ib2vFRdG2 { width: 1104px }`. Пункт «Частным лицам» раздувался с ширины
своей подписи (140 px) до 1104 px, четыре таких пункта не помещались в строку —
это и был пункт 1 [BUG-733](BUG-733-FIXED.md). После фикса (вместе с
[BUG-737](BUG-737-FIXED.md)) — четыре пункта в строку по 140.3 / 83.9 / 93.3 /
54.6 px в `<ul>` шириной 1042 px.

## Фикс

Предикат `contributes_to_intrinsic_width(child)` — `false` для `BoxKind::Skip` и
для `position: absolute|fixed`; применён во всех обходах детей в трёх функциях
(блочная ветка, float-сумма, `InlineBlockRow`) и переиспользован
`flex_row_intrinsic_sum`.

## Проверка

5 юнит-тестов `bug738_*` (flex-элемент, флоат, `display: none` с padding-ом,
`position: fixed`, контрольный in-flow случай), полный `lumen-layout` — 3533
теста зелёные. `--dump-display-list` по всем 158 страницам `graphic_tests/` до и
после — побайтово идентичны.
