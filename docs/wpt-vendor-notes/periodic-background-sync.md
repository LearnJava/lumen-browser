# WPT vendor notes — `periodic-background-sync`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-05 (`tests/wpt/periodic-background-sync/`, 2 тестовых файла + `service_workers/sw.js`), Service Worker расширение — фоновая ОС-интеграция. Прогон `run_report.py --all --root periodic-background-sync --recursive`: 2 id, 0/2 harness OK — оба `.https.`, TLS-гэп `UnknownIssuer`. `PeriodicSyncManager` реализован (Phase 0, `crates/js/src/periodic_sync.rs`); живая проба подтвердила реконфирмацию [BUG-386](../bugs/BUG-386-FIXED.md) (`permissions.query()` не валидирует имя). Новый баг не заведён
