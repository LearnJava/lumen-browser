# BUG-187

**Статус:** FIXED 2026-06-20 (DEBTOR → KNOWN_DEBTORS BUG-128)
**Компонент:** layout/paint
**Тест:** TEST-34 (diff 4.78% → 3.02%)

## Описание

form controls: input/checkbox/radio/button/textarea/select static rendering.

## Прогресс (2026-06-20)

Закрыты основные структурные расхождения с Edge (4.78% → 3.02%):

- **inline-block-поток.** Form controls (`<input>`/`<button>`/`<select>`/
  `<textarea>`/`<meter>`/`<progress>`) рисовались как `display:block` —
  каждый занимал свою строку, разъезжаясь в столбик. UA-дефолт переключён на
  `inline-block` (`default_display`, `style.rs`), а `is_inline_block`
  (`box_tree.rs`) перестал исключать form controls — теперь они собираются в
  `InlineBlockRow` и текут в строку рядом с текстом/соседними контролами, как
  в Edge. Author `display:` поверх — выигрывает.
- **radio-точка стала кругом.** Индикатор checked-radio рисовался квадратным
  `FillRect`; теперь `FillRoundedRect` с радиусом в полстороны (круг).
  Checkbox остаётся квадратом.
- **`<option>` не утекает текстом.** `<option>` получил UA `display:none`
  (HTML rendering §15.5.3) — текст опций больше не вытекает под/над закрытым
  `<select>`; ярлык по-прежнему читается из DOM (`collect_select_label`).
  `<optgroup>` остаётся в потоке, чтобы стили вложенных опций считались
  (`:disabled`/`:checked` селекторы).
- **color-swatch.** `<input type=color>` теперь рисует свой value-цвет
  (дефолт `#000000`), игнорируя author `background`, как нативный виджет.

## Прогресс (2026-06-20, этап 2)

- **value-текст text-инпутов рисуется.** `emit_input_value_text`
  (`display_list.rs`) рисует статический `value` у `text`/`email`/`password`/
  `tel`/`url`/`number`/`search`/`date`/`time`/… как `DrawText`, вертикально
  центрированный в content-box и клиппленный по нему (`PushClipRect`/`PopClip`).
  Password маскируется U+2022 BULLET. Button-инпуты (`submit`/`reset`/`button`)
  рисуют `value` как горизонтально центрированный лейбл (дефолтные UA-лейблы
  «Submit»/«Reset» при отсутствии `value`). Поля больше не выглядят пустыми —
  совпадают с Edge (user@example.com / •••••• / 42 / query / Submit / disabled
  input). Вертикальное центрирование инпутов/кнопок-инпутов закрыто тем же кодом.
  TEST-34 (ipc): 2.95% (в пределах ±2% noise-band → baseline 3.02% сохранён).

## Остаток (DEBTOR, KNOWN_DEBTORS 3.02%)

- **Placeholder-текст** пустых полей (`placeholder="text input"`) не рисуется —
  Edge показывает серый плейсхолдер. Отдельная фича.
- **checkbox-галочка / radio-тик** — Edge рисует белую галочку в синем чекбоксе
  и синий кружок-с-кольцом у radio; Lumen — сплошной синий квадрат/круг.
- Font-parity лейблов кнопок/опции (Inter vs Edge UI font — категория BUG-128).
- Вертикальное центрирование текста-ребёнка внутри `<button>` (flow-контент,
  отдельно от инпут-лейблов).

## Как чинить (остаток)

1. Рендер placeholder-текста (атрибут `placeholder`) серым, когда value пуст.
2. Белая галочка checkbox + кольцо radio в `emit_form_control_indicator`.

## Фикс (2026-06-20, финал)

Закрыты оба оставшихся form-control-специфичных пункта.

- **Placeholder.** `FormControlKind::Input` получил поле `placeholder` (из атрибута
  `placeholder`, `box_tree.rs`). В `emit_form_control_indicator` (`display_list.rs`)
  text-подобные инпуты при пустом `value` и непустом `placeholder` рисуют его новой
  `emit_input_value_text`-зеркальной функцией `emit_input_placeholder_text` серым
  `#757575`, без password-маскировки, вертикально центрированным и клиппленным по
  content-box (HTML rendering §15.5.5).
- **Checkbox-галочка / radio-точка.** Checked checkbox/radio теперь заливают весь
  control accent-цветом (перекрывая author `background`, как нативный виджет), а
  поверх рисуется белый глиф: для checkbox — галочка триангуляцией
  (`checkmark_triangles` + `push_thick_segment` → `DrawSvgPath`, двухсегментная
  толстая полилиния ✓); для radio — белая точка-в-центре (`FillRoundedRect`
  радиусом в полстороны, диаметр ≈0.44 бокса). Это совпадает с эталоном Edge
  (синий чекбокс с белой галочкой, синий radio с белой точкой).

Визуальная сверка (`run.py --only 34`): placeholder/галочка/точка совпадают с Edge;
остаток 3.28% (в пределах noise-band baseline 3.02%) — это чисто font-parity лейблов
кнопок/значений инпутов (Inter vs Edge UI-шрифт) + вертикальный line-height сдвиг,
класс **BUG-128**. KNOWN_DEBTORS-запись TEST-34 перенаправлена на BUG-128.

Регресс-тесты (`display_list.rs`): `empty_text_input_paints_placeholder`,
`filled_input_paints_value_not_placeholder`, `checked_checkbox_paints_white_tick`,
`unchecked_checkbox_paints_no_tick`, `checked_radio_paints_white_center_dot`.
Без регрессий (paint 776 lib + 21, layout 2945 lib).
