# BUG-1486 — `text-transform`: греческая огласовка (`lang=el`) и контекст `capitalize` между текстовыми узлами

**Статус:** FIXED 2026-10-23 (P3)
**Заведён:** 2026-10-08 (P6, остаток [BUG-1329](BUG-1329-FIXED.md))
**Область:** layout/js (`crates/engine/layout/src/style/values/typography.rs` — `case_upper`, `TextTransform::apply_lang`; `crates/engine/layout/src/box_tree/inline_build.rs` — каждый текстовый узел преобразуется отдельно)

## Симптом

1. `lang=el`, `uppercase`: слова, целиком переведённые в верхний регистр, теряют тонос (`καλημέρα αύριο` → `ΚΑΛΗΜΕΡΑ ΑΥΡΙΟ`), `ΰ`/`ΐ` теряют тонос и сохраняют диалитику, дифтонг с тоносом на первом символе даёт диалитику на втором (`ευφυΐα Νεράιδα` → `ΕΥΦΥΪΑ ΝΕΡΑΪΔΑ`); дизъюнктивная `ή` сохраняет тонос (`ήσουν ή εγώ` → `ΗΣΟΥΝ Ή ΕΓΩ`). `capitalize`: ударный гласный первого слога сохраняет тонос (`όμηρος` → `Όμηρος`). Сейчас греческий идёт по обычному Unicode: тонос остаётся (`ΚΑΛΗΜΈΡΑ`). WPT `text-transform-tailoring-002/002a/003/004/005`.
2. `capitalize` не видит символ до границы текстового узла: `a<span style="text-transform:capitalize">b</span>c` даёт `B`, ожидается `b` (слово началось в предыдущем узле). Состояние `is_word_start` живёт внутри одного вызова. Нужен контекст «предыдущий символ» из соседнего сегмента — но пробельный узел между inline-элементами (`<b>a</b> <i>b</i>`) в `collect_inline_segments` пропускается и контекста не оставляет, так что простое «последний символ предыдущего сегмента» ломает обычный случай. WPT `text-transform-capitalize-036` («not starting at word boundary»); `innerText` повторяет раскладку.
3. Не разобрано: `capitalize-*` reftest'ы `thick` (6 из 7 id), остальные `text-transform-upperlower-*` (кроме `-039…-044`, `-107`) — см. [`docs/wpt-vendor-notes/css.md`](../docs/wpt-vendor-notes/css.md). Ирландское правило `ga` (`tAthair`) реализовано по тексту CSS Text L3 §2.1, WPT-теста на него в наборе нет.

## Что делать

Греческая таблица: тонос/диалитика по правилам «Modern Greek uppercase» (CLDR `el-Upper`), с признаком слова целиком в верхнем регистре и исключением для `ή`; передавать в `case_upper` соседей слова. Для п. 2 — «предыдущий символ» с учётом схлопнутого пробела между сегментами (в раскладке и в `_lumen_rt_*`).

## Исправление

- `typography.rs`: `CaseLang::Greek` (`el`); `case_upper` для него идёт через `greek_upper` (CLDR `el-Upper`, слово за словом): тонос снимается, диалитика остаётся (`ΐ`/`ΰ` → `Ϊ`/`Ϋ`), дифтонг с тоносом на первом символе даёт диалитику на втором (`άι` → `ΑΪ`), отдельное слово `ή` сохраняет тонос. Комбинируемые U+0301/U+0308 (NFD) сливаются с предыдущей гласной. `capitalize` остаётся обычным Unicode — ударная первая гласная и так сохраняет тонос (`όμηρος` → `Όμηρος`).
- Контекст слева для `capitalize`: `TextTransform::apply_lang_after` / `TextTransformExtra::apply_after` / `TextCssomExtra::transform_text_after` принимают `before` (две последние буквы текста перед узлом в той же inline-строке). Раскладка (`inline_build.rs::text_before`) берёт хвост последнего сегмента `out`: схлопнутый пробел между inline-элементами уже дописан в его `text`, поэтому `<b>a</b> <i>b</i>` по-прежнему даёт `B`, а `a<span>b</span>c` — `b`; принудительный перенос и атомарный бокс (`escapes`) — граница слова.
- `innerText`: шим (`_lumen_rt_before` / `_lumen_rt_tail`) собирает тот же хвост по предыдущим соседям в тех же inline-предках и передаёт его в `_lumen_text_transform` (`style::transform_text_after_by_value`).

Тесты: `style/tests/text_cssom_tests.rs::case_lang` (`greek_*`, `capitalize_continues_*`), `tests/box_sizing_text_props.rs::text_transform_greek_and_capitalize_across_nodes`, сквозной `crates/driver/tests/cases/bug1329_text_transform_lang.rs::greek_tonos_and_capitalize_across_nodes`. WPT: `text-transform-tailoring-002/002a/003/004/005` — текст теста и эталона совпадают побайтно (шрифтовая подстановка у обоих одна); `capitalize-036` — подтест «not starting at word boundary».

Остаток п. 3 (`capitalize-*` reftest'ы `thick`, прочие `upperlower-*`) в этом баге не разбирался — каждый разбирается по результату прогона категории (см. `docs/wpt-vendor-notes/css.md`).
