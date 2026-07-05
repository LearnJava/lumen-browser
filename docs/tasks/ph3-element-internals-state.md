# Задача: ElementInternals + custom element states (:state())

**Developer:** P1
**Ветка:** `p1-customstate`
**Размер:** S
**Крейты:** `lumen-js`, `lumen-css-parser`, `lumen-layout`

## Goal
Замкнуть цепочку custom element states (WHATWG HTML §4.13.2): JS-часть
(`ElementInternals` + `CustomStateSet`) уже готова; остаётся распарсить CSS
`:state(ident)` в `css-parser` и сматчить его в `layout` через существующий
нативный мост `_lumen_element_internals_get_states(nid)`, чтобы правило
`:state(x)` применяло стиль к элементу с активным состоянием `x`.

## Current state (сверено с кодом 2026-07-05)
- JS-часть **готова**: `crates/js/src/element_internals.rs:1-332`.
  - `CustomStateSet` (add/has/delete/clear/size/values/forEach/iterator) —
    `element_internals.rs:21-59`.
  - `ElementInternals` + `Element.prototype.attachInternals` —
    `element_internals.rs:161-170`.
  - Нативный мост в Rust: `_lumen_element_internals_get_states(nid)` возвращает
    JSON-массив активных состояний — `element_internals.rs:175-183`.
  - Явный комментарий-handoff: «`:state()` CSS selector handoff → P4
    (css-parser)» — `element_internals.rs:6`, дубль `:174`.
- CSS-парсер: `:state()` **не распознаётся**. `SimpleSelector::PseudoClass` —
  `crates/engine/css-parser/src/parser.rs:44`; enum `PseudoClass` — `:77`;
  всё нераспознанное → `PseudoClass::Unsupported(String)` (`:340`), а
  `is_supported_pseudo_class` для него возвращает `false` (`:532-536`).
  Записи `State`/`"state"` в enum нет.
- Матчинг псевдоклассов против DOM-узла — `crates/engine/layout/src/style.rs`
  (диспетчер: `FirstChild` :7658, `Empty` :7663, `Root` :7664 и т.д.).
  `Unsupported(_)` → `false`. Ветки `:state()` нет.
- `rule_index.rs:46-51` перечисляет «сложные» псевдоклассы для индексации —
  `:state()` туда добавлять по аналогии не требуется (это простой функциональный
  класс по одному ident).

## Entry points
- `crates/engine/css-parser/src/parser.rs:77` — enum `PseudoClass` (добавить
  `State(String)`).
- `crates/engine/css-parser/src/parser.rs:340` / `:532` — fallback
  `Unsupported` и `is_supported_pseudo_class` (учесть `State`).
- парсинг функционального псевдокласса (рядом с разбором `:nth-child(...)` /
  `:not(...)` в `parser.rs`) — добавить приём `:state(<ident>)`.
- `crates/engine/layout/src/style.rs:~7658` — диспетчер матчинга
  `PseudoClass` → добавить ветку `PseudoClass::State(name)`.
- `crates/js/src/element_internals.rs:175` — готовый мост `get_states`.

## Срезы (декомпозиция)
### Срез 1 — XS — enum + парсинг `:state(ident)`
Добавить вариант `PseudoClass::State(String)` (`parser.rs:77`). В функциональном
разборе псевдоклассов принять имя `state` с одним `<custom-ident>` аргументом →
`State(ident)`. Невалидный аргумент → `Unsupported`. Учесть в
`is_supported_pseudo_class` (`parser.rs:532`) и в сериализаторе
(`nth_to_css_str`/pseudo-serialize рядом с `:625`).

### Срез 2 — S — матчинг в layout через JS-мост
В диспетчере `style.rs:~7658` добавить ветку `PseudoClass::State(name)`:
вызвать `_lumen_element_internals_get_states(nid)` через существующий JS-контекст
(тот же путь, каким layout уже дёргает JS для других запросов — найти вызов
`eval`/binding в `style.rs`/`box_tree.rs`), распарсить JSON-массив, вернуть
`array.contains(name)`. Если JS-контекст недоступен (headless layout-dump) →
`false`, без паники.

### Срез 3 — XS — кэш/инвалидация (если требуется)
Проверить, не кэшируется ли результат матчинга между перерисовками
(`RULE_IDX_CACHE`, см. `rule_index.rs`). Состояния меняются в рантайме
(`internals.states.add(...)`), поэтому `:state()` должен пере-оцениваться при
перерисовке — убедиться, что матч не мемоизируется на всю сессию. При
необходимости — точечная инвалидация, как в других динамических псевдоклассах
(`:hover` в `style.rs`).

## Tests
- `css-parser` юнит (`parser.rs` tests): `:state(loading)` парсится в
  `PseudoClass::State("loading")`; round-trip сериализации; `:state()` без
  аргумента → `Unsupported`.
- `layout` юнит/интеграция: узел с internals-состоянием `loading` матчит правило
  `x:state(loading)`, не матчит `x:state(other)`; после удаления состояния —
  перестаёт матчить.
- Графический тест (опционально, если визуально проверяемо): демо в
  `graphic_tests/` с элементом, меняющим фон по `:state()`.

## Definition of done
- [ ] `:state(ident)` парсится в `PseudoClass::State` (не `Unsupported`).
- [ ] Матчинг в `layout` через `_lumen_element_internals_get_states`.
- [ ] Динамическое изменение состояния пере-оценивается при перерисовке.
- [ ] Юниты css-parser + layout зелёные.
- [ ] Комментарии-handoff `element_internals.rs:6,174` сняты/обновлены.
- [ ] `CAPABILITIES.md` — ElementInternals/`:state()` 🟡 → ✅.
