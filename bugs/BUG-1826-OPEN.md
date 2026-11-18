# BUG-1826 — claude.ai: управляемый челлендж Cloudflare вешает движковый поток >4 мин (PoW-скрипт не завершается)

**Статус:** OPEN
**Заведён:** 2026-11-18 (Devin, при проверке BUG-1803 — системный прокси; маршрут уже исправлен, остался дефект движка)
**Компонент:** js/engine (выполнение обфусцированного скрипта `cdn-cgi/challenge-platform/.../chl_page`); семья BUG-935/BUG-1778/BUG-1800 (engine thread busy)

## Симптом

Живое окно `lumen.exe --maximized --mcp-live-port N https://claude.ai` (системный прокси Windows применён, BUG-1803):

- `claude.ai` → `302` → `/login` → `403` + страница «Just a moment...» (Cloudflare managed challenge, `[gate] cloudflare site=claude.ai подряд=1`).
- `GET /cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=…` → `200`, после чего `CSS пересобран после правки <style>: 210 правил` и `[chip] blank_screen site=claude.ai`.
- Дальше **одна JS-задача на движковом потоке идёт >135 с непрерывно** (зафиксировано `eval` через MCP: `engine thread busy: eval not run within 110.0 s (running task from crates\shell\src\app\about_to_wait.rs:339:13 for 135.1 s)`; первый непустой кадр — 238 с после старта процесса). Ошибок JS в `resource://console` нет — скрипт не падает, а крутится.
- Скрипт челленджа в Chrome проходит за секунды (proof-of-work + сбор фингерпринта); в Lumen он не завершается и за 4+ минуты — для пользователя это зависшая страница (сторож `script_stall` показывает «Страница не отвечает»).

## Гипотезы (не проверено)

- Busy-wait на shared-состоянии, которое выставила бы другая асинхронная задача, — а движковый поток один и занят этим скриптом (deadlock-by-cooperative-scheduling).
- Патологическая медлительность V8 на обфусцированном PoW (большие BigInt/typed-array циклы, отсутствие JIT-tier-up по какой-то причине).
- Ожидание события, которое в Lumen не наступает (`requestAnimationFrame`? `crypto.subtle`? `Atomics.wait` на main-потоке?), реализованное циклом.

## Репро

```
lumen.exe --maximized --mcp-live-port <N> https://claude.ai   # Windows, системный прокси включён
# через ~30 с: tools/call eval {code: "document.title", timeout_ms: 110000}
# → engine thread busy … running task from about_to_wait.rs:339 for >130 s
```

Скрипты пробы: `.tmp/mcp_probe2.py` в worktree `devin-proxy` (MCP JSON-RPC по TCP, token из stderr). Headless `--screenshot` челлендж не проходит by design (нет живого цикла кадров) — reproduce только в живом окне.

## Что делать

1. Снять, ЧТО крутит скрипт: stack sampling V8 (`SIGPROF`-аналога нет — кандидат: периодический `v8::StackTrace`/`CpuProfiler` из watchdog `script_stall`, или `LUMEN_SCRIPT_STALL_MS` + дамп текущего script source по `Script::Name`/`SourceURL`).
2. Если busy-wait — найти, какое API он ждёт, и почему тот не стреляет именно в этом рантайме.
3. Если медлительность — профиль (tracy/`--features tracy`), сравнить с Chrome на том же `chl_page` скрипте (он отдаётся по `ray`, можно скачать и прогнать офлайн в обоих движках).
