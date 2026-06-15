# BUG-071

**Статус:** FIXED 2026-06-08
**Компонент:** mcp
**Файл:** `crates/mcp/src/server.rs:508`

## Описание

`MockSession` в lumen-mcp не реализует методы `set_clock`, `set_rng_seed`, `freeze_fingerprint` из трейта `BrowserSession` (добавлены P1 в N-2 deterministic mode) — компиляция `--workspace` падает с E0046
