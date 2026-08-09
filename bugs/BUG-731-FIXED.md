# BUG-731: font-size не видит var()/calc() и затирается keyword-веткой main-pass

**Статус:** FIXED 2026-08-09
**Компонент:** layout (`crates/engine/layout/src/style.rs` — `apply_font_size`,
`parse_font_shorthand`, `is_font_size_token`, `apply_declaration`)
**Найден:** P3 при пересъёмке [BUG-725](BUG-725-FIXED.md), 2026-08-09

## Симптом

Заголовок первого экрана `https://www.tbank.ru/` рисовался 16px/400 (шрифтом по
умолчанию) вместо 44px/700 — при том что вес и семейство шрифта из того же
объявления применялись. То же самое на всей странице: 103 объявления размера
сайта записаны как `font: var(--tui-font-*)`, ни одно не действовало.

## Три дефекта одного места

`font-size` (и `<font-size>`-компонент `font`-shorthand) считает **только**
pre-pass `apply_font_size` — он идёт до main-pass, потому что `em`/`%` остальных
свойств меряются от уже вычисленного размера. Но делал он это в обход всей
машинерии значений:

1. **`var()`/`env()` не раскрывались.** `apply_declaration` (main-pass) свои
   значения раскрывает, pre-pass — нет: он парсил сырую строку `var(--fs)`,
   `parse_length_q` возвращал `None`, декларация молча исчезала. Отсюда же
   асимметрия «вес применился, размер нет»: остальные longhand-ы shorthand-а
   выставляет main-pass. Вдобавок custom-properties pass стоял **после**
   pre-pass-а, так что даже с раскрытием переменная, объявленная на том же
   элементе (`.card { --fs: 20px; font-size: var(--fs) }`), была ему не видна.
2. **Shorthand не переживал `calc()`.** `is_font_size_token` перечислял варианты
   `Length` вручную и не включал `Calc` (и cq*-единицы), хотя
   `resolve_font_size` их считает; а токенизация shorthand-а резала строку по
   пробелам и `/` без учёта скобок. Поэтому
   `font: 700 calc(0px + 44px) / calc(44px * 1.09) X` признавался невалидным
   целиком, тогда как longhand `font-size: calc(0px + 44px)` работал.
3. **CSS-wide keyword применялся дважды.** `font-size: inherit` обрабатывался в
   pre-pass (там более поздний `font`-shorthand его корректно перебивал) и ещё
   раз в main-pass — общей keyword-веткой `apply_declaration`, которая про
   shorthand ничего не знает и возвращала размер обратно в унаследованный.
   Значения-длины в main-pass и так не доходили (арм `"font-size"` был явным
   no-op), то есть асимметричной была ровно keyword-ветка.

Каждый пункт по отдельности выглядит экзотикой. Вместе они выносят типографику
любой дизайн-системы на custom properties: у `tbank.ru` значение переменной —
`700 calc(var(--tui-font-offset, 0px) + 44px) / calc((var(--tui-font-offset, 0px)
+ 44px) * 1.0909091) var(--tui-font-heading)` (пункты 1+2), а элемент несёт ещё
и `font-size: inherit` из соседнего класса той же специфичности (пункт 3).

## Фикс

* `expand_vars_and_env` — общий хелпер (`var()`, затем `env()`, семантика отказа
  = CSS Variables L1 §3.3); вызывается и из `apply_declaration`, и из
  `apply_font_size`, чтобы порядок и правила отказа не разъезжались.
* Custom-properties pass + `apply_property_initial_values` перенесены **перед**
  pre-pass-ом font-size (зависят только от `matched` и registry).
* `split_font_shorthand_tokens` — скобко-аварная токенизация: пробел и `/`
  разделяют только на глубине 0.
* `is_font_size_token` расширен до набора, который умеет `resolve_font_size`
  (`Calc` + cq*); intrinsic-keyword-ы (`min-content` и Ко) по-прежнему отсеяны.
* `apply_declaration` отсекает `font-size` до keyword-ветки — размер целиком
  принадлежит pre-pass-у; дублирующий арм в `apply_css_wide_keyword` удалён.

## Проверка

11 юнит-тестов, в том числе обратный порядок (`font: …; font-size: inherit;` —
более поздний keyword обязан победить, иначе каскад ломается в другую сторону) и
прямой тест токенизатора. Полный `lumen-layout` — 3514 тестов, зелёный.
`graphic_tests/dump_golden.py` — 12/12 дампов без изменений; ни одна страница
корпуса `graphic_tests/`/`samples/`/`assets/` не использует
`font-size: var(`/`font: var(`/`color-scheme: var(`, поэтому пиксельный корпус
затронут быть не мог. Живая проверка: заголовок `tbank.ru` 16px/400 → 44px/700,
подзаголовок и межстрочный интервал совпали с эталонным Edge.
