# BUG-1674 — `document.title` не обрезает и не схлопывает пробелы

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), rust-lang.org)
**Область:** js/dom (геттер `document.title`)

## Симптом

```html
<!doctype html><title>
   Rust   Lang
</title>
```

`JSON.stringify(document.title)`: Chrome `"Rust Lang"`, Lumen `"\r\n   Rust   Lang \r\n"` (файл с
CRLF). На rust-lang.org заголовок вкладки и `document.title` — `"\n   Rust Programming Language\n"`.

## Что делать

HTML §3.1.3 `document.title` (getter): текст элемента `title` после «strip and collapse ASCII
whitespace» (обрезать по краям, внутренние последовательности пробелов/табов/переводов строк — один
пробел). То же для SVG-`title`, если корень — `svg`. Проверить, что заголовок вкладки берётся из того
же значения.

## Как проверить

Репро выше: `"Rust Lang"`. WPT: `html/dom/documents/dom-tree-accessors/document.title-*.html`.
