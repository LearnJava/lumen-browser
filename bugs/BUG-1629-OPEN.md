# BUG-1629 — Живое окно (`--mcp-live-port`): первый кадр приходит через 32–131 с, `wait document_ready` истекает по таймауту

**Статус:** OPEN
**Заведён:** 2026-10-14 (P2, при проверке краулера COMPAT-CORPUS-1)
**Область:** shell/mcp (живое окно, обработка автоматизационных команд; точное место не найдено)

## Симптом

Сборка `cargo build -p lumen-shell --profile dev-release` от `main` 71cf66b5d. Окно запускается как в `scripts/perf_audit.py` (`lumen --mcp-live-port <port> --maximized about:blank`, `LUMEN_NO_ADBLOCK=1`), затем MCP `new_tab`/`navigate` на `https://example.com/` и `wait {condition: document_ready}`.

Сеть и разбор работают: в stderr `→ GET https://example.com/`, `← 200`, `Streaming завершён`, скрипт `s.js` загружен. Дальше:

- `[bench] first non-empty frame: 32517ms since process start` (`wait` с таймаутом 45 с — статус `TIMEOUT`);
- при `wait` с таймаутом 120 с — `first non-empty frame: 131334ms`. То есть кадр появляется ровно тогда, когда истекает таймаут `wait`, а не после какого-то фиксированного времени работы движка;
- `perf_audit.py --only example` на более старой сборке p2-work (13.10 17:15): `HUNG ↻`, `Navigate error: automation command timed out`;
- повтор на `hn` и `ya` (по два сайта из `docs/perf/corpus.txt`) — тоже `TIMEOUT`/`HUNG`, то есть не специфика `example.com`.

Подозрение: команда `wait` блокирует поток, который должен выдать первый кадр / отметить готовность документа, и разблокируется только по таймауту. Это гипотеза, не проверена.

## Что отличает рабочую сборку

Сборка 07.10 (`p3-work/target/release/lumen.exe`) доходит до `document_ready` за 2,5 с, но `resource://screenshot` кадра не отдаёт: `perf_audit` даёт `DEGRADED`, PNG нет. Debug-сборка 13.10 даёт кадр через 48 с. Машина в момент замеров свободна (CPU 7%, 18 ГБ свободно), рабочий стол не заблокирован (`OpenInputDesktop` успешен).

## Воспроизведение

```
cargo build -p lumen-shell --profile dev-release
python scripts/perf_audit.py --only example --exe target/dev-release/lumen.exe
```

Ожидание: `example` — `OK`/`DEGRADED` с готовностью за секунды. Факт: `HUNG`, `automation command timed out`.

## Что нужно

1. Отбисектить, с какого коммита после 07.10 первый кадр в живом окне зависит от конца `wait`.
2. Исправить; проверка — `perf_audit.py --only example,hn,ya` даёт готовность < 10 с и PNG.

## Кого блокирует

COMPAT-CORPUS-1 (`scripts/compat_crawl.py`, ветка `p2-compat-corpus-1`): плечо Lumen не проверить, приёмка на `corpus.txt`+top100 невозможна. Любой прогон `perf_audit.py` и COMPAT-AB-IDENTITY-HARNESS на живом окне — по той же причине.

См. также BUG-1630: на `main` e0c276e88 testharness по BiDi стоит в TIMEOUT из-за `loading` в `chrome_ui.rs` (UX-LOADING); вероятно, та же причина — проверить в живом окне.

## Обновление 2026-10-18 (P3, при исправлении BUG-1630)

Зависание `wait document_ready` исправлено вместе с BUG-1630 (бесконечная анимация спиннера хрома не давала
`about_to_wait` дойти до перепроверки `wait`): `perf_audit.py --only example` даёт `ready_s` 0,98 вместо `HUNG`.
**Остаток:** `resource://screenshot` не возвращает кадр — статус `DEGRADED`, `no_png_reason: resource://screenshot не вернул кадр`.
Критерий «PNG есть» этой записи пока не выполнен.

