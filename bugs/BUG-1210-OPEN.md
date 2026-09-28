# BUG-1210 — флаг `--proxy` молча не действует: профиль уже зафиксирован в `OnceLock`

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/cli_args.rs` — `config::init_global(startup_profile)` до
`extract_proxy`; повторный `config::init_global(cfg)` для `--proxy` и `--tor`;
`crates/shell/src/config.rs:49` — `GLOBAL.set(profile).is_ok()`, результат игнорируется)
**Найден:** P2, подготовка прогона top100 через split-прокси, 2026-09-28

## Симптом

dev-release `8bd5c5dd1`, чистый каталог `lumen.exe` без `data/`, на `127.0.0.1:8977` слушает сокет,
который принимает соединение и сразу его закрывает:

```
lumen --proxy http://127.0.0.1:8977 --dump-source https://example.com/?a=…   -> rc=0, страница получена
data/fingerprint.toml: proxy = "http://127.0.0.1:8977"; lumen --dump-source … -> rc=1 (прокси использован)
```

С `--proxy` ни одного соединения на прокси: трафик идёт напрямую, без сообщения об ошибке. В живом окне
(`--mcp-live-port … --proxy …`) то же — прокси прогона top100 не получил от Lumen ни одного `CONNECT`,
пока адрес не был записан в `fingerprint.toml`.

## Причина (по коду)

`run_cli` читает `fingerprint.toml` и вызывает `config::init_global(startup_profile)` ещё до разбора
аргументов (так нужно для BUG-315). Позже ветка `--proxy` клонирует профиль, ставит `cfg.proxy` и снова
вызывает `init_global(cfg)` — `OnceLock::set` уже занят, возвращает `false`, результат не проверяется.
Та же конструкция у `--tor` (`socks5_proxy`, профиль TorBrowser, `no_persistent_state`) — по коду она
тоже no-op; опытом не проверено.

Для `--tor` это вопрос приватности: пользователь видит «Tor-режим активирован», а трафик может идти
мимо Tor.

## Что сделать

Разбирать `--proxy`/`--tor` до первого `init_global` (как уже сделано для `no_persistent_state` в BUG-315)
и сделать повторный `init_global` ошибкой (`debug_assert!` / `Result`), чтобы такой no-op не повторился.
Тест: `--proxy` на закрытый порт даёт ошибку загрузки, а не страницу.
