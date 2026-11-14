# BUG-1784 — getComputedStyle боксового элемента отдаёт пустой `display` (`""` вместо `"inline"`)

**Статус:** FIXED 2026-11-14 (P3)
**Заведён:** 2026-11-04 (воркер 1 zshift, найден в проверке проекта задачи VTT-RENDER-S2 — к ней не относится)
**Область:** layout / driver (`crates/driver/tests/cases/bug1191_boxless_computed_style.rs`, getComputedStyle элемента без `LayoutBox`)

## Симптом

Тест `cases::bug1191_boxless_computed_style::boxless_elements_report_full_computed_style`
(введён фиксом [BUG-1191](BUG-1191-FIXED.md), b114c0c4c1) падает на `main`:

```
thread 'cases::bug1191_boxless_computed_style::boxless_elements_report_full_computed_style'
panicked at crates\driver\tests\cases\bug1191_boxless_computed_style.rs:32:5:
assertion `left == right` failed
  left: ""
 right: "inline"
```

Страница теста: пустой `<span id="empty">` в плоском дереве (`#host a<span>b`), плюс
элементы внутри `display:none` и `content-visibility:hidden`. Предшествующие ассерты
`getComputedStyle(...).length > 0` для всех трёх элементов **проходят** — computed style
не пуст, пусто именно значение `display` у `#empty` (до первого падения дело
до `in-none`/`in-cv` не доходит).

## Падает на чистом main — проверено

2026-11-04, слот zs1-work: stash-эксперимент — все правки VTT-RENDER-S2 сняты
(`git stash push`), дерево = `d107c6571` (merge VTT-RENDER-S1) + строка TASKS.md
(69a17ed7d), прогон повторён, правки возвращены (`git stash pop`). Результат
одинаков с правками и без (exit 101): правки S2 (настройки cue, vtt_geometry.rs,
js-пламбинг) не касаются каскада/getComputedStyle.

## Механизм (найден 2026-11-14, без бисекта — чтением снимка)

Запись `#empty` в снапшоте содержит ровно три свойства (`text-transform`,
`visibility`, `white-space`) — это **текстовая** карта сегмента
(`inline_segment_style_map`), а не бокслесс-запись BUG-1191. Цепочка:

1. [BUG-1558](BUG-1558-OPEN.md) добавил пустому inline-элементу маркерный нулевой
   сегмент (`InlineSegment::EMPTY_INLINE_OFFSET`, `source_node` = сам элемент) —
   только для `getBoundingClientRect`/`offsetLeft`, которые читают сегменты прямо
   из box tree (`inline_run_empty_elements`).
2. `collect_computed_styles_parts` для каждого сегмента публикует текстовую
   трёхсвойственную карту по `seg.source_node` (`or_insert_with`) — для маркера
   это ключ самого пустого span'а.
3. Бокслесс-бэкефил BUG-1191 (`publish_boxless_computed_style`) заполняет только
   свободные ключи — ключ span'а занят, полная каскадная карта с `BOXLESS_KEY`
   не публикуется. `display` = `""`, а `checkVisibility` пустого inline отвечал
   false по ветке `disp === ''` (по бокслесс-ключу — тоже false, наблюдаемое
   значение не менялось).

Подозреваемые из гипотезы (af1cda3ba FIELDSET-LEGEND-4, 46b046133) не при чём:
конфликт двух слайсов — BUG-1558 (маркер, позже) против BUG-1191 (бэкефил, раньше).

## Исправление

`collect_computed_styles_parts` (`crates/engine/layout/src/lib.rs`) пропускает
маркерные сегменты (`seg.is_empty_inline_marker()`) целиком: они не публикуют
ни трёхсвойственной карты по source-узлу, ни карт предков. Ключ элемента
остаётся свободным, и бэкефил BUG-1191 публикует полную каскадную карту с
`BOXLESS_KEY` — тот же контракт, что для любого бокслесс-элемента. Геометрия
(`getBoundingClientRect`) не задета: она читает `InlineRun.segments` из box tree,
не из снимка computed styles.

## Как проверить фикс

`cargo test -p lumen-driver --test all bug1191` — оба теста зелёные
(2026-11-14). Scoped-гейт (layout + driver + обратные зависимости) и
`dump_golden.py` — без новых красных относительно чистого main.

## Остаток (вне этого репро, тот же слайс-конфликт)

Пустой inline с ненулевыми горизонтальными edge (`<span style="padding-left:2px"></span>`,
слайс BUG-1530) публикует **не** маркерный сегмент: `edge(pre, post)` с
`source_char_offset = 0`, поэтому пропуск выше его не берёт — та же ветка
`or_insert_with` занимает ключ элемента трёхсвойственной картой, и `display`
ожидаемо `""`. Заводить без замера не стал; воспроизводимость — один
driver-пробный eval по образцу пробы «до» правки.
