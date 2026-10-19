# BUG-1642 — Лишняя горизонтальная полоса прокрутки страницы, нарисованная на 72 px выше низа

**Статус:** OPEN
**Заведён:** 2026-10-19 (живой прогон https://otpravka.pochta.ru/specification#/enums-base-mail-type)
**Область:** shell (`crates/shell/src/display_list_metrics.rs::content_width_of`, `crates/shell/src/scrollbar.rs::build_hscrollbar_overlay`)

## Симптом

Поверх таблицы и подвала во всю ширину окна лежит серая полоса 8 px (thumb `#767676` на дорожке
`#dfdfdf`) — горизонтальная полоса прокрутки страницы. Прокрутить вбок нельзя (`scrollTo(300,0)` →
`scrollX = 0`), в Chrome полосы нет. Висит на одной высоте окна при любой прокрутке; заметнее
всего внизу, где ложится на пустой фон над подвалом.

Источник ширины — декоративная подложка шапки:
`.header-v2__section:before { position:absolute; left:-50vw; width:150vw }` внутри
`.header-v2__section { position:relative }`, предок `div.page { overflow:hidden }`. Правый край
подложки ≈ на 20 px правее viewport-а, но обрезан `overflow:hidden`. Проверено вживую:
`left:0` / `width:100vw` / `position:static` у секции убирают полосу, `width:300vw` уменьшает
thumb до 28 %.

## Причины (две, независимые)

1. **Ширина контента без учёта клипа.** `content_width_of` берёт максимум `x + width` по всем
   прямоугольникам display list-а, включая содержимое под `PushClipRect` (и сами клип-рекы);
   `overflow:hidden` предка не учитывается. `hbar_track` (`scrollbar.rs:217`) показывает полосу при
   `content_width > viewport_width`. Ширину надо брать из scrollable overflow корня по layout
   (с клипом `overflow` не-`visible` предков), а не из display list-а.
2. **Позиция по y.** `track_y = viewport_height - SCROLLBAR_WIDTH` (`scrollbar.rs:255`), где
   `viewport_height_css()` уже без `CHROME_H` (72), но команда кладётся в `overlay_buf` в координатах
   окна без сдвига на `page_offset()` (`app/window_event/redraw_requested.rs:753`). Полоса встаёт на
   `CHROME_H` выше нижнего края области страницы (замер: строки окна 968–975 при низе ≈ 1040).

Минимальный локальный репро с той же структурой полосу в свежем окне не воспроизвёл — причина
не выяснена; при фиксе сначала собрать репро (возможно, нужен `ng-include`-перерасчёт / relayout).

## Что сделать

- `content_width` — из layout (scrollable overflow корня с учётом клипов), тест на
  `overflow:hidden`-предка с `left:-50vw; width:150vw`.
- Позиция полосы — в тех же координатах, что и вертикальная полоса / page offset; тест на
  `build_hscrollbar_overlay` + место вызова.
