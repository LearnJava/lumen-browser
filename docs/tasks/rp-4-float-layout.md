# RP-4 — Общий float-поток: проброс float-контекста в вложенные блоки

**Developer:** P1 · **Ветка:** `p1-rp-4-float-layout` · **Размер:** L · **Крейты:** `lumen-layout`

> Roadmap: `ROADMAP.md` строка `RP-4` (родитель `RP`).
> Capability gap: `CAPABILITIES.md:78` — «`float` (only first-letter drop-cap)».
> **Этот gap — дрейф.** Реальный остаток описан ниже (его и закрываем).

---

## Контекст — float УЖЕ во многом реализован (не greenfield)

CAPABILITIES утверждает «float только для first-letter drop-cap» — это **устарело**. В коде есть
полноценная float-машинерия в block-потоке:

- `FloatSide` / `ClearSide` enum + поля `float_side` / `clear` (style.rs:1503/1532/2443/2446),
  парсятся css-parser'ом.
- `FloatContext` со `add_left`/`add_right`, `left_edge_at_y`/`right_edge_at_y` (с поддержкой
  **CSS Shapes L1** — polygon/ellipse/inset), `clear_y` (§9.5.2), `next_float_bottom`
  (§9.5.1 rule 8) — box_tree.rs:4757-4793.
- `establishes_bfc` (float ⇒ новый BFC, box_tree.rs:4859), схлопывание margin с учётом float'ов
  (`first_collapsible_child`, box_tree.rs:4891), float в intrinsic-sizing (max/min-content side-by-side
  суммирование, box_tree.rs:4196/4284).
- Inline-текст **уже обтекает** float'ы в пределах того же блока (line-box'ы сужаются через
  `left_edge_at_y`/`right_edge_at_y`).

## Реальный остаток (его и делаем)

Сам код помечает главный недочёт — box_tree.rs:4867-4876:

```
/// CSS 2.1 §9.5: a block-level box beside a float keeps full containing-block
/// width while only its *line boxes* are shortened. Lumen cannot yet shorten
/// line boxes inside a child block (floats are not propagated into nested
/// layout), so it approximates the narrowing by clipping the box itself.
```

То есть: float, объявленный в блоке, сужает строки **прямого** инлайн-контента этого блока, но
**не пробрасывается** в вложенные block-дети (которые не создают свой BFC). Их сейчас
**клипают** вместо корректного сужения внутренних line-box'ов. На реальных сайтах (sidebar
floats, float-картинка, вокруг которой обтекает многоабзацный текст во вложенных `<div>`/`<p>`)
это даёт неверную раскладку.

Задача: **пробросить активный `FloatContext` родителя в раскладку in-flow block-детей**, не
создающих собственный BFC, чтобы их line-box'ы сужались координатами float'ов (в системе
координат, сдвинутой на смещение ребёнка), вместо клипа. Блоки, создающие BFC
(`establishes_bfc == true`), должны, наоборот, **не** пересекаться с float'ами (сдвигаются вбок
или вниз) — это тоже проверить.

## Пред-запуск

- [ ] Прочитать box_tree.rs:4620-4795 целиком — `FloatContext` (edge_at_y, add_left/right,
      clear_y, next_float_bottom).
- [ ] Прочитать `fn lay_out` (главный проход; grep `fn lay_out\b` в box_tree.rs) — как сейчас
      создаётся/живёт `FloatContext`, где размещаются float-дети, где клипается соседний блок.
- [ ] Прочитать box_tree.rs:4859-4907 — `establishes_bfc`, `has_in_flow_content`,
      `first_collapsible_child` (текущая аппроксимация клипом завязана на `has_in_flow_content`).
- [ ] Прочитать `extract_first_letter_float` (box_tree.rs:1942) и drop-cap float (box_tree.rs:2003)
      — НЕ ломать: они переиспользуют ту же машинерию.
- [ ] `git status` чист, ветка main.

## Ключевые точки (реальные file:line)

- `crates/engine/layout/src/style.rs:1503` — `enum FloatSide`; `:1532` — `enum ClearSide`.
- `crates/engine/layout/src/box_tree.rs:4757/4765` — `FloatContext::add_left`/`add_right`.
- `crates/engine/layout/src/box_tree.rs:~4700` — `left_edge_at_y`/`right_edge_at_y` (узкое место —
  их надо звать с координатой ребёнка).
