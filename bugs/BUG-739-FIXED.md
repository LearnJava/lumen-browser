# BUG-739: `display: inline-flex` / `inline-grid` не создают бокс вообще

**Статус:** FIXED 2026-08-10
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs` — построение
дерева боксов, `collect_inline_segments` / выбор формат-контекста)
**Найден:** P3 при разборе [BUG-733](BUG-733-FIXED.md), 2026-08-10

## Симптом

Элемент с `display: inline-flex` или `display: inline-grid` не получает
собственного бокса: его содержимое уплощается в `InlineRun` родителя, как если
бы `display` был `inline`. Не рисуются ни фон, ни рамка; flex/grid-алгоритм не
запускается; ширины/высоты элемента нет.

`--dump-layout` по странице

```html
<div><div style="display:inline-flex;background:#ccf"><span>AAAA</span><span>BBBB</span></div></div>
```

даёт один `InlineRun` с сегментами `"AAAA"`, `"BBBB"` и ни одного бокса с фоном.
Так же ведут себя `<span style="display:inline-flex">` и
`display: inline-grid` (`grid-template-columns` игнорируется целиком). Соседний
`display: inline-block` работает штатно — то есть проблема не в inline-уровне
как таковом, а в том, что именно эти два значения нигде не блокифицируются в
atomic inline-level бокс.

По спеке (CSS Display L3 §2.1) `inline-flex`/`inline-grid` — atomic inline-level
боксы: снаружи ведут себя как `inline`, внутри создают flex/grid formatting
context, то есть должны собираться в тот же `BoxKind::InlineBlockRow`, что и
`inline-block`, но лэйаутиться `lay_out_flex`/`lay_out_grid`.

При этом `CSS-SPECS.md` объявляет `display` ✅ со списком значений, включающим
`inline-flex`/`inline-grid`: парсинг и `ComputedStyle` действительно есть
(`Display::InlineFlex`/`InlineGrid`), не реализован именно layout.

## Влияние

Прямой вклад в пункт 2 [BUG-733](BUG-733-FIXED.md) — схлопнувшуюся CTA-кнопку
`tbank.ru`: её содержимое (`span.dbwTheaUM`, `span.abErxfKrf`,
`span.cbErxfKrf { width: 16px }` — иконка) объявлено `display: inline-flex`,
поэтому боксов не имеет и ширины кнопке не даёт. Пункт 2 закрывается только
вместе со вторым дефектом — `width: 100%` внутри inline-escape
(см. заметку в [BUG-733](BUG-733-FIXED.md)).

## Как воспроизводить

`.tmp/p3/inline-flex.html` и `.tmp/p3/inline-grid.html` в ветке
`p3-bug-733-flex` (обе страницы читаются `--dump-layout` напрямую).

## Направление фикса (записано при заведении)

Блокификация в атомарный inline-level бокс: там, где `display: inline-block`
кладётся в `InlineBlockRow`, тот же путь должен принимать `InlineFlex`/
`InlineGrid`, а внутренний лэйаут диспетчеризоваться в `lay_out_flex` /
`lay_out_grid` (диспетчер уже умеет оба значения — см. ветки
`Display::Flex | Display::InlineFlex` в `lay_out`). Intrinsic-ширина
`inline-flex` после [BUG-737](BUG-737-FIXED.md) считается правильно
(`is_row_flex_container` учитывает `InlineFlex`), отдельной работы не требует.

## Что сделано (2026-08-10, P3)

Направление подтвердилось целиком: движок уже умел всё, кроме одного — считать
эти два значения не-inline. Три точки в `box_tree.rs`:

1. `produces_inline_segments` — убраны `InlineFlex`/`InlineGrid`, остался ровно
   `Display::Inline`. Это и есть корень: функция решала «уплощать ли узел в
   `InlineSegment`-ы», и через неё же (`is_inline_content`,
   `produces_inline_segments_nested`) элемент терял бокс и как ребёнок блока, и
   как потомок inline-элемента.
2. `is_inline_block` → `is_atomic_inline_level`: принимает все три значения
   `inline-block`/`inline-flex`/`inline-grid` (CSS Display L3 §2.1), то есть все
   они собираются в общий `BoxKind::InlineBlockRow` и текут рядом с текстом.
   `breaks_inline_row` их и так не разрывал — правка не потребовалась.
3. Phase-0 shrink-to-fit auto-ширины (`lay_out`) распространён с `InlineBlock`
   на все три. Без этого бокс появлялся, но растягивался на всю строку:
   auto-ширина atomic inline-level бокса — shrink-to-fit (CSS 2.1 §10.3.9), а не
   «весь доступный inline-размер», как у блока.

Внутренний лэйаут дописывать не пришлось: `lay_out` диспетчеризует по
`Display::Flex | Display::InlineFlex` и `Display::Grid | Display::InlineGrid`,
`is_item_container` (блокификация детей) уже перечисляет оба inline-значения.

### Сверка с headless Edge (10 форм, `.tmp/p3/bug739.html`)

Эталон снят self-printing страницей (`getBoundingClientRect` → `<pre>` →
`msedge --headless=new --dump-dom`), поэтому числа точные, а не с картинки.

| Форма | Edge | Lumen после |
|---|---|---|
| `<div inline-flex>` с двумя спанами | 85.38×19.19 | 85.38×19.20 |
| то же на `<span>` | 85.38 | 85.38 |
| `width:200px; padding:5px; border:2px` | 214×33.19 | 214×33.20 |
| `flex-direction: column` | 42.69×38.38 | 42.69×38.40 |
| `gap: 10px` | 95.38 | 95.38 |
| внутри `<a>` (путь `InlineEscape`) | 85.38 | 85.38 + цвет ссылки |
| два `inline-flex` подряд | на одной строке | на одной строке |
| `inline-grid` с треками `60px 30px` | элементы 60 / 30 | элементы 60 / 30 |

Два расхождения остались и **вынесены в отдельные заявки**, а не замаскированы:

* ширина самого `inline-grid`-контейнера — shrink-to-fit по-прежнему меряет его
  блочным правилом «самый широкий ребёнок» (42.69 и 10.67 против 85.38 и 90 у
  Edge), поэтому фон/рамка уже́ рисуются, но уже́ содержимого. Это ровно
  [BUG-740](BUG-740-OPEN.md) — там сознательно записано, что честная ширина
  grid-а требует прогона track sizing; после этого фикса дефект стал видимым, а
  не только измеримым;
* `before <inline-flex> after` — бокс переносится на следующую строку вместо
  того, чтобы встать в строку с текстом. Это НЕ регресс 739: в `InlineBlockRow`
  анонимный `InlineRun` занимает всю доступную ширину, поэтому любой atomic
  inline-level бокс после текста переносится — воспроизведено на `inline-block`
  тем же дампом на бинарнике ДО фикса. Заведено как
  [BUG-741](BUG-741-OPEN.md).

### Проверка

8 юнит-тестов `bug739_*` (бокс + shrink-to-fit, элементы бок о бок, `gap`,
колонка, `width`+padding+border, grid track sizing, `InlineEscape` внутри `<a>`,
два соседних на одной строке), `lumen-layout` 3541 (было 3533), `dump_golden`
12/12, `--dump-display-list` по всем 174 страницам `graphic_tests/` + `samples/`
до и после — идентичны (ни одна страница корпуса не объявляет `inline-flex`/
`inline-grid`, так что визуальный прогон не мог дать другого результата).

Два теста в `lib.rs` (`display_inline_flex_parses_and_stores`,
`display_inline_grid_parses_as_inline_family`) утверждали сам дефект —
«inline-flex должен попасть в `InlineRun`» — и переписаны на спековое ожидание;
второй заодно переименован в `display_inline_grid_creates_its_own_box`.

**Пункт 2 [BUG-733](BUG-733-FIXED.md) этим НЕ закрыт**: остаётся второй дефект
кнопки — `width: 100%` внутри `InlineEscape` резолвится не от блочного предка.
