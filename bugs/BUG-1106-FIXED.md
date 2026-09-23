# BUG-1106 — `dump_golden.py` красный на свежепересобранном `main` (8/12), несмотря на «зелёный» BUG-1008 в тот же день

**Статус:** FIXED 2026-09-23 (P1)
**Тип:** дефект гейта (пиксель-нейтральность не гарантирована) или регрессия каскада/`ShareCache`.
**Заведён:** 2026-09-23 (P6, попутно при закрытии BUG-1101).
**Область:** не локализовано (кандидат — `crates/engine/layout/src/style/cascade.rs`/`share_cache.rs`, трек `THREAD-4`, но не подтверждено — см. ниже).

## Симптом

`LUMEN_PROFILE=dev-release python graphic_tests/dump_golden.py` на **свежей
пересборке чистого `main`** (`b1643eefa`, ветка `p6-bug-1101` до слияния
своих же правок) даёт **8 несовпадений из 12**:

```
samples/page.html [--dump-layout]: FAIL
  Block[0]/Block[1]: bg исчез (было #fafafaff)
  Block[0]/Block[1]/Block[2]: bg добавлен: #fafafaff
samples/page.html [--dump-display-list]: FAIL
samples/test-06-layout.html [--dump-layout]: FAIL   (тот же паттерн)
graphic_tests/25-table-layout.html [--dump-layout]: FAIL
graphic_tests/35-grid-named-areas.html [--dump-layout]: FAIL
graphic_tests/65-flex-align-content.html [--dump-layout]: FAIL
graphic_tests/65-flex-align-content.html [--dump-display-list]: FAIL
graphic_tests/106-transform-zindex.html [--dump-layout]: FAIL
```

Паттерн однородный по всем layout-несовпадениям: фон исчезает с одного
`Block` и появляется на его же прямом ребёнке (`Block[1]` → `Block[1]/
Block[2]`) — похоже на утечку/подмену кэшированного вычисленного стиля
между узлами, а не на геометрический сдвиг.

## Почему это неожиданно

[BUG-1008](BUG-1008-FIXED.md) закрыт **в тот же день**, чуть раньше:
регенерировал оба сталых golden-набора (`snapshot_cpu` PNG и
`dump_golden.py`) и явно подтвердил «`dump_golden.py` — 'Все 12 дампов
совпадают с эталоном'» на коммите `191eb6a070`. Старый бинарь в корневом
чекауте (`target/dev-release/lumen.exe`, собранный примерно в это время)
до сих пор проходит все 12 — драйфует именно **новая** сборка того же
исходника (`git diff` между веткой и `main` в момент замера — пусто).

## Что проверено

- Воспроизведено трижды подряд, в двух разных local-чекаутах (корневой
  `main` и worktree `p6-bug-1101`, `git diff` = пусто), при полном `cargo
  clean` затронутых крейтов (`lumen-js`, `lumen-shell`, `lumen-layout`) и
  при чистой пересборке `target/dev-release` с нуля (`rm -rf`) — не кэш
  sccache/incremental, воспроизводимо и без обёртки (`RUSTC_WRAPPER=""`).
- Не связано с локальным `last_session.db` (удалён, эффекта нет).
- Попытка отбисектить на `THREAD-4 срез 4` (`0869c13ab` — «ShareCache:
  контекстная дисквалификация... + fix ложного шаринга по ключу», трогает
  ровно `cascade.rs`+`share_cache.rs`, и в своём же коммите заявляет
  «dump_golden.py — все 12 дампов совпадают») — откат этих двух файлов к
  состоянию родителя (`e1fadc45f`) **не убрал** дефект, так что либо это не
  тот коммит, либо реконструкция через `git checkout <rev> -- <2 файла>`
  недостаточна (не пересобирает какое-то состояние, зависящее от других
  файлов).
- Диапазон коммитов между `191eb6a070` (BUG-1008, гейт зелёный) и текущим
  `main` включает `THREAD-4 срез 4`/`срез 5` (ShareCache) и BUG-925 (медиа
  `loading=lazy`, не трогает layout/cascade вовсе — маловероятный кандидат).
- В момент находки `git stash list` уже показывал активную сессию P1 на
  ветке `p1-thread4-srez6-matchopt` (tag `thread4-srez6-ab-baseline`) —
  похоже, трек `THREAD-4` уже занимается тем же классом проблемы
  («ложное разделение кэша стилей»).

## Что нужно для локализации

`git bisect` между `191eb6a070` и текущим `main` по критерию «`dump_golden.py`
даёт 12/12» — быстрее, чем гадать по стату коммитов. Если бисекция упрётся
в `THREAD-4`, вероятно нужен полный ревёрт/переразбор того среза, а не
точечный.

## Решение (P1, 2026-09-23, ветка `p1-bug1106-dump-golden`)

**Корень — не `ShareCache` и не утечка стиля между узлами.** Дрейф внёс
`b3d109c94` ([BUG-1103](BUG-1103-FIXED.md)): пропагация фона `<body>` на
канву (CSS Backgrounds L3 §2.11.2) перестала мутировать `ComputedStyle` —
раньше фон *переезжал* со стиля `body` на стиль `html`, теперь оба сохраняют
свои авторские значения, а цвет канвы read-only вычисляет
`lumen_layout::canvas_background_color`. Отсюда весь «однородный паттерн»:
`bg` у `Block[0]/Block[1]` (html) исчез, у `Block[0]/Block[1]/Block[2]`
(body) появился; в display list `FillRect` бокса html
`(0,0,1024,415.19)` сменился `FillRect` бокса body `(8,8,1008,399.19)`.
BUG-1103 гейтил себя `run.py --ipc`, а `dump_golden.py` не гонял — эталоны
остались от старой семантики. Бисекция по `THREAD-4` не могла сработать:
тот срез ни при чём.

**Под дрейфом эталонов нашёлся реальный дефект CPU-пути.** Цвет канвы
после BUG-1103 живёт только в `canvas_background_color`, а его читал лишь
wgpu-кадр (`set_canvas_background`). CPU-растеризатор
(`cpu_raster.rs::rasterize_cpu_with_fonts`) всегда чистил поверхность в
белый, так что `--screenshot`, IPC `Screenshot` (`render_source_to_png`),
automation `render_current_page_to_png` и driver `screenshot_cpu_rgba`
потеряли пропагацию: у `<body style="background:red">` поля 8px и всё ниже
контента выходили белыми. Проба `--screenshot --viewport 200x100`:
пиксель (2,2) — `(255,255,255)` до фикса, `(255,0,0)` после.

**Фикс:** `rasterize_cpu_with_fonts` / `Renderer::render_to_image_cpu_with_fonts`
получили параметр `canvas_background: Option<Color>` (`None` — UA-белый), все
три шелл/driver-вызова передают `canvas_background_color` своего layout-корня;
`CpuBackend` реализует `set_canvas_background`, как wgpu-бэкенд (юнит-тест
`cpu_backend_clears_to_canvas_background`). Эталоны `dump_golden.py`
перегенерированы под семантику BUG-1103 (8 файлов, в диффе только перенос
`bg` html→body и смена `FillRect` html→body).

**Гейты:** `dump_golden.py` — 12/12; `snapshot_cpu` — ok; `cargo test -p
lumen-paint` — ok; `run.py --ipc --continue-on-fail` — дельта против
предыдущего прогона «Изменений нет» (те же 20 FAIL, что до правки).