- `crates/engine/layout/src/box_tree.rs:4770` — `clear_y` (§9.5.2).
- `crates/engine/layout/src/box_tree.rs:4788` — `next_float_bottom` (§9.5.1 rule 8).
- `crates/engine/layout/src/box_tree.rs:4859` — `establishes_bfc`.
- `crates/engine/layout/src/box_tree.rs:4877` — `has_in_flow_content` (gate текущего клип-приближения).
- `crates/engine/layout/src/box_tree.rs:4196/4284` — float в intrinsic sizing (не регрессировать).

## Спек-ориентиры (CSS 2.1 §9.5)

- In-flow **block без своего BFC** рядом с float сохраняет полную ширину containing-block, но его
  **line-box'ы** укорачиваются на ширину float'а. Дети должны видеть float-контекст родителя,
  смещённый в их локальные координаты.
- In-flow block, **создающий BFC** (`overflow != visible`, сам float, и т.п.), **не перекрывается**
  с float'ами: его border-box сдвигается вбок (если влезает) или вниз (CSS 2.1 §9.5).
- `clear` уже работает через `clear_y` — проверить, что для вложенных блоков тоже учитывается.

## Шаги (декомпозировать на под-PR — L!)

1. Ветка + worktree (`p1-rp-4-float-layout`). Эту задачу резать на под-срезы:
   **4a** проброс контекста + сужение line-box'ов вложенных не-BFC блоков;
   **4b** не-перекрытие BFC-блоков с float'ами (shift вбок/вниз);
   **4c** edge-cases (`clear` в глубине, вложенные float'ы, float внутри float'а).
2. (4a) Передавать `&FloatContext` (или его срез) в рекурсивный `lay_out` in-flow block-детей,
   не создающих BFC; внутри звать `left_edge_at_y(y_local + child_offset)` так, чтобы line-box'ы
   ребёнка укорачивались. Убрать/сузить клип-аппроксимацию, завязанную на `has_in_flow_content`.
3. (4b) Для `establishes_bfc`-детей рядом с float — реализовать сдвиг border-box (не клип).
4. (4c) Проверить вложенные float'ы и `clear` на глубине; не сломать drop-cap (first-letter).

## Тесты (box_tree.rs)

- `float_narrows_line_boxes_in_nested_block` — float:left 100px + вложенный `<p>` с текстом:
  строки `<p>` начинаются после float'а, не клипаются.
- `bfc_block_does_not_overlap_float` — `overflow:hidden` блок рядом с float сдвигается, не лезет
  под float.
- `clear_in_nested_block_clears_parent_floats` — `clear:left` у вложенного блока опускается ниже
  float'а родителя.
- `nested_floats_stack` — float внутри блока-рядом-с-float размещается корректно.
- Регресс: `extract_first_letter_float` drop-cap тесты (`bug100_*` / first-letter) остаются зелёными;
  intrinsic-sizing float-тесты не меняются.

## Графический тест

Добавить `graphic_tests/NN-float-flow.html` (магента-рамка по правилам): float:left картинка/блок,
вокруг обтекает многоабзацный текст во вложенных `<p>`; float:right; `clear:both`. Записать в
`run.py TESTS` + `COVERAGE.md`. Текст игнорируем (rule 3) — проверяем геометрию обтекания/боксов.

## Definition of done

- Float'ы родителя сужают line-box'ы вложенных in-flow не-BFC блоков (а не клипают их).
- BFC-блоки рядом с float не перекрываются с ним.
- `clear` работает в глубине дерева; drop-cap (first-letter) не регрессировал.
- `CAPABILITIES.md:78` — заменить «float (only first-letter drop-cap)» на актуальный статус
  (общий float-поток; перечислить оставшиеся edge-cases, если 4c отложен).
- `cargo clippy -p lumen-layout --all-targets -- -D warnings` + `cargo test -p lumen-layout` зелёные.
- Удалить указатель `ROADMAP.md:182` из `STATUS-P1.md`; `RP-4` → `done`.
