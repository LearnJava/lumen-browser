# WPT-RUN-14 — срезы S1…S28: прогон `css` по модулям

Родительская задача — WPT-RUN-14 (ROADMAP:970). Владелец — P2. Цель родителя: у всей категории `css` есть вердикт, топ-10 кластеров разнесены по очередям P1/P4.

Этот файл описывает первую волну из десяти модулей (S1…S10), пять срезов CSS2 (S11…S15), `css-overflow` + `css-sizing` (S16) и раскладку остальных модулей `css` (S17…S28, заведена 2026-10-07 при закрытии S16: 33 496 id `css` минус 18 000+ уже разобранных). Агрегирующий срез (сводка «кластер → id → владелец» по всему `css`) заводится после S28.

## Почему через `run_corpus.py`, а не `run_report.py`

`run_report.py` исполняет только `testharness` и `test262` (`RUNNABLE_ITEM_TYPES`, `run_report.py:68`). Reftest'ы — 64 % балла, и это главная ценность прогона. Их исполняет только `run_corpus.py` (wptrunner с `LumenRefTestExecutor`).

Но `run_corpus.py --categories` принимает только верхний уровень (`css` целиком, ~11 ч), потому что категория — это `path[0]` в `corpus_stats.iter_entries`.

## Предусловие (входит в S1)

Флаги `run_corpus.py --prefixes css/css-flexbox[,…]` и `--exclude-prefixes …`:

- id из манифеста фильтруются по префиксу пути (и по исключающему префиксу) **до** `plan_shards` (`run_corpus.py:183`);
- шарды режутся тем же `_split` по `SHARD_THRESHOLD`;
- ветка в `_selftest` (`:998`) проверяет фильтр на ручном манифесте;
- около 40 строк.

Без этого флага S2…S10 невозможны.

## Метод одного среза

1. Свежая `dev-release`-сборка. `$BIN` — по `tests/wpt/README.md`. Окно не нужно, сеть локальная.
2. Прогон (около 30 мин на 1 500 id; S8/S10 короче):
   ```
   tests/wpt/.venv/Scripts/python.exe tests/wpt/run_corpus.py --binary "$BIN" \
       --prefixes <префиксы из таблицы> --out-dir .tmp/wpt-run14/<slug> --resume
   ```
   Каталог вывода **отдельный** от `.tmp/wpt-corpus` (WPT-RUN-9).
3. Учёт: `score_audit.py --out-dir .tmp/wpt-run14/<slug>` (утечек нет) и `type_audit.py --out-dir …` (разбивка по типам).
4. Кластеры: FAIL/TIMEOUT/CRASH группируются по первопричине по `docs/probe-method.md` §8. Причина устанавливается пробой, не по имени файла.
   - Дефект — BUG-NNN (номер брать по `origin/main`).
   - Нереализованная функциональность — строка ROADMAP.
   - Свойство не разобрано — `CSS-SPECS.md` для P4.
5. Итог — раздел модуля в `docs/wpt-vendor-notes/css.md`: таблица «кластер → число id → пример id → владелец (P1/P4/P3) → BUG/ROADMAP/CSS-SPECS».

## Готово, когда (для каждого среза)

- У каждого id модуля есть вердикт: в `score_audit` нет «lost inside a completed shard».
- Таблица кластеров в `css.md`.
- Для кластеров крупнее 20 id заведены записи с указателями в очереди.

## Срезы

