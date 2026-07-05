# Задача: Picture-in-Picture API

**Developer:** P1
**Ветка:** `p1-picture-in-picture`
**Размер:** S
**Крейты:** `lumen-js`, `lumen-shell`

## Goal

Закрыть остаточные разрывы Picture-in-Picture: связать Document PiP
(`documentPictureInPicture.requestWindow`) с реальным OS-окном (сейчас у него
только native-заглушка), унифицировать `PictureInPictureWindow` между video- и
document-PiP, отдавать корректные размеры окна из шелла обратно в JS
(`_lumen_pip_deliver_resize`).

## Current state (сверено с кодом 2026-07-05)

Video-PiP и OS-окно уже работают; главный разрыв — Document PiP не открывает
реальное окно. Это меньше, чем говорит семя ROADMAP.

- `crates/js/src/video_pip.rs:23` — `install_video_pip_api`: W3C PiP Level 1
  для `<video>` полностью в JS-шиме (`video_pip.rs:29`).
  `requestPictureInPicture`/`exitPictureInPicture`/`pictureInPictureElement`/
  `pictureInPictureEnabled`, события `enter`/`leavepictureinpicture`, зовёт
  `_lumen_pip_enter(nid)`/`_lumen_pip_exit(nid)` (`video_pip.rs:105/122`).
- `crates/js/src/pip_bindings.rs:67` — `install_pip_bindings`: нативы
  `_lumen_pip_enter`/`_exit` → очередь `PipRequest` (`pip_bindings.rs:24`),
  дренится шеллом.
- `crates/shell/src/main.rs:8719` — шелл дренит `take_pip_requests()`;
  `PipRequest::Enter` → `pip_controller.on_enter(nid)` (`main.rs:8723`).
- `crates/shell/src/panels/pip_os_window.rs:1` — **реальное** OS floating-окно
  (winit child window), poster letterbox (`pip_os_window.rs:104/205`),
  `PipOsController::on_enter` (`pip_os_window.rs:205`). Video-PiP OS-окно готово.
- `crates/js/src/document_pip.rs:8` — `install_document_pip_api`: W3C Document
  PiP. `requestWindow()` (`document_pip.rs:83`) создаёт JS
  `PictureInPictureWindow` с **фейковым** `document` (`document_pip.rs:41`:
  in-memory `body.children`, не настоящий DOM). Зовёт
  `_lumen_pip_request_window(width, height)` (`document_pip.rs:99`) — но
  **этот натив нигде не зарегистрирован** (grep по коду: только вызов, нет
  установки в `pip_bindings.rs`/`lib.rs`), т.е. `typeof !== 'function'` → no-op.
- **Разрывы:**
  1. Document PiP не открывает реальное окно (`_lumen_pip_request_window` не
     реализован; нет `PipRequest`-варианта для document-окна).
  2. Document PiP `window.document` — заглушка, реальный DOM-контент туда не
     переносится.
  3. Два разных класса `PictureInPictureWindow` (video-PiP `video_pip.rs:46`
     vs document-PiP `document_pip.rs:18`) — дублирование.
  4. `_lumen_pip_deliver_resize` (`video_pip.rs:173`) есть в JS, но проверить,
     зовёт ли шелл его при ресайзе OS-окна (`pip_os_window.rs`).

## Entry points

- `crates/js/src/document_pip.rs:83` — `requestWindow` (нативный вызов-заглушка).
- `crates/js/src/pip_bindings.rs:24` — `PipRequest` enum (расширить вариантом Document).
- `crates/js/src/pip_bindings.rs:67` — `install_pip_bindings` (регистрация нативов).
- `crates/shell/src/panels/pip_os_window.rs:205` — `on_enter` (образец для document-окна).
- `crates/shell/src/main.rs:8719` — drain-цикл `take_pip_requests`.
- `crates/js/src/video_pip.rs:46` / `document_pip.rs:18` — два `PictureInPictureWindow`.

## Срезы (декомпозиция)

### Срез 1 — XS — Зарегистрировать `_lumen_pip_request_window`
`document_pip.rs:99` зовёт несуществующий натив. Добавить его в
`pip_bindings.rs` (по образцу `_lumen_pip_enter`, `pip_bindings.rs:68`),
enqueue-ить новый `PipRequest::OpenDocument { width, height }`.

### Срез 2 — S — Вариант `PipRequest` для Document PiP + drain
Расширить enum `PipRequest` (`pip_bindings.rs:24`) вариантом Document-окна.
В `main.rs:8719` обработать его → `pip_controller` открывает floating-окно
(переиспользовать инфраструктуру `pip_os_window.rs`, но без `<video>`-poster —
пустое/содержимое-контейнер).

### Срез 3 — S — OS-окно для Document PiP
В `pip_os_window.rs` (или соседнем модуле) добавить путь открытия окна не под
`<video>`, а под произвольный размерный контейнер. Минимум — окно с фоном
(`pip_os_window.rs:100` background fill) нужного размера; отрисовка DOM-контента
контейнера — отдельный follow-up (пометить в DoD как ограничение).

### Срез 4 — XS — Унифицировать `PictureInPictureWindow`
Свести два класса (`video_pip.rs:46`, `document_pip.rs:18`) к одному общему в
одном шиме, чтобы `instanceof PictureInPictureWindow` был консистентен и
`_lumen_pip_deliver_resize` (`video_pip.rs:173`) обновлял оба сценария.

### Срез 5 — XS — Resize round-trip
Убедиться, что шелл при ресайзе floating-окна (winit `WindowEvent::Resized` для
pip-window id, `main.rs:7912`) зовёт `_lumen_pip_deliver_resize(w, h)`; если нет —
прокинуть вызов из drain-цикла/обработчика окна.

## Tests

- Юнит `crates/js/src/document_pip.rs` (добавить mod tests по образцу
  `video_pip.rs:181`): `requestWindow()` возвращает Promise; после установки
  `_lumen_pip_request_window` вызов доходит до очереди.
- Юнит `crates/js/src/pip_bindings.rs` (mod tests, `pip_bindings.rs:80`):
  новый `PipRequest::OpenDocument` попадает в `take_pip_requests`.
- Ручной прогон в окне: `documentPictureInPicture.requestWindow({width,height})`
  открывает реальное floating-окно (шелл-часть, вне graphic_tests).

## Definition of done

- [ ] `_lumen_pip_request_window` зарегистрирован и enqueue-ит `PipRequest`.
- [ ] Document PiP открывает реальное OS floating-окно нужного размера.
- [ ] `PictureInPictureWindow` унифицирован между video- и document-PiP.
- [ ] Resize OS-окна доставляет `_lumen_pip_deliver_resize` в JS (обе ветки).
- [ ] Ограничение «DOM-контент внутри document-PiP окна» задокументировано как follow-up.
- [ ] `CAPABILITIES.md` + `subsystems/js.md`/`shell.md` обновлены (Document PiP ✅/🟡).
