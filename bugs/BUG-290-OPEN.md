# BUG-290 — TEST-145 (writing-mode) residual diff: font-parity + upright per-character advance

**Статус:** OPEN (DEBTOR)
**Компонент:** paint/layout (font metrics Inter vs Edge sans; `upright` per-glyph advance)
**Найден:** 2026-07-16, P3-vertical Срез 5, после фикса [BUG-289](BUG-289-FIXED.md)

## Симптом

`graphic_tests/145-writing-mode.html` (writing-mode vertical-rl/vertical-lr ×
text-orientation mixed/upright/sideways, Latin+CJK) — 3.68% diff vs Edge
после BUG-289 (было полностью сломано до фикса).

## Анализ

Два независимых источника, оба — известный класс, не layout/paint-геометрия:

1. **Font-parity (rule 3, `docs/graphic-tests.md`).** Тест текстовый — Inter
   (bundled) vs Edge `sans-serif` дают разные метрики глифов/переносы, как и
   в TEST-58 (2.47%, BUG-100) и TEST-71. Не трекается до отдельной задачи по
   шрифтам.
2. **`text-orientation: upright` — приближённый (не per-glyph) аванс.**
   Edge раскладывает каждый символ строки индивидуально по вертикали (глиф
   за глифом, каждый свой advance ≈ высота глифа). Lumen использует
   пословный горизонтальный экстент (`measure_text_w_varied` на слово
   целиком) как вертикальный аванс для ЛЮБОГО text-orientation, включая
   `upright` — корректно для `sideways`/повёрнутой части `mixed` (после
   поворота на 90° горизontal-экстент слова становится его вертикальным
   экстентом), но не для `upright`, где символы не поворачиваются и должны
   идти каждый своим advance. Уже задокументированный, сознательно
   отложенный пробел — см. `docs/tasks/ph3-writing-mode-vertical.md`
   ("`Upright`'s per-glyph vertical advance is a separate follow-up") и
   `vertical.rs`/`cpu_raster.rs`/`renderer.rs` комментарии Срезов 1–3.

## Оценка

`mixed`/`upright`/`sideways` дают ВИЗУАЛЬНО РАЗНЫЙ результат (DoD этой
задачи) — подтверждено скриншотом. Полный паритет с Edge для `upright`
требует per-glyph advance модели (отдельная задача — layout, не paint).

`KNOWN_DEBTORS['145'] = ('BUG-290', <baseline>)` в `graphic_tests/run.py`.
