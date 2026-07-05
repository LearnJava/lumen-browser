# Задача: CSS Anchor Positioning L1 (anchor() / anchor-size() / position-area)

**Developer:** P1
**Ветка:** `p1-anchor-positioning`
**Размер:** S (основной остаток — парсинг функции `anchor()` в inset-свойствах)
**Крейты:** `lumen-layout` (парсинг value + ComputedStyle + резолв в box_tree)

## Goal
Абсолютно/фиксированно позиционированный элемент привязывается к анкеру: `top: anchor(--foo bottom)`, `left: anchor(--foo right)`, `width: anchor-size(--foo width)`, а также `position-area`/`inset-area` для сеточного размещения (CSS Anchor Positioning L1). Ключевой остаток — функция `anchor()` внутри inset-свойств.

## Current state (сверено с кодом 2026-07-05)
Роадмап-семя (ROADMAP.md:129) помечает как PARTIAL «анкер-алгоритм готов; дошить CSS-проводку». По коду большая часть уже сделана; **реально отсутствует только парсинг функции `anchor()` в top/right/bottom/left**.

### Готово ✅
- **Алгоритм.** `crates/engine/layout/src/anchor.rs`: `AnchorRegistry` (163-215), `collect_anchors` (228-247), `resolve_anchor_function` (288-315), `resolve_anchor_size` (330-348), `resolve_inset_area` (407-421), `AnchorSide` (44-62). ~40 юнит-тестов (549-1111).
- **Свойства парсятся.** `anchor-name` — `crates/engine/layout/src/style.rs:13268-13270` (поле `3195`); `position-anchor` — `style.rs:13276-13278` (поле `3198`); `inset-area`/`position-area` (алиас) — `style.rs:13282-13294`, разбор keyword `19185-19206`, поля `3200-3202`; `anchor-scope` — `style.rs:13298-13310` (поле `3205`).
- **`anchor-size()` в width/height.** `parse_anchor_size_func` — `style.rs:19208-19237`; поля `anchor_size_w`/`anchor_size_h: Option<AnchorSizeFunc>` — `style.rs:3208-3211`; резолв — `box_tree.rs:7896-7907`.
- **Post-layout проход.** `apply_anchor_positions` — `crates/engine/layout/src/box_tree.rs:10392-10471`, вызывается из `layout()` (`box_tree.rs:2557`) и `layout_measured_hyp()` (`box_tree.rs:2604`). `inset-area` резолвится через `resolve_inset_area_scoped` в `lay_out_abs_children` (`box_tree.rs:7923-7933`).
- **`position-area` = алиас `inset-area`** — подтверждено комментарием `style.rs:13282`; оба пишут в `inset_area_row`/`inset_area_col`.

### Отсутствует ⬜
- **Функция `anchor()` в inset-свойствах** (`top`/`right`/`bottom`/`left`). Сейчас эти свойства идут через `set_margin_side` (`style.rs:18943-18949`) → `parse_length_q` (`style.rs:10828-10857`) → `parse_math_function_value` (`style.rs:10959-10967`), где распознаются только `calc/min/max/clamp/round` (`parse_function_call` — `style.rs:11169-11230`). `anchor(...)` **не парсится**. Нет типа `AnchorSideFunc` и полей под top/left/right/bottom.
- **CSS-SPECS.md:675** — `anchor()`/`anchor-size()` functions = ⬜. **CSS-SPECS.md:674** — anchor-name/position-anchor/inset-area = ✅.

Важно: резолвер `resolve_anchor_function` (`anchor.rs:288`) уже готов и парно к `is_horizontal`. Нужно только пробросить распарсенное значение до него.

## Entry points
- `crates/engine/layout/src/style.rs:10828` — `parse_length_q` (сюда добавлять распознавание `anchor(...)`)
- `crates/engine/layout/src/style.rs:11169` — `parse_function_call` (реестр math-функций)
- `crates/engine/layout/src/style.rs:18943` — `set_margin_side` (проводка top/left/right/bottom)
- `crates/engine/layout/src/anchor.rs:288` — `resolve_anchor_function` (готовый резолвер)
- `crates/engine/layout/src/box_tree.rs:7896` — `lay_out_abs_children` (место резолва размеров/позиций)
- `crates/engine/layout/src/box_tree.rs:10392` — `apply_anchor_positions`

## Срезы (декомпозиция на мелкие задачи)
### Срез 1 — XS — Тип `AnchorSideFunc` + поля ComputedStyle
Добавить тип, описывающий `anchor(<name>? <side> [, <fallback>]?)` (имя анкера, `AnchorSide`, опциональный fallback-Length). Добавить в `ComputedStyle` поля под 4 inset-свойства (например `inset_anchor_top/right/bottom/left: Option<AnchorSideFunc>`). Файлы: `style.rs` (рядом с `anchor_size_*` полями `3208-3211`).

### Срез 2 — S — Парсинг `anchor()` в inset-значениях
Научить путь `top/right/bottom/left` распознавать `anchor(...)`. Варианты: (а) отдельная проверка в `set_margin_side`/inset-ветке до `parse_length_q`; (б) расширение `parse_function_call`. Разобрать имя `--ident` (или неявный `position-anchor`), side-keyword (`top/bottom/left/right/start/end/center/<percentage>`) → `AnchorSide` (`anchor.rs:44-62`), опциональный fallback. Учесть ось: side должен соответствовать свойству (`top`→вертикаль). Юнит-тесты парсинга.

### Срез 3 — S — Резолв `anchor()` в layout
В `lay_out_abs_children`/`apply_anchor_positions` при наличии `inset_anchor_*` звать `resolve_anchor_function(registry, name, side, is_horizontal)` (`anchor.rs:288`) и подставлять пиксель как значение inset; при `None` — вести себя как `auto` (или fallback, если задан). Файлы: `box_tree.rs:7896`, `box_tree.rs:10392`.

### Срез 4 — XS — Графический тест
Демо `position: absolute; top: anchor(--a bottom); left: anchor(--a right)` в `graphic_tests/NN-*.html` + `1000000-final.html` + `run.py`/`COVERAGE.md`. (Проверить, нет ли уже теста на inset-area — BUG-126/TEST-77 про устаревший inset-area в памяти; не путать.)

### Срез 5 — XS — Синхронизация доков
`CSS-SPECS.md:675` ⬜→✅ (после Срезов 1-3). ROADMAP.md:129 — снять PARTIAL (владелец). `CAPABILITIES.md` — CSS/positioning.

## Tests
- Юнит: парсинг `anchor()` (Срез 2), резолв в пиксели (переиспользовать/дополнить тесты `anchor.rs:549-1111`).
- Графический: демо anchor() (Срез 4), порог 0.5%.

## Definition of done
- [ ] Тип `AnchorSideFunc` + поля ComputedStyle под 4 inset-свойства
- [ ] `anchor(--name side [, fallback])` парсится в top/right/bottom/left; юнит-тесты зелёные
- [ ] Резолв через `resolve_anchor_function` в layout; отсутствующий анкер → auto/fallback
- [ ] Графический тест anchor() добавлен
- [ ] `cargo test -p lumen-layout` зелёный; CSS-SPECS:675 ✅; ROADMAP/CAPABILITIES синхронизированы
