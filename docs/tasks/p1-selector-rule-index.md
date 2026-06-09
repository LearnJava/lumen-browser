# Задача: Selector rule index (ускорение каскада на реальных страницах)

**Developer:** P1
**Ветка:** `p1-selector-rule-index`
**Размер:** M (~150–200 строк + 6–8 тестов)
**Крейты:** `lumen-layout` (`style.rs`)

> Это **чистая perf-оптимизация**: пиксели и поведение каскада не меняются.
> Можно (нужно) держать включённой всегда — это не экспериментальный флаг.

---

## Контекст

`compute_style` для **каждого** DOM-узла прогоняет brute-force **все** правила
таблицы стилей: `for rule in sheet.rules { for sel in rule.selectors { matches_complex(...) } }`
([style.rs:4957](../../crates/engine/layout/src/style.rs)). Сложность — O(узлы × все_правила).

На своих lean-страницах (≤50 правил) это незаметно. Но на реальном вебе с
CSS-фреймворком стоимость взрывается. Замер на сохранённой странице
**Bootstrap 5.3 docs** (3317 узлов, 1661 правило) — см. таблицу ниже.

### Данные замера (инструментированный прогон, 2026-06-09)

| Страница | Узлов | Правил | Layout | Match-попыток | Hit-rate | Каскадный waste |
|---|---|---|---|---|---|---|
| `page` (своя) | 51 | 2 | 1 мс | 102 | 6.9% | 0.0% |
| `heavy` (своя) | 4717 | 49 | 99 мс | 231 K | 4.1% | 6.1% |
| **`bootstrap` (реальная)** | **3317** | **1661** | **1948 мс** | **6.88 M** | **0.1%** | **17.5%** |

**Выводы замера:**

1. **Узкое место — селектор-матчинг, а не применение свойств.** `bootstrap`
   имеет *меньше* узлов, чем `heavy` (3317 < 4717), но раскладывается **в 20× медленнее**.
   Единственная структурная разница — 34× больше правил. Время layout трекает
   число match-попыток, а не число узлов.
2. **99.9% работы матчинга — впустую.** 6.88 M попыток дают всего 10 117 попаданий
   (hit-rate 0.1%): для каждого узла прогоняются ~2074 селектора, из которых
   подходят ~3.
3. **Применение свойств пренебрежимо.** 18 K вызовов `apply_declaration` даже по
   ~2 мкс = ~36 мс = <2% от 1948 мс. Каскадный dedup проигравших (17.5%) сэкономил
   бы ~0.3% — не стоит возни. **Поэтому НЕ делаем dedup и НЕ делаем «пропуск
   невизуальных свойств» — лечат не ту стадию.**

Цель задачи: O(узлы × все_правила) → O(узлы × кандидаты), где кандидаты ≈ 10–30
правил вместо 1661. Ожидаемо ~6.88 M → ~50–100 K match-попыток (≈ ×50–100).

Это то, что делают все настоящие браузеры (Chromium `RuleSet` с bucket-ами по
правому простому селектору + ancestor Bloom-filter).

---

## Идея

Селектор-цепочка матчится **справа налево**; правый (subject) compound определяет,
может ли правило вообще подойти узлу. Раскладываем правила по бакетам по
самому селективному простому селектору правого compound-а:

- есть `#id` в правом compound → бакет `by_id["id"]`
- иначе есть `.class` → бакет `by_class["class"]` (по каждому классу)
- иначе есть `Type(tag)` → бакет `by_type["tag"]`
- иначе (Universal / только attribute / functional pseudo в subject) →
  бакет `universal` (проверяется всегда)

Для узла собираем кандидатов: `by_id[node.id]` + `by_class[c]` для каждого
класса узла + `by_type[node.tag]` + `universal`. Полный `matches_complex`
запускаем **только** на кандидатах — он по-прежнему валидирует всю цепочку
(предков, sibling-ов, pseudo), просто отсечены заведомо-непохожие правила.

**Корректность сохраняется полностью**, потому что:
- кандидат-сет — надмножество реально матчащих правил (subject-ключ — необходимое
  условие);
- порядок сбора неважен: существующий `matched.sort_by_key((imp, inline, lp, spec,
  rule_idx, decl_idx))` ([style.rs:5098](../../crates/engine/layout/src/style.rs))
  восстанавливает каскадный порядок;
- per-rule цикл по `rule.selectors` (выбор best specificity) переиспользуется
  как есть — индекс лишь поставляет список rule_idx-кандидатов.

---

## Структуры (css-parser, уже существуют)

