# BUG-1109 — кадр с открытым `<select>`-дропдауном блокирующе ждёт мьютекс документа

**Статус:** OPEN
**Заведён:** 2026-09-23 (P1, остаток [BUG-1108](BUG-1108-FIXED.md)).
**Область:** shell (`crates/shell/src/app/window_event/redraw_requested.rs` —
сбор оверлея открытого `<select>`: `src.document.lock().unwrap()` перед
`forms::collect_select_options` / `forms::build_base_select_dropdown`).

## Симптом (по коду, живьём не воспроизведено)

Тот же механизм, что [BUG-683](BUG-683-FIXED.md) срез 7 и
[BUG-1108](BUG-1108-FIXED.md): off-thread relayout движкового потока держит
`Document` на всё время каскада (на github.com 11-16 с), а кадр при открытом
списке `<select>` берёт его блокирующим `lock()` — окно не перерисовывается,
пока каскад не кончится. Дополнительно `unwrap()` на пути кадра: отравленный
лок (паника в потоке, державшем документ) роняет весь shell вместо того,
чтобы просто не рисовать дропдаун.

## Что сделать

1. Перевести на `try_lock`. Дропдауну из документа нужны опции
   (`collect_select_options`) и, для `appearance: base-select`, стили опций
   (`build_base_select_dropdown` читает `&doc`) — кэшировать собранные
   данные по ключу (адрес `Arc` документа, `NodeId` селекта), по образцу
   `Lumen::modal_dialog_cache` / `FocusedFieldSnapshot`; геометрию (скролл,
   вьюпорт) пересчитывать каждый кадр.
2. `unwrap()` → пропуск оверлея на `Err`.
