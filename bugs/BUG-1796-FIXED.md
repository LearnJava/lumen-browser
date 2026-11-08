# BUG-1796 — тик `<video>` из фрейма решает nid-домен по верхнему документу

**Статус:** FIXED 2026-11-08 (P3)
**Заведён:** 2026-11-08 (попутная находка при починке BUG-1720, main b15459285, Windows 10, dev-release)
**Область:** shell (`crates/shell/src/video_tick.rs`), js (`crates/js/src/video_gif_store.rs`, `crates/js/src/video_bindings.rs`, `crates/js/src/v8_runtime/runtime.rs`)

## Симптом (латентный)

`tick_video_sessions` обрабатывает nid видео из под-документа фрейма как будто это nid узла ВЕРХНЕГО документа, в трёх местах:

1. **База URL** (`video_tick.rs:326`): `src` резолвится против `self.source` (страница верхнего уровня) и `effective_base` ДОКУМЕНТА ВЕРХНЕГО уровня — относительный `src="rain.mp4"` внутри документа фрейма обязан резолвиться от базы документа ФРЕЙМА (HTML LS §4.8.4 «media element base URL» — base URL самого документа элемента).
2. **`has_poster`** (`video_tick.rs:417`): атрибут `poster` читается у узла верхнего документа с тем же числовым nid — совпадение по индексу, не по документу; несуществующий или чужой `poster` держит кадр withheld / пропускает регистрацию.
3. **`apply_intrinsic_size`** (`video_tick.rs:436`): intrinsic-размеры пишутся на узел ВЕРХНЕГО документа с тем же nid + `note_shell_attr_writes(vec![nid])` — может мутировать чужой узел верхнего документа (у него свои layout-последствия).

На w3schools `howto_css_fullscreen_video.asp` (BUG-1720) ничего не проявилось: `src` фрейма абсолютный, узел верхнего документа с совпавшим nid оказался без `poster`/влияющих атрибутов. Путь GIF-тиков (`video_tick.rs:179`) — та же верхне-документная база, подтверждён при расследовании.

## Диагностика

- Локальный репро: страница `<iframe src="pages/child.html">`, фрейм в подкаталоге, в нём `<video src="media/rain.mp4">` и `<video src="media/anim.gif">` с ОТНОСИТЕЛЬНЫМИ путями (от базы фрейма это `/pages/media/*`, от базы страницы — `/media/*`, где такого файла нет).
- Живая проверка до фикса: HTTP-сервер в логе показывает `GET /media/rain.mp4 404`, stderr — `video: пропуск media/rain.mp4: network error: HTTP 404` для обоих путей (контейнер и GIF): nid-домен решался по `self.source`/`layout_source` верхнего уровня.
- Третий `<video>` фрейма без авторских width/height — путь intrinsic-записи: до фикса размеры уезжали в узел верхнего документа (тихая порча, без видимого эффекта на репро).
- Причина несоответствия: очередь загрузок магазина (`VideoGifStore`) ключуется голым nid, а nid уникален только ВНУТРИ своего документа — у страницы и каждого фрейма своя арена `NodeId`. Сам запрос не нёс информации, из чьего документа ушёл.

## Исправление

- **`crates/js/src/video_gif_store.rs`** — очередь `pending_loads`/`pending_container_loads` хранит `VideoLoadRequest { nid, src, doc }`: рантайм, ставящий запрос, прикладывает СВОЙ документ (`Option<Arc<Mutex<Document>>>`).
- **`crates/js/src/v8_runtime/runtime.rs`** — `own_doc_slots()` отдаёт `FrameDocRegistry` изолята; нативы `__lumen_video_load`/`__lumen_video_container_load` читают `FrameDocSlots::self_doc` (заполняется в `install_dom` ДО установки видео-нативов) и кладут его в запрос.
- **`crates/shell/src/video_tick.rs`** — новый `video_load_domain(&VideoLoadRequest) -> Option<VideoNidDomain>`: документ запроса сопоставляется `Arc::ptr_eq` с живым `FrameHandle.doc` — совпал: база от `FrameHandle::base` + `effective_base` документа ФРЕЙМА, nid-чтения (`poster`, referrer, CSP upgrade) и intrinsic-запись идут в документ фрейма; иначе — верхний путь как до BUG-1796 (включая `doc: None` от минимального изолята без `install_dom`). Оба тика (`tick_video_gifs`, `tick_video_sessions`) проходят очередь через домен: `has_poster` читает `domain.doc`, относительный `src` резолвится от `domain.base`.
- Intrinsic-запись в под-документ не проходит через каскадный флаш страницы (`note_shell_attr_writes` — только верхний уровень), поэтому фрейму нужен явный релейаут: после успешной intrinsic-записи индекс фрейма копится в `frames_relaid`, одним проходом после цикла загрузок `refresh_frames_with_intrinsic_writes` вызывает `refresh_frames(Some(idx))` по разу на фрейм — тот же вход, что и при нативном переключении контрола формы (BUG-480 срез 18).
- Устаревшие/чужие документы (навигация фрейма, `apply_loaded_page` чистит обе очереди) обрабатываются по прежнему верхнему пути — регрессионного окна нет.

## Ограничения

- **MSE-путь** (`crates/shell/src/mse_sessions.rs`, `tick_mse_sessions`): та же латентная верхне-документная база для `mse_attached` nid — не тронуто, отдельный дефект при живом MSE-репро из фрейма.
- **Ключ рисования `video:{nid}`** (`crates/paint` → `DisplayCommand::DrawImage`): голый nid может коллидировать между документами (видео фрейма nid=5 и видео страницы nid=5) — коллизия ключа текстуры, требует переклейки ключей в paint+композиторе, вне среза.

## Проверка

- `cargo clippy -p lumen-shell --all-targets -- -D warnings` — чисто; `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` — чисто.
- `cargo test -p lumen-js --features v8-backend --lib -- video` — 92 passed (обновлённые тесты нативов: запрос несёт `doc`); `cargo test -p lumen-shell --bin lumen -- video_tick` — 3 passed.
- Живой репро (`LUMEN_NO_ADBLOCK=1`, `--maximized`, локальный HTTP): до фикса `GET /media/rain.mp4` → 404 + `video: пропуск media/rain.mp4: network error: HTTP 404`; после фикса `GET /pages/media/rain.mp4` → 200 + `video: загружен nid=12 (1920×1080, 640мс)` и `video GIF: загружен nid=15`; третий видео без авторских размеров загрузился без паники — intrinsic-запись в под-документ и релейаут фрейма отработали.
