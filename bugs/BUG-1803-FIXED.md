# BUG-1803 — Lumen не следует системному прокси Windows (WinINET): маршрут отличается от Chrome у пользователя с прокси

**Статус:** FIXED 2026-11-18 (минимум записи: `ProxyEnable`/`ProxyServer` как дефолт + `ProxyOverride` как список «напрямую»; PAC — отдельный срез, см. «Остаток»)
**Открыт:** 2026-11-12 (P3, перф-аудит ebay; замечено пользователем с включённым системным прокси)
**Компонент:** network (стек не читал WinINET-настройки; единственный механизм прокси — `proxy = "…"` в `data/fingerprint.toml`, BUG-1210)

## Симптом

Машина пользователя: `ProxyEnable=1`, `ProxyServer=http://127.0.0.1:12334` (HKCU\...\Internet Settings). Chrome и другие браузеры идут через прокси; **Lumen всегда напрямую** — grep по `crates/` не находил чтения системных настроек прокси (WinHttpGetIEProxyConfigForCurrentUser/реестра нет), `--proxy`-флаг не действует (BUG-1210).

Последствия (наблюдение 2026-11-12, ebay): выход-нода пользовательского прокси была забанена DataDome — **Chrome (через прокси) получал 403, Lumen (напрямую) — 200**: у пользователя Lumen «работает там, где Chrome не может», но и обратное верно — сайт, доступный только через прокси, в Lumen открыть нельзя. Отчёт: [docs/perf/runs/2026-11-12-ebay-audit.md](../docs/perf/runs/2026-11-12-ebay-audit.md) §8.

## Что сделано (2026-11-18)

`crates/shell/src/system_proxy.rs` — чтение `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`
(`ProxyEnable`, `ProxyServer`, `ProxyOverride`, `AutoConfigURL`) через `windows-sys` (`Win32_System_Registry`,
зависимость уже была). Правила:

- `ProxyEnable != 0` → `ProxyServer` становится дефолтом `FingerprintProfile::proxy`: `host:port` получает
  схему `http://`, из перечислительного вида `http=…;https=…;socks=…` берётся `https=`, иначе `http=`, иначе `socks=`.
- Приоритет выше системного у `--proxy` и у `proxy =` в `fingerprint.toml`; Tor-режим (`--tor`) системный прокси
  не берёт. Решение принимается в `cli_args::run_cli` до единственного `config::init_global` (профиль — `OnceLock`).
- `ProxyOverride` нормализуется в шаблоны «напрямую» (`*.example.ru` → `example.ru`; записи с `*` в середине,
  `127.*`/`192.168.*`, отбрасываются — их семантику `no_proxy_for_host` не выражает) и подмешивается в тот же
  реестр, что и пользовательский список COMPAT-NOPROXY (`chrome_ui::sync_no_proxy_rules`).
- `lumen-network`: loopback-хост (`localhost`, `127.0.0.1`, `::1`) минует и HTTP-прокси, и SOCKS5 **всегда**
  (`is_loopback_host` в `fetch_single`): прокси на loopback иначе замкнулся бы сам на себя, а серверы автотестов
  (wptserve, MCP-порт) живут именно там.
- Отказной выключатель `LUMEN_NO_SYSTEM_PROXY=1` — прежнее прямое соединение автотестам и отладке.
- Если системный прокси задан только PAC (`AutoConfigURL`), в stderr уходит одна строка о том, что PAC не
  исполняется и соединение прямое: «включён прокси, а Lumen молчит» иначе неотличимо от «прокси выключен».

## Проверка

- Живое окно `lumen --maximized --mcp-live-port … https://claude.ai` на машине с `ProxyEnable=1`:
  stderr `lumen: системный прокси Windows: http://127.0.0.1:12334`, далее `→ GET https://claude.ai/`,
  `← 302`, `→ GET https://claude.ai/login`, `← 403` — маршрут через прокси, как у Chrome.
- `--proxy http://127.0.0.1:9` при включённом системном прокси: соединение идёт на указанный в флаге адрес
  (ошибка `connect 127.0.0.1:9`), системная строка не печатается — приоритет флага подтверждён.
- `cargo check -p lumen-network -p lumen-shell`; `cargo test -p lumen-network --lib proxy_tests` — 31/31,
  включая новый `loopback_hosts_bypass_proxy`.

## Остаток

- PAC / автоопределение прокси (`AutoConfigURL`, `WinHttpGetProxyForUrl`) — отдельный срез: PAC-скрипт нужно
  исполнять в JS-движке и кэшировать результат по URL.
- `HTTPS_PROXY`/`HTTP_PROXY` в окружении по-прежнему не читаются (Chrome их тоже не читает на Windows —
  источник там WinINET; docs/compat-scenarios.md поправлен).

## Репро (до фикса)

Включить системный прокси, открыть в Lumen любой сайт, смотреть маршрут в сетевом логе — соединения идут мимо прокси.
