# SPLIT-CP2 — css-parser: три файла сверх потолка 2000 строк

Дорожка SPLIT (BUG-1100). Владелец — P1. Крейт — `lumen-css-parser`. Метод и железные правила — [p1-monolith-split-queue.md](p1-monolith-split-queue.md) §2, §3, §5.

## Файлы (длины на 2026-10-02)

| Файл | Строк | Как резать |
|---|---|---|
| `crates/engine/css-parser/src/parser.rs` | 2021 | Вынести один самодостаточный блок, например `enum CssomOp` (949–1020) или `impl LayerState` (1095–1162), в подмодуль `parser/`. Реэкспорт сохраняет пути. |
| `crates/engine/css-parser/src/parser/at_rules.rs` | 2051 | Домен `@supports` перенести в новый `parser/supports.rs`, около 350 строк. Состав: `SupportsRule`, `SupportsCondition` + impl, `SUPPORTED_FONT_TECH`/`_FORMAT`, `parse_supports_*`, `match_func_arg`, а также методы `impl Parser` про supports. |
| `crates/engine/css-parser/src/parser/tests/at_rules.rs` | 2385 | Разделить по баннерам. Блок Media Queries (1253 — конец) уходит в новый `parser/tests/media.rs`, регистрация рядом с остальными тестовыми модулями. |

Тестовые модули регистрируются в `parser.rs` (`#[path = "parser/tests/…"]`, около `:1994-2020`). Регистрация `media.rs` добавит ~4 строки в `parser.rs`, поэтому из него нужно вынести не меньше 30 строк. `CssomOp` (72) этого хватает.

Точные границы снимать `python scripts/split_census.py <file>` (и `--inner` для тестового файла), а не на глаз.

## Правило

Чисто механический перенос, без правок логики:

- тела функций и тестов переезжают байт в байт;
- меняются только `use`, видимость (`pub(super)` / `pub(crate)`) и объявления `mod`;
- pub-API и serde-поверхность не меняются;
- число тестов не уменьшается (сравнить `cargo test -p lumen-css-parser` до и после).

## Не трогать

Другие файлы крейта и любые поведенческие правки.

## Готово, когда

- Все три файла ≤2000 строк.
- `python scripts/check_file_sizes.py 2>&1 | grep css-parser` пусто.
- Строка `parser/tests/at_rules.rs` удалена из `scripts/file-size-baseline.tsv` вручную. **Не** запускать `--update`: он перезапишет baseline и у чужих выросших файлов.
- Таблица §4 плана дополнена строкой CP-2 DONE.

## Гейт

```
cargo clippy -p lumen-css-parser --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
python scripts/check_file_sizes.py   # по крейту css-parser — пусто
```

## Зависимости

Нет.
