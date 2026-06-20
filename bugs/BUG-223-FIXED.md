# BUG-223 — `lumen-network-service` не компилируется: non-exhaustive match по `IpcRequest`

**Статус:** FIXED 2026-06-20
**Компонент:** network / ipc
**Обнаружен:** 2026-06-19 (при `cargo clippy --workspace` в ходе B-1)

## Исправление (2026-06-20)

В `network_service.rs` добавлены явные ветки для таб-вариантов `IpcRequest`:
`CreateTab` и `CloseTab`/`NavigateTab`/`Screenshot` (через `tab_id`-биндинг)
отвечают `IpcResponse::TabError` — сетевой процесс вкладками не управляет.
Match оставлен **исчерпывающим** (без `_ =>`), чтобы будущие варианты снова
вызывали ошибку компиляции, а не молчаливое игнорирование. `cargo clippy
--workspace --all-targets -- -D warnings` снова зелёный. Закрыто попутно при
BUG-221 (обнаружено финальным workspace-гейтом).

## Симптом

`cargo clippy --workspace` / `cargo build --workspace` падает с hard-ошибкой
`E0004`:

```
error[E0004]: non-exhaustive patterns: `lumen_ipc::IpcRequest::CreateTab`,
`lumen_ipc::IpcRequest::CloseTab { .. }`, `lumen_ipc::IpcRequest::NavigateTab { .. }`
and 1 more not covered
  --> crates/network/src/bin/network_service.rs:51:15
```

## Причина

TAB-4/TAB-5 (влиты 2026-06-18) расширили `lumen_ipc::IpcRequest` таб-вариантами
(`CreateTab` / `CloseTab` / `NavigateTab` / `Screenshot`), но `match` в
`crates/network/src/bin/network_service.rs:51` (отдельный сетевой процесс,
`--network-service`) не был обновлён под новые варианты. Бинарь
`lumen-network-service` с тех пор не компилируется → `--workspace` сборка красная.

Не связано с B-1: `lumen-network` не зависит от `lumen-js`; per-crate сборки
(`-p lumen-js`, `-p lumen-shell`) проходят.

## Что сделать (P3)

В `crates/network/src/bin/network_service.rs:51` добавить ветки для таб-вариантов
`IpcRequest`. Сетевой процесс не обслуживает вкладки, поэтому корректный ответ —
`IpcResponse::TabError` (или аналог «не поддерживается в network-service») для
`CreateTab`/`CloseTab`/`NavigateTab`/`Screenshot`, либо явный `_ =>` с
диагностикой. После — `cargo build -p lumen-network --all-targets` зелёный.
