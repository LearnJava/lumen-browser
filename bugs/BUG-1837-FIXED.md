# BUG-1837 — Каскад стилей на okko.tv: индекс на каждый блок `@media` и полный пересчёт после любой правки `<style>`

**Статус:** FIXED 2026-11-23 — один индекс по активным блокам `@media`/`@supports`, диф листа по содержимому, правила с
псевдоэлементами как корни рестайла. Профиль: `maybe_flush` за загрузку 9 692 → 2 881 мс. Чередующийся A/B старого и нового
бинарника (по 4 прогона): `load` 9 163 → 4 996 мс, сумма longtask 32,1 → 13,9 с, CPU процесса 106 → 56 с.
**Заведён:** 2026-11-23 (ZCode, задача «okko.tv: сравнить с Chrome и ускорить», после [BUG-1836](BUG-1836-FIXED.md))
**Компонент:** layout/style ([`style/cascade_index.rs`](../crates/engine/layout/src/style/cascade_index.rs),
[`style/cascade.rs`](../crates/engine/layout/src/style/cascade.rs), [`style/share_safety.rs`](../crates/engine/layout/src/style/share_safety.rs),
[`style/rule_roots.rs`](../crates/engine/layout/src/style/rule_roots.rs)), css-parser
([`parser/sheet_diff.rs`](../crates/engine/css-parser/src/parser/sheet_diff.rs)), js ([`v8_runtime/style_flush.rs`](../crates/js/src/v8_runtime/style_flush.rs))

## Симптом (до правки)

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

## Измерено (до правки)

**Принудительные пересчёты из JS** (`LUMEN_PROFILE_TREE=1 LUMEN_FRAME_LOG=1`, один прогон): `[engine] maybe_flush done` —
52 вызова, 9692 мс.

| Путь | Число | Сумма | Максимум |
|---|---|---|---|
| `full` | 6 | 5130 мс | 1269 мс |
| `incremental` | 46 | 4562 мс | 670 мс |

Отказы от инкремента: 5 × `stylesheet revision changed (N changed rules not expressible as restyle roots)`, N = 59, 1132,
1267, 1304, 6852; 1 × `no basis`.

**Ожидание другого потока.** Прогон с `LUMEN_V8_FLAGS=--prof` (31 575 тактов): 46.3% тактов — `NtWaitForAlertByThreadId` /
`NtWaitForSingleObject` (поток JS ждёт), 44.4% — нативный код `lumen.exe`, чистый JS — 0.6%.

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

По крейтам: `lumen_layout` 4395, `v8` 4195, `core` 2845, `alloc` 599. Точность атрибуции тактов к функциям по публичным
символам (без полных PDB, при инлайне) не проверялась; правки ниже опираются на счётчики `CascadeStats` и разбивку отказов.

**Счётчики инкрементальных проходов** (`[engine] incr cascade CascadeStats`, 46 проходов, 1546 пересчитанных элементов):
166 444 кандидата, из них 145 311 (87%) из корзины universal — 94 universal-кандидата на пересчитанный элемент; 91 771 вызов
`matches_complex`.

## Диагностика

Один прогон со временной диагностикой (та же сборка, что в «Измерено», плюс `eprintln!` в дифе листа и в построении индекса).

**Состав листа.** 3 749–4 346 plain-правил и **2 879–3 486 блоков `@media`** (почти по одному правилу), 51–61 `@supports`.
Активны 33 `@media` и 4 `@supports`. То есть на узел приходилось 2 + 33 + 4 = 39 запросов `RuleIndex::candidates_into`
(верхний уровень, `@layer`, каждый активный блок) там, где групп три. Корзина universal: 28–29 правил наверху и 32 по активным
блокам.

**Отказы от инкремента** — разбивка N по причинам:

