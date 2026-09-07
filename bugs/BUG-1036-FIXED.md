# BUG-1036 — остаток непроверенных `doc.get(nid)` в `dom_core.rs`/`platform.rs` панически падал на чужом/устаревшем `NodeId`

**Статус:** FIXED 2026-09-08 (P3)
**Заведён:** 2026-09-08 (P3), при разборе верхнего указателя `STATUS-P3.md` → [BUG-1031](BUG-1031-OPEN.md)
**Область:** js (`crates/js/src/v8_runtime/install/dom_core.rs`, `crates/js/src/v8_runtime/install/platform.rs`)

## Симптом

[BUG-1024](BUG-1024-FIXED.md)/[BUG-1030](BUG-1030-FIXED.md) закрыли часть класса «прямой из JS `NodeId` бьёт в
`Document::get`/`get_mut` без `contains_id`/`try_get`-проверки» (панику ловит `catch_unwind` в
`native_fn_trampoline`, но до фикса BUG-1024 панике внутри `Mutex<Document>` удавалось отравить
мьютекс — следующий `.lock().unwrap()` того же документа тоже паниковал, каскадом). BUG-1030's
описание явно называло один оставшийся живой вход: `_lumen_get_style_property` (platform.rs).
Аудит (`grep -n "doc\.get(" crates/js/src/v8_runtime/install/{dom_core,platform}.rs`, вручную
проверен каждый результат — валиден ли `nid` до чтения) нашёл ещё пять непроверенных путей:

- `_lumen_get_style_property`/`_lumen_set_style_property`/`_lumen_delete_style_property`/
  `_lumen_get_style_entries` (platform.rs) — `CSS Typed OM`'s `attributeStyleMap`, читается/
  пишется напрямую с раскладки `nid` от JS без всякой проверки;
- `_lumen_is_shadow_root`/`_lumen_is_document_fragment` (dom_core.rs) — прямой `doc.get(id).data`
  без guard'а;
- `_lumen_get_shadow_root_host` (dom_core.rs) — вход цикла шёл от непроверенного `nid`, хотя
  последующие шаги цикла уже безопасны (`node.parent` — внутренний, валидный id);
- `_lumen_set_selection` (dom_core.rs) — не читает документ напрямую, но **пишет** непроверенный
  `NodeId` в `Document::selection`; паника случается позже, в любом натива, читающем
  `Selection::anchor`/`focus` (`_lumen_contenteditable_delete_backward`/`_forward`).

## Почему это важно

Тот же класс, что и растущий счётчик «N `--check` подряд без изменений дают N разных наборов
регрессий» ([BUG-1003](BUG-1003-OPEN.md)/[BUG-1004](BUG-1004-OPEN.md)/[BUG-1005](BUG-1005-OPEN.md)/
[BUG-1011](BUG-1011-OPEN.md)/[BUG-1022](BUG-1022-OPEN.md)/[BUG-1031](BUG-1031-OPEN.md)) —
любой из этих шести живых входов, если тест его заденет, отравляет `Mutex<Document>` и валит
случайный набор последующих тестов в том же процессе, не только собственный.

**Проверено и отклонено как причина [BUG-1031](BUG-1031-OPEN.md):** живой прогон
`soft-navigation-heuristics` (`run_report.py --update-expected`, затем два `--check` подряд) **до
и после** этого фикса дал непересекающиеся наборы регрессий оба раза (18 и 21 соответственно) —
симптом BUG-1031 воспроизводится этим же бинарём один в один, значит его причина — другой, ещё
не найденный механизм, не эта конкретная дыра. BUG-1031 остаётся `OPEN`.

## Фикс

Все шесть натив-точек переведены на bounds-checked доступ, тем же паттерном, что BUG-986/1024:

- четыре style-натива в platform.rs — `doc.try_get(nid)` вместо `doc.get(nid)`, деградация в
  пустую строку/no-op;
- `_lumen_is_shadow_root`/`_lumen_is_document_fragment` — `doc.try_get(id).map(|n| &n.data)`
  вместо `doc.get(id).data`, деградация в `false`;
- `_lumen_get_shadow_root_host` — `doc.try_get(cur)?` перед циклом (единственная точка входа
  чужого id — остальные шаги цикла идут по `node.parent`, уже внутренним id);
- `_lumen_set_selection` — `contains_id` на оба id ДО записи в `Document::selection` (тем же
  `log_foreign_node_id`-паттерном, что `_lumen_append_child`/BUG-986), не только деградация на
  чтении.

## Регресс-тесты

- `crates/js/src/dom/tests/v8_perf_typedom_node.rs::native_binding_get_style_property_foreign_node_id_is_silently_skipped`
  (переименован из `native_binding_panic_does_not_abort_process` — вход больше не панический, см.
  ниже про перенос покрытия BUG-418);
- `crates/js/src/dom/tests/v8_fontface_shadow_custom.rs::shadow_natives_degrade_on_foreign_node_id_instead_of_panicking`;
- `crates/js/src/dom/tests/v8_selection_range_editing.rs::set_selection_rejects_foreign_node_id_instead_of_corrupting_state`;
- `crates/js/src/frame_bridge.rs::tests::f_children_native_binding_panic_does_not_abort_process` —
  **новый живой вход** для регресс-теста BUG-418 (catch_unwind), поскольку `dom_core.rs`/
  `platform.rs` больше не содержат ни одного непроверенного прямого-из-JS `doc.get(nid)`.
  `frame_bridge.rs`'s кросс-фрейм читающие нативы (`_lumen_f_*`) документированы в файле
  (`checked_node`'s doc comment) как намеренно не проверяющие id на чтение — отдельный,
  больший периметр, не в объёме этого бага.

## Гейты

`cargo build -p lumen-js --profile dev-release --features v8-backend` — чисто.
`cargo test -p lumen-js --lib --features v8-backend` — 3544/3544.
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` не прогнан до
конца: системный `rustc 1.98.1` вместо пина 1.97.0 на этой машине красит несвязанные
`lumen-image`/`lumen-font` (`chunks_exact_to_as_chunks` — лint, которого нет в 1.97) ещё на
этапе сборки зависимостей `lumen-js`, до того как clippy доходит до кода этой задачи (тот же
случай, что в BUG-1024/BUG-1030).
