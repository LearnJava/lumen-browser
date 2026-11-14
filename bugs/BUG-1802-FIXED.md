# BUG-1802 — ebay.com: 65–69 кадров за прогон стоят 0.5–1.9 с в статье `sbar` (перестройка scrollbar-overlay), хотя внутри интервала только чистые операции на кэшированных данных

**Статус:** FIXED 2026-11-14 (P3)
**Открыт:** 2026-11-12 (P3, перф-аудит ebay; до исправления — точечная бисекция)
**Компонент:** shell (`crates/shell/src/app/window_event/redraw_requested.rs` — интервал `bmarks[0]..bmarks[1]`; `crates/shell/src/lumen/media_controls.rs`)

## Симптом

`https://www.ebay.com/`, dev-release, живое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, `LUMEN_FRAME_LOG=1` (3 compat-прогона 2026-11-12):

- кадров >500 мс за прогон: **68 / 65 / 69**, max **1748/1706/1907 мс**, при p50 кадра 0.73–7.8 мс;
- разбиение тяжёлого кадра всегда одно: `build: chrome 0.70 sbar 1736.94 panels 6.15` — вся стоимость в интервале `sbar`;
- часто `delta ContentChanged`; кластеризация по времени — последняя треть сессии, синхронно со штормом задач движкового потока (BUG-1800).

Пользователь видит это как фризы интерфейса по 0.5–1.9 с при живой странице; порог «Не отвечает» (5 с) не достигался — 0 с за все прогоны.

## Причина (подтверждена бисекцией)

Внутри интервала `sbar` единственным блокирующим вызовом был `ls.document.lock()` в
`build_media_controls_overlay` (`media_controls.rs:438`): сборка UA-панели медиа на UI-потоке
захватывала мьютекс DOM-документа **каждый кадр**, а движковый поток держит его секундами
во время JS-хода (BUG-935 S79/S86, шторм задач BUG-1800). Гипотеза «а» карточки (скрытая
блокировка), уточнение: не render-мьютекс wgpu, а document-мьютекс страницы.

**Локализация** (временные `Instant`-таймеры, сняты после фикса):

- auto-ru--inner: `media doc-lock wait 605.2–658.4 мс` — вся стоимость интервала
  (точки до media — ~4 мс от начала кадра), в соседних строках того же кадра
  `[engine] maybe_flush done 675.1ms` — движковый поток в hold-е замка;
- pikabu-ru: `doc-lock wait 228.7–354.0 мс`;
- ebay 2026-11-12 (числа карточки): 0.5–1.9 с.

## Фикс (P3, 2026-11-14) — `controls` у `<video>` едет в `BoxKind`, DOM на UI-потоке не читается

Из документа в интервале читалось ровно одно — `get_attr("controls")` для `BoxKind::Video`
(у `<audio>` флаг уже лежит в kind с BUG-1719). Флаг переехал в `BoxKind::Video { controls }`
(`lumen-layout`), читается при построении бокса под уже взятым движком замком;
`paint_eq`/`incremental` сравнивают поле (как у Audio) — динамический `el.controls = …`
доезжает до панели при инкрементальной перестройке бокса. `build_media_controls_overlay`
больше не трогает `layout_source`/DOM: `PanelCtx` без `doc`, walk читает флаг из kind.

**Гейты:** clippy `--workspace --all-targets -D warnings` чистый;
`cargo test -p lumen-layout video_captures_controls` (репро флага в kind);
scoped-test по замыканию lumen-layout+lumen-shell — зелёный;
`dump_golden.py` — diff пустой (display-list-нейтрально, печатная строка video-бокса
`--dump-layout` дополнена `controls=` по образцу Audio).

**Живая сверка** (пикабу/авто — те же свидетели, что у BUG-1800 срез 5; ebay в этот день
закрыт антиботом — SITE_REFUSED 403, как в BUG-1800 срез 5, числа ebay — до-фиксные с
карточки): кадров `sbar` >50 мс до/после — pikabu-ru 18/0 (max 404.6 → 0), pikabu-ru--inner
15/0 (358.4 → 0), auto-ru 11/0 (132.0 → 0), auto-ru--inner 19/0 (**935.0 → 0**).

**Осталось:** шторм движкового потока (BUG-1800) — сам hold замка при JS-ходе; здесь снят
только его эхо-эффект на UI-потоке. Попутная находка гейта: `lumen-driver`
`bug1191_boxless_computed_style` и `bug489_display_contents_computed_style` красные на
чистом main (`display` = "" вместо "inline"/"contents") — [BUG-1809](BUG-1809-OPEN.md);
`cpu_snapshots_match_references` красный на чистом main (49/44/45 по 1024 байта) — дополнено
наблюдение в [BUG-1236](BUG-1236-OPEN.md).
