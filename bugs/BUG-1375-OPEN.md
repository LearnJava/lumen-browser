# BUG-1375 — кодировки таблиц стилей: Shift_JIS, Big5, windows-1252 и ISO-8859-x не поддерживаются

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** shell/encoding (`crates/shell/src/stylesheets.rs::document_encoding`, `crates/engine/encoding/src/lib.rs::Encoding::from_label` — нет Shift_JIS, Big5, windows-1252, ISO-8859-x)

## Симптом

`Encoding` (`encoding/src/lib.rs:41`) — `Utf8`, `Utf16Le/Be`, `Utf32Le/Be`, `Windows1251`, `Koi8R`, `Cp866`. `from_label` для `shift_jis`, `windows-1252`, `iso-8859-1`, `big5` … возвращает `None`, и `document_encoding` откатывается на UTF-8. Файл с байтами `½a` (`平和` в Shift_JIS) читается как мусор.

Проба: страница `<meta charset=utf-8>` + `<link rel=stylesheet href=sup/sj.css>`, где `sj.css` — `@charset "shift-JIS"; .<95 bd 98 61>, #d2{color:green}` (байты), `<div class="平和">`: селектор по классу **не совпал** (`#ff0000` вместо `#008000`); `#d2` в той же таблице — совпал. Тот же результат по http (`python -m http.server`) и при `Content-Type: text/css; charset=Shift_JIS`. UTF-8-вариант той же таблицы работает. `<meta charset=windows-1252>` и `shift_jis` у самой страницы: `--dump-display-list` печатает кодировку `koi8-r`/`ibm866` (эвристика) — страница в этих кодировках не читается вовсе.

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/at-charset-002…076` (24 id) и `character-encoding-031…036` (6 id) — тест кладёт CSS-файл в чужой кодировке и проверяет, что `.平和`/`.tést` совпал. Два id (`at-charset-039`, `-044`) — BOM/UTF-32 и MIME, `content-type-000/001` — `text/plain` вместо `text/css`.

## Что делать

Добавить в `lumen-encoding` таблицы для WHATWG-меток `shift_jis`, `big5`, `windows-1252` (и `iso-8859-1`), `iso-8859-5/6/7/8/11`, `euc-jp`, `gbk`, `euc-kr` — либо через `encoding_rs` (уже в `Cargo.lock` как транзитивная; новая прямая зависимость — блок «Why this dependency», ADR-027), либо своими таблицами по правилу «своё vs вендоренное». Порядок выбора кодировки таблицы — CSS Syntax L3 §3.2: BOM → HTTP charset → `@charset` → `<link charset>` → кодировка документа (всё уже стоит в `stylesheets.rs`, нет только декодеров). Браузерный мир читает много legacy-страниц: ширина дефекта — вся не-кириллическая не-UTF-8 часть веба, а не только WPT.

## Как проверить

`css/CSS2/syntax/at-charset-007.xht`, `at-charset-024.xht`, `character-encoding-031.xht`; `tests/wpt/encoding/` — отдельный корпус.