| N | реальное изменение | блоки, равные по содержимому, но с другой позицией | правил с псевдоэлементом в субъекте |
|---|---|---|---|
| 6852 | весь лист: базис был с пустым листом | 0 | 352 |
| 1172 | 541 plain-правило + 616 новых блоков | 0 | 53 |
| 1304 | 42 plain-правила | 1232 блока | 24 |
| 1267 | 1 plain-правило | 1230 блоков | 24, все из «сдвинутых» |
| 59 | 55 plain-правил | 0 | 5 |

Гипотеза отчёта («сдвиг позиций») подтвердилась частично: чистым сдвигом был один отказ из четырёх (1267). В трёх остальных
среди *реально* изменённых правил стоял `::before`, `::after` или `::-webkit-scrollbar*`, а такие правила путь инкремента не
выражал вовсе. Отказ 6852 — первый стилевой флаш после навигации: полный путь там уместен.

## Механизм

- `CascadeIndex` держал по одному `RuleIndex` на каждый блок `@media`/`@supports`, и `compute_style` опрашивал каждый активный
  блок отдельно (`candidates_into`: поиск в `HashMap` по tag/id/class/attr, `split_whitespace`, сортировка) — то же, что для
  `@layer` до THREAD-4 срез 7. `share_safety::dynamic_fingerprint` повторял это на узел ещё раз.
- `Stylesheet::changed_style_rules` сравнивал блоки вместе с `BlockPos` (сколько plain-правил перед блоком). Шелл после правки
  `<style>` пересобирает весь CSS (`relayout.rs` → `merge_sheet_chunks` → `parse`), и блоки, стоящие за отредактированным
  `<style>`, получали новую позицию — «изменённые» целиком.
- `restyle_roots_for_rule_changes` возвращал `None` для любого правила с псевдоэлементом в селекторе. Это было ограничением
  сферы среза 64 (BUG-935), а не найденным дефектом.

## Правка

1. **Один индекс по активным блокам.** `CascadeIndex::conditional` — `RuleIndex` по правилам всех активных блоков
   `@media`/`@supports` (плоская позиция → `ConditionalRule { block, rule, key }`), построен как индекс `@layer`. Неактивные
   блоки не индексируются: индекс живёт под одной парой (ревизия листа, медиа-ключ). Запросов кандидатов на узел — три.
   `dynamic_fingerprint` ходит в него же. Полный проход получил скоуп `cascade_walk` (как у инкрементального).
2. **Диф по содержимому.** `changed_style_rules` идёт по одной последовательности — plain-правила и блоки вперемешку, в
   порядке каскада (`cascade_items`) — и блок равен блоку по условию, правилам и объемлющим условиям, без позиции. Блок,
   который действительно сдвинулся относительно других, выпадает из наибольшей упорядоченной подпоследовательности и
   называется, как раньше. Отпечаток для корзин считается из `Debug` сразу в хешер, без `String`.
3. **Правила с псевдоэлементами — корни рестайла.** `restyle_roots_for_rule_changes` матчит селектор без `PseudoElement`-частей
   субъекта (то же разбиение, что `matches_complex_for_pseudo`) и берёт исходный элемент корнем. Достаточность: корень всегда
   пересчитывается, пересчитанный элемент не объявляет себя чистым (`counters::walk` → `Reuse::Dirty`), бокс строится заново и
   берёт псевдоэлементы из нового листа. Функция возвращает `Result<_, RuleChangeNotExpressible>`, и отказ печатает причину.
   Четыре крейта: `css-parser`, `layout`, `js` (`style_flush.rs`), `shell` (`page_load.rs`, стриминговая загрузка).

Не сделано из плана отчёта: быстрый хешер и отдельные `split_whitespace`/`with_front_cascade_index` на узел. После слияния
индексов обращений к индексу на узел стало 39 → 3 (каждое — набор поисков по хешу: tag, id, каждый класс, каждое имя атрибута),
а для строк со страницы в `crates/core/src/id_hash.rs` прямо записано оставаться на стандартном хешере.

## Результат

**Профиль** (тот же способ, что в «Измерено», по одному прогону старой и новой сборки):

