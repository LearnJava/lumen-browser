# BUG-1201 — флак `download::tests::build_bar_shows_progress_track_for_in_progress_only`

**Статус:** OPEN
**Заведён:** 2026-09-28 (P6, гейт `scoped-test.sh` при закрытии BUG-1144; к той правке не относится).
**Область:** shell — [`crates/shell/src/download.rs`](../crates/shell/src/download.rs)
(тест `:1781`, `DownloadManager::start_download` — настоящий `std::thread::spawn` на загрузку).

## Симптом

В полном прогоне `lumen-shell --bin lumen` тест падает с `expected track + fill rounded rects
while in progress`; изолированно проходит.

## Механизм (гипотеза, не замерена)

`start_download("file:///tmp/prog.bin", …)` поднимает реальный поток загрузки. Тест шлёт в
канал `Progress{50/100}` и зовёт `poll()`, но поток к этому моменту может уже прислать своё
событие завершения/ошибки (файла нет), и после `poll()` загрузка не `InProgress` — дорожки
прогресса в панели нет. Соседний тест (`:1294`) эту гонку уже признаёт в комментарии.

## Что требуется

Тест не должен зависеть от потока: строить состояние загрузки напрямую (без `start_download`)
или поднимать `DownloadManager` без рабочего потока. Критерий — тест стабилен в полном прогоне.

## Повторные наблюдения

- 2026-09-28, P3, гейт BUG-678: упал в полном `cargo test -p lumen-shell` (2113 passed, 1 failed),
  два повторных прогона `download::tests` — 39/39 зелёные. Правка ветки (JS-шим) крейт shell не трогает.
