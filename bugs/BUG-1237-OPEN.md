# BUG-1237 — `v8_gap_uashadowslot`: два теста красные после BUG-1191

**Статус:** OPEN
**Тип:** тесты / корректность (`getComputedStyle` у содержимого закрытого `<details>`).
**Найден:** P1, гейт среза 55 [BUG-935](BUG-935-OPEN.md), 2026-10-01.
**Область:** `crates/js/src/dom/tests/v8_gap_uashadowslot.rs`; код — след
[BUG-1191](BUG-1191-FIXED.md) (`collect_computed_styles`, boxless-запись `BOXLESS_KEY`).

## Симптом

`cargo test -p lumen-js --lib --features v8-backend v8_gap_uashadowslot` — 2 из 5 падают
детерминированно:

- `closed_details_renders_summary_only_and_open_reveals_content` — `false,true,true` вместо
  `true,true,true`;
- `details_takes_first_summary_by_position_ignoring_slot_attribute` — `true,true,true` вместо
  `true,false,false`.

Оба утверждают «скрыто» через `getComputedStyle(el).length === 0`. После BUG-1191 у скрытого
элемента flat tree запись есть (boxless), поэтому проверка устарела по смыслу.

## Что проверено

Падает и с выключенным журналом содержимого среза 55 (`CONTENT_JOURNAL_DISABLED`), т. е. не
следствие среза 55; на чистом `main` не прогонялось (изолирующий прогон — первое действие
исполнителя).

Замена «скрыто» на `checkVisibility()` чинит второй тест, но первый остаётся красным: в состоянии
`open` `p.checkVisibility()` даёт `false`, а после `removeAttribute('open')` у `p` `length === 0`
и `display === ''` — то есть boxless-запись публикуется непоследовательно между полным и
инкрементальным флашем. Причина не разобрана.

## Что требуется

Выяснить, какая из двух сторон неверна (тест или публикация boxless-записи в инкрементальной
ветке `maybe_flush`), починить, оставить тест, различающий «скрыт» и «отрисован» через
`checkVisibility()`.
