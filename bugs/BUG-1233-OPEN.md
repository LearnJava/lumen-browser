# BUG-1233 — У отсоединённого документа нет `createRange`

**Статус:** OPEN
**Заведён:** 2026-10-01 (P3, остаток [BUG-1161](BUG-1161-FIXED.md))
**Область:** js — `_lumen_make_range` хранит границы как `nid` арены, а документ из
`createHTMLDocument`/`createDocument`/`new Document()` узла арены не имеет.

## Симптом

`foreignDoc.createRange is not a function` — `dom/ranges/Range-selectNode.html` ERROR без сабтестов.

## Что чинить

Границы Range, допускающие контейнер без `nid` (отсоединённый документ), с учётом `Range-*`
сравнений по tree order.
