# BUG-1275 — `text-indent` в вертикальном `writing-mode` игнорируется

**Статус:** OPEN
**Заведён:** 2026-10-05 (P2, WPT-RUN-14 срез 2 — `css/css-writing-modes/text-indent-*`)
**Область:** layout (`crates/engine/layout/src/vertical.rs::wrap_inline_run_vertical`)

## Симптом

```html
<div style="writing-mode:vertical-lr;height:200px;text-indent:80px;font:20px monospace">A</div>
```

`--dump-layout`: `InlineRun … text-indent=80.00`, но `frag[0] x=0.00 "A"` — первая строка (колонка) начинается от
начала inline-оси. Тот же блок в `horizontal-tb` даёт `frag[0] x=80.00`.

## Причина

Горизонтальный перенос строк (`box_tree/inline_wrap.rs`) применяет `text-indent` к первой строке; вертикальный
(`wrap_inline_run_vertical`) его не читает вовсе — `grep text_indent crates/engine/layout/src/vertical.rs` пуст.
Вместе с ним в вертикальном пути не учтены `each-line`/`hanging` и процентный `text-indent` от inline-размера
контейнера (его высоты).

## Как проверить

WPT `css/css-writing-modes/text-indent-v{lr,rl}-*` (16 reftest), например `text-indent-vlr-003.xht`: зелёный квадрат,
без красного.
