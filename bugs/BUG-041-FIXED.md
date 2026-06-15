# BUG-041

**Статус:** FIXED 2026-05-27
**Компонент:** css-parser
**Файл:** `layout/src/style.rs:19855`

## Описание

style::tests::line_clamp_integer_value / _standard_property / _not_inherited fail: CSS rule `div { -webkit-line-clamp: 3 }` produces None — test accesses doc.root().children[0] which is `<html>` after full HTML5 parsing, so rule doesn't match `<div>`
