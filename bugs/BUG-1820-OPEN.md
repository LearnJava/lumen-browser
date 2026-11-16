# BUG-1820 — `clippy -p lumen-js --all-targets` красный на чистом main: два тест-модуля без `#[cfg(feature = "v8-backend")]`

**Статус:** OPEN
**Заведён:** 2026-11-17 (P3, гейт среза 14 [BUG-1800](BUG-1800-OPEN.md))
**Область:** js (`crates/js/src/dom/tests/mod.rs:335,388`)

## Симптом

`cargo clippy -p lumen-js --all-targets -- -D warnings` на чистом main `14a917d79`
(срез 13 влит) падает в `lib test` — 8 ошибок:

- `crates\js\src\dom\tests\v8_bug935_s101_stand.rs:11` — `unresolved import
  super::v8_bug935_s55_content_journal` (модуль объявлен под cfg, mod.rs:416–417);
- `crates\js\src\dom\tests\v8_bug1408_font_cssom.rs:6` — `unresolved import
  crate::v8_runtime`, дальше `make_doc` not found (файл тянет v8-only код);
- плюс unused-import/never-used предупреждения этих двух файлов в `-D warnings`
  (артефакт того же: они не должны компилироваться без фичи вовсе).

Код дерева среза 14 бит-в-байт HEAD — регресс не от среза.

## Причина

`mod v8_bug935_s101_stand;` (mod.rs:335, `cd454631c` — BUG-935 срез 101) и
`mod v8_bug1408_font_cssom;` (mod.rs:388, `da0f00b62` — BUG-1408 срез 2) объявлены
без `#[cfg(feature = "v8-backend")]`, который имеет каждый соседний v8-зависимый
тест-модуль в этом файле. Голый `-p lumen-js` собирается без фичи (`v8-backend`
не default — её включает shell), файлы компилируются и натыкаются на
cfg-нутые/несуществующие без фичи зависимости. Это повтор семьи BUG-1804
(там чинился «`cargo test -p lumen-js --lib` не компилируется» на
`v8_ux_selection_api.rs`).

## Что делать

Добавить `#[cfg(feature = "v8-backend")]` над обоими `mod`-строками. Проверка:
`cargo clippy -p lumen-js --all-targets -- -D warnings` зелёный без фичи и с
`--features v8-backend`; `cargo test -p lumen-js --features v8-backend -- dom::tests::v8_bug935_s101_stand dom::tests::v8_bug1408_font_cssom`
зелёные (содержимое файлов не трогать).
