# BUG-216

**Статус:** OPEN (DEBTOR)
**Компонент:** layout
**Тест:** TEST-117 (2.28% → 2.23%, KNOWN_DEBTOR)

## Описание

CSS Generated Content L3 §3.2 — `quotes` + `content: open-quote`/`close-quote`:
auto curly, вложенные `<q>`, кастомные пары, `quotes: none`.

## Расследование

Свойство `quotes` и `open-quote`/`close-quote` уже работали (правильные пары,
вложенность, кастомные, `none`). Реальный дефект был в inline-раскладке: между
`::before`/`::after` generated content и текстом `<q>` вставлялся лишний
inter-word пробел — `“ auto quotes ”` вместо `“auto quotes”` (Edge).

Корень: `wrap_inline_run` (merge-путь) и `one_line_fallback` вставляли пробел на
**любой** границе inline-сегментов. Так как whitespace-only текстовые узлы
между inline-боксами выбрасываются, движок полагался на этот безусловный пробел,
чтобы воссоздать collapsed whitespace — но не различал «была collapsible
whitespace» и «не было». Поэтому вплотную примыкающие сегменты (кавычки,
`<span>a</span><span>b</span>`, `<em>x</em>!`) ошибочно получали пробел.

## Фикс

CSS Text L3 §4.1.1 — пробел на границе сегментов только когда границу разделял
collapsible whitespace:
- collapsed whitespace-only узел между inline-сиблингами записывается как
  trailing-пробел на предыдущем сегменте (`collect_inline_segments` + `had_ws`
  в блочном inline-цикле);
- `wrap_inline_run`/`one_line_fallback` ведут `prev_trailing_ws` и вставляют
  inter-word зазор у первого слова сегмента только при наличии граничной
  whitespace; слова **внутри** сегмента разделяются как прежде.

Заодно исправляет `<span>a</span><span>b</span>`→«ab» и `<em>x</em>!`→«x!».

Регресс-тесты: `bug216_open_close_quote_abut_quoted_text`,
`bug216_adjacent_inline_boxes_join_tight`,
`bug216_inter_box_whitespace_collapses_to_one_space` (box_tree.rs).

## Остаток (DEBTOR)

TEST-117 2.23% (CPU `--ipc`) — чистый font-parity: Edge рисует тело serif,
Lumen — Inter sans, каждый глиф расходится по ширине/начертанию (построчный
ghosting). Класс BUG-128, rule 3. KNOWN_DEBTOR (baseline 2.23%).
