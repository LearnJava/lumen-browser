# BUG-926 — `<button>` и `<select>` без явной ширины схлопываются в 0 и не видны на странице

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, попутно к задаче IFC-1)
**Область:** layout (`crates/engine/layout/src/box_tree.rs` — `is_replaced` в `lay_out_inner`, затем `b.rect.width = pref_w.min(b.rect.width)` в ветке shrink-to-fit)
**Владелец:** P3

## Симптом

`<button>Btn</button>` и `<select><option>Opt</option></select>` без CSS-ширины
получают `width = 0`. Подпись есть в дереве боксов, но её `InlineRun` тоже
нулевой ширины, так что на экране от контрола остаётся вертикальная полоска из
двух рамок.

Замер `--dump-layout` (dev-release, 2026-08-25), страница
`<div><button>Btn</button></div><div><select><option>Opt</option></select></div><div><input size=4></div>`:

```
FormControl rect=(0.00,  0.00, 0.00, 23.00) bg=#efefefff display=inline-block h=21.00 bw=(1,1,1,1)
  InlineRun rect=(1.00, 1.00, 0.00, 19.20)          <- подпись «Btn», ширина 0
FormControl rect=(0.00, 23.00, 0.00, 23.00) bg=#ffffffff display=inline-block h=21.00 bw=(1,1,1,1)
  Skip      rect=(1.00, 24.00, 0.00,  0.00) display=none
FormControl rect=(0.00, 46.00, 176.00, 23.00) w=174.00 h=21.00   <- <input>: ширина есть
```

Контроль на той же странице: `<span style="display:inline-block">Btn</span>`
даёт `width = 23.12` — shrink-to-fit сам по себе работает, ломается он именно у
`BoxKind::FormControl`. `<input>` не задет, потому что UA-стиль выводит ему
явную `width` из атрибута `size` (в дампе `w=174.00`).

## Механизм

Две строки `lay_out_inner`, каждая по отдельности осмысленная:

1. `is_replaced` относит `BoxKind::FormControl` к замещаемым элементам
   (CSS 2.1 §10.3.2 — «auto-ширина = intrinsic, а не вся ширина контейнера»),
   и `b.rect.width` получает intrinsic-ширину, то есть **0**: у формы нет
   декодированных пикселей, из которых её взять.
2. Ниже ветка shrink-to-fit для atomic inline-level боксов пишет
   `b.rect.width = pref_w.min(b.rect.width)`. `preferred_inline_block_width`
   считает по подписи правильную ширину, но `min` с нулём из шага 1 её
   обнуляет.

То есть shrink-to-fit у формы вычисляется и тут же выбрасывается. `min` здесь
нужен для настоящих замещаемых элементов (картинка не должна раздуться шире
предложенного места), а у формы предыдущее значение — не «предложенное место»,
а нулевой intrinsic.

Это третий заход на ту же строку. [BUG-425](BUG-425-FIXED.md) (2026-07-31)
исключил из `is_replaced` формы с авторским `display: flex/grid` — ровно потому,
что они схлопывались в 0; UA-дефолт `inline-block` тогда не тронули.

## Что проверить при починке

- `<input type=checkbox|radio|range|color|file>` — у них intrinsic-размер
  действительно свой (виджет), подпись отсутствует, и `preferred_inline_block_width`
  вернёт по ним 0: они должны остаться на старом пути, иначе схлопнутся уже они.
- `<input type=submit|reset|button>` — подпись рисует
  `emit_input_value_text` из `FormControlKind::Input.value_text`, а не дочерний
  бокс, так что ширину по ней `preferred_inline_block_width` не увидит; нужна
  отдельная ветка (ср. `field_sizing_content_intrinsic`, который уже так делает
  для `field-sizing: content`).
- `<select>` — подпись тоже не бокс, а `FormControlKind::Select { selected_text }`
  (`<option>` по UA-стилю `display: none`), плюс место под стрелку.
- Правка двигает пиксели: полный графический прогон + регенерация CPU-снапшотов
  в том же коммите. Затронуты как минимум `34-forms` и любая страница с кнопкой.

## Связанное

- [BUG-425](BUG-425-FIXED.md) — тот же `is_replaced`, ветка `display: flex/grid`.
- IFC-1 (ROADMAP.md) — базовая линия у форм; найден при её проверке. Выравнивание
  кнопки по строке уже правильное, видна она от этого не становится.
