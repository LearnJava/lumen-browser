# BUG-1837 — Каскад стилей на okko.tv многократно дороже Chrome: полный пересчёт 0.7–1.3 с на ~900 узлов, за загрузку 5–6 полных пересчётов

**Статус:** OPEN
**Заведён:** 2026-11-23 (ZCode, задача «okko.tv: сравнить с Chrome и ускорить», после [BUG-1836](BUG-1836-FIXED.md))
**Компонент:** layout/style ([`crates/engine/layout/src/style/cascade.rs`](../crates/engine/layout/src/style/cascade.rs), [`rule_index.rs`](../crates/engine/layout/src/rule_index.rs), [`counters.rs`](../crates/engine/layout/src/counters.rs)), shell (`maybe_flush`, инкрементальный путь)

## Симптом

`https://okko.tv/` без challenge (BUG-1836 закрыт), direct, холодный профиль, `scripts/perf/site_ab.py`,
3 чередующихся прогона на браузер, медианы:

| | Lumen | Chrome 157 |
|---|---|---|
| `DOMContentLoaded` | 828 мс | 731 мс |
| первый кадр (`first-paint`) | 2573 мс | 1352 мс |
| `load` (`loadEventEnd`) | 7868 мс | 2787 мс |
| сумма longtask главного потока | 24.5 с | 3.3 с |
| CPU дерева процессов за прогон | 90–120 с | 21 с |

Сеть и разбор до `DOMContentLoaded` идут одинаково; всё отставание — главный поток после него.

## Измерено

**Ожидание другого потока.** Отдельный прогон с `LUMEN_V8_FLAGS=--prof` (главный изолят, 31 575 тактов; с
профилировщиком загрузка медленнее чистой — числа годятся для долей, не для секунд): 46.3% тактов —
`NtWaitForAlertByThreadId` / `NtWaitForSingleObject` (поток JS ждёт), 44.4% — нативный код `lumen.exe`,
чистый JS — 0.6%.

**Принудительные пересчёты из JS** (отдельный прогон, `LUMEN_PROFILE_TREE=1 LUMEN_FRAME_LOG=1`):
`[engine] maybe_flush done` — 52 вызова, 9692 мс.

| Путь | Число | Сумма | Максимум |
|---|---|---|---|
| `full` | 6 | 5130 мс | 1269 мс |
| `incremental` | 46 | 4562 мс | 670 мс |

Причины отказа от инкремента (`incr declined`): 5 × `stylesheet revision changed (N changed rules not
expressible as restyle roots)`, N = 59, 1132, 1267, 1304, 6852; 1 × `no basis`. Полный пересчёт — это
`layout_measured_hyp` → `precompute_counters` (каскад по всему документу): в дереве профиля 5 вызовов на 4673 мс.

**Нативное время нити JS по функциям** (PDB-публик-символы, такты = мс):

| Функция | мс |
|---|---|
| `lumen_layout::rule_index::RuleIndex::candidates_into` | 1786 |
| `core::str::iter::SplitWhitespace` | 920 |
| `core::hash::sip::Hasher::write` + `BuildHasher<RandomState>` | 1065 |
| `lumen_layout::style::cascade::compute_style_shareable` | 562 |
| `lumen_layout::style::cascade_index::with_front_cascade_index` | 409 |
| `miniz_oxide::deflate::core::compress_inner` | 342 |
| V8 (`initialize_elements`, `LookupIterator::*`, `Scanner::Next`, `Object::Set`…) — сумма | 4195 |
| `share_safety::dynamic_fingerprint`, `cascade::part_scopes_of`, `pseudo::collect_pseudo_matches`, `share_safety::complex_is_share_safe`, `has_cache::record_fresh` | 190, 160, 135, 86, 85 |

По крейтам: `lumen_layout` 4395, `v8` 4195, `core` 2845, `alloc` 599.

## Механизм (вывод из кода и профиля)

- `compute_style` на каждый узел обходит все активные блоки `@media`/`@supports`/`@layer` и в каждом вызывает
  `RuleIndex::candidates_into` (`cascade.rs:974–1019`, `:1027–1060`); комментарий в `rule_index.rs:234` это и
  описывает: «once per node PER block». Стоимость ∝ узлы × активные блоки. Каждый вызов: поиск в `HashMap`
  с SipHash по tag/id/class/attr, `class_attr.split_whitespace()` заново, сортировка + dedup.
- Любая правка `<style>` приложением меняет revision таблицы стилей; инкрементальный путь отклоняется
  («changed rules not expressible as restyle roots») и каскад идёт целиком. N = 6852 больше, чем правил в
  пересобранном листе (stderr того же прогона: `CSS пересобран после правки <style>: 3791 правил`), поэтому
  похоже на сдвиг позиций при пересборке, а не на реальные изменения — **не проверено**.
- Пока поток движка считает, поток JS ждёт (46% тактов) — это время прямо в критическом пути.

## Что делать

1. Дешёво и локально: один `split_whitespace` на узел (сейчас — на блок), `with_front_cascade_index` один раз на
   узел, быстрый хешер вместо `RandomState` в `RuleIndex`. Ожидание по профилю — минус ~2 с из 14 с нативного
   времени (оценка, не измерено).
2. Диф правил по revision: сравнивать содержимое, а не позицию, чтобы правка `<style>` не давала «6852 changed rules».
3. Структурно: один объединённый индекс по активным блокам (как `RuleSet` Blink) вместо «индекс на блок × узел».
4. Содержимое задачи 2 после пунктов 1–2 — заново снять профиль тем же способом.

## Репро

```
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 3 --exe target/dev-release/lumen.exe \
    --challenge-selector 'js-challenge-loader,#id_spinner' --out .tmp/site-ab/okko
# профиль (Lumen only):
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 1 --arms lumen --settle 12 \
    --env LUMEN_PROFILE_TREE=1 --env LUMEN_FRAME_LOG=1 --env LUMEN_JS_STALL_SAMPLE_MS=250 --out .tmp/site-ab/prof
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 1 --arms lumen --settle 10 \
    --env LUMEN_V8_FLAGS=--prof --out .tmp/site-ab/v8prof      # isolate-*-v8.log в корне репо
```

`node --prof-process` на этом логе относит 99.4% тактов к «Shared libraries» и имён не даёт. Такты делятся по
модулям по `shared-library`-строкам лога (`ntdll` — ожидание, `lumen.exe` — работа), RVA внутри `lumen.exe` —
в имена через `scripts/native_resolve.py` (`docs/perf-method.md`).
