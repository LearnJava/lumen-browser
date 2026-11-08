# BUG-1720 — `<video>` внутри `<iframe>` не рисуется (область фрейма — пустая серая коробка)

**Статус:** FIXED 2026-11-08 (P3)
**Заведён:** 2026-10-26 (по живой проверке, main 677ee5487, Windows 10, dev-release)
**Область:** shell (`crates/shell/src/frames.rs`, `crates/shell/src/app/window_event/redraw_requested.rs`)

## Симптом

`LUMEN_NO_ADBLOCK=1 target/dev-release/lumen.exe --maximized https://www.w3schools.com/howto/howto_css_fullscreen_video.asp`. На странице `<iframe src="tryhow_css_fullscreen_video.htm" style="border:11px solid #f1f1f1;border-radius:5px;width:100%;height:700px">`. Документ фрейма содержит `<video autoplay muted loop id="myVideo"><source src="rain.mp4" type="video/mp4"></video>` (`#myVideo{position:fixed;…}`) и фиксированную текстовую полосу `.content`. В логе видео ЗАГРУЖЕНО и декодировано внутри фрейма (`[media] видео H264: Media Foundation…`, `video: загружен nid=13`), но область iframe рисуется пустой светло-серой коробкой: не видно ни видео, ни `.content` фрейма.

## Диагностика

- Гипотезы из постановки опровергнуты трассировкой (`LUMEN_BUG1720_TRACE`, снята в коммите): текстура кадра регистрируется в рендерер и рисуется им (`hit=true`); содержимое фрейма попадает в display list СТРАНИЦЫ — `splice_frame_content` в `set_display_list` вклеивает его вместо заглушки.
- Настоящая причина — compositor-offload анимаций. Пока на странице активна хоть одна CSS-анимация (на w3schools крутится спиннер), каждый анимационный кадр пересобирает `anim_dl` из `layout_box` (`build_display_list_ordered_with_anim_split`) МИМО записи `Lumen::display_list` — единственного места вклейки содержимого фреймов. Выбор базы рендера (`anim_dl` → `page_buf` → `display_list`) на такой странице почти всегда берёт несвлейсенный `anim_dl`, и пользователь видит серую заглушку, хотя редкие кадры без анимаций видео рисуют.
- Локальный репро (iframe с `<video>`, но без CSS-анимации) дефект не воспроизводит — оттого баг выглядел «нестабильным».
- Методическая ловушка: `resource://screenshot` растеризует `self.display_list` на CPU и не видит ни GPU-текстур `video:` (`CpuBackend::register_image` — no-op), ни пути `anim_dl` — CPU-снимки «опровергали» дефект. Живая правда — только снимок окна через PrintWindow с `PW_RENDERFULLCONTENT` (CopyFromScreen на заблокированной машине снимает экран блокировки).

## Исправление

- **`crates/shell/src/frames.rs`** — `splice_one_frame` возвращает `Option<(usize, usize)>` — (индекс заглушки, число вставленных команд). Новая `splice_frame_content_anim(dl, frames, ranges)`: та же вклейка для списка anim-split'а с правкой диапазонов анимируемых сегментов. Вклейка удаляет заглушку и вставляет `wrapped.len()` команд — чистый прирост длины `inserted − 1`; сегмент, НАКРЫВАВШИЙ заглушку (анимированный предок фрейма), удлиняется на прирост, чтобы его переопределение (opacity/transform) доезжало и до содержимого ребёнка; сегменты после вставки сдвигаются на прирост; до — не трогаются.
- **`crates/shell/src/app/window_event/redraw_requested.rs`** — сразу после `build_display_list_ordered_with_anim_split` вызывается `splice_frame_content_anim(&mut dl, &self.frames, &mut ranges)`; позднейшие append-ы (cue, squiggles) идут в конец списка и диапазоны не сдвигают, поэтому правка приствязана к моменту сборки.

## Ограничения

- Латентные дефекты nid-домена тика видео во фрейме (база URL, `poster`, intrinsic-размеры читаются из ВЕРХНЕГО документа) вынесены в BUG-1796 — на w3schools не проявлялись, отдельный воспроизводимый случай.
- `tryit.asp?filename=…` (пустой iframe результата + `[JS error] Element with ID "iframecontainer" not found`) — скрипт страницы ищет элемент в родительском документе; вне этой задачи, наблюдение осталось в силе.

## Проверка

- `cargo test -p lumen-shell --bin lumen frames` — 102/102, включая новый `splice_frame_content_anim_shifts_ranges_around_insertion` (сегмент до/после вставки и накрывавший заглушку; поймал off-by-one первой версии: сдвиг на полный размер вставки вместо чистого прироста).
- Clippy `-p lumen-shell --all-targets -- -D warnings` — зелёный. `-p lumen-paint` задачей не задет (diff пуст); его lib-clippy без фичи `backend-cpu` красный вне задачи (`rasterize_cpu*` мертвы без cpu_backend) — CI исключает paint из clippy.
- Живая проверка w3schools: PrintWindow-снимки живого окна до/после скролла — видео играет внутри фрейма (кадры меняются между снимками), серой коробки нет; локальный репро — без регрессии.
- Graphic-гейт: машина заблокирована, TEST-00 калибровка gdigrab-плана невалидна — полный прогон выполнен в режиме `--ipc` (CPU-снимок всех 172 фикстур против Edge-базлайна, `results/latest.json` в коммите). Новый провалов НЕТ: 21/61/150/151/155/159 — FAIL уже в базлайне с теми же числами до сотых (1.54/10.78/7.49/6.06/1.32/2.51%); 17 FAIL (19, 45, 52, 53, 56, 59, 62, 78, 79, 80, 97, 100, 109, 117, 130, 147, 166) — члены `GPU_CAPTURE`, которых план по документации раннера обязан снимать gdigrab-ом («baseline, снятый одним способом, к другому неприменим»), под принудительным IPC они красные по построению; TEST-18 (6.09% против должника 2.08%) экзонерирован бинарником ТОЧКИ ВЕТВЛЕНИЯ (`b15459285` без единой строки коммита): в одиночном `--only 18` прогоне те же 6.09% — дрейф возник до задачи. 65/165 из базлайна в этом прогоне PASS (там — артефакты блокировки). Путь `anim_dl` CPU-снимками не покрывается (ловушка выше), живая часть проверена PrintWindow-снимками; gdigrab-подмножество перевалидировать на разблокированной машине: `LUMEN_PROFILE=dev-release python graphic_tests/run.py --continue-on-fail`.
