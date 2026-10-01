# BUG-1231 — `document.referrer` внутри `<iframe>` всегда `''`

**Статус:** FIXED 2026-10-01
**Заведён:** 2026-10-01 (P3, остаток [BUG-1156](BUG-1156-FIXED.md))
**Область:** js — фасад документа фрейма в `crates/js/src/frame_bridge.rs` (`Object.defineProperty(d, 'referrer', …)`)
возвращает `''`; запрос `<iframe src>` уже несёт `Referer` (GAP-REFERRER срез 4), но значение в
документ ребёнка не протянуто.

## Что сделать

Отдать фасаду посчитанный запросом `Referer` фрейма (Chrome: URL родителя по политике реферера).
Критерий: `iframe.contentDocument.referrer` совпадает с `Referer`, полученным сервером фрейма.

## Решение

`frames.rs` после разбора ребёнка ставит `Document::set_document_referrer(compute_referrer(политика, URL родителя, URL ребёнка))` — то же значение, что ушло в `Referer` запроса `<iframe src>` (только сетевой источник). Фасад читает его нативом `_lumen_f_referrer` (`frame_bridge.rs`). Тесты: `v8_runtime/tests/bug1231_frame_referrer.rs`.
