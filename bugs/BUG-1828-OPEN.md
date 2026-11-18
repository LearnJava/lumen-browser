# BUG-1828-OPEN: WPT-прогон красный при включённом системном прокси Windows

- **Статус:** OPEN
- **Область:** shell/network ↔ wpt-runner (`crates/shell/src/cli_args.rs` — установка системного прокси; `tests/wpt/run_corpus.py`, `tools/wptrunner/wptrunner/browsers/lumen.py` — окружение запускаемого браузера)
- **Приоритет:** P3 (блокирует весь WPT-конвейер на машине с прокси; обход существует)

## Симптом

При включённом системном прокси Windows (`ProxyEnable=1`, на этой машине
`127.0.0.1:12334`) каждый WPT-тест падает как TIMEOUT: в логе браузера

```
GET http://web-platform.test:18300/dom/nodes/Element-hasAttribute.html
502 http://web-platform.test:18300/dom/nodes/Element-hasAttribute.html
Ошибка загрузки ...: http status 502
```

и исполнитель ждёт `testharnessreport.js` результаты до таймаута. `pass-rate: 0.00%`,
все статусы `TIMEOUT`.

## Причина

Браузер применяет системный прокси (`crates/shell/src/system_proxy.rs`) к запросу по
имени хоста. Обход BUG-1803 (не проксировать loopback) работает по имени (`localhost`,
`127.0.0.1`), а `web-platform.test` — не loopback-имя: маппинг в `127.0.0.1` делает
`LUMEN_RESOLVE` уже ПОСЛЕ решения о проксировании. Прокси имени не знает → 502.

Переменные `HTTP_PROXY`/`HTTPS_PROXY` в окружении тут ни при чём: воспроизводится и
с пустым окружением — читается реестр (WinINET).

## Обход (2026-11-18, IDB-EVENTS-S2)

```bash
LUMEN_NO_SYSTEM_PROXY=1 python tests/wpt/run_corpus.py --binary target/dev-release/lumen.exe ...
```

Браузер наследует окружение раннера, `cli_args.rs:797` читает
`LUMEN_NO_SYSTEM_PROXY` и не применяет системный прокси. С переменной dom-проба
даёт 100%, IndexedDB — штатно.

## Что чинить (на выбор)

1. `tools/wptrunner/wptrunner/browsers/lumen.py` — ставить `LUMEN_NO_SYSTEM_PROXY=1`
   рядом с `LUMEN_NO_ADBLOCK` для каждого запускаемого браузера (минимальный дифф,
   закрывает раннер).
2. В сетевом стеке: не проксировать имена, входящие в `LUMEN_RESOLVE` (в духе
   BUG-1803, но по имени, а не по loopback).

Найден P1 при IDB-EVENTS-S2, 2026-11-18.
