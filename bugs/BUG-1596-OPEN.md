# BUG-1596 — `no_persistent_state` (автоматизация, `--tor`) не соблюдается хранилищами окна: история, FTS, разрешения, закладки, загрузки, idb пишутся на диск

**Статус:** OPEN
**Заведён:** 2026-10-11 (P6, перепроверка отчёта по UI; находка P6, подтверждена живым прогоном)
**Область:** shell (`crates/shell/src/window_mode.rs` — `open_persistent` и прямые `open(browser_data_dir()…)`; `crates/shell/src/page_load.rs` — запись визита)

## Контракт

`Config::no_persistent_state` ([crates/shell/src/config.rs:208](../crates/shell/src/config.rs#L208)): «все хранилища в памяти, при завершении сессии сбрасываются». Флаг ставится принудительно для `--bidi-port`/`--mcp-live-port`/`--mcp`/`--mcp-port` ([crates/shell/src/cli_args.rs:695](../crates/shell/src/cli_args.rs#L695), BUG-315) и для `--tor` ([crates/shell/src/cli_args.rs:727](../crates/shell/src/cli_args.rs#L727), stderr: «без персистентного хранилища»). Тот же контракт — в [docs/automation.md](../docs/automation.md) (строка `LUMEN_SITE_MEMORY`: «memory only under `no_persistent_state`/Tor») и в комментарии [crates/shell/src/cli_args.rs:739](../crates/shell/src/cli_args.rs#L739) («never touch disk»).

Флаг уважают: HTTP-кэш, HSTS, `autofill_store` ([autofill_store.rs:88](../crates/shell/src/autofill_store.rs#L88)), `password_store`, `site_memory`, `threat_store`, `zoom` ([zoom.rs:100](../crates/shell/src/zoom.rs#L100)), бэкап при обновлении и проверка обновлений.

## Нарушение

Не уважают — всё, что открывает `window_mode.rs`:

| Файл в `<exe>/data` | Где открывается | Как |
|---|---|---|
| `history.db` | [window_mode.rs:707](../crates/shell/src/window_mode.rs#L707) | `open_persistent` — приватной ветки нет ([window_mode.rs:141](../crates/shell/src/window_mode.rs#L141)) |
| `history_fts.db` | [window_mode.rs:580](../crates/shell/src/window_mode.rs#L580) | `open_persistent` |
| `bookmarks.db` | [window_mode.rs:699](../crates/shell/src/window_mode.rs#L699) | `open_persistent` |
| `permissions.db` | [window_mode.rs:357](../crates/shell/src/window_mode.rs#L357) | `open_persistent` |
| `downloads.db` | [window_mode.rs:633](../crates/shell/src/window_mode.rs#L633) | `DownloadManager::open_history(browser_data_dir()…)` |
| `profiles.db` | [window_mode.rs:320](../crates/shell/src/window_mode.rs#L320) | `ProfileRegistry::open` напрямую |
| `newtab_tiles.db` | [window_mode.rs:725](../crates/shell/src/window_mode.rs#L725) | напрямую |
| `settings.db` | [window_mode.rs:772](../crates/shell/src/window_mode.rs#L772) | напрямую |
| `idb/` | [window_mode.rs:522](../crates/shell/src/window_mode.rs#L522) → [storage_stores.rs:21](../crates/shell/src/storage_stores.rs#L21) `lumen_idb_dir()` | каталог создаётся безусловно |

Запись визита ([page_load.rs:2415](../crates/shell/src/page_load.rs#L2415)) — `history_fts.index` + `history_store.record_visit` — отсекается только условием `!active_profile_is_anonymous()`; приватный флаг не проверяется.

## Воспроизведение (измерено 2026-10-11, `target/release/lumen.exe` от 2026-10-10, `main` `7b37f06f2`)

1. Скопировать `lumen.exe` в пустую папку (чтобы не трогать `data/` рабочего профиля), рядом поднять `python -m http.server 8137` со страницей `p.html` (`<title>PrivProbe</title>`).
2. `LUMEN_NO_ADBLOCK=1 lumen.exe --mcp-live-port 9417 http://127.0.0.1:8137/p.html`, подождать 15 с, завершить процесс. Ни одной команды MCP, ни одного нажатия.
3. В `data/` появились: `bookmarks.db`, `downloads.db`, `history.db`, `history_fts.db`, `newtab_tiles.db`, `permissions.db`, `profiles.db`, `settings.db` (все с `-wal`/`-shm`), каталог `idb/`.
4. `history.db`: `(1, 'http://127.0.0.1:8137/p.html', 'PrivProbe', <ts>, visit_count=1, …)`; `history_fts.db`: `('http://127.0.0.1:8137/p.html', 'PrivProbe', '')` — омнибокс её найдёт. `permissions.db` создан, пуст (разрешений не запрашивалось).

Независимое наблюдение P6 на рабочем профиле: `history.db` 9 → 10 строк после одного прогона `--mcp-live-port` на `fixture2.html`, FTS — 1 совпадение.

## Последствия

- Каждый прогон автоматизации (WPT, `perf_audit.py`, MCP-проверки) пишет историю, FTS, решения по разрешениям и т. д. в тот же `<exe>/data`, что и настоящий профиль разработчика: засоряет омнибокс/историю и делает результаты зависимыми от прошлых прогонов (класс BUG-315).
- **`--tor` (вывод из кода, не измерено — Tor-демон не поднимался):** тот же флаг, тот же путь кода, Tor не является профилем Anonymous ⇒ история и FTS посещённых через Tor адресов, решения по разрешениям, закладки и загрузки ложатся на диск. Для режима, который обещает «без персистентного хранилища», это утечка приватности.

## Направление исправления (не реализовано)

Единая точка: `open_persistent` и прямые открытия в `window_mode.rs` при `no_persistent_state || http_profile == TorBrowser` берут `open_in_memory` (тот же предикат, что в `zoom.rs`/`autofill_store.rs` — стоит вынести в одну функцию `config`), `lumen_idb_dir()` возвращает `None`. Отдельный вопрос к владельцу — нужно ли в приватном режиме читать (а не только не писать) `settings.db`/`profiles.db` рабочего профиля. Тест: `no_persistent_state` ⇒ после загрузки страницы в `browser_data_dir()` не создано ни одного файла.
