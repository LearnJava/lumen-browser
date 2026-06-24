# BUG-100

**Статус:** OPEN (DEBTOR)
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

> **Ревизия 2026-06-23 (дрейф трекера):** ::first-letter / ::first-line **реализованы** —
> `apply_first_letter_style`, `first_line_style`/`is_first_line`, drop-cap `float:left` через
> `extract_first_letter_float` (+7 тестов). Diff-картинка TEST-58 подтверждает: фича работает;
> остаток 4.92% = font-parity тела абзаца (Inter vs Edge → разные метрики/перенос) + edge-AA
> 48px drop-cap-глифа (rule 3). Внесён в KNOWN_DEBTORS (4.92%).

(исходное описание) ::first-letter drop-cap / ::first-line — TEST-58: CSS Pseudo-elements L4 §5.3-5.4
