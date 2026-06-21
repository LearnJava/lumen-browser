# BUG-211

**Статус:** FIXED 2026-06-21
**Компонент:** layout (style.rs)
**Тест:** TEST-93 (4.11% → 3.54% → KNOWN_DEBTORS, BUG-225)

## Описание

`field-sizing: content` — input/textarea подгоняют размер под содержимое.

## Корень (не то, что в заголовке)

Field-sizing layout УЖЕ был реализован (`field_sizing.rs` + проводка в `box_tree.rs`):
`--dump-layout` показывал корректные контент-ширины (24.78/85.99/229.12px для inputs).
Реальная причина провала TEST-93 — все контролы рисовались **невидимыми**: их
`background: #b3d9ff` и `border: 2px solid #003366` (авторский CSS) терялись.

`apply_ua_appearance` (CSS Basic UI L4 §5, `appearance: none`) вызывался ПОСЛЕ
авторского каскада и безусловно обнулял `border-width`, `padding` и `background`.
Цвет/стиль рамки (`#003366`, solid) автор задал — они выживали, а ширина рамки
сбрасывалась в 0 и фон — в transparent. Итог: `DrawBorder w=[0,0,0,0]` без `FillRect`.

## Фикс

Стрип UA-appearance перенесён ПЕРЕД главным циклом каскада: `compute_style`
пред-сканирует каскад-побеждающее значение `appearance` (matched отсортирован,
inline учтён), и при `none` зовёт `strip_ua_appearance_box_styling` до применения
авторских деклараций. Теперь авторские `border`/`background`/`padding` ложатся
поверх очищенных UA-дефолтов и побеждают (CSS Cascade — author > UA).

`apply_ua_appearance` переименован в `strip_ua_appearance_box_styling` (без
self-gate на `appearance`, т.к. гейтит вызывающая сторона).

## Остаток (3.54%) → KNOWN_DEBTORS

Текст значения inputs ("ab"/"abcdefghij"/...) не рисуется: при `appearance: none`
`emit_form_control_indicator` рано выходит и подавляет в т.ч. value-текст
(заведён **BUG-225**). Остальное — font-parity (Inter vs Edge monospace) → класс BUG-128.

## Тест

`appearance_none_preserves_author_border_and_background` (style.rs) — авторский
`border:2px`/`background:#b3d9ff` выживают при `appearance:none`.
