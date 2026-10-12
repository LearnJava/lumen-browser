# BUG-1613 — Закрытие фоновой вкладки слева сбивает активную и затирает снимок

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** shell (`crates/shell/src/tabs/strip.rs:201`, `lumen/tabs_cmd.rs:531`)

## Симптом и место в коде

TabStrip::remove (crates/shell/src/tabs/strip.rs:201-210) не уменьшает self.active, если удалена вкладка левее активной. Он только ограничивает индекс длиной списка. close_tab для фоновой вкладки (lumen/tabs_cmd.rs:250-257) результат remove не использует. Сценарий: вкладки [A, B*, C], закрываем A крестиком в вертикальной полосе. В полосе активной становится C, а на экране остаётся живая страница B. Следующий switch_tab (tabs_cmd.rs:531-547) сохранит живую страницу B под id вкладки C (bg_tabs.insert(old_id)) и затрёт снимок C. close_others и close_right находят активную вкладку заново по id (strip.rs:311-356), а remove — нет. Тест remove_tab_clamps_active (strip.rs:578-585) проверяет только удаление самой активной вкладки. Найдено при сверке, а не в исследовании.

## Где смотреть

- crates/shell/src/tabs/strip.rs:201-210
- crates/shell/src/lumen/tabs_cmd.rs:250-257
- crates/shell/src/lumen/tabs_cmd.rs:531-547
- crates/shell/src/tabs/strip.rs:351-354
- crates/shell/src/tabs/strip.rs:578-585
- crates/shell/src/chrome_ui.rs:1659-1665

## Объём и приёмка

TabStrip::remove (tabs/strip.rs:201-210) не уменьшает active, когда удалена вкладка левее активной: после закрытия фоновой вкладки по × (вертикальная полоса, lumen/tabs_cmd.rs:250-257) подсвечена правая соседка, а на экране прежняя страница; следующий switch_tab (tabs_cmd.rs:531-547) сохраняет живую страницу под id чужой вкладки и затирает её снимок. Исправление: искать active по id, как close_right (strip.rs:351-354). Приёмка: [A,B*,C], закрыть A → активна B; переход на C и обратно показывает C и B без потерь; юнит-тест remove(idx<active). Найдено при сверке по docs/research/browser-ux/report.md §9 №14

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