| Путь | до | после |
|---|---|---|
| `full` | 6 / 5130 мс (макс 1269) | 3 / 538 мс (макс 279) |
| `incremental` | 46 / 4562 мс (макс 670) | 46 / 2343 мс (макс 322) |
| всего | 52 / 9692 мс | 49 / 2881 мс |

Отказы после правки (один прогон): `no basis`; `6804 changed rules: more rules changed than a root walk pays for` (первый
стилевой флаш); `948 changed rules: a changed rule selects ::first-line or ::first-letter`.

**Та же команда, что в «Симптом»** (новая сборка, 3 прогона, медианы; сайт дрейфует в течение часа, поэтому ниже — A/B):
Lumen `DOMContentLoaded` 690 мс, `first-paint` 2955 мс, `load` 3753 мс, longtask 10 271 мс, CPU 48 с; Chrome в тех же
прогонах: 854, 1364, 2508 мс, 3001 мс.

**Чередующийся A/B старого и нового бинарника** (старый — `main` 0d708d2b8d, две пары по 2 прогона подряд, 21:04–21:10):

| медиана, 4 прогона | старый | новый |
|---|---|---|
| `DOMContentLoaded` | 1124 мс | 832 мс |
| `first-paint` | 4678 мс (3824–4940) | 2945 мс (2413–3193) |
| `load` (`loadEventEnd`) | 9163 мс (7540–10 694) | 4996 мс (4244–5296) |
| сумма longtask | 32 080 мс | 13 856 мс |
| CPU процесса | 106 с | 56 с |

**Счётчики инкрементальных проходов после правки** (46 проходов): на пересчитанный элемент 118 кандидатов (97 universal),
77 вызовов `matches_complex` — число кандидатов на элемент не изменилось, правка убрала 36 обращений к индексу на узел.
Пересчитанных элементов стало 3522 против 1546: флаши, уходившие в полный путь, теперь инкрементальные.

**Нейтральность к display list:** `python graphic_tests/dump_golden.py` даёт одинаковый вывод для старого и нового бинарника
на всех 12 проверках (diff отчётов пуст).

**Тесты:** `lumen-layout` 6208, `v8_bug935_s64_sheet_delta` — новый сценарий `pseudo_element_rules` (правила с псевдоэлементами
поглощаются, `getComputedStyle` и прямоугольники всех элементов совпадают с полным путём) и `line_pseudo_and_quotes_fall_back`.

## Остаток

- **Корзина universal — 87–88% кандидатов каскада, ~95 на пересчитанный элемент.** Состав не измерен; кандидат на следующий
  срез — [BUG-1839](BUG-1839-OPEN.md).
- `::first-line`, `::first-letter`, контент с кавычками в изменённом правиле с псевдоэлементом, `:host`/`::part`/`::slotted`
  остаются на полном пути (`RuleChangeNotExpressible`): их результат пишется в разложенные строки или в глубину кавычек,
  которую чистое поддерево пропускает.
- Первый стилевой флаш после навигации (пустой лист → весь лист) идёт полным путём — переиспользовать нечего.
- В профиле после правки из ~15 с прогона `js.run` — 12,2 с, `maybe_flush` — 2,7 с: дальше JS страницы и [BUG-1838](BUG-1838-OPEN.md).

## Репро

```
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 3 --exe target/dev-release/lumen.exe \
    --challenge-selector 'js-challenge-loader,#id_spinner' --out .tmp/site-ab/okko
# профиль (Lumen only):
python scripts/perf/site_ab.py --url https://okko.tv/ --runs 1 --arms lumen --settle 12 \
    --env LUMEN_PROFILE_TREE=1 --env LUMEN_FRAME_LOG=1 --env LUMEN_JS_STALL_SAMPLE_MS=250 --out .tmp/site-ab/prof
```

A/B двух сборок — та же команда с `--arms lumen --runs 2` и другим `--exe`, пары подряд; `site_ab.py` копирует бинарник в
`<out>/bin/`, старая сборка сохраняется там вместе с результатами.
