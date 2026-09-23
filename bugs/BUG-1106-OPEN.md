# BUG-1106 — `dump_golden.py` красный на свежепересобранном `main` (8/12), несмотря на «зелёный» BUG-1008 в тот же день

**Статус:** OPEN
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
