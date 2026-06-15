# BUG-027

**Статус:** FIXED 2026-05-20
**Компонент:** layout

## Описание

block element ignores explicit width — body stretches to viewport

## Детали

Block-элемент с `width: 400px` берёт 100% ширины viewport. После фикса: если задан явно (не `auto`) — использовать это значение; если `auto` — брать `available_width`.
