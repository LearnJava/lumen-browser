# BUG-1809 — computed `display` boxless-элементов = "" (красные детерминированные тесты на чистом main)

**Статус:** FIXED 2026-11-19 (P6)
**Открыт:** 2026-11-14 (P3, гейт BUG-1802)
**Область:** layout (`crates/engine/layout/src/lib.rs::collect_computed_styles_parts` — публикация
сегментных записей снапшота computed-style)

## Симптом

- `getComputedStyle(document.getElementById('empty')).display` → "" вместо "inline"
  (`bug1191…boxless_elements_report_full_computed_style`) — к моменту взятия задачи уже зелёный
  (позже закрыт на main независимо);
- `getComputedStyle(<display:contents>).display` → "" вместо "contents"
  (`bug489…display_contents_element_reports_its_own_computed_style`) — красный на main `e666d197f`.

## Причина

`collect_computed_styles` (BUG-935 срез 72) публикует для текстовых сегментов `InlineRun`
трёхсвойственную запись (`INLINE_SEGMENT_PROPERTIES`) под ключом `seg.source_node`. Маркер
пустого инлайна (BUG-1558) BUG-1784 издалека отсекал проверкой `is_empty_inline_marker()`, но
хранитель маркера — нулевой offset при пустом тексте: обёртка, дописывающая пробел переносимого
whitespace в последний сегмент строки (`last.text.push(' ')`), делает маркер неопознаваемым.
На странице bug489 (`<div display:contents></div>\n<div display:contents></div>` — переносы строк
DOM дают discardable-текстовые узлы) `body` строит `InlineRun`, чьи сегменты источники — сами
contents-элементы: у первого текст `« »`, у второго — маркер. Трёхсвойственная запись под ключом
t1 занимала слот, и бэкфилл boxless-стилей (`out.contains_key` → skip) не публиковал настоящую
каскадную запись — элемент отвечал "" по всем свойствам, кроме трёх текстовых.

## Что сделано (2026-11-19, P6)

Проверка в `collect_computed_styles_parts` заменена с «маркер — пропустить» на «публиковать
трёхсвойственную карту только для текстового узла» (`matches!(doc.get(seg.source_node).data,
NodeData::Text(_))`): карта — запись текстового узла по определению (свойства `innerText`),
элементно-источникованный сегмент не должен занимать слот ни в какой форме. Проверка поглощает
как BUG-1784 (маркер — всегда элемент), так и форму с дописанным whitespace; текстовые записи
настоящих текстовых узлов, публикация полных карт обычных инлайнов через
`inline_element_ancestors` (BUG-488) и `inline_run_empty_elements` (прямой чтитель маркеров
для `getBoundingClientRect`) не затронуты.

## Проверка

- `bug489_display_contents_element_reports_its_own_computed_style` и оба теста bug1191 — зелёные
  (`cargo test -p lumen-driver --test all` 244 passed; единственный FAIL `snapshot_cpu`
  предсуществует на чистом main — те же 42-position-sticky/49-background-blend-mode при
  прогоне без правок, флакующий растр);
- регрессионный тест формы на уровне layout:
  `fixtures_and_core_selectors::contents_element_between_whitespace_texts_reports_its_own_display`;
- `cargo test -p lumen-layout --lib` — 6103 passed; `cargo test -p lumen-js --features v8-backend`
  (computedstyle 20, bug935 S5x, inner_text 11) и `bug1671_inline_rects` — зелёные;
- гейт: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чистый. Изменение правит
  только CSSOM-снапшот (`getComputedStyle`), пиксели не трогает — граф-гейт не требуется.

Заведён P3 при scoped-гейте BUG-1802; окно регрессии не бисектилось (механизм — взаимодействие
BUG-1558/1784 с whitespace-обёрткой, а не отдельный коммит).
