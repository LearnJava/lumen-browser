# RP-1 — Проценты в block-потоке (width/height/margin/padding)

**Developer:** P1 · **Ветка:** `p1-rp-1-percentage-sizing` · **Размер:** M · **Крейты:** `lumen-layout`

> Roadmap: `ROADMAP.md` строка `RP-1` (родитель `RP` — рендер-паритет реального веба).
> Capability gap: `CAPABILITIES.md:78` — «`%` в margin/padding/width/height».

---

## Контекст

Это **не greenfield**. Типизированная длина `Length` уже умеет резолвить `Percentage`
против ширины содержащего блока — у `resolve`/`resolve_or_zero` есть параметр `cb_width`,
и **flexbox уже резолвит проценты** (`flex_item_height_percentage_resolves_against_container`,
box_tree.rs:13466). Проблема в **block-потоке**: на многих сайтах вызова `resolve`
передаётся `cb_width = 0` как аппроксимация, поэтому `width: 50%` / `margin: 0 5%` /
`padding: 2%` молча схлопываются в 0.

Прямое свидетельство в коде — `preferred_inline_block_width` (box_tree.rs:~4131):

```
/// Для typed-Length полей используем em = font_size, cb_width = 0 как
/// аппроксимацию (shrink-to-fit не знает cb_width заранее).
...
// % ширины на этом этапе не разрешима — трактуем как отсутствие.
let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
```

Цель: при раскладке block-бокса прокинуть **реальную ширину содержащего блока** (inner
content-width родителя) в `resolve`/`resolve_or_zero` для `width`, `height`, `margin-*`,
`padding-*`, чтобы проценты в обычном потоке считались по спеку CSS 2.1 §10.

## Пред-запуск

- [ ] Прочитать `crates/engine/layout/src/style.rs:9840-9920` — `LengthOrAuto`/`Length` и их
      `resolve(em, cb_width, vp)` / `resolve_or_zero(...)`.
- [ ] Прочитать `crates/engine/layout/src/box_tree.rs:4916-5060` — существующие
      `resolve percentage margins` хелперы (часть уже учитывает cb_width).
- [ ] Прочитать главный проход block-layout (функция, раскладывающая children блока в поток —
      grep `fn lay_out_block` / `fn layout_block` в box_tree.rs) и найти, какая величина там
      доступна как «inner width родителя».
- [ ] `git status` чист, ветка main.

## Ключевые точки (реальные file:line)

- `crates/engine/layout/src/style.rs:9865` — `LengthOrAuto::resolve(em, cb_width, vp)`.
- `crates/engine/layout/src/style.rs:9873` — `LengthOrAuto::resolve_or_zero(em, cb_width, vp)`.
- `crates/engine/layout/src/style.rs:9884` — `enum Length` (варианты Px/Em/Rem/Percentage/Vw/Vh…).
- `crates/engine/layout/src/box_tree.rs:4131` — `preferred_inline_block_width` (cb_width=0 здесь).
- `crates/engine/layout/src/box_tree.rs:4916` / `:4975` — `resolve percentage margins` (образец
  правильного резолва — копировать паттерн).
- `crates/engine/layout/src/box_tree.rs:13466` — flex-тест: проценты уже работают, ориентир.

## CSS-спек (что считать против чего)

CSS 2.1 §10.2 / §8: **все** процентные `width`, `margin-*`, `padding-*` блока резолвятся
против **ширины** содержащего блока (даже вертикальные `margin-top`/`padding-bottom` — против
**width**, не height — это частая ошибка). Процентный `height` (§10.5) резолвится против
**высоты** содержащего блока, и только если та определена; иначе `height: %` → `auto`. Этот
последний нюанс уже частично учтён (box_tree.rs:5047 «None means percentage heights … compute
to 'auto'») — не сломать.

## Шаги

1. Ветка + worktree (`p1-rp-1-percentage-sizing`).
2. В главном block-layout проходе определить `cb_inner_width` (content-width текущего блока,
   используемого как containing block для children) и прокинуть его в каждый вызов
   `width/margin_*/padding_*.resolve*(em, cb_inner_width, vp)` вместо `0.0`.
   - Горизонтальные margin/padding и width → против `cb_inner_width`.
   - height → против `cb_inner_height` **только** если она определена, иначе `auto` (не трогать
     существующую ветку «None → auto»).
3. `preferred_inline_block_width`: оставить cb_width=0 как **намеренную** shrink-to-fit
   аппроксимацию (intrinsic-проход не знает cb), но добавить комментарий-ссылку на RP-1, что
   используемая ширина резолвится позже в основном проходе. Не пытаться резолвить % в intrinsic.
4. Проверить взаимодействие с `margin: 0 auto` (центрирование) — auto-margin не должен
   ломаться, когда width задан в %.

## Тесты (box_tree.rs, по образцу flex-тестов)

- `percent_width_resolves_against_containing_block` — родитель 400px, child `width:50%` → 200px.
- `percent_horizontal_margin_against_cb_width` — `margin: 0 10%` у child в 400px родителе → 40px
  с каждой стороны.
- `percent_vertical_padding_against_cb_width` — `padding-top: 25%` в 400px родителе → 100px
  (против width, НЕ height).
- `percent_height_auto_when_cb_height_indefinite` — `height: 50%` при auto-высоте родителя → auto.
- `percent_height_resolves_when_cb_height_definite` — родитель `height: 300px`, child `height:50%`
  → 150px.
- Регресс: существующие flex-процентные тесты остаются зелёными.

## Графический тест

Добавить unit-тест в серию (например новый `graphic_tests/NN-percentage-sizing.html` с магента-рамкой
по правилам CLAUDE.md), демонстрирующий `width:50%`, `margin:0 auto` + `padding:%`. Записать в
`run.py TESTS` и `COVERAGE.md` тем же коммитом. Текст игнорируем (rule 3) — проверяем геометрию боксов.

## Проверка

```bash
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo test -p lumen-layout
```

## Definition of done

- Проценты `width/height/margin/padding` блока в обычном потоке резолвятся против containing-block
  (горизонталь и vertical-padding/margin — против width; height — против height-if-definite).
- `margin: 0 auto` с процентной width продолжает центрировать.
- Новые + существующие тесты зелёные; графический тест добавлен.
- `CAPABILITIES.md:78` — убрать `%` из списка ⬜-gap'ов (в коммите мержа).
- Удалить указатель `ROADMAP.md:179` из `STATUS-P1.md`; `RP-1` → `done` в ROADMAP
  (`python scripts/gen_roadmap.py`).
