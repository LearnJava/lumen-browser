# BUG-1772 — синхронное чтение стиля: `<style>`, вставленный скриптом перед статическим, оказывается в каскаде после него

**Статус:** OPEN
**Заведён:** 2026-10-28 (P3, найдено при [BUG-1556](BUG-1556-FIXED.md))
**Область:** js (`crates/js/src/v8_runtime/sheet_sync.rs::patched_cascade`)

## Симптом

Страница с `<style>#a{height:200px}</style>`; скрипт делает `document.head.prepend(style)` со `#a{height:184px}` и тут же читает `getComputedStyle(a).height`. По порядку документа побеждает **последний** лист (200px), у нас `184px`: лист, которого нет в тексте каскада флаша, `patched_cascade` вливает «в конец» (`unlocated` → `merge_from`), а не на своё место в документе (на своё место встаёт только пустой `<style>` с `@layer`).

Так устроена идиома WPT `support/getComputedStyle-insets.js` (`document.head.prepend(stylesheet)`) — тесты `getComputedStyle-insets-sticky-container-for-abspos.html` её не замечают, потому что ожидание считают из того же `clientHeight`.

## Проба

`InProcessSession::navigate_html` страницы выше и `eval("var st=document.createElement('style'); st.textContent='#a{height:184px;padding:8px 0}'; document.head.prepend(st); getComputedStyle(document.getElementById('a')).height")` → `184px`, ожидается `200px`. Живой шелл не проверялся: его каскад собирается из DOM по порядку (`merge_unlocated_text` включается только для листа синхронного флаша, присланного до вставки).

## Что делать

В `patched_cascade` вставлять лист с текстом в позицию `cursor` (между уже найденными листами) заменой `(cursor, cursor, text)`, а не в конец — как уже делается для `@layer`; следить за тем, что подмена текста сдвигает индексы CSSOM-правок других листов (`bases`).

## Как проверить

Юнит-тест рядом с `layer_inserted_into_an_empty_style_lands_at_its_document_position` + проба выше в `crates/driver/tests/cases/`.
