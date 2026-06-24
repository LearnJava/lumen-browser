# BUG-238

> Переномерован из BUG-235 при мерже ветки декомпозиции roadmap (2026-06-23):
> на main номер BUG-235 уже занят paint-build-багом (FIXED). Содержание то же.

**Статус:** OPEN
**Компонент:** shell (storage)
**Файл:** `crates/shell/src/main.rs:4304` (`lumen_idb_dir`),
`crates/shell/src/extensions/mod.rs:80` (`extensions_dir`),
`crates/shell/src/config.rs:278` (`fingerprint_path`, родственный случай)

## Описание

Несколько подсистем шелла хранят данные в OS-каталогах
(`%APPDATA%\lumen\…` на Windows, `~/.config/lumen/…` на Unix) вместо
портативного `<exe_dir>/data/`. Это нарушает политику портативного хранения
из `CLAUDE.md` (решение пользователя 2026-06-16): «новые данные хранить только
в папке браузера, не в `%APPDATA%`/XDG; использовать `browser_data_dir()`».

Точки нарушения:

| Подсистема | Функция | Текущий путь |
|---|---|---|
| IndexedDB (per-origin SQLite) | `lumen_idb_dir()` `main.rs:4304` | `%APPDATA%\lumen\idb\` |
| Extensions (manifest scan) | `extensions_dir()` `extensions/mod.rs:80` | `%APPDATA%\lumen\extensions\` |
| Fingerprint config (родственный) | `fingerprint_path()` `config.rs:278` | `%APPDATA%\lumen\fingerprint.toml` |

Эталон правильного хранения — `browser_data_dir()` в
[`crates/shell/src/adblock.rs:44`](../crates/shell/src/adblock.rs): возвращает
`<current_exe()>/data/`, создаёт подпапку на подсистему (`data/adblock/`).

## Как починить

1. IndexedDB: заменить тело `lumen_idb_dir()` на
   `browser_data_dir().join("idb")` (с per-origin подпапками как сейчас).
2. Extensions: заменить тело `extensions_dir()` на
   `browser_data_dir().join("extensions")`.
3. Fingerprint config (если решено мигрировать конфиг тоже): аналогично через
   `browser_data_dir().join("fingerprint.toml")`. Конфиг — пограничный случай
   (не «данные», а настройки), поэтому отдельный под-пункт — согласовать.
4. Миграция существующих данных: при первом запуске, если старый
   `%APPDATA%\lumen\…` существует, а нового нет — перенести (или хотя бы не
   терять). Опционально, низкий приоритет.

Размер: S (механическая замена тела трёх функций + тест на путь). Сложность —
только в решении про fingerprint.toml и опциональную миграцию.

## Контекст

Найдено при декомпозиции roadmap в `docs/tasks/` (2026-06-22): фазовые task-файлы
`ph3-indexeddb.md` и `ph3-extensions.md` помечают этот переход как Step 1.
Подсистемы рабочие end-to-end — это долг по политике хранения, а не функциональный
дефект. Связано с конвенцией `browser_data_dir` (`shell/src/adblock.rs`).
