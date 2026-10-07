# BUG-1242 — инкрементальный флаш теряет `margin-left`/`margin-top` у неизменённых боксов

**Статус:** FIXED 2026-10-02 (P1)
**Тип:** корректность раскладки (геометрия, видимая JS: `getBoundingClientRect`, `getComputedStyle`, observers).
**Найден:** P1 при [BUG-935](BUG-935-OPEN.md) срезе 59, 2026-10-02. Не связан со срезом: воспроизводится с
`LUMEN_NO_STYLE_SKIP=1`, с отключённым журналом содержимого (`CONTENT_JOURNAL_DISABLED`) и с
`set_incremental_box_build(false)`.
**Область:** same-tick флаш `crates/js/src/v8_runtime/style_flush.rs` →
`layout_mutation_incremental_restyle` (`crates/engine/layout/src/box_tree/entry.rs`) →
`lay_out_incremental`.

## Симптом

```js
// .c { margin: 3px; padding: 2px; width: 200px; height: 20px }   body { margin: 0 }
var root = document.createElement('div'); document.body.appendChild(root);
for (var i = 0; i < 3; i++) { var d = document.createElement('div'); d.className = 'c'; root.appendChild(d); }
root.children[0].getBoundingClientRect();    // x=3 y=3   — первый флаш: полная раскладка
root.children[1].style.color = 'red';        // любая правка, дающая рестайл
root.children[0].getBoundingClientRect();    // x=0 y=0   — второй флаш: инкрементальный
```

Третья карточка: y 57 → 51. Потеряны ровно левое и верхнее поля у боксов, которых правка не касалась;
изменённая карточка (корень рестайла) своё поле сохраняет. По вариантам стиля карточки (`width:200px;
height:20px` + одно поле):

| правило | до | после правки соседа |
|---|---|---|
| `margin-left: 3px` | x=3 | x=0 |
| `margin-top: 3px` | y=3 | y=0 |
| `margin-bottom: 3px` | y=46 у третьей | y=46 (не меняется) |
| `margin: 3px; display: inline-block` | x=3 | x=0, y=3 сохранён |

`getComputedStyle(el).marginTop` при этом остаётся верным (`3px`): страдает `rect`, а не стиль.
Расхождение между «одним полным флашем в конце» и «флашем после каждого шага» — константа
(+27 px высоты `root` на 10 карточек с `margin:3px` в `v8_bug935_s59_style_skip.rs`, строка `gap`).

## Гипотеза (не подтверждена правкой)

`lay_out_inner` ставит `rect.x = start_x + margin_left`, `rect.y = start_y + margin_top`
(`layout_dispatch.rs:739`) — родитель передаёт `start_*` **без** собственных полей ребёнка. Быстрый путь
для чистого поддерева делает `translate_subtree(b, start_x - b.rect.x, start_y - b.rect.y)`
(`layout_dispatch.rs:596`): он переносит `rect` в `start_*`, то есть «съедает» то, что нормальный путь
прибавляет сверху — левое/верхнее поле, `auto`-центрирование (`:928`), `justify-self` (`:998`) и
смещение `position: relative` (то же, что симптом 2 [BUG-1240](BUG-1240-FIXED.md)). `margin-bottom`
не теряется, потому что курсор потока родителя считает по высоте, а не по `rect`.

Не сходится: проба на уровне крейта layout (`layout_measured_hyp_with_counters` →
`layout_mutation_incremental_restyle` с `RestyleDelta`, `ContentDirty::Nodes`, коробки переиспользуются)
**даёт верный результат** — до и после `(3,3)`, `(3,30)`, `(3,57)`. Чем флаш отличается от неё,
не выяснено: возможно, чистыми в флаше оказываются боксы, чьё поддерево приехало из `basis.layout`
после `scroll_restore`/коллекторов, а в пробе — нет.

## Почему не видно раньше

[BUG-1238](BUG-1238-FIXED.md) сравнивал инкрементальную геометрию с полной на карточках `.c` **без**
полей (`padding: 2px; width: 200px`); сценарий `auto_margin` красит только изменённую карточку.
В живом окне путь не включён по умолчанию (ADR-016 M4 мёртв, BUG-935), так что вред — чтения геометрии
из JS в том же такте, что и правка DOM: после [BUG-935](BUG-935-OPEN.md) срез 58 на страницах с правкой
CSSOM или `:has()` такие флаши стали инкрементальными.

## Как проверить

Проба выше через `V8JsRuntime::eval` (`crates/js/src/dom/tests/v8_bug935_s59_style_skip.rs` строит
ту же страницу: ветка `gap` в `skipped_computed_style_entries_equal_the_rebuilt_ones` печатает
расхождение при `--nocapture`). Исправление проверять добавлением `margin: 3px` и `.c:first-child
{ margin-left: 7px }` в сценарии `v8_bug1238_scoped_collectors.rs`.

## Исправление (BUG-935 срез 60)

Гипотеза подтвердилась, а «проба на уровне крейта даёт верный результат» объяснилась: проба не имела
чистых боксов с полями. Воспроизведение на уровне layout нашлось, когда срез 60 оставил чистые коробки под
перестроенным родителем (`box_tree::tests::bug935_shallow_roots`, карточки с `margin: 1px`).

* `layout_dispatch.rs::clean_box_shift` — быстрый путь чистого бокса переносит его не в `start_*`, а в
  `start_* + margin-left/top` (то, что `lay_out_inner` прибавляет: `rect.x = start_x + margin_left`).
  Боксы, чьё положение поля не объясняют, — `auto`-поля, `justify-self: center|end`, `position: relative` —
  раскладываются по-настоящему, а не переносятся.
* `incremental.rs::containing_block_style_changed` не знал про `margin-left`/`margin-right` (ширина,
  которую автоширинный бокс отдаёт детям), `position`, `float`, `left`/`right`: ребёнок-`InlineRun` брал
  устаревшую ширину после смены поля у родителя.

Тесты: `clean_boxes_keep_their_margins_centering_and_relative_offsets` (layout, поля/`auto`/`relative`/
`justify-self`) и `a_flush_after_an_unrelated_write_keeps_the_margins` (JS, проба из «Симптома»; падает на
старом быстром пути — проверено). Симптом 2 [BUG-1240](BUG-1240-FIXED.md) (смещение `relative` у чистого
поддерева пропадает) этим тоже снят; симптом 1 (смещение утекает в поток при полной раскладке) — нет.
