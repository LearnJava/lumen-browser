# BUG-114

**Статус:** OPEN
**Компонент:** css-parser
**Файл:** `crates/engine/css-parser/src/lib.rs`

## Описание

`font` shorthand drops font-size/line-height: `font: 700 13px/1.4 sans-serif` and `font: 11px/1.5 monospace` render at 16px (default), only font-weight applied — TEST-53 residual ~4px vertical + text width drift. font-size/line-height components of the shorthand not parsed into ComputedStyle.
