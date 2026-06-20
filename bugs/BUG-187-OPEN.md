# BUG-187

**Статус:** OPEN (DEBTOR)
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

## Остаток (DEBTOR, KNOWN_DEBTORS 3.02%)

- Статический текст value у text-инпутов (`text`/`email`/`password`/`number`/
  `search`/`submit`) **не рисуется** — поля выглядят пустыми, Edge показывает
  значения. Отдельная фича (рендер value + маскирование password + клиппинг +
  вертикальное центрирование).
- Font-parity лейблов кнопок/опции (Inter vs Edge UI font — категория BUG-128).
- Вертикальное центрирование текста внутри кнопок/инпутов (Edge центрирует,
  Lumen прижимает к верху).

## Как чинить (остаток)

1. Рендер value-текста для text-инпутов в `display_list.rs`
   (`FormControlKind::Input { value_text }`), с маскированием password и
   клиппингом по content-box.
2. Вертикальное центрирование контента кнопок/инпутов.
