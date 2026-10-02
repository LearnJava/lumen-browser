# BUG-1246 — MCP-сервер Lumen не соответствует спецификации MCP: стандартные клиенты не подключаются

**Статус:** OPEN
**Тип:** несоответствие протоколу (реализованный код, не недостающая функциональность).
**Заведён:** 2026-10-02 (по заданию пользователя, назначен P6)
**Область:** mcp (`crates/mcp/src/server.rs`, `crates/mcp/src/protocol.rs`).

## Симптом

ИИ-агент со стандартным MCP-клиентом (Claude Desktop, Claude Code, Cursor и т.п.), подключённый
к `lumen-mcp` / `lumen --mcp` по stdio, не может работать с браузером: ответ `initialize` не
проходит валидацию клиента, схемы аргументов инструментов не видны, результаты `tools/call` не
читаются как содержимое. Внутренние Python-скрипты репозитория при этом работают — они пишут
собственный JSON-RPC-клиент под нынешний формат, поэтому дефект не ловился.

## Расхождения со спецификацией (MCP 2024-11-05, сверено по коду 2026-10-02)

1. **`initialize`** (`server.rs::on_initialize`, ~стр. 105):
   - нет обязательного `serverInfo: {name, version}` — вместо него нестандартный `serverVersion`;
   - версия захардкожена `"0.1.0"` — нарушает правило CLAUDE.md, брать из `env!("CARGO_PKG_VERSION")`;
   - `capabilities.sampling` — это возможность **клиента**, сервер её объявлять не должен.
2. **`tools/list`**: `McpTool::input_schema` сериализуется как `input_schema`, клиент ждёт
   `inputSchema` (`protocol.rs:33`, нет `#[serde(rename_all = "camelCase")]`). То же у
   `McpResource::mime_type` → должно быть `mimeType`.
3. **`tools/call`** (`server.rs`, конец `on_tools_call`): возвращает «голый» объект
   (`{"success": true, "url": …}`), спецификация требует
   `{"content": [{"type": "text", "text": …}], "isError": bool}`; ошибка инструмента —
   `isError: true` в результате, а не JSON-RPC-ошибка (JSON-RPC-ошибка — только для
   неизвестного инструмента / неверных параметров). Скриншот — `{"type": "image", "data", "mimeType"}`.
4. **`resources/read`**: элементы `contents` должны нести `uri` и `text` **или** `blob`
   (base64), сейчас — `type`/`data` без `uri`.
5. **Уведомления**: на `notifications/initialized` (сообщение без `id`) сервер отвечает ошибкой
   `-32601` с `id: null`. На уведомления не отвечают вовсе.
6. **`ping`** не реализован (`-32601`), клиенты используют его для keep-alive.

## Ограничение при исправлении

12 скриптов репозитория читают нынешнюю форму результата `tools/call` (`graphic_tests/run.py`,
`scripts/perf_audit.py`, `scripts/input_perf.py`, `scripts/mem_perf.py`, `scripts/scroll_perf.py`,
`scripts/mt_stall_bench.py`, `scripts/scroll_blit_accept.py`, `scripts/miss_probe.py`,
`scripts/bench_scroll.py`, `scripts/bug935_*.py`, `tests/wpt/verify_slice51_gaps.py`). Варианты:
мигрировать их в том же коммите, либо отдавать прежний объект в `structuredContent` (MCP
2025-06-18) рядом с `content` и переводить скрипты на него. Не ломать графический прогон:
`graphic_tests/run.py` — гейт пикселей.

Токен на TCP-портах (ADR-024 §Access model) — сознательное расширение, не трогать.

## Как проверить

Интеграционный тест полного рукопожатия на `InMemoryTransport`: `initialize` (есть `serverInfo`,
версия = `CARGO_PKG_VERSION`) → `notifications/initialized` (ответа нет) → `ping` →
`tools/list` (у каждого инструмента `inputSchema`) → `tools/call navigate`/`eval`
(`content[0].type == "text"`) → `tools/call` на ошибке (`isError: true`). Плюс ручная проверка:
`lumen-mcp` прописан в конфиг Claude Code (`claude mcp add`) и отвечает на `navigate` + `eval`.

## Связанные

- DEVX-17 (`instructions` и описания инструментов), DEVX-18 (stdio-мост к живому окну),
  DEVX-19 (руководство по подключению ИИ-агента) — все после этого бага.
