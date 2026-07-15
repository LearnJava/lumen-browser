# BUG-287 — graphic_tests: массовая регрессия против Edge-эталонов на чистом main

**Статус:** OPEN (не диагностирован, не забисекчен)
**Компонент:** неизвестен — вероятнее всего layout/paint (см. «Подозреваемые» ниже)
**Найден:** 2026-07-15, при верификации ветки `p3-cascade-perf` (задача 1 из
`docs/tasks/p3-cascade-perf.md`) графическими тестами на Windows

## Симптом

Полный прогон `LUMEN_PROFILE=dev-release python graphic_tests/run.py
--continue-on-fail` на **чистом `main`** (без каких-либо локальных правок,
коммит после мержа `p3-cascade-perf`, база — `afdc823d`) даёт:

- **43 FAIL** (из них 13 — `KNOWN_DEBTORS РЕГРЕССИЯ`, т.е. хуже
  зафиксированного в KNOWN_DEBTORS baseline)
- **38 DEBTOR** (известные долги, в допуске)
- **63 PASS**
- из 144 тестов

Это прямо противоречит записи в [BUG-284](BUG-284-FIXED.md) (тот же день,
раньше): «`graphic_tests/run.py --continue-on-fail` показал «Изменений нет»
vs эталонный прогон на main». Значит регрессия внесена ПОСЛЕ фикса BUG-284,
в диапазоне коммитов `afdc823d..7e4f2a97` (см. «Подозреваемые»).

Проверено дважды независимо (ветка `p3-cascade-perf` и чистый `main` дают
**побайтово одинаковые проценты** на пересекающихся тестах) — это не шум
рендера и не флуктуация, а детерминированная регрессия.

## Диапазон коммитов (подозреваемые)

`git log --oneline afdc823d..7e4f2a97`:

```
7e4f2a97 STATUS-P1: удалить указатель (Step 5 ph3-ai-module) — doc-only
be209c7c Влить p1-ph3-ai-rag-engine: RagEngine — RAG над semantic-индексом
0de3b1dd lumen-ai: RagEngine
64dffc51 STATUS-P1: doc-only
58311513 Влить p1-ph3-ai-summarisation: GenerationBackend
c6cb327b lumen-ai: GenerationBackend
1fc9b066 P1: doc-only
147d804a Влить p1-ai-semantic-search: SemanticIndex
c2b68bed Влить p3-cascade-perf-doc — doc-only
afeba57e docs(cascade-perf) — doc-only
4ff739a9 lumen-knowledge: SemanticIndex
a0fd3a0a Влить p3-cv-relayout-async: BUG-286 — маршрутизация cv-relevant relayout
05c68e1d BUG-286: relayout_raf_dirty()
4553ccf2 P1: doc-only
f6ec8e36 Влить p1-ai-embedding-backend: EmbeddingBackend
d5a5df46 lumen-ai: EmbeddingBackend
82c44385 STATUS-P1 doc-only
06b2844d STATUS-P4: doc-only
2653265f Влить p4-anchor-functions: anchor()/anchor-size()
b76967d9 CSS Anchor Positioning L1: anchor()/anchor-size()
```

`lumen-ai`/`lumen-knowledge` (RagEngine, EmbeddingBackend, GenerationBackend,
SemanticIndex) не подключены к layout/paint пайплайну — маловероятные
кандидаты. Два реальных подозреваемых, трогающих рендер:

1. **`p4-anchor-functions`** (`b76967d9`/`2653265f`) — `anchor()`/
   `anchor-size()` в `lumen-layout`/`lumen-css-parser`. Новая функциональность
   в резолюции CSS-значений — если резолвер задет для общего пути (не только
   элементов с `anchor()`), может задевать многие тесты сразу.
2. **`p3-cv-relayout-async`** (`05c68e1d`/`a0fd3a0a`) — маршрутизация
   `content-visibility:auto` relevance-триггера через `relayout_raf_dirty()`.
   Затрагивает shell/relayout — менее вероятно (это про тайминг relayout,
   не про сами вычисленные стили), но не исключено на interaction-тестах
   (не в headless-скриншотах обычно).

Не бисекчено — нужен `git bisect` (или ручной чекаут каждого коммита) +
`graphic_tests/run.py --only <NN>` на 1-2 показательных тестах (например,
TEST-14 — простой `14-overflow.html`, без анимаций/интеракций, 1.63% diff,
подозрительно даже для базового теста).

## Полный список расхождений (чистый main, `afdc823d`+мерж cascade-perf)

FAIL (не known-debtor): 14 (1.63%), 24 (0.50%), 26 (11.24%), 39 (12.66%),
49 (28.15%), 54 (2.32%), 56 (14.12%), 60 (0.74%), 68 (3.17%), 72 (1.29%),
74 (3.74%), 81 (3.44%), 103 (1.79%), 109 (7.53%), 111 (1.27%), 112 (7.18%),
116 (2.40%), 130 (1.00%), 140 (2.17%), 141 (1.59%) и другие (полный список —
перегенерировать прогон, лог не сохранён).

KNOWN_DEBTORS РЕГРЕССИЯ (хуже зафиксированного baseline): 30 (10.24% vs 4.27%,
BUG-144), 31 (3.99% vs 0.60%, BUG-184), 36 (7.80% vs 0.96%, BUG-176),
45 (5.81% vs 1.02%, BUG-240), 53 (5.45% vs 1.71%, BUG-128), 59 (23.65% vs
17.15%, BUG-101), 62 (16.07% vs 8.85%, BUG-128), 63 (5.46% vs 2.02%, BUG-176),
65 (5.45% vs 2.08%, BUG-127), 76 (20.15% vs 0.64%, BUG-176), 83 (11.91% vs
7.88%, BUG-128), 101 (20.00% vs 0.71%, BUG-247), 113 (6.10% vs 1.41%, BUG-215).

Разное направление (тест 61, 132, 138) показал УЛУЧШЕНИЕ против записанного
baseline — само по себе не проблема, но подтверждает, что рендер вообще
изменился в этом диапазоне коммитов (не только деградация).

## Как воспроизвести

```bash
git log --oneline afdc823d..HEAD                    # подтвердить диапазон
LUMEN_PROFILE=dev-release python graphic_tests/run.py --build
LUMEN_PROFILE=dev-release python graphic_tests/run.py --only 00   # калибровка (иногда флак — см. reference_gdigrab_test00_retry)
LUMEN_PROFILE=dev-release python graphic_tests/run.py --continue-on-fail
```

## Следующие шаги

1. `git bisect` по диапазону `afdc823d..7e4f2a97`, гейт — `graphic_tests/run.py
   --only 14` (простой headless-тест без интеракций, дешёвый).
2. Если бисект укажет на `p4-anchor-functions` — проверить, не задевает ли
   новый резолвер `anchor()`/`anchor-size()` общий путь вычисления layout для
   элементов БЕЗ этих CSS-функций (например, добавочная стоимость/ветвление
   в `compute_style`/`box_tree.rs`, срабатывающая безусловно).
3. Если укажет на `p3-cv-relayout-async` — проверить, не изменился ли порядок/
   момент relayout для headless `--screenshot`-снятия (гонка «снят кадр раньше
   `relayout_raf_dirty()` успел доехать»).
