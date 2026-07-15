# BUG-287 — graphic_tests: массовая регрессия против Edge-эталонов на чистом main

**Статус:** OPEN (частично триажено 2026-07-16: 1/33 тестов диагностирован — TEST-14 → [BUG-288](BUG-288-OPEN.md) DEBTOR; исходная гипотеза диапазона коммитов ОПРОВЕРГНУТА, см. «Ревизия P3 2026-07-16» ниже)
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

## Ревизия P3 2026-07-16

Диагностика начата с TEST-14 (простейший кандидат из списка — без текста,
без интеракций). Прежде чем бисектить по подозреваемому диапазону, проверен
baseline `afdc823d`: **TEST-14 уже FAIL 1.63% на `afdc823d`** — то есть на
той самой ревизии, которую BUG-284 в тот же день пометил «Изменений нет».
Проверено ещё глубже — `247856a1` (`2570b8f4~1`, commit прямо ПЕРЕД
переписыванием `RuleIndex`/`CascadeIndex` из BUG-284, на 20+ коммитов раньше
`afdc823d`): **TEST-14 всё ещё FAIL 1.63%**, побайтово идентично.

**Вывод: гипотеза диапазона `afdc823d..7e4f2a97` (p4-anchor-functions /
p3-cv-relayout-async) для TEST-14 неверна — регрессия внесена раньше.**
«Изменений нет» в BUG-284 означало лишь «нет дельты vs непосредственно
предыдущий прогон» (rolling-сравнение в `run.py`), а не «совпадает с
KNOWN_DEBTORS/эталоном» — если регрессия просочилась раньше и ни разу не
всплыла в rolling-сравнении, она молча переносится из прогона в прогон.
Это может объяснять и часть остальных 32 нетриаженных тестов — для каждого
нужна отдельная проверка, диапазон может отличаться по тесту.

Полная диагностика TEST-14 через `--dump-display-list 14-overflow.html` +
сравнение diff-картинки с Edge → корень найден и это НЕ дефект движка:
overflow-axis coercion (BUG-020) корректно переводит `overflow-y:visible`
(при `overflow-x:hidden`) в `auto`, а `auto` — scrollable-значение, для
которого `emit_scrollbars` (BUG-220, 2026-06-24 — на месяц позже BUG-020)
теперь рисует статический scrollbar; Edge использует overlay-scrollbar,
невидимый в headless-скриншоте (тот же класс, что уже задокументирован для
TEST-83 в `bugs/BUG-220-FIXED.md`). Полный разбор → [BUG-288](BUG-288-OPEN.md).
TEST-14 добавлен в `KNOWN_DEBTORS` (baseline 1.63%), из списка BUG-287
вычёркивается.

## Следующие шаги (обновлено)

1. **Диапазон `afdc823d..7e4f2a97` для TEST-14 опровергнут — не тратить время
   на `git bisect` внутри него без предварительной проверки, что регрессия
   вообще НЕ старше `afdc823d`** (тем же способом: checkout старой ревизии +
   `--build` + `--only <NN>`), как сделано выше для TEST-14.
2. Для каждого из оставшихся 32 тестов — тот же метод: (а) `--dump-display-list`
   на исходном HTML, найти отличающиеся команды (обычно быстрее чем полный
   graphic-test прогон — не требует gdigrab/окна); (б) визуально сравнить
   `graphic_tests/screenshots/<NN>-*-{edge,lumen-cropped,diff}.png` после
   `--only <NN>`; (в) если причина — легитимная фича, разошедшаяся с Edge по
   той же причине (шрифт/AA/overlay-виджет/тайминг) — классифицировать как
   `KNOWN_DEBTOR` с новым BUG-NNN (см. BUG-288 как образец); если реальный
   дефект — чинить движок.
3. Приоритет — 13 `KNOWN_DEBTORS РЕГРЕССИЯ` записей (30, 31, 36, 45, 53, 59,
   62, 63, 65, 76, 83, 101, 113): это диффы ХУЖЕ уже принятого baseline,
   значит там либо новая порча повторно эксплуатирует тот же класс (как
   TEST-83 уже отмечал «faint overlay scrollbar» — возможно BUG-220 усилил
   именно эти долги тоже), либо независимая вторая причина. Проверить
   первым делом, не объясняются ли они тем же BUG-220 (overflow:auto/scroll
   контейнеры с переполнением на этих страницах).
4. `p4-anchor-functions`/`p3-cv-relayout-async` остаются кандидатами ТОЛЬКО
   для тестов, у которых регрессия НЕ воспроизводится на `afdc823d~20` —
   не предполагать по умолчанию.