```rust
// crates/engine/css-parser/src/parser.rs
enum SimpleSelector { Type(String), Class(String), Id(String), Universal,
                      Attribute(..), PseudoClass(..), PseudoElement(..) }   // :38
struct CompoundSelector { parts: Vec<SimpleSelector> }                       // :432
struct ComplexSelector  { head: CompoundSelector,
                          tail: Vec<(Combinator, CompoundSelector)> }        // :449
```

**Важно:** в этом коде `head` — **левый** compound, `tail` идёт вправо. Значит
правый (subject) compound = `complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head)`.

---

## Шаги

### 1. Ветка

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/selector-rule-index -b p1-selector-rule-index
cd .claude/worktrees/selector-rule-index
```
В первом коммите — пометить «In progress» в `STATUS-P1.md`.

### 2. Новый модуль `crates/engine/layout/src/rule_index.rs`

```rust
//! Selector rule index: buckets rules by the rightmost (subject) simple
//! selector so compute_style tests only candidate rules per node, not all.
//! Pure performance — does not change which rules match. See
//! docs/tasks/p1-selector-rule-index.md.

use std::collections::HashMap;
use lumen_css_parser::{ComplexSelector, CompoundSelector, SimpleSelector, Stylesheet};

/// Index into `Stylesheet.rules`: which rule, used only to collect candidates.
type RuleIdx = usize;

/// Subject-keyed buckets over a stylesheet's top-level `rules`.
pub struct RuleIndex {
    by_id: HashMap<String, Vec<RuleIdx>>,
    by_class: HashMap<String, Vec<RuleIdx>>,
    by_type: HashMap<String, Vec<RuleIdx>>,
    /// Rules whose subject compound has no id/class/type discriminator
    /// (universal, attribute-only, or functional pseudo like :is/:where/:has
    /// in the subject) — must be tested against every node.
    universal: Vec<RuleIdx>,
}

/// The rightmost compound of a complex selector is the "subject".
fn subject<'a>(c: &'a ComplexSelector) -> &'a CompoundSelector {
    c.tail.last().map(|(_, comp)| comp).unwrap_or(&c.head)
}

/// Most-selective discriminator of a subject compound.
enum Key<'a> { Id(&'a str), Class(&'a str), Type(&'a str), Universal }

/// Pick the strongest indexable key. If the subject contains a functional
/// pseudo-class whose match depends on inner selectors (:is/:where/:has/:not),
/// be conservative → Universal (always-check), to never miss a match.
fn subject_key(comp: &CompoundSelector) -> Vec<Key<'_>> {
    // 1. functional pseudo in subject → conservative universal
    let has_functional = comp.parts.iter().any(|p| matches!(p,
        SimpleSelector::PseudoClass(pc) if pc_is_functional(pc)));
    if has_functional {
        return vec![Key::Universal];
    }
    // 2. prefer Id
    for p in &comp.parts {
        if let SimpleSelector::Id(s) = p { return vec![Key::Id(s)]; }
    }
    // 3. all classes (a node with any of them is a candidate; but a rule with
    //    multiple classes `.a.b` — index under ONE class is enough since the
    //    full matches_complex re-checks the rest). Index under first class.
    for p in &comp.parts {
        if let SimpleSelector::Class(s) = p { return vec![Key::Class(s)]; }
    }
    // 4. type
    for p in &comp.parts {
        if let SimpleSelector::Type(s) = p { return vec![Key::Type(s)]; }
    }
    // 5. universal / attribute-only
    vec![Key::Universal]
}
```

> `pc_is_functional` — helper: true для `Is/Where/Has/Not/NthChild(_, Some)/…`
> (всё, чей матч зависит от вложенных селекторов). Уточнить по `enum PseudoClass`
> в `parser.rs:77`.

`RuleIndex::build(sheet: &Stylesheet) -> Self`: для каждого `(idx, rule)`, для
каждого `sel in rule.selectors`, по `subject_key(subject(sel))` положить `idx` в
нужный бакет (дедуп idx внутри одного бакета не обязателен — кандидаты дедупятся
при сборе).

`RuleIndex::candidates(&self, tag: &str, id: Option<&str>, classes: &[&str]) ->
Vec<RuleIdx>`: собрать из `by_type[tag]`, `by_id[id]`, `by_class[c]`, `universal`
в `BTreeSet<usize>` (дедуп + сортировка по rule_idx), вернуть `Vec`.

### 3. Подключить в `compute_style`

Файл: `crates/engine/layout/src/style.rs`, цикл [строки 4957–4974](../../crates/engine/layout/src/style.rs).

**Сейчас:**
```rust
for (rule_idx, rule) in sheet.rules.iter().enumerate() {
    let mut best = None;
    for complex in &rule.selectors { if matches_complex(complex, doc, node) { ... } }
    if let Some(spec) = best { ... matched.push(...) }
}
```

**Станет** (индекс строится один раз и кэшируется — см. шаг 4):
```rust
let cands = index.candidates(node_tag, node_id, &node_classes);
for &rule_idx in &cands {
    let rule = &sheet.rules[rule_idx];
    let mut best = None;
    for complex in &rule.selectors { if matches_complex(complex, doc, node) { ... } }
    if let Some(spec) = best { ... matched.push(...) }   // тело идентично
}
```

Извлечь `node_tag` / `node_id` / `node_classes` из `doc.get(node)` один раз
наверху (tag = `name.local`, id/class — из атрибутов).

### 4. Кэш индекса (не строить на каждый узел!)

Индекс зависит только от `sheet`. Варианты:
- **A (просто):** строить `RuleIndex::build(sheet)` один раз в публичной точке
  входа layout (`box_tree.rs::layout_measured`, [box_tree.rs:1657](../../crates/engine/layout/src/box_tree.rs))
  и прокидывать `&RuleIndex` параметром в `compute_style`.
- **B (позже):** мемоизация по указателю/хешу sheet, если layout зовётся
  многократно на одной таблице.

Начать с **A**. Сигнатуру `compute_style` расширить на `index: &RuleIndex`.

### 5. Scope-ограничение Phase 1

Индексировать **только** `sheet.rules` (top-level) — это доминирующий бакет
(на Bootstrap 1661 из ~1700). `sheet.layers` / `sheet.media_rules` /
`sheet.scope_rules` оставить старым brute-force в их существующих циклах
([style.rs:4984+](../../crates/engine/layout/src/style.rs)). На реальных страницах
их немного; расширение индекса на них — отдельный Phase 2 (см. ниже).

---

## Тесты (обязательно — корректность важнее скорости)

`crates/engine/layout/src/rule_index.rs` `#[cfg(test)]`:

