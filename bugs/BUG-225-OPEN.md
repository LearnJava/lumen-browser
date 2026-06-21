# BUG-225

**Статус:** OPEN
**Компонент:** paint (display_list.rs)
**Тест:** TEST-93 (остаток 3.54% после BUG-211)

## Описание

`appearance: none` подавляет value/placeholder-текст у text-подобных `<input>`.

## Корень

`emit_form_control_indicator` (`crates/engine/paint/src/display_list.rs:4207`)
делает ранний `return` при `b.style.appearance == Appearance::None`. Гейт призван
скрыть нативные примитивы (галочка checkbox, точка radio, ползунок range, бар
progress/meter, стрелка select), но он стоит ДО ветки, которая рисует value/
placeholder text-подобных инпутов (`emit_input_value_text`/`emit_input_placeholder_text`).
В итоге `<input type=text value="ab" style="appearance:none">` не рисует "ab".

## Как чинить

Сузить гейт: подавлять только нативные примитивы (checkbox/radio/range/progress/
meter/select-arrow/color-swatch), но НЕ value/placeholder text-подобных инпутов и
не label кнопок. Т.е. перенести `appearance == None` проверку внутрь веток
примитивов, оставив text/button-ветки без гейта.

## Воспроизведение

`echo '<input type=text value=hi style="appearance:none">' | lumen --dump-display-list -`
→ нет `DrawText "hi"` (без `appearance:none` — есть).
