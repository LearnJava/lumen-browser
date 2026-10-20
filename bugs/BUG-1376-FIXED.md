# BUG-1376 — CSS Syntax §5.3.1: `<!--` и `-->` между правилами должны игнорироваться

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** css-parser (`crates/engine/css-parser/src/parser.rs::parse_stylesheet` — токены `<!--` и `-->` вне правил не пропускаются)

## Симптом

CSS Syntax L3 «consume a list of rules» (top-level): токены `<CDO-token>` и `<CDC-token>` пропускаются. Lumen разбирает `<!--` как начало селектора и теряет правило.

`<style>p{color:red} <!-- p{color:green}</style>` → красный (ожидается зелёный); `<style>p{color:red} p{color:green} --></style>` → зелёный (верно: `-->` в конце); `<style>p{color:red} --> p{color:green}</style>` → красный (ожидается зелёный); `<style><!-- p{color:green} --></style>` → чёрный (правило потеряно).

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/sgml-comments-000/001/002.xht`, `css/CSS2/css1/c11-import-000.xht` (старые страницы оборачивали `<style>` в `<!-- … -->` для не-CSS браузеров).

## Что делать

В цикле верхнего уровня `parse_stylesheet` (`parser.rs:1152`) пропускать `<!--` и `-->` вместе с пробелами, как комментарии. Не пропускать внутри блока объявлений и значений.

## Как проверить

`css/CSS2/syntax/sgml-comments-000.xht`, `-001.xht`, `-002.xht`.
