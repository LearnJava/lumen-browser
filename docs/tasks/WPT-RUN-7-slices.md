# WPT-RUN-7 — срезы S1…S6: baseline expectations для категорий без покрытия

Родительская задача — WPT-RUN-7 (ROADMAP:756, active). Владелец — P2. Механизм — TEST-3: `tests/wpt/expectations.py`, `run_report.py --update-expected` / `--check` (`tests/wpt/README.md` §Per-category regression gate).

## Метод одного среза

1. Свежая `dev-release`-сборка, `$BIN` по `tests/wpt/README.md`. Окно не нужно.
2. Для каждой категории среза:
   ```
   tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --all --root <cat> --update-expected
   tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --all --root <cat> --check
   tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --all --root <cat> --check
   ```
3. Если второй `--check` флапает (TIMEOUT в одном прогоне и не в другом):
   - найти причину пробой;
   - флапающий сабтест пометить в `.ini` по правилам `expectations.py`, с комментарием-ссылкой на BUG;
   - не повышать таймауты вслепую.
4. Category целиком без testharness (только reftest/manual) baseline не получает. Отметить это в ROADMAP-строке и взять следующую категорию из списка кандидатов `docs/tasks/p2-test-track.md`.
5. Ни один `.ini` в чужих категориях не меняется.

Срез — до часа прогонов (~1.1 с/id при параллельном запуске).

## Готово, когда (для каждого среза)

- По каждой категории среза есть `tests/wpt/metadata/<cat>/`.
- Два `--check` подряд дают exit 0.
- Число категорий с baseline и запись среза — в ячейке `WPT-RUN-7` ROADMAP.md и в `docs/tasks/p2-test-track.md`.

## Срезы

| Срез | Категории `--root` | Файлов тестов (оценка) |
|---|---|---|
| S1 | `fenced-frame` | ~198 |
| S2 | `webrtc` | ~241 |
| S3 | `upgrade-insecure-requests`, `html-ruby-extensions` | ~206 + ~193 |
| S4 | `WebCryptoAPI` | ~181 (есть долгие тесты генерации ключей; если срез дольше часа — делить по подкаталогам) |
| S5 | `permissions-policy`, `encrypted-media` | ~115 + ~155 |
| S6 | `html/anonymous-iframe`, `html/capability-delegation`, `html/user-activation`, `html/cross-origin-embedder-policy`, `html/cross-origin-opener-policy` | ~21 + 6 + 19 + 77 + 114 |

Не брать в эти срезы:

- `IndexedDB` — флап, срез 34;
- `referrer-policy` 4K — многочасовой;
- повтор `mixed-content` — диффузный флап;
- `html/browsers`.

## Не трогать

- Движок.
- Исполнители wptrunner.
- `--root dom/nodes`: `run_report.py` его отвергает, это гейт `run_suite.py`.

## Гейт

Два `--check` с exit 0, как выше. Кода в срезах нет.