1. `by_id`/`by_class`/`by_type` бакетизация одиночных селекторов.
2. Compound `.a.b` → индексируется под `.a`, но `matches_complex` отсекает узел
   только с `.a`.
3. Descendant `.card .title` → subject = `.title`, бакет `by_class["title"]`.
4. `:is(.a, .b) span` → subject = `span` (type), не universal.
5. `div:is(.x)` (functional pseudo в subject) → universal-бакет.
6. Universal `* { }` и attribute-only `[hidden]` → universal-бакет.

**Регрессия (критично):** в `style.rs` тестах добавить — для набора правил
результат `compute_style` через индекс **побайтово равен** brute-force результату
на нескольких узлах. Это гарантирует «пиксели не изменились».

Прогнать существующие снапшоты: `cargo test -p lumen-driver --features cpu-render`
— все 57 страниц должны остаться identical.

---

## Проверка эффекта (опционально)

Воспроизвести замер: временно вернуть атомик-счётчики `SEL_ATTEMPTS`/`SEL_HITS`
вокруг `matches_complex` (см. историю этой задачи) и прогнать на сохранённой
Bootstrap-странице. Ожидание: match-попытки 6.88 M → ~50–100 K, layout с ~1900 мс
до ~150–250 мс.

> Сохранённая страница для замера НЕ коммитится (большой бинарь). Скачать заново:
> `curl -sSL -A "Mozilla/5.0" https://getbootstrap.com/docs/5.3/getting-started/introduction/`
> + две её таблицы `bootstrap.min.css` и `_slug_*.css`, склеить в один CSS-файл.

---

## Definition of Done

- [ ] `rule_index.rs` + подключение в `compute_style`, brute-force заменён на кандидатов
- [ ] Регрессионный тест «индекс == brute-force» зелёный
- [ ] `cargo test -p lumen-driver --features cpu-render` — 57 страниц identical
- [ ] `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист
- [ ] `cargo test -p lumen-layout` зелёный
- [ ] `SYMBOLS.md` регенерирован (`python scripts/gen_symbols.py`)

---

## Phase 2 (отдельная задача, не сейчас)

- Расширить индекс на `sheet.layers` / `media_rules` / `scope_rules`.
- Ancestor Bloom-filter для descendant-комбинаторов (как Chromium) — отсекает
  `.card .title`, если ни один предок не похож на `.card`, ещё до `matches_chain`.
- Мемоизация индекса по таблице стилей (вариант B шага 4).
