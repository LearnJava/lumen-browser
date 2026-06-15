# BUG-135

**Статус:** OPEN
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

INTERACTION TEST-104 (mask×gradient×radius) 51.97%: градиентная маска поверх градиентного фона/скруглений/бордера расходится во всех ячейках, включая контроль без маски (gradient+radius) — вероятно два независимых дефекта; --bisect 104
