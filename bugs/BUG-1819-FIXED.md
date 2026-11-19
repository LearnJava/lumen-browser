# BUG-1819 — `object-fit` не хранит `[ contain | cover ] || scale-down` (CSS Images L4): computed-style теряет значение

**Статус:** FIXED 2026-11-19 (P6)
**Заведён:** 2026-11-16 (P3, срез 9 [BUG-1408](BUG-1408-FIXED.md))
**Область:** layout (`crates/engine/layout/src/style/values/background.rs::ObjectFit`, `style/apply/layout.rs`,
`selector_query.rs::computed_style_to_map`), paint (`crates/engine/paint/src/display_list/geometry.rs`)

## Симптом

`el.style.objectFit = "cover scale-down"` (inline-CSSOM принимает и канонизирует значение с BUG-1408 срез 9),
но `getComputedStyle(el).objectFit` отдаёт `fill` — начальное значение. Ожидается `cover scale-down`
(CSS Images L4 §5.5). WPT `css/css-images/parsing/object-fit-computed.html`: 1 красный сабтест
(`'cover scale-down'`), остальные 5 зелёные.

## Причина

Каскадный парсер (`ObjectFit::parse`) знал только одиночные ключи
(`fill`/`contain`/`cover`/`none`/`scale-down`), а `ComputedStyle.object_fit` — enum без пары значений;
сериализатор computed-style (`computed_style_to_map`) тоже одиночный. Отсюда падение в `fill`.

## Что сделано (2026-11-19, P6)

- **Хранение:** новый вариант `ObjectFit::CoverScaleDown` — канонизатор inline-CSSOM (BUG-1408 срез 19)
  уже сворачивал `contain scale-down` в синоним `scale-down`, поэтому пара из спецификации требует
  ровно одного нового значения (`cover scale-down`); отдельное поле-флаг не понадобилось.
- **Парсер** (`ObjectFit::parse`): `[contain|cover] scale-down` в любом порядке (`||` — порядок свободный);
  `contain scale-down` → `ScaleDown`; дубликаты ключа и пара `contain cover` отвергаются как невалидные.
- **Сериализация computed:** `computed_style_to_map` отдаёт `cover scale-down`; в paint `object_fit_name`
  (строка попадает в GPU-ключ текстуры) различает вариант от `cover` и `scale-down`.
- **Геометрия** (`fit_image_rect`): `CoverScaleDown` = `min(none, cover)` (§5.5: `scale-down` при паре
  заменяет базу сравнения) через вынесенный хелпер `scale_down_or`; математика существующего
  `ScaleDown` (`min(none, contain)`) не изменилась.

## Проверка

- WPT `css/css-images/parsing/object-fit-computed.html`: **6/6** сабтестов (было 5/6); ожидание `.ini`
  удалено. Прогон: `run_corpus.py --prefixes css/css-images/parsing/object-fit-computed.html`,
  `LUMEN_NO_SYSTEM_PROXY=1` (обход BUG-1828).
- Юнит-тесты: парсинг пар (в обоих порядках) и невалидных значений (`style/tests/box_model.rs`),
  computed-map (`selector_query/tests.rs`), геометрия `min(none, cover)` на обоих ветках
  (`paint/display_list/tests/background_and_layers.rs`).
- Гейт: `cargo clippy -p lumen-layout/-p lumen-paint --all-targets -D warnings` чистый;
  граф-гейт `run.py --continue-on-fail` — 9 FAIL, идентичных базе (прогоны 2026-11-18 на чужих
  коммитах `df0212c41`/`efa9299fc`: тот же набор 21/56/61/62/150/151/155/159/173) — регрессий нет.
