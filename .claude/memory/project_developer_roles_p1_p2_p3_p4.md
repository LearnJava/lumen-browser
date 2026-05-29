---
name: Developer roles reorganization — 4-person system
description: P1/P2 разработка фич, P3 только баг-фиксы, P4 только CSS
type: project
originSessionId: dc5387dd-66d8-40ae-8fd5-7976d99ac95d
---
**Дата:** 2026-05-28 — переделана система разработчиков с 5 (P1-P5) на 4 (P1-P4).

## Новое распределение:

| Роль | Обязанности | Файл STATUS |
|------|------------|-----------|
| **P1** | Feature development — любая система из roadmap | STATUS-P1.md |
| **P2** | Feature development — любая система из roadmap (координировать с P1) | STATUS-P2.md |
| **P3** | **ТОЛЬКО баг-фиксы** — BUGS.md OPEN items, graphic test regressions | STATUS-P3.md |
| **P4** | **ТОЛЬКО CSS-свойства** — parsing → ComputedStyle → cascade → layout/paint wiring | STATUS-P4.md |

## Старая система (DELETE):
- Удалена P5 (bug fixes) — merged into P3
- Переделана P3 (была runtime+system) → P3 теперь баг-фиксы только
- Переделана P4 (была всё CSS) → остаётся CSS, но упрощена документация

## Ключевые правила:

**P1/P2 разработка фич:**
- Берут задачи из lumen-plan.md Track P1 раздела
- Координируют через STATUS-P1.md и STATUS-P2.md (избегают дублирования)
- При нужности нового CSS-свойства: добавляют `// CSS: property` комментарий и пишут в STATUS-P4.md "Needs wiring"

**P3 баг-фиксы (только):**
- Запускают `python graphic_tests/run.py --continue-on-fail`
- Читают BUGS.md для открытых issues
- Выбирают bug с наибольшей deviation
- Фиксят + тест + mark FIXED в BUGS.md
- Ветки: `p3-bug-<id>`

**P4 CSS (только):**
- Берут задачи из "Needs wiring" в STATUS-P4.md (P1/P2 алгоритмы готовы)
- Реализуют CSS property end-to-end: парсинг → ComputedStyle → cascade → wiring
- Добавляют графический тест в graphic_tests/
- Ветки: `p4-<property-name>`

## Сделано 2026-05-28:
- ✅ Обновлён CLAUDE.md — новые роли и workflow
- ✅ Переделаны STATUS-P1.md, P2.md, P3.md, P4.md
- ✅ Удалён STATUS-P5.md
- ✅ Отмечен DECISIONS.md как deprecated
- ✅ Коммит: 2e6393b — docs: reorganize developer roles
