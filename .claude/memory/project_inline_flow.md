---
name: inline-flow реализован
description: Line wrapping (word wrap) текста завершён в ветке inline-flow, влит в main (90b849a)
type: project
originSessionId: 95cab192-d21f-47da-9940-00be718f94cf
---
Line wrapping реализован и влит в main 2026-05-12.

**Что сделано:**
- `TextMeasurer` trait в `lumen-layout/src/lib.rs` — интерфейс для измерения ширины символов
- `layout_measured(doc, sheet, viewport, &dyn TextMeasurer)` — новый публичный API с wrapping
- `BoxKind::Text(Vec<String>)` — каждый элемент Vec = одна строка после wrap
- `wrap_text()` в box_tree.rs — word-wrap по пробелам
- `FontMeasurer<'a>` в `lumen-paint/src/lib.rs` — реализует TextMeasurer через TTF hmtx/cmap
- shell использует `layout_measured` + `FontMeasurer::new(&font)` для реального wrapping
- 168 тестов (было 159)

**Why:** Без wrapping текст обрезался на краю экрана, строки не переносились.
**How to apply:** Следующий пункт roadmap — encoding detection (cp1251/KOI8-R) или HTTP/TLS.
True inline-flow (элементы <a>/<span> в одной строке с текстом) — ещё не реализован.
