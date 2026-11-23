# BUG-1839 — Корзина universal индекса правил: 82–87% кандидатов каскада, ~95 на пересчитанный элемент (okko.tv)

**Статус:** OPEN
**Заведён:** 2026-11-23 (остаток [BUG-1837](BUG-1837-FIXED.md) после его правки)
**Компонент:** layout/style ([`crates/engine/layout/src/rule_index.rs`](../crates/engine/layout/src/rule_index.rs), [`counters.rs`](../crates/engine/layout/src/counters.rs) — `note_candidate_bucket`)

## Симптом

`https://okko.tv/`, `[engine] incr cascade CascadeStats` (`LUMEN_FRAME_LOG=1`), 46 инкрементальных проходов за загрузку:

| | до правки BUG-1837 | после |
|---|---|---|
| пересчитано элементов | 1546 | 3522 |
| кандидатов, всего | 166 444 | 414 576 |
| из них корзина universal | 145 311 (87%) | 341 654 (82%) |
| universal на пересчитанный элемент | 94 | 97 |
| вызовов `matches_complex` | 91 771 | 270 866 |
| на пересчитанный элемент | 59 | 77 |

Правка BUG-1837 убрала обращения к индексу (39 → 3 на узел), но не число кандидатов: каждый узел получает все
universal-правила верхнего уровня, `@layer` и активных блоков, и по каждому идёт `best_match`.

## Что измерено, что нет

- Измерено: кандидаты по корзинам и число вызовов матча — счётчики `CascadeStats` (`counters.rs`, `note_candidate_bucket`,
  `note_candidates`, `note_match_outcome`).
- Не измерено: состав корзины universal на okko.tv — сколько в ней правил без дискриминатора (`*`, `:root`, `:hover`,
  `:not(…)`), сколько с псевдоэлементом в субъекте (`*::before`, `::placeholder`, `::-webkit-scrollbar`).

## Гипотеза — не проверена

Правило, у которого в субъекте каждого селектора стоит псевдоэлемент, элементу matchиться не может:
`matches_simple` по `SimpleSelector::PseudoElement` даёт `false` (`style/matching.rs`, `matches_simple`). В индексах пути
элемента (`CascadeIndex::rules`/`layers`/`conditional`, `InvalidatedRules`, `style/rule_roots.rs`) такие правила — мёртвые
кандидаты. В диффе «пустой лист → весь лист» на okko.tv 352 из 6 852 правил имеют псевдоэлемент в субъекте (`Before` 159,
`After` 137, `-webkit-scrollbar*` 31, `FirstLetter` 14, `*placeholder*` ~30); какая их доля попадает в корзину universal, не
считалось.

## Что делать

1. Сосчитать состав корзины universal (одна диагностическая сборка): сколько правил с псевдоэлементом в субъекте против
   остальных.
2. Если это большая доля — строить индексы пути элемента только по селекторам без псевдоэлемента в субъекте. Правило
   `*, ::before, ::after { … }` остаётся кандидатом через `*`; правило только с псевдоэлементом в субъекте не попадает в
   индекс. Индексы псевдопробы (`pseudo_rules_by_name`, `pseudo_media`/`pseudo_supports`) строятся как раньше — им такие
   правила и нужны.

## Репро

```
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 1 --arms lumen --settle 12 \
    --env LUMEN_FRAME_LOG=1 --out .tmp/site-ab/casc
# сумма полей CascadeStats по всем проходам — по строкам "incr cascade CascadeStats {" в live.stderr.*.log
```
