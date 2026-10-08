# BUG-1237 — `closed_details_renders_summary_only_and_open_reveals_content` и `details_takes_first_summ

**Статус:** OPEN
**Тип:** не локализован.
**Заведён:** 2026-10-01 (P4, найден в гейте задачи `pointer-events`, не относится к ней)
**Область:** js (`crates/js/src/dom/tests/v8_gap_uashadowslot.rs:101,117`).

## Симптом

`closed_details_renders_summary_only_and_open_reveals_content` и `details_takes_first_summary_by_position_ignoring_slot_attribute` падают на чистом `main` (без правок P4): первая даёт `"false,true,true"` вместо `"true,true,true"` — `getComputedStyle(summary).length` равно 0 у `<summary>` закрытого `<details>`. Заодно `v8_runtime::tests::dom_suspend_focus::bounded_document_lock_waits_out_another_thread` (`dom_suspend_focus.rs:970`) упал один раз под нагрузкой полного прогона и прошёл в одиночном запуске — похоже на флаки по таймингу.

## Как проверить

`cargo test -p lumen-js --lib -- v8_gap_uashadowslot` на чистом `main` (стабильно красные два теста); `bounded_document_lock_waits_out_another_thread` — только под нагрузкой.

## Дополнение P1 (BUG-935 срез 55, 2026-10-01)

Независимая находка того же дефекта в гейте среза 55: оба теста падают детерминированно и
с отключённым журналом содержимого (`CONTENT_JOURNAL_DISABLED`), то есть срез 55 ни при чём.
Вероятный след — [BUG-1191](BUG-1191-FIXED.md): после него у скрытого элемента flat tree есть
boxless-запись, и проверки «скрыто» через `getComputedStyle(el).length === 0` устарели.
Замена «скрыто» на `checkVisibility()` чинит второй тест, но первый остаётся красным: в
состоянии `open` `p.checkVisibility()` даёт `false`, а после `removeAttribute('open')` у `p`
`length === 0` и `display === ''` — boxless-запись публикуется непоследовательно между полным и
инкрементальным флашем (причина не разобрана; на чистом `main` изолирующий прогон не делался).