| Срез | Модуль | Префиксы `--prefixes` | id (файлов) | Каталог вывода |
|---|---|---|---|---|
| S1 | css-flexbox (+ флаг `--prefixes`) | `css/css-flexbox` | ~1432 | `.tmp/wpt-run14/flexbox` |
| S2 | css-writing-modes | `css/css-writing-modes` | ~1383 | `.tmp/wpt-run14/writing-modes` |
| S3 | css-ui | `css/css-ui` | ~1356 | `.tmp/wpt-run14/ui` |
| S4 | css-break | `css/css-break` | ~1178 | `.tmp/wpt-run14/break` |
| S5 | css-transforms | `css/css-transforms` | ~940 | `.tmp/wpt-run14/transforms` |
| S6 | css-backgrounds | `css/css-backgrounds` | ~955 | `.tmp/wpt-run14/backgrounds` |
| S7 | css-grid, часть 1 | `--prefixes css/css-grid --exclude-prefixes <все префиксы S8>`: abspos, alignment, animation, grid-definition, grid-items и 72 файла в корне | ~1000 | `.tmp/wpt-run14/grid-1` |
| S8 | css-grid, часть 2 | `css/css-grid/grid-lanes,css/css-grid/grid-model,css/css-grid/implicit-grids,css/css-grid/layout-algorithm,css/css-grid/parsing,css/css-grid/placement,css/css-grid/subgrid,css/css-grid/test-plan` | ~1220 | `.tmp/wpt-run14/grid-2` |
| S9 | css-text, часть 1 | `--prefixes css/css-text --exclude-prefixes <все префиксы S10>`: подкаталоги от `animations` до `text-align` по алфавиту и 9 файлов в корне | ~1040 | `.tmp/wpt-run14/text-1` |
| S10 | css-text, часть 2 | `css/css-text/<d>` для каждого подкаталога от `text-autospace` до `writing-system` по алфавиту (`text-transform`, `white-space`, `word-break` и др.) | ~870 | `.tmp/wpt-run14/text-2` |
| S11 | CSS2: backgrounds + borders | `css/CSS2/backgrounds,css/CSS2/borders` | ~843 | `.tmp/wpt-run14/css2-1` |
| S12 | CSS2: normal-flow + margin-padding-clear | `css/CSS2/normal-flow,css/CSS2/margin-padding-clear` | ~1500 | `.tmp/wpt-run14/css2-2` |
| S13 | CSS2: tables, positioning, floats | `css/CSS2/tables,css/CSS2/positioning,css/CSS2/floats,css/CSS2/floats-clear,css/CSS2/abspos,css/CSS2/stacking-context,css/CSS2/zindex,css/CSS2/zorder` | ~1340 | `.tmp/wpt-run14/css2-3` |
| S14 | CSS2: text, linebox, fonts, lists | `css/CSS2/text,css/CSS2/linebox,css/CSS2/fonts,css/CSS2/generated-content,css/CSS2/lists,css/CSS2/bidi-text` | ~1260 | `.tmp/wpt-run14/css2-4` |
| S15 | CSS2: остальное | `css/CSS2 --exclude-prefixes <все префиксы S11…S14>`: box, box-display, cascade, cascade-import, colors, css1, css21-errata, csswg-issues, media, other-formats, pagination, sec5, selectors, syntax, ui, values, visudet, visufx, visuren и файлы в корне | ~1420 | `.tmp/wpt-run14/css2-5` |
| S16 | css-overflow + css-sizing | `css/css-overflow,css/css-sizing` | 1 498 | `.tmp/wpt-run14/overflow-sizing` |
| S17 | css-multicol + css-contain | `css/css-multicol,css/css-contain` | 1 297 | `.tmp/wpt-run14/multicol-contain` |
| S18 | css-fonts + css-masking + WOFF2 | `css/css-fonts,css/css-masking,css/WOFF2` | 1 392 | `.tmp/wpt-run14/fonts-masking` |
| S19 | css-images + css-values + css-color | `css/css-images,css/css-values,css/css-color` | 1 371 | `.tmp/wpt-run14/images-values-color` |
| S20 | selectors + css-pseudo + css-nesting + css-namespaces + css-cascade | `css/selectors,css/css-pseudo,css/css-nesting,css/css-namespaces,css/css-cascade` | 1 371 | `.tmp/wpt-run14/selectors-pseudo` |
| S21 | css-anchor-position + css-position + css-display | `css/css-anchor-position,css/css-position,css/css-display` | 1 126 | `.tmp/wpt-run14/anchor-position-display` |
| S22 | css-view-transitions + css-conditional + css-variables + css-properties-values-api + css-mixins | `css/css-view-transitions,css/css-conditional,css/css-variables,css/css-properties-values-api,css/css-mixins` | 1 350 | `.tmp/wpt-run14/view-transitions-variables` |
| S23 | css-text-decor + css-gaps + css-shapes | `css/css-text-decor,css/css-gaps,css/css-shapes` | ~1 143 | `.tmp/wpt-run14/text-decor-gaps-shapes` |
| S24 | filter-effects + css-inline + css-tables + css-align | `css/filter-effects,css/css-inline,css/css-tables,css/css-align` | ~1 368 | `.tmp/wpt-run14/filter-inline-tables-align` |
| S25 | css-typed-om + cssom-view + cssom + css-lists + css-counter-styles | `css/css-typed-om,css/cssom-view,css/cssom,css/css-lists,css/css-counter-styles` | ~1 282 | `.tmp/wpt-run14/typed-om-cssom-lists` |
| S26 | css-page + css-animations + css-transitions + css-shadow + css-borders + css-scroll-snap | `css/css-page,css/css-animations,css/css-transitions,css/css-shadow,css/css-borders,css/css-scroll-snap` | ~1 194 | `.tmp/wpt-run14/page-animations-borders` |
| S27 | css-ruby + css-layout-api + css-box + motion + css-highlight-api + css-paint-api + css-viewport + mediaqueries | `css/css-ruby,css/css-layout-api,css/css-box,css/motion,css/css-highlight-api,css/css-paint-api,css/css-viewport,css/mediaqueries` | ~1 059 | `.tmp/wpt-run14/ruby-box-motion-mq` |
| S28 | остальные 30 малых модулей | `--prefixes` по каждому: css-logical, compositing, css-scroll-anchoring, css-content, css-scrollbars, css-will-change, css-syntax, css-color-adjust, css-rhythm, geometry, css-font-loading, printing, css-forms, css-image-animation, css-style-attr, css-easing, css-exclusions, css-overscroll-behavior, fill-stroke, css-env, css-device-adapt, css-size-adjust, css-link-params, css-forced-color-adjust, css-color-hdr, reference, css-zoom, css-motion-path, fetching, css-parser-api | ~794 | `.tmp/wpt-run14/small-modules` |

Числа S17…S28 — automatable id по манифесту (без `manual`/`visual`), посчитаны 2026-10-07; числа S1…S10 — файлы тестов на диске, без `support/`, `reference/` и `-ref.*`. Точные id даёт манифест. Если срез заметно длиннее часа, разделить его на два и записать это в ROADMAP-строку среза.

## Не трогать

- Движок.
- `tests/wpt/metadata/**/*.ini`: baseline — это WPT-RUN-7, а не этот срез.
- `.tmp/wpt-corpus`.

## Гейт (S1, код)

```
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_corpus.py --selftest
```

У S2…S15 кода нет; гейт — п. «Готово, когда».
